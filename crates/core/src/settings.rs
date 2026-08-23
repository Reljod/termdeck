//! What TermDeck remembers between launches.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::targets::{self, ApplyOptions};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The theme applied last.
    pub theme: Option<String>,
    /// The two themes the toggle switches between.
    pub light_theme: String,
    pub dark_theme: String,
    /// Which targets are switched on.
    pub enabled: Vec<String>,
    pub options: ApplyOptions,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: None,
            // The setup is on latte today, so that is the light half of the
            // toggle, and mocha is its natural opposite.
            light_theme: "catppuccin-latte".to_string(),
            dark_theme: "catppuccin-mocha".to_string(),
            enabled: targets::ALL.iter().map(|id| id.to_string()).collect(),
            options: ApplyOptions::default(),
        }
    }
}

pub fn settings_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".config/termdeck/settings.json")
}

impl Settings {
    /// Loads the saved settings, falling back to the defaults.
    ///
    /// A corrupt or half-written settings file gives defaults rather than an
    /// error. Losing which targets were ticked is a small thing; refusing to
    /// start over it would not be.
    pub fn load() -> Self {
        let path = settings_path();
        let Ok(text) = fs::read_to_string(path) else {
            return Self::default();
        };
        serde_json::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        let mut text = serde_json::to_string_pretty(self)?;
        text.push('\n');
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// The theme the toggle would switch to, given what is applied now.
    pub fn other_theme(&self, current: Option<&str>) -> String {
        match current {
            Some(id) if id == self.dark_theme => self.light_theme.clone(),
            _ => self.dark_theme.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_toggle_between_the_two_catppuccin_flavours() {
        let settings = Settings::default();
        assert_eq!(settings.light_theme, "catppuccin-latte");
        assert_eq!(settings.dark_theme, "catppuccin-mocha");
    }

    #[test]
    fn every_target_is_on_by_default() {
        // The point of the app is that one action changes all of them.
        let settings = Settings::default();
        assert_eq!(settings.enabled.len(), targets::ALL.len());
    }

    #[test]
    fn the_default_themes_exist() {
        let settings = Settings::default();
        assert!(crate::theme::find(&settings.light_theme).is_some());
        assert!(crate::theme::find(&settings.dark_theme).is_some());
    }

    #[test]
    fn the_toggle_alternates() {
        let settings = Settings::default();
        assert_eq!(settings.other_theme(Some("catppuccin-latte")), "catppuccin-mocha");
        assert_eq!(settings.other_theme(Some("catppuccin-mocha")), "catppuccin-latte");
    }

    #[test]
    fn an_unrecognised_current_theme_toggles_to_dark() {
        let settings = Settings::default();
        assert_eq!(settings.other_theme(None), "catppuccin-mocha");
        assert_eq!(settings.other_theme(Some("something-else")), "catppuccin-mocha");
    }

    #[test]
    fn settings_round_trip_through_json() {
        let mut settings = Settings::default();
        settings.theme = Some("dracula".to_string());
        settings.options.claude_use_ansi = true;

        let text = serde_json::to_string(&settings).unwrap();
        let back: Settings = serde_json::from_str(&text).unwrap();

        assert_eq!(back.theme.as_deref(), Some("dracula"));
        assert!(back.options.claude_use_ansi);
        assert_eq!(back.enabled, settings.enabled);
    }

    #[test]
    fn a_partial_settings_file_fills_in_the_rest() {
        // Older versions of the file will be missing newer keys.
        let settings: Settings = serde_json::from_str(r#"{"theme": "nord"}"#).unwrap();
        assert_eq!(settings.theme.as_deref(), Some("nord"));
        assert_eq!(settings.dark_theme, "catppuccin-mocha");
        assert_eq!(settings.enabled.len(), targets::ALL.len());
    }

    #[test]
    fn corrupt_settings_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str("{ broken").unwrap_or_default();
        assert_eq!(settings.light_theme, "catppuccin-latte");
    }
}
