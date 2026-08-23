//! TermDeck's engine.
//!
//! One theme is chosen and pushed to several targets that have nothing in
//! common: a plist, two shell configs, and a JSON settings file. The rules that
//! hold across all of them are in [`edit`] — back up before writing, and only
//! ever own the text between our own markers.

pub mod color;
pub mod edit;
pub mod settings;
pub mod targets;
pub mod theme;

use anyhow::{anyhow, Result};

use settings::Settings;
use targets::{ApplyOptions, Detection, Outcome, Status};
use theme::Theme;

/// Everything the interface needs to draw itself.
#[derive(Debug, serde::Serialize)]
pub struct State {
    pub themes: Vec<Theme>,
    pub targets: Vec<Detection>,
    pub settings: Settings,
    /// The theme the machine looks to be on, worked out from the targets
    /// themselves rather than from what we last recorded.
    pub detected_theme: Option<String>,
}

/// Inspects the machine.
pub fn state() -> State {
    let detections: Vec<Detection> = targets::all_targets()
        .iter()
        .map(|target| target.detect())
        .collect();

    // Believe the targets over our own settings file: the user may well have
    // edited a dotfile by hand since the last apply.
    let detected = detections
        .iter()
        .filter(|d| d.id != targets::CLAUDE) // Claude only knows light or dark.
        .find_map(|d| d.current_theme.clone());

    State {
        themes: theme::built_in_themes(),
        targets: detections,
        settings: Settings::load(),
        detected_theme: detected,
    }
}

/// Applies a theme to the chosen targets.
///
/// Every target is attempted even if an earlier one fails, because a broken
/// iTerm2 preferences file is no reason to leave tmux on the old theme. The
/// caller gets one outcome per target and decides what to say about it.
pub fn apply(theme_id: &str, options: &ApplyOptions, enabled: &[String]) -> Result<Vec<Outcome>> {
    let theme = theme::find(theme_id).ok_or_else(|| anyhow!("no theme called `{theme_id}`"))?;
    Ok(apply_theme(&theme, options, enabled))
}

fn apply_theme(theme: &Theme, options: &ApplyOptions, enabled: &[String]) -> Vec<Outcome> {
    let mut outcomes = Vec::new();

    for target in targets::all_targets() {
        let id = target.id();

        if !enabled.iter().any(|wanted| wanted == id) {
            outcomes.push(Outcome::skipped(
                id,
                target.name(),
                "Switched off in TermDeck.",
            ));
            continue;
        }

        let outcome = target
            .apply(theme, options)
            .unwrap_or_else(|error| Outcome::failed(id, target.name(), format!("{error:#}")));
        outcomes.push(outcome);
    }

    // Only remember a theme that actually reached something.
    if outcomes
        .iter()
        .any(|o| matches!(o.status, Status::Applied | Status::Pending))
    {
        let mut settings = Settings::load();
        settings.theme = Some(theme.id.clone());
        settings.enabled = enabled.to_vec();
        settings.options = options.clone();
        let _ = settings.save();
    }

    outcomes
}

/// Switches between the two themes the user keeps returning to.
///
/// This is the toggle the app is built around: one action, and everything
/// changes together. It flips to `dark` when the machine currently reads as
/// light and back again.
pub fn toggle(light: &str, dark: &str, options: &ApplyOptions, enabled: &[String]) -> Result<Vec<Outcome>> {
    let current = state().detected_theme;
    let next = match current.as_deref() {
        Some(id) if id == dark => light,
        Some(id) if id == light => dark,
        // Nothing recognisable is applied, so fall back to whichever theme
        // opposes the current mode.
        _ => {
            let on_light = theme::find(light).map(|t| t.mode) == Some(theme::Mode::Light);
            if on_light {
                dark
            } else {
                light
            }
        }
    };
    apply(next, options, enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applying_an_unknown_theme_is_an_error_not_a_panic() {
        let result = apply("no-such-theme", &ApplyOptions::default(), &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("no-such-theme"));
    }

    #[test]
    fn disabled_targets_are_skipped_without_touching_anything() {
        // No target is enabled, so nothing is written and every target reports
        // that it was switched off.
        let outcomes = apply_theme(
            &theme::find("catppuccin-latte").unwrap(),
            &ApplyOptions::default(),
            &[],
        );

        assert_eq!(outcomes.len(), targets::ALL.len());
        for outcome in &outcomes {
            assert_eq!(outcome.status, Status::Skipped, "{}", outcome.id);
            assert!(outcome.changed.is_empty());
        }
    }

    #[test]
    fn every_target_reports_back_even_when_disabled() {
        let outcomes = apply_theme(
            &theme::find("dracula").unwrap(),
            &ApplyOptions::default(),
            &[],
        );
        for id in targets::ALL {
            assert!(
                outcomes.iter().any(|o| o.id == id),
                "{id} should report an outcome"
            );
        }
    }

    #[test]
    fn state_describes_every_target() {
        let state = state();
        assert_eq!(state.targets.len(), targets::ALL.len());
        assert!(!state.themes.is_empty());
    }
}
