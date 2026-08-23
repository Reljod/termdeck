//! iTerm2.
//!
//! iTerm2 needs two different things done, because neither one alone is enough.
//!
//! AppleScript reaches every session that is already open and recolours it now.
//! That is the visible half of the toggle, but it does not persist: a window
//! opened afterwards would come back with the old palette.
//!
//! So the palette is also written into the preferences plist, which is what new
//! windows read. The catch is that iTerm2 keeps its preferences in memory while
//! it runs and writes them out on quit, so a plist edit made while iTerm2 is
//! running can be overwritten when it exits. That is why the outcome says so
//! rather than claiming a clean success.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use plist::{Dictionary, Value};

use crate::color::Rgb;
use crate::theme::Theme;

use super::{Detection, Outcome, Target, ITERM2};

/// Where we record which theme we last wrote, so the UI can show it.
const THEME_KEY: &str = "TermDeckTheme";

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

/// Every colour key a theme sets, paired with its value.
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
    let mut entries: Vec<(String, Rgb)> = NAMED_KEYS
        .iter()
        .zip(named)
        .map(|(key, color)| ((*key).to_string(), color))
        .collect();

    for (index, color) in p.ansi.indexed().into_iter().enumerate() {
        entries.push((format!("Ansi {index} Color"), color));
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

        let uses_custom_folder = read_default("LoadPrefsFromCustomFolder")
            .map(|value| value == "1")
            .unwrap_or(false);

        let detail = if !installed {
            "iTerm2 is not installed".to_string()
        } else if !path.exists() {
            "no preferences file found".to_string()
        } else if uses_custom_folder {
            "preferences load from your own folder, so changes are versioned with it".to_string()
        } else if is_running() {
            "running; open sessions recolour immediately".to_string()
        } else {
            "installed and not running".to_string()
        };

        Detection {
            id: ITERM2.to_string(),
            name: "iTerm2".to_string(),
            installed,
            paths: vec![super::display_path(&path)],
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

        // Persist for windows opened later.
        if path.exists() {
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
            if touched == 0 {
                notes.push("no profiles found in the preferences file".to_string());
            }
        } else {
            notes.push("no preferences file to update, so only open sessions changed".to_string());
        }

        // Recolour what is open now.
        let mut live_applied = false;
        if options.live && is_running() {
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

        let running = is_running();
        let mut message = if live_applied {
            format!("Recoloured every open session to {}.", theme.name)
        } else if running {
            format!("Wrote {} to preferences.", theme.name)
        } else {
            format!("Wrote {} to preferences for the next launch.", theme.name)
        };

        if !notes.is_empty() {
            message.push(' ');
            message.push_str(&capitalise(&notes.join("; ")));
            message.push('.');
        }

        // iTerm2 writes its in-memory preferences over this file when it quits,
        // so persistence is only guaranteed once it has been restarted.
        let follow_up = running.then(|| {
            "iTerm2 saves its preferences when it quits, which can overwrite what TermDeck \
             wrote. To make the change stick for new windows, quit and reopen iTerm2, or set \
             Settings → General → Preferences → Save changes to \"Manually\"."
                .to_string()
        });

        Ok(match follow_up {
            Some(note) => Outcome::pending(ITERM2, "iTerm2", message, changed, note),
            None => Outcome::applied(ITERM2, "iTerm2", message, changed),
        })
    }
}

fn capitalise(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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
        assert_eq!(entries.len(), NAMED_KEYS.len() + 16);
        for index in 0..16 {
            let key = format!("Ansi {index} Color");
            assert!(
                entries.iter().any(|(k, _)| k == &key),
                "{key} should be written"
            );
        }
        for key in NAMED_KEYS {
            assert!(entries.iter().any(|(k, _)| k == key), "{key} should be written");
        }
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
            assert_eq!(preset_dict(&theme).len(), NAMED_KEYS.len() + 16, "{}", theme.id);
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
    fn a_tilde_in_the_prefs_folder_is_expanded() {
        let expanded = shellexpand_home("~/Developer/Config");
        assert!(!expanded.starts_with('~'));
        assert!(expanded.ends_with("Developer/Config"));
        assert_eq!(shellexpand_home("/absolute/path"), "/absolute/path");
    }
}
