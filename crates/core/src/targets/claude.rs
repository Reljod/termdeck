//! Claude Code.
//!
//! Claude Code has no palette to configure — it picks one of its own schemes,
//! so the only thing a theme decides here is light or dark. That is exactly the
//! part the user asked for: the terminal changing without the agent in it
//! staying the wrong colour.
//!
//! There is usually more than one Claude Code profile on a machine. This user
//! has four (`~/.claude`, `~/.claude-personal`, `~/.claude-work`,
//! `~/.claude-super`), and a theme that only reached one of them would look
//! broken half the time, so every profile that already has a settings file is
//! updated.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value};

use crate::theme::{Mode, Theme};

use super::{Detection, Outcome, Target, CLAUDE};

pub struct ClaudeCode;

/// Which of Claude Code's schemes to use.
///
/// `Ansi` is the interesting one for this app: it tells Claude Code to draw
/// with the terminal's own sixteen ANSI colours, which are the very colours
/// TermDeck just set. That makes it match the rest of the terminal exactly
/// rather than approximately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Standard,
    Ansi,
}

/// The value to write into `settings.json`.
pub fn theme_value(mode: Mode, scheme: Scheme) -> &'static str {
    match (mode, scheme) {
        (Mode::Light, Scheme::Standard) => "light",
        (Mode::Dark, Scheme::Standard) => "dark",
        (Mode::Light, Scheme::Ansi) => "light-ansi",
        (Mode::Dark, Scheme::Ansi) => "dark-ansi",
    }
}

/// Every Claude Code settings file on this machine.
///
/// Anything matching `~/.claude*` counts, because profiles are just differently
/// named config directories and hard-coding the four this user happens to have
/// would break the moment they add a fifth.
pub fn settings_files() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };

    let Ok(entries) = fs::read_dir(&home) else {
        return Vec::new();
    };

    let mut found: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name == ".claude" || name.starts_with(".claude-"))
        })
        .map(|dir| dir.join("settings.json"))
        .filter(|file| file.exists())
        .collect();

    found.sort();
    found
}

/// Sets `theme` in a settings document, leaving every other key alone.
///
/// The JSON is parsed and re-emitted rather than patched textually, so a
/// malformed write cannot corrupt a settings file. Key order is preserved
/// through `serde_json`'s `preserve_order` feature, which keeps the diff to the
/// one line that actually changed.
pub fn set_theme(existing: &str, value: &str) -> Result<String> {
    let mut document: Value = if existing.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str(existing).context("settings.json is not valid JSON")?
    };

    let object = document
        .as_object_mut()
        .context("settings.json is not a JSON object")?;
    object.insert("theme".to_string(), Value::String(value.to_string()));

    let mut text = serde_json::to_string_pretty(&document)?;
    text.push('\n');
    Ok(text)
}

/// Reads the theme currently set in a settings document.
pub fn read_theme(existing: &str) -> Option<String> {
    let document: Value = serde_json::from_str(existing).ok()?;
    document.get("theme")?.as_str().map(|s| s.to_string())
}

/// Whether a settings file already matches the mode we want.
pub fn matches_mode(text: &str, mode: Mode) -> bool {
    read_theme(text).is_some_and(|theme| theme.starts_with(mode.as_str()))
}

/// Whether every profile is already on the same mode.
///
/// Profiles drift apart easily — one gets set to `auto`, another to `light` —
/// and the result is Claude Code looking different depending on which profile
/// the session happened to start under. Worth saying out loud in the interface.
fn profiles_agree(files: &[PathBuf]) -> bool {
    let modes: Vec<Option<Mode>> = files
        .iter()
        .map(|file| {
            let text = fs::read_to_string(file).unwrap_or_default();
            [Mode::Light, Mode::Dark]
                .into_iter()
                .find(|mode| matches_mode(&text, *mode))
        })
        .collect();

    modes.windows(2).all(|pair| pair[0] == pair[1])
}

fn profile_name(settings: &Path) -> String {
    settings
        .parent()
        .and_then(|dir| dir.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or(".claude")
        .to_string()
}

impl Target for ClaudeCode {
    fn id(&self) -> &'static str {
        CLAUDE
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self) -> Detection {
        let files = settings_files();

        let detail = match files.len() {
            0 => "no Claude Code settings found".to_string(),
            1 => "one profile, following the theme's light or dark mode".to_string(),
            n if profiles_agree(&files) => {
                format!("{n} profiles, all following the theme's light or dark mode")
            }
            n => format!("{n} profiles, currently disagreeing — applying will line them up"),
        };

        // Report the mode rather than a theme id: Claude Code has no idea which
        // terminal theme it is sitting inside, only whether it is light or dark.
        let current = files.first().and_then(|file| {
            let text = fs::read_to_string(file).ok()?;
            read_theme(&text)
        });

        Detection {
            id: CLAUDE.to_string(),
            name: "Claude Code".to_string(),
            installed: !files.is_empty(),
            paths: files.iter().map(super::display_path).collect(),
            detail,
            current_theme: current,
        }
    }

    fn apply(&self, theme: &Theme, options: &super::ApplyOptions) -> Result<Outcome> {
        let files = settings_files();
        if files.is_empty() {
            return Ok(Outcome::skipped(
                CLAUDE,
                "Claude Code",
                "no Claude Code settings.json found",
            ));
        }

        let scheme = if options.claude_use_ansi {
            Scheme::Ansi
        } else {
            Scheme::Standard
        };
        let value = theme_value(theme.mode, scheme);

        let mut changed = Vec::new();
        let mut failures = Vec::new();

        for file in &files {
            let existing = crate::edit::read_or_empty(file)?;
            match set_theme(&existing, value) {
                Ok(updated) if updated != existing => {
                    match crate::edit::write_with_backup(file, &updated) {
                        Ok(_) => changed.push(file.clone()),
                        Err(error) => failures.push(format!("{}: {error}", profile_name(file))),
                    }
                }
                // Already correct; nothing to do.
                Ok(_) => {}
                Err(error) => failures.push(format!("{}: {error}", profile_name(file))),
            }
        }

        if !failures.is_empty() && changed.is_empty() {
            return Ok(Outcome::failed(
                CLAUDE,
                "Claude Code",
                format!("Could not update any profile — {}", failures.join("; ")),
            ));
        }

        let profiles = if changed.is_empty() {
            format!("Already on {value}.")
        } else {
            let names: Vec<String> = changed.iter().map(|f| profile_name(f)).collect();
            format!("Set {value} in {}.", names.join(", "))
        };

        let message = if failures.is_empty() {
            profiles
        } else {
            format!("{profiles} Some profiles failed: {}", failures.join("; "))
        };

        Ok(Outcome::pending(
            CLAUDE,
            "Claude Code",
            message,
            changed,
            "Claude Code reads its theme at startup, so restart any running session.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::find;

    #[test]
    fn dark_themes_give_claude_a_dark_scheme() {
        assert_eq!(theme_value(Mode::Dark, Scheme::Standard), "dark");
        assert_eq!(theme_value(Mode::Light, Scheme::Standard), "light");
    }

    #[test]
    fn the_ansi_scheme_tracks_the_terminal_palette() {
        assert_eq!(theme_value(Mode::Dark, Scheme::Ansi), "dark-ansi");
        assert_eq!(theme_value(Mode::Light, Scheme::Ansi), "light-ansi");
    }

    #[test]
    fn setting_the_theme_leaves_every_other_key_intact() {
        // This is the real shape of the user's settings file, and losing any of
        // it would break their setup far more than a wrong colour would.
        let existing = r#"{
  "includeCoAuthoredBy": false,
  "permissions": {
    "allow": ["Bash(pnpm build)"],
    "defaultMode": "acceptEdits"
  },
  "model": "opus",
  "theme": "light"
}"#;
        let out = set_theme(existing, "dark").unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();

        assert_eq!(parsed["theme"], "dark");
        assert_eq!(parsed["model"], "opus");
        assert_eq!(parsed["includeCoAuthoredBy"], false);
        assert_eq!(parsed["permissions"]["defaultMode"], "acceptEdits");
        assert_eq!(parsed["permissions"]["allow"][0], "Bash(pnpm build)");
    }

    #[test]
    fn key_order_survives_so_the_diff_stays_small() {
        let existing = r#"{"alpha": 1, "theme": "light", "zulu": 2}"#;
        let out = set_theme(existing, "dark").unwrap();
        let alpha = out.find("alpha").unwrap();
        let theme = out.find("theme").unwrap();
        let zulu = out.find("zulu").unwrap();
        assert!(alpha < theme && theme < zulu, "got:\n{out}");
    }

    #[test]
    fn a_settings_file_without_a_theme_key_gains_one() {
        let out = set_theme(r#"{"model": "opus"}"#, "dark").unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["theme"], "dark");
        assert_eq!(parsed["model"], "opus");
    }

    #[test]
    fn an_empty_file_becomes_a_valid_settings_object() {
        let out = set_theme("", "light").unwrap();
        let parsed: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["theme"], "light");
    }

    #[test]
    fn malformed_json_is_refused_rather_than_overwritten() {
        // Better to report a broken settings file than to replace it with a
        // fresh one and lose whatever the user had.
        let result = set_theme("{ not json", "dark");
        assert!(result.is_err());
    }

    #[test]
    fn a_json_array_is_refused() {
        assert!(set_theme("[1, 2, 3]", "dark").is_err());
    }

    #[test]
    fn the_written_file_ends_with_a_newline() {
        let out = set_theme(r#"{"model": "opus"}"#, "dark").unwrap();
        assert!(out.ends_with('\n'));
    }

    #[test]
    fn theme_round_trips() {
        let out = set_theme(r#"{"model":"opus"}"#, "dark-ansi").unwrap();
        assert_eq!(read_theme(&out).as_deref(), Some("dark-ansi"));
    }

    #[test]
    fn reading_a_file_without_a_theme_gives_nothing() {
        assert_eq!(read_theme(r#"{"model": "opus"}"#), None);
        assert_eq!(read_theme("not json"), None);
    }

    #[test]
    fn mode_matching_ignores_the_scheme_suffix() {
        let dark = set_theme("{}", "dark-ansi").unwrap();
        assert!(matches_mode(&dark, Mode::Dark));
        assert!(!matches_mode(&dark, Mode::Light));

        let light = set_theme("{}", "light").unwrap();
        assert!(matches_mode(&light, Mode::Light));
    }

    #[test]
    fn profiles_on_the_same_mode_agree() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let one = dir.path().join("one.json");
        let two = dir.path().join("two.json");
        fs::write(&one, set_theme("{}", "dark")?)?;
        fs::write(&two, set_theme("{}", "dark-ansi")?)?;
        // Different schemes, same mode — that is agreement as far as this goes.
        assert!(profiles_agree(&[one, two]));
        Ok(())
    }

    #[test]
    fn profiles_on_different_modes_are_reported_as_disagreeing() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let one = dir.path().join("one.json");
        let two = dir.path().join("two.json");
        fs::write(&one, set_theme("{}", "light")?)?;
        fs::write(&two, set_theme("{}", "dark")?)?;
        assert!(!profiles_agree(&[one, two]));
        Ok(())
    }

    #[test]
    fn a_single_profile_always_agrees_with_itself() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let one = dir.path().join("one.json");
        fs::write(&one, set_theme("{}", "light")?)?;
        assert!(profiles_agree(&[one]));
        assert!(profiles_agree(&[]));
        Ok(())
    }

    #[test]
    fn a_theme_value_we_do_not_recognise_counts_as_no_mode() {
        // `auto` is a real Claude Code value and belongs to neither mode.
        let auto = set_theme("{}", "auto").unwrap();
        assert!(!matches_mode(&auto, Mode::Light));
        assert!(!matches_mode(&auto, Mode::Dark));
    }

    #[test]
    fn a_light_terminal_theme_produces_a_light_claude() {
        let latte = find("catppuccin-latte").unwrap();
        assert_eq!(theme_value(latte.mode, Scheme::Standard), "light");

        let mocha = find("catppuccin-mocha").unwrap();
        assert_eq!(theme_value(mocha.mode, Scheme::Standard), "dark");
    }

    #[test]
    fn every_built_in_theme_maps_to_a_claude_scheme() {
        for theme in crate::theme::built_in_themes() {
            for scheme in [Scheme::Standard, Scheme::Ansi] {
                let value = theme_value(theme.mode, scheme);
                assert!(
                    value.starts_with("light") || value.starts_with("dark"),
                    "{} produced {value}",
                    theme.id
                );
            }
        }
    }
}
