//! iTerm2.
//!
//! Getting a theme to stick here takes two mechanisms, because neither one
//! alone covers both what is open now and what opens later.
//!
//! AppleScript reaches every session that is already open and recolours it
//! immediately. That is the visible half, but it only touches sessions: a new
//! tab reads its colours from a profile, so it comes back with the old palette.
//!
//! Writing the preferences plist looks like the answer and is not. iTerm2 holds
//! its preferences in memory the whole time it runs and writes them back out
//! when it quits, so an edit made while it is running is discarded — the theme
//! appears to persist and then silently does not. The plist is therefore only
//! written when iTerm2 is closed, when it genuinely is the right place.
//!
//! The mechanism that does work is a dynamic profile. iTerm2 watches
//! `~/Library/Application Support/iTerm2/DynamicProfiles/`, reloads anything
//! written there within a second, and never writes over it. TermDeck keeps one
//! profile there, copying the user's own default profile so their font and
//! keybindings come along, and overwriting only the colours. Once that profile
//! is the default, new windows pick up every theme change with nothing left to
//! do.
//!
//! Two things about that profile are easy to get wrong and were:
//!
//! It copies rather than inherits. `Dynamic Profile Parent Name` looks like the
//! right way to keep the user's settings, but for colours the parent wins over
//! the child, so an inheriting profile shows the parent's palette and the theme
//! never appears at all.
//!
//! It turns off [`SEPARATE_LIGHT_DARK`]. A profile with that switch on ignores
//! `Background Color` entirely and reads `Background Color (Light)` or
//! `(Dark)` according to the system appearance, so a correctly written theme
//! silently does nothing. Every colour is written to all three keys and the
//! switch is turned off, which makes the theme independent of whether macOS is
//! in light or dark mode.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use plist::{Dictionary, Value};

use crate::color::Rgb;
use crate::theme::Theme;

use super::{Detection, Outcome, Target, ITERM2};

/// Where we record which theme we last wrote, so the UI can show it.
const THEME_KEY: &str = "TermDeckTheme";

/// The identifier of the profile TermDeck maintains.
///
/// Fixed, so every apply updates the same profile rather than creating a new
/// one. iTerm2 accepts any string here.
pub const PROFILE_GUID: &str = "termdeck-managed-profile";
pub const PROFILE_NAME: &str = "TermDeck";

pub struct Iterm2;

/// The plist keys iTerm2 uses for a profile's non-ANSI colours.
const NAMED_KEYS: [&str; 7] = [
    "Background Color",
    "Foreground Color",
    "Bold Color",
    "Cursor Color",
    "Cursor Text Color",
    "Selection Color",
    "Selected Text Color",
];

/// Builds the plist dictionary iTerm2 stores for one colour.
pub fn color_dict(color: Rgb) -> Dictionary {
    let (r, g, b) = color.components();
    let mut dict = Dictionary::new();
    dict.insert("Red Component".into(), Value::Real(r));
    dict.insert("Green Component".into(), Value::Real(g));
    dict.insert("Blue Component".into(), Value::Real(b));
    dict.insert("Alpha Component".into(), Value::Real(1.0));
    dict.insert("Color Space".into(), Value::String("sRGB".into()));
    dict
}

/// The switch that makes a profile follow the system appearance.
///
/// When it is on, iTerm2 ignores `Background Color` entirely and reads
/// `Background Color (Light)` or `Background Color (Dark)` depending on whether
/// macOS is in light or dark mode. A profile with this set silently discards
/// every colour written to the plain keys, which looks exactly like the theme
/// failing to persist.
pub const SEPARATE_LIGHT_DARK: &str = "Use Separate Colors for Light and Dark Mode";

/// Every colour key a theme sets, paired with its value.
///
/// Each colour is written three times: to the plain key, and to the `(Light)`
/// and `(Dark)` variants. TermDeck decides what the terminal looks like, so a
/// theme should not change again because macOS switched appearance — and
/// writing all three means it does not matter which set iTerm2 consults.
pub fn color_entries(theme: &Theme) -> Vec<(String, Rgb)> {
    let p = &theme.palette;

    // Same order as NAMED_KEYS, so the two lists cannot drift apart.
    let named = [
        p.background,
        p.foreground,
        p.foreground,
        p.cursor,
        p.cursor_text,
        p.selection_background,
        p.selection_foreground,
    ];

    let base: Vec<(String, Rgb)> = NAMED_KEYS
        .iter()
        .zip(named)
        .map(|(key, color)| ((*key).to_string(), color))
        .chain(
            p.ansi
                .indexed()
                .into_iter()
                .enumerate()
                .map(|(index, color)| (format!("Ansi {index} Color"), color)),
        )
        .collect();

    let mut entries = Vec::with_capacity(base.len() * 3);
    for (key, color) in base {
        entries.push((format!("{key} (Light)"), color));
        entries.push((format!("{key} (Dark)"), color));
        entries.push((key, color));
    }
    entries
}

/// The colour preset iTerm2 shows in its Colors tab, so the theme is also
/// selectable by hand.
pub fn preset_dict(theme: &Theme) -> Dictionary {
    let mut preset = Dictionary::new();
    for (key, color) in color_entries(theme) {
        preset.insert(key, Value::Dictionary(color_dict(color)));
    }
    preset
}

/// The name this theme gets in iTerm2's preset list.
pub fn preset_name(theme: &Theme) -> String {
    format!("TermDeck {}", theme.name)
}

/// The AppleScript that recolours every open session.
///
/// Written as one script rather than one per session so it is a single
/// `osascript` call regardless of how many windows are open.
pub fn live_script(theme: &Theme) -> String {
    let p = &theme.palette;

    let mut assignments = vec![
        format!("set background color to {}", p.background.applescript()),
        format!("set foreground color to {}", p.foreground.applescript()),
        format!("set bold color to {}", p.foreground.applescript()),
        format!("set cursor color to {}", p.cursor.applescript()),
        format!("set cursor text color to {}", p.cursor_text.applescript()),
        format!(
            "set selection color to {}",
            p.selection_background.applescript()
        ),
        format!(
            "set selected text color to {}",
            p.selection_foreground.applescript()
        ),
    ];

    const ANSI_NAMES: [&str; 16] = [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "magenta",
        "cyan",
        "white",
        "bright black",
        "bright red",
        "bright green",
        "bright yellow",
        "bright blue",
        "bright magenta",
        "bright cyan",
        "bright white",
    ];

    for (name, color) in ANSI_NAMES.iter().zip(p.ansi.indexed()) {
        assignments.push(format!("set ANSI {name} color to {}", color.applescript()));
    }

    let body = assignments
        .iter()
        .map(|line| format!("            {line}"))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "\
tell application \"iTerm2\"
  repeat with theWindow in windows
    repeat with theTab in tabs of theWindow
      repeat with theSession in sessions of theTab
        tell theSession
{body}
        end tell
      end repeat
    end repeat
  end repeat
end tell
"
    )
}

/// The directory iTerm2 watches for profiles written by other programs.
///
/// This is the only place iTerm2 lets an outside program change a profile and
/// have it stick. Files here are reloaded within a second of being written, and
/// iTerm2 never writes over them — unlike the preferences plist, which it holds
/// in memory while running and saves back on quit.
pub fn dynamic_profiles_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Application Support/iTerm2/DynamicProfiles")
}

pub fn dynamic_profile_path() -> PathBuf {
    dynamic_profiles_dir().join("termdeck.json")
}

/// Converts a preferences value into the JSON a dynamic profile is written in.
///
/// Binary blobs and dates have no JSON equivalent and are dropped. Nothing in a
/// profile that matters for appearance is stored that way.
fn plist_to_json(value: &Value) -> Option<serde_json::Value> {
    use serde_json::{Map, Number, Value as Json};

    Some(match value {
        Value::String(text) => Json::String(text.clone()),
        Value::Boolean(flag) => Json::Bool(*flag),
        Value::Integer(number) => Json::Number(number.as_signed().map(Number::from)?),
        Value::Real(number) => Json::Number(Number::from_f64(*number)?),
        Value::Array(items) => Json::Array(items.iter().filter_map(plist_to_json).collect()),
        Value::Dictionary(dict) => {
            let mut out = Map::new();
            for (key, item) in dict {
                if let Some(converted) = plist_to_json(item) {
                    out.insert(key.clone(), converted);
                }
            }
            Json::Object(out)
        }
        _ => return None,
    })
}

/// The profile TermDeck maintains, as the JSON iTerm2 expects.
///
/// `base` is the user's own default profile, copied in so the managed profile
/// keeps their font, keybindings and everything else. It is copied rather than
/// inherited through `Dynamic Profile Parent Name` because that key does the
/// opposite of what it looks like for colours: the parent's colours win over
/// the child's, so a profile that inherits would never show the theme at all.
pub fn dynamic_profile(theme: &Theme, base: Option<&Dictionary>) -> serde_json::Value {
    use serde_json::{json, Map, Value as Json};

    let mut profile = Map::new();

    if let Some(base) = base {
        for (key, value) in base {
            // Identity is ours; everything else is theirs.
            if key == "Guid" || key == "Name" {
                continue;
            }
            if let Some(converted) = plist_to_json(value) {
                profile.insert(key.clone(), converted);
            }
        }
    }

    profile.insert("Name".into(), json!(PROFILE_NAME));
    profile.insert("Guid".into(), json!(PROFILE_GUID));
    // The theme picks the colours, not the system appearance.
    profile.insert(SEPARATE_LIGHT_DARK.into(), json!(false));

    // Colours last, so they overwrite whatever the copied profile had.
    for (key, color) in color_entries(theme) {
        let (r, g, b) = color.components();
        profile.insert(
            key,
            json!({
                "Red Component": r,
                "Green Component": g,
                "Blue Component": b,
                "Alpha Component": 1.0,
                "Color Space": "sRGB",
            }),
        );
    }

    json!({ "Profiles": [Json::Object(profile)] })
}

/// The user's default profile, to copy into the managed one.
pub fn default_profile(root: &Dictionary) -> Option<Dictionary> {
    let guid = root.get("Default Bookmark Guid")?.as_string()?;
    let bookmarks = root.get("New Bookmarks")?.as_array()?;
    bookmarks
        .iter()
        .filter_map(|bookmark| bookmark.as_dictionary())
        .find(|profile| {
            let this = profile.get("Guid").and_then(|g| g.as_string());
            // Never copy from ourselves, or the theme would compound.
            this == Some(guid) && this != Some(PROFILE_GUID)
        })
        .cloned()
}

/// The name of the profile iTerm2 opens new windows with.
pub fn default_profile_name(root: &Dictionary) -> Option<String> {
    let guid = root.get("Default Bookmark Guid")?.as_string()?;
    let bookmarks = root.get("New Bookmarks")?.as_array()?;
    bookmarks
        .iter()
        .filter_map(|bookmark| bookmark.as_dictionary())
        .find(|profile| profile.get("Guid").and_then(|g| g.as_string()) == Some(guid))
        .and_then(|profile| profile.get("Name"))
        .and_then(|name| name.as_string())
        .map(|name| name.to_string())
}

/// Whether the managed profile is the one new windows use.
///
/// Until it is, writing the profile changes nothing the user can see, so this
/// decides whether the interface still has something to ask of them.
pub fn managed_profile_is_default() -> bool {
    read_default("Default Bookmark Guid").as_deref() == Some(PROFILE_GUID)
}

/// Whether iTerm2 is running right now.
fn is_running() -> bool {
    Command::new("osascript")
        .args(["-e", "application \"iTerm2\" is running"])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).trim() == "true")
        .unwrap_or(false)
}

fn iterm_installed() -> bool {
    Path::new("/Applications/iTerm.app").exists()
        || dirs::home_dir()
            .map(|home| home.join("Applications/iTerm.app").exists())
            .unwrap_or(false)
}

/// Reads one string setting out of iTerm2's preferences.
///
/// This goes through `defaults` rather than the file because macOS keeps
/// preferences in a caching daemon, and the file on disk can lag behind what
/// iTerm2 actually believes.
fn read_default(key: &str) -> Option<String> {
    let out = Command::new("defaults")
        .args(["read", "com.googlecode.iterm2", key])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// The preferences file iTerm2 is actually reading.
///
/// iTerm2 can be pointed at a folder of its own, which this user does — their
/// preferences live in a git repository rather than in `~/Library`. Writing to
/// the default location in that case would change nothing at all.
pub fn prefs_path() -> PathBuf {
    let custom_folder = read_default("PrefsCustomFolder");
    let loads_custom = read_default("LoadPrefsFromCustomFolder")
        .map(|value| value == "1")
        .unwrap_or(false);

    if let (Some(folder), true) = (custom_folder, loads_custom) {
        let path = PathBuf::from(shellexpand_home(&folder)).join("com.googlecode.iterm2.plist");
        if path.exists() {
            return path;
        }
    }

    dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Preferences/com.googlecode.iterm2.plist")
}

/// Expands a leading `~`, which `defaults` may hand back unexpanded.
fn shellexpand_home(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .unwrap_or_default()
            .join(rest)
            .to_string_lossy()
            .to_string(),
        None => path.to_string(),
    }
}

/// Whether a plist file is in the binary format, so it can be written back the
/// way it was found.
fn is_binary_plist(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.starts_with(b"bplist")
}

/// Writes the palette into the preferences document.
///
/// Kept separate from the file handling so the transformation can be tested on
/// a plist built in memory.
pub fn apply_to_prefs(root: &mut Dictionary, theme: &Theme, all_profiles: bool) -> usize {
    let entries = color_entries(theme);

    // Register the preset, so the theme also appears in iTerm2's Colors tab.
    // Any presets already there are the user's own and are left in place.
    if !root
        .get("Custom Color Presets")
        .is_some_and(|value| value.as_dictionary().is_some())
    {
        root.insert(
            "Custom Color Presets".to_string(),
            Value::Dictionary(Dictionary::new()),
        );
    }
    if let Some(presets) = root
        .get_mut("Custom Color Presets")
        .and_then(|value| value.as_dictionary_mut())
    {
        presets.insert(preset_name(theme), Value::Dictionary(preset_dict(theme)));
    }

    root.insert(THEME_KEY.to_string(), Value::String(theme.id.clone()));

    let default_guid = root
        .get("Default Bookmark Guid")
        .and_then(|value| value.as_string())
        .map(|s| s.to_string());

    let Some(bookmarks) = root
        .get_mut("New Bookmarks")
        .and_then(|value| value.as_array_mut())
    else {
        return 0;
    };

    let mut touched = 0;
    for bookmark in bookmarks.iter_mut() {
        let Some(profile) = bookmark.as_dictionary_mut() else {
            continue;
        };

        if !all_profiles {
            let guid = profile.get("Guid").and_then(|value| value.as_string());
            if guid.map(|g| g.to_string()) != default_guid {
                continue;
            }
        }

        // Otherwise the plain colour keys below would be ignored whenever
        // macOS is in the appearance the profile has separate colours for.
        profile.insert(SEPARATE_LIGHT_DARK.to_string(), Value::Boolean(false));

        for (key, color) in &entries {
            profile.insert(key.clone(), Value::Dictionary(color_dict(*color)));
        }
        touched += 1;
    }

    touched
}

/// Reads back the theme id recorded in the preferences.
pub fn theme_from_prefs(root: &Dictionary) -> Option<String> {
    root.get(THEME_KEY)
        .and_then(|value| value.as_string())
        .map(|s| s.to_string())
}

impl Target for Iterm2 {
    fn id(&self) -> &'static str {
        ITERM2
    }

    fn name(&self) -> &'static str {
        "iTerm2"
    }

    fn detect(&self) -> Detection {
        let installed = iterm_installed();
        let path = prefs_path();

        let current = Value::from_file(&path)
            .ok()
            .and_then(|value| value.into_dictionary())
            .as_ref()
            .and_then(theme_from_prefs);

        let detail = if !installed {
            "iTerm2 is not installed".to_string()
        } else if managed_profile_is_default() {
            "the TermDeck profile is your default, so themes stick on their own".to_string()
        } else if dynamic_profile_path().exists() {
            format!(
                "the \"{PROFILE_NAME}\" profile exists but is not your default yet, so new \
                 windows keep their old colours"
            )
        } else {
            "open sessions recolour immediately; a managed profile makes it persist".to_string()
        };

        Detection {
            id: ITERM2.to_string(),
            name: "iTerm2".to_string(),
            installed,
            paths: vec![
                super::display_path(&dynamic_profile_path()),
                super::display_path(&path),
            ],
            detail,
            current_theme: current,
        }
    }

    fn apply(&self, theme: &Theme, options: &super::ApplyOptions) -> Result<Outcome> {
        if !iterm_installed() {
            return Ok(Outcome::skipped(
                ITERM2,
                "iTerm2",
                "iTerm2 is not installed",
            ));
        }

        let path = prefs_path();
        let mut changed = Vec::new();
        let mut notes = Vec::new();
        let running = is_running();

        // The managed profile is the part that actually persists. iTerm2 picks
        // it up within a second and never writes over it.
        let base = Value::from_file(&path)
            .ok()
            .and_then(|value| value.into_dictionary())
            .as_ref()
            .and_then(default_profile);

        let profile_path = dynamic_profile_path();
        let profile_json = format!(
            "{}\n",
            serde_json::to_string_pretty(&dynamic_profile(theme, base.as_ref()))?
        );
        if crate::edit::read_or_empty(&profile_path)? != profile_json {
            crate::edit::write_with_backup(&profile_path, &profile_json)?;
        }
        changed.push(profile_path);

        // Writing the preferences plist only helps while iTerm2 is closed. If
        // it is running it holds preferences in memory and saves them back on
        // quit, so a write now would simply be discarded — and claiming it
        // worked would be worse than not doing it.
        if !running && path.exists() {
            let binary = is_binary_plist(&path);
            let value = Value::from_file(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let mut root = value
                .into_dictionary()
                .context("iTerm2 preferences are not a dictionary")?;

            let touched = apply_to_prefs(&mut root, theme, options.iterm_all_profiles);

            crate::edit::backup(&path)?;
            let document = Value::Dictionary(root);
            if binary {
                document.to_file_binary(&path)
            } else {
                document.to_file_xml(&path)
            }
            .with_context(|| format!("writing {}", path.display()))?;

            changed.push(path.clone());
            if touched > 0 {
                notes.push(format!(
                    "and into {touched} saved profile{}",
                    if touched == 1 { "" } else { "s" }
                ));
            }
        }

        // Recolour what is open now.
        let mut live_applied = false;
        if options.live && running {
            let script = live_script(theme);
            let out = Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
                .context("running osascript")?;

            if out.status.success() {
                live_applied = true;
            } else {
                let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                notes.push(format!("could not recolour open sessions ({stderr})"));
            }
        }

        let is_default = managed_profile_is_default();

        let mut message = if live_applied {
            format!("Recoloured every open session to {}", theme.name)
        } else {
            format!("Wrote {} into the TermDeck profile", theme.name)
        };
        if !notes.is_empty() {
            message.push(' ');
            message.push_str(&notes.join("; "));
        }
        message.push('.');

        if is_default {
            // The managed profile is the default, so new windows get the theme
            // too and nothing is left for the user to do.
            return Ok(Outcome::applied(ITERM2, "iTerm2", message, changed));
        }

        // The profile is written and iTerm2 has already reloaded it, but new
        // windows will keep using whichever profile is the default until that
        // is switched over — once, by hand, because iTerm2 keeps the default in
        // the preferences it holds in memory.
        Ok(Outcome::pending(
            ITERM2,
            "iTerm2",
            message,
            changed,
            format!(
                "New windows still open with your current profile. To make this stick for good, \
                 open iTerm2 Settings → Profiles, select \"{PROFILE_NAME}\", and use Other Actions \
                 → Set as Default. That is a one-time step; after it, every theme change applies \
                 to new windows on its own."
            ),
        ))
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::find;

    fn sample_prefs() -> Dictionary {
        let mut personal = Dictionary::new();
        personal.insert("Guid".into(), Value::String("GUID-PERSONAL".into()));
        personal.insert("Name".into(), Value::String("Personal".into()));
        personal.insert("Normal Font".into(), Value::String("Menlo 12".into()));

        let mut other = Dictionary::new();
        other.insert("Guid".into(), Value::String("GUID-OTHER".into()));
        other.insert("Name".into(), Value::String("Test".into()));

        let mut root = Dictionary::new();
        root.insert(
            "New Bookmarks".into(),
            Value::Array(vec![
                Value::Dictionary(personal),
                Value::Dictionary(other),
            ]),
        );
        root.insert(
            "Default Bookmark Guid".into(),
            Value::String("GUID-PERSONAL".into()),
        );
        root
    }

    #[test]
    fn a_colour_becomes_the_dictionary_iterm_expects() {
        let dict = color_dict(Rgb::new(255, 0, 0));
        assert_eq!(dict.get("Red Component").unwrap().as_real(), Some(1.0));
        assert_eq!(dict.get("Green Component").unwrap().as_real(), Some(0.0));
        assert_eq!(dict.get("Alpha Component").unwrap().as_real(), Some(1.0));
        assert_eq!(dict.get("Color Space").unwrap().as_string(), Some("sRGB"));
    }

    #[test]
    fn a_theme_sets_all_sixteen_ansi_slots_plus_the_named_ones() {
        let entries = color_entries(&find("catppuccin-mocha").unwrap());
        // Every colour three times: plain, (Light) and (Dark).
        assert_eq!(entries.len(), (NAMED_KEYS.len() + 16) * 3);

        for index in 0..16 {
            for suffix in ["", " (Light)", " (Dark)"] {
                let key = format!("Ansi {index} Color{suffix}");
                assert!(
                    entries.iter().any(|(k, _)| k == &key),
                    "{key} should be written"
                );
            }
        }
        for key in NAMED_KEYS {
            for suffix in ["", " (Light)", " (Dark)"] {
                let key = format!("{key}{suffix}");
                assert!(
                    entries.iter().any(|(k, _)| k == &key),
                    "{key} should be written"
                );
            }
        }
    }

    #[test]
    fn the_light_and_dark_variants_get_the_same_colour() {
        // A theme is one appearance. If the variants differed, the terminal
        // would change again whenever macOS switched between light and dark.
        let entries = color_entries(&find("catppuccin-mocha").unwrap());
        let lookup = |name: &str| {
            entries
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, color)| *color)
                .unwrap()
        };

        for base in ["Background Color", "Foreground Color", "Ansi 4 Color"] {
            let plain = lookup(base);
            assert_eq!(lookup(&format!("{base} (Light)")), plain, "{base}");
            assert_eq!(lookup(&format!("{base} (Dark)")), plain, "{base}");
        }
    }

    #[test]
    fn the_managed_profile_stops_following_the_system_appearance() {
        // This is the setting that made a correctly written theme appear not to
        // apply at all: with it on, iTerm2 reads only the (Light)/(Dark) keys.
        let json = dynamic_profile(&find("catppuccin-mocha").unwrap(), None);
        assert_eq!(json["Profiles"][0][SEPARATE_LIGHT_DARK], false);
    }

    #[test]
    fn writing_preferences_also_stops_following_the_appearance() {
        let mut root = sample_prefs();
        apply_to_prefs(&mut root, &find("catppuccin-mocha").unwrap(), true);

        let bookmarks = root.get("New Bookmarks").unwrap().as_array().unwrap();
        for bookmark in bookmarks {
            let profile = bookmark.as_dictionary().unwrap();
            assert_eq!(
                profile.get(SEPARATE_LIGHT_DARK).unwrap().as_boolean(),
                Some(false)
            );
            // And the variant keys carry the theme too.
            let light = profile
                .get("Background Color (Light)")
                .unwrap()
                .as_dictionary()
                .unwrap();
            let red = light.get("Red Component").unwrap().as_real().unwrap();
            assert!((red - 30.0 / 255.0).abs() < 1e-6);
        }
    }

    #[test]
    fn a_profile_that_followed_the_appearance_is_corrected() {
        // The real profile on this machine had the switch on, with latte in the
        // (Light) keys. Applying a dark theme has to overwrite both.
        let mut root = sample_prefs();
        if let Some(bookmarks) = root
            .get_mut("New Bookmarks")
            .and_then(|value| value.as_array_mut())
        {
            for bookmark in bookmarks.iter_mut() {
                let profile = bookmark.as_dictionary_mut().unwrap();
                profile.insert(SEPARATE_LIGHT_DARK.into(), Value::Boolean(true));
                profile.insert(
                    "Background Color (Light)".into(),
                    Value::Dictionary(color_dict(Rgb::parse("#eff1f5").unwrap())),
                );
            }
        }

        apply_to_prefs(&mut root, &find("catppuccin-mocha").unwrap(), true);

        let base = default_profile(&root).unwrap();
        assert_eq!(base.get(SEPARATE_LIGHT_DARK).unwrap().as_boolean(), Some(false));
        let light = base
            .get("Background Color (Light)")
            .unwrap()
            .as_dictionary()
            .unwrap();
        let red = light.get("Red Component").unwrap().as_real().unwrap();
        assert!((red - 30.0 / 255.0).abs() < 1e-6, "latte survived into mocha");
    }

    #[test]
    fn writing_a_theme_colours_every_profile() {
        let mut root = sample_prefs();
        let touched = apply_to_prefs(&mut root, &find("catppuccin-mocha").unwrap(), true);
        assert_eq!(touched, 2);

        let bookmarks = root.get("New Bookmarks").unwrap().as_array().unwrap();
        for bookmark in bookmarks {
            let profile = bookmark.as_dictionary().unwrap();
            let bg = profile
                .get("Background Color")
                .unwrap()
                .as_dictionary()
                .unwrap();
            // Mocha's background is #1e1e2e, so red is 0x1e/255.
            let red = bg.get("Red Component").unwrap().as_real().unwrap();
            assert!((red - 30.0 / 255.0).abs() < 1e-6);
        }
    }

    #[test]
    fn only_the_default_profile_changes_when_asked() {
        let mut root = sample_prefs();
        let touched = apply_to_prefs(&mut root, &find("dracula").unwrap(), false);
        assert_eq!(touched, 1);

        let bookmarks = root.get("New Bookmarks").unwrap().as_array().unwrap();
        let personal = bookmarks[0].as_dictionary().unwrap();
        let other = bookmarks[1].as_dictionary().unwrap();
        assert!(personal.contains_key("Background Color"));
        assert!(!other.contains_key("Background Color"));
    }

    #[test]
    fn the_rest_of_a_profile_is_left_alone() {
        // Colours are the only thing we own; fonts and keybindings are not.
        let mut root = sample_prefs();
        apply_to_prefs(&mut root, &find("nord").unwrap(), true);

        let bookmarks = root.get("New Bookmarks").unwrap().as_array().unwrap();
        let personal = bookmarks[0].as_dictionary().unwrap();
        assert_eq!(
            personal.get("Normal Font").unwrap().as_string(),
            Some("Menlo 12")
        );
        assert_eq!(personal.get("Name").unwrap().as_string(), Some("Personal"));
    }

    #[test]
    fn the_theme_is_registered_as_a_selectable_preset() {
        let mut root = sample_prefs();
        let theme = find("tokyo-night").unwrap();
        apply_to_prefs(&mut root, &theme, true);

        let presets = root
            .get("Custom Color Presets")
            .unwrap()
            .as_dictionary()
            .unwrap();
        let preset = presets
            .get("TermDeck Tokyo Night")
            .unwrap()
            .as_dictionary()
            .unwrap();
        assert!(preset.contains_key("Ansi 0 Color"));
        assert!(preset.contains_key("Background Color"));
    }

    #[test]
    fn existing_presets_are_not_thrown_away() {
        // The user already has four hand-made presets in this file.
        let mut root = sample_prefs();
        let mut presets = Dictionary::new();
        presets.insert("darkmatrix".into(), Value::Dictionary(Dictionary::new()));
        root.insert("Custom Color Presets".into(), Value::Dictionary(presets));

        apply_to_prefs(&mut root, &find("dracula").unwrap(), true);

        let presets = root
            .get("Custom Color Presets")
            .unwrap()
            .as_dictionary()
            .unwrap();
        assert!(presets.contains_key("darkmatrix"), "existing preset dropped");
        assert!(presets.contains_key("TermDeck Dracula"));
    }

    #[test]
    fn the_applied_theme_can_be_read_back() {
        let mut root = sample_prefs();
        apply_to_prefs(&mut root, &find("gruvbox-dark").unwrap(), true);
        assert_eq!(theme_from_prefs(&root).as_deref(), Some("gruvbox-dark"));
    }

    #[test]
    fn preferences_without_profiles_do_not_panic() {
        let mut root = Dictionary::new();
        let touched = apply_to_prefs(&mut root, &find("dracula").unwrap(), true);
        assert_eq!(touched, 0);
        // The preset is still registered, so the theme is selectable by hand.
        assert!(root.contains_key("Custom Color Presets"));
    }

    #[test]
    fn applying_twice_is_stable() {
        let theme = find("nord").unwrap();
        let mut once = sample_prefs();
        apply_to_prefs(&mut once, &theme, true);
        let mut twice = once.clone();
        apply_to_prefs(&mut twice, &theme, true);
        assert_eq!(once, twice);
    }

    #[test]
    fn the_live_script_sets_every_colour_for_every_session() {
        let script = live_script(&find("catppuccin-mocha").unwrap());
        assert!(script.contains("tell application \"iTerm2\""));
        assert!(script.contains("repeat with theSession in sessions of theTab"));
        assert!(script.contains("set background color to"));
        assert!(script.contains("set ANSI bright white color to"));
        assert_eq!(script.matches("set ANSI ").count(), 16);
    }

    #[test]
    fn the_live_script_uses_sixteen_bit_colour_values() {
        // AppleScript colour properties are 0-65535, not 0-255.
        let script = live_script(&find("catppuccin-latte").unwrap());
        // Latte's background #eff1f5 -> 239,241,245 scaled to 16 bits.
        let expected = Rgb::parse("#eff1f5").unwrap().applescript();
        assert!(
            script.contains(&format!("set background color to {expected}")),
            "got:\n{script}"
        );
    }

    #[test]
    fn the_live_script_is_balanced_applescript() {
        let script = live_script(&find("dracula").unwrap());
        assert_eq!(script.matches("repeat with").count(), 3);
        assert_eq!(script.matches("end repeat").count(), 3);
        assert_eq!(script.matches("tell ").count(), 2);
        assert_eq!(script.matches("end tell").count(), 2);
    }

    #[test]
    fn every_theme_produces_a_usable_script_and_preset() {
        for theme in crate::theme::built_in_themes() {
            let script = live_script(&theme);
            assert_eq!(script.matches("set ANSI ").count(), 16, "{}", theme.id);
            assert_eq!(preset_dict(&theme).len(), (NAMED_KEYS.len() + 16) * 3, "{}", theme.id);
        }
    }

    #[test]
    fn preset_names_are_distinct_per_theme() {
        let names: std::collections::HashSet<String> = crate::theme::built_in_themes()
            .iter()
            .map(preset_name)
            .collect();
        assert_eq!(names.len(), crate::theme::built_in_themes().len());
    }

    #[test]
    fn the_managed_profile_carries_every_colour() {
        let theme = find("catppuccin-mocha").unwrap();
        let json = dynamic_profile(&theme, None);
        let profile = &json["Profiles"][0];

        assert_eq!(profile["Name"], PROFILE_NAME);
        assert_eq!(profile["Guid"], PROFILE_GUID);
        for index in 0..16 {
            assert!(
                profile[format!("Ansi {index} Color")].is_object(),
                "Ansi {index} Color missing"
            );
        }
        assert!(profile["Background Color"].is_object());
        // Mocha's background is #1e1e2e.
        let red = profile["Background Color"]["Red Component"].as_f64().unwrap();
        assert!((red - 30.0 / 255.0).abs() < 1e-6);
        assert_eq!(profile["Background Color"]["Color Space"], "sRGB");
    }

    #[test]
    fn the_managed_profile_keeps_the_users_other_settings() {
        // The font and everything else has to come along, or switching to this
        // profile would change far more than the colours.
        let root = sample_prefs();
        let base = default_profile(&root).expect("a default profile");
        let json = dynamic_profile(&find("nord").unwrap(), Some(&base));
        let profile = &json["Profiles"][0];

        assert_eq!(profile["Normal Font"], "Menlo 12");
        // But its identity is ours, not theirs.
        assert_eq!(profile["Name"], PROFILE_NAME);
        assert_eq!(profile["Guid"], PROFILE_GUID);
    }

    #[test]
    fn the_theme_wins_over_the_copied_profile() {
        // The copied profile carries its own colours, and ours have to be the
        // ones that survive.
        let mut root = sample_prefs();
        apply_to_prefs(&mut root, &find("catppuccin-latte").unwrap(), true);
        let base = default_profile(&root).expect("a default profile");

        let json = dynamic_profile(&find("catppuccin-mocha").unwrap(), Some(&base));
        let red = json["Profiles"][0]["Background Color"]["Red Component"]
            .as_f64()
            .unwrap();
        // Mocha's #1e1e2e, not latte's #eff1f5.
        assert!((red - 30.0 / 255.0).abs() < 1e-6, "got {red}");
    }

    #[test]
    fn the_profile_never_inherits() {
        // `Dynamic Profile Parent Name` makes the parent's colours win over the
        // child's, so a profile carrying that key would never show the theme.
        let root = sample_prefs();
        let base = default_profile(&root);
        for parent in [None, base.as_ref()] {
            let json = dynamic_profile(&find("nord").unwrap(), parent);
            assert!(
                json["Profiles"][0]
                    .get("Dynamic Profile Parent Name")
                    .is_none(),
                "inheriting would silently discard the theme"
            );
        }
    }

    #[test]
    fn the_managed_profile_is_never_copied_from_itself() {
        // Otherwise the profile would accumulate its own previous state.
        let mut root = sample_prefs();
        root.insert(
            "Default Bookmark Guid".into(),
            Value::String(PROFILE_GUID.into()),
        );
        assert!(default_profile(&root).is_none());
    }

    #[test]
    fn unrepresentable_values_are_dropped_rather_than_breaking_the_file() {
        // Profiles can hold binary blobs, which have no JSON form. Dropping one
        // is fine; writing invalid JSON would make iTerm2 ignore the profile.
        let mut base = Dictionary::new();
        base.insert("Normal Font".into(), Value::String("Menlo 12".into()));
        base.insert("Some Blob".into(), Value::Data(vec![1, 2, 3]));
        base.insert("Transparency".into(), Value::Real(0.15));
        base.insert("Blur".into(), Value::Boolean(true));
        base.insert("Columns".into(), Value::Integer(120.into()));

        let json = dynamic_profile(&find("nord").unwrap(), Some(&base));
        let profile = &json["Profiles"][0];
        assert_eq!(profile["Normal Font"], "Menlo 12");
        assert_eq!(profile["Transparency"], 0.15);
        assert_eq!(profile["Blur"], true);
        assert_eq!(profile["Columns"], 120);
        assert!(profile.get("Some Blob").is_none());

        // And the result is still valid JSON.
        let text = serde_json::to_string(&json).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).expect("valid JSON");
    }

    #[test]
    fn the_guid_is_stable_across_themes() {
        // A changing guid would leave a new profile behind on every apply.
        let a = dynamic_profile(&find("nord").unwrap(), None);
        let b = dynamic_profile(&find("dracula").unwrap(), None);
        assert_eq!(a["Profiles"][0]["Guid"], b["Profiles"][0]["Guid"]);
    }

    #[test]
    fn the_profile_is_the_shape_iterm_reads() {
        // iTerm2 expects a top-level "Profiles" array, not a bare object.
        let json = dynamic_profile(&find("nord").unwrap(), None);
        assert!(json["Profiles"].is_array());
        assert_eq!(json["Profiles"].as_array().unwrap().len(), 1);
        // And it has to survive a round trip through a file. Colours are
        // compared with a tolerance rather than exactly: writing a float and
        // reading it back can shift the last bit, which is around 1e-17 of a
        // channel that only has 256 distinguishable values.
        let text = serde_json::to_string_pretty(&json).unwrap();
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();

        let original = json["Profiles"][0].as_object().unwrap();
        let reloaded = back["Profiles"][0].as_object().unwrap();
        assert_eq!(original.len(), reloaded.len());

        for (key, value) in original {
            let other = reloaded.get(key).unwrap_or_else(|| panic!("lost {key}"));
            match value.as_object() {
                Some(color) => {
                    for (channel, number) in color {
                        match number.as_f64() {
                            Some(expected) => {
                                let got = other[channel].as_f64().unwrap();
                                assert!(
                                    (expected - got).abs() < 1e-9,
                                    "{key}/{channel}: {expected} became {got}"
                                );
                                // Still the same 8-bit colour, which is all
                                // iTerm2 can actually display.
                                assert_eq!(
                                    (expected * 255.0).round(),
                                    (got * 255.0).round()
                                );
                            }
                            None => assert_eq!(number, &other[channel]),
                        }
                    }
                }
                None => assert_eq!(value, other),
            }
        }
    }

    #[test]
    fn the_default_profile_name_is_read_from_preferences() {
        let root = sample_prefs();
        assert_eq!(default_profile_name(&root).as_deref(), Some("Personal"));
    }

    #[test]
    fn preferences_without_a_default_give_no_parent() {
        let mut root = sample_prefs();
        root.remove("Default Bookmark Guid");
        assert_eq!(default_profile_name(&root), None);

        // A guid pointing at a profile that is gone is also handled.
        let mut root = sample_prefs();
        root.insert(
            "Default Bookmark Guid".into(),
            Value::String("GONE".into()),
        );
        assert_eq!(default_profile_name(&root), None);
    }

    #[test]
    fn every_theme_produces_a_valid_managed_profile() {
        for theme in crate::theme::built_in_themes() {
            let json = dynamic_profile(&theme, None);
            let profile = json["Profiles"][0].as_object().unwrap();
            // Name, Guid, the appearance switch, plus every colour key.
            assert_eq!(profile.len(), 3 + (NAMED_KEYS.len() + 16) * 3, "{}", theme.id);
        }
    }

    #[test]
    fn the_profile_lands_where_iterm_watches() {
        let path = dynamic_profile_path();
        assert!(path.ends_with("DynamicProfiles/termdeck.json"), "{path:?}");
    }

    #[test]
    fn a_tilde_in_the_prefs_folder_is_expanded() {
        let expanded = shellexpand_home("~/Developer/Config");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.ends_with("Developer/Config"));
        assert_eq!(shellexpand_home("/absolute/path"), "/absolute/path");
    }
}
