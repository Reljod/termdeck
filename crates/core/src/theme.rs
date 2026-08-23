//! What a theme is, and where the built-in ones come from.

use serde::{Deserialize, Serialize};

use crate::color::Rgb;

/// Whether a theme reads as light or dark.
///
/// This is the single fact that drives Claude Code, which has no palette of its
/// own to configure — it only picks a light or a dark scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Light,
    Dark,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
        }
    }
}

/// The sixteen ANSI slots, in the order every terminal numbers them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ansi {
    pub black: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
    pub white: Rgb,
    pub bright_black: Rgb,
    pub bright_red: Rgb,
    pub bright_green: Rgb,
    pub bright_yellow: Rgb,
    pub bright_blue: Rgb,
    pub bright_magenta: Rgb,
    pub bright_cyan: Rgb,
    pub bright_white: Rgb,
}

impl Ansi {
    /// The slots in numeric order, which is how iTerm2 keys them
    /// (`Ansi 0 Color` through `Ansi 15 Color`).
    pub fn indexed(&self) -> [Rgb; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.magenta,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_magenta,
            self.bright_cyan,
            self.bright_white,
        ]
    }
}

/// The colours a theme provides beyond the ANSI sixteen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub background: Rgb,
    pub foreground: Rgb,
    pub cursor: Rgb,
    pub cursor_text: Rgb,
    pub selection_background: Rgb,
    pub selection_foreground: Rgb,
    /// The theme's signature colour, used for the active tmux window and the
    /// zsh prompt character.
    pub accent: Rgb,
    /// A dimmed foreground, for inactive tmux windows and fzf hints.
    pub subtle: Rgb,
    /// Panel and border fill, one step away from the background.
    pub surface: Rgb,
    pub ansi: Ansi,
}

/// How this theme should drive tmux.
///
/// The catppuccin tmux plugin already knows its four flavours, so for those we
/// set the flavour and let the plugin draw the status line it was designed to
/// draw. Every other theme gets an explicit status line built from the palette.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum TmuxStyle {
    /// One of `latte`, `frappe`, `macchiato`, `mocha`.
    CatppuccinFlavour(String),
    /// Draw the status line ourselves from the palette.
    Palette,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    /// Stable identifier, used in config files and as the React key.
    pub id: String,
    pub name: String,
    /// Family name, so the UI can group the four catppuccin flavours together.
    #[serde(default)]
    pub family: String,
    pub mode: Mode,
    pub tmux: TmuxStyle,
    pub palette: Palette,
}

impl Theme {
    /// The nearest catppuccin flavour, used as the tmux plugin's base even for
    /// themes that then override the status line. The plugin has to be set to
    /// something, and a same-mode flavour keeps anything we do not override
    /// from clashing.
    pub fn nearest_catppuccin_flavour(&self) -> &'static str {
        match &self.tmux {
            TmuxStyle::CatppuccinFlavour(flavour) => match flavour.as_str() {
                "latte" => "latte",
                "frappe" => "frappe",
                "macchiato" => "macchiato",
                _ => "mocha",
            },
            TmuxStyle::Palette => match self.mode {
                Mode::Light => "latte",
                Mode::Dark => "mocha",
            },
        }
    }
}

/// The themes that ship with TermDeck.
const BUILT_IN: &str = include_str!("../themes.json");

/// Loads the built-in themes.
///
/// The list is embedded at compile time, so a broken `themes.json` is a build
/// failure rather than a runtime one — except for the parse, which is checked
/// by a test below.
pub fn built_in_themes() -> Vec<Theme> {
    serde_json::from_str(BUILT_IN).expect("themes.json is embedded and must parse")
}

/// Looks up a built-in theme by id.
pub fn find(id: &str) -> Option<Theme> {
    built_in_themes().into_iter().find(|theme| theme.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn built_in_themes_parse() {
        let themes = built_in_themes();
        assert!(themes.len() >= 10, "expected a useful starting set");
    }

    #[test]
    fn theme_ids_are_unique() {
        let themes = built_in_themes();
        let ids: HashSet<_> = themes.iter().map(|theme| theme.id.as_str()).collect();
        assert_eq!(ids.len(), themes.len(), "duplicate theme id");
    }

    #[test]
    fn the_users_current_theme_is_present() {
        // The whole setup is on catppuccin latte today, so toggling away from
        // it and back has to be possible.
        let latte = find("catppuccin-latte").expect("catppuccin latte should ship");
        assert_eq!(latte.mode, Mode::Light);
        assert_eq!(
            latte.tmux,
            TmuxStyle::CatppuccinFlavour("latte".to_string())
        );
        assert_eq!(latte.palette.background, Rgb::parse("#eff1f5").unwrap());
    }

    #[test]
    fn every_mode_matches_its_background_luminance() {
        for theme in built_in_themes() {
            let luminance = theme.palette.background.luminance();
            let expected = if luminance > 0.18 {
                Mode::Light
            } else {
                Mode::Dark
            };
            assert_eq!(
                theme.mode, expected,
                "{} claims {:?} but its background luminance is {luminance:.3}",
                theme.id, theme.mode
            );
        }
    }

    #[test]
    fn foreground_contrasts_with_background() {
        // A theme whose text is unreadable on its own background is a typo, and
        // it is cheaper to catch here than on screen.
        for theme in built_in_themes() {
            let bg = theme.palette.background.luminance();
            let fg = theme.palette.foreground.luminance();
            let (lighter, darker) = if bg > fg { (bg, fg) } else { (fg, bg) };
            let ratio = (lighter + 0.05) / (darker + 0.05);
            assert!(
                ratio >= 4.0,
                "{} has only {ratio:.1}:1 contrast between text and background",
                theme.id
            );
        }
    }

    #[test]
    fn catppuccin_flavours_name_a_flavour_the_plugin_knows() {
        let known = ["latte", "frappe", "macchiato", "mocha"];
        for theme in built_in_themes() {
            if let TmuxStyle::CatppuccinFlavour(flavour) = &theme.tmux {
                assert!(
                    known.contains(&flavour.as_str()),
                    "{} names flavour {flavour}, which the plugin does not have",
                    theme.id
                );
            }
            // Every theme must resolve to a usable plugin base either way.
            assert!(known.contains(&theme.nearest_catppuccin_flavour()));
        }
    }

    #[test]
    fn palette_themes_fall_back_to_a_same_mode_flavour() {
        let dracula = find("dracula").expect("dracula should ship");
        assert_eq!(dracula.tmux, TmuxStyle::Palette);
        assert_eq!(dracula.nearest_catppuccin_flavour(), "mocha");

        let dawn = find("rose-pine-dawn").expect("rose pine dawn should ship");
        assert_eq!(dawn.nearest_catppuccin_flavour(), "latte");
    }

    #[test]
    fn both_modes_are_available_to_toggle_between() {
        let themes = built_in_themes();
        assert!(themes.iter().any(|theme| theme.mode == Mode::Light));
        assert!(themes.iter().any(|theme| theme.mode == Mode::Dark));
    }

    #[test]
    fn ansi_slots_stay_in_numeric_order() {
        let theme = find("catppuccin-mocha").unwrap();
        let indexed = theme.palette.ansi.indexed();
        assert_eq!(indexed[0], theme.palette.ansi.black);
        assert_eq!(indexed[7], theme.palette.ansi.white);
        assert_eq!(indexed[8], theme.palette.ansi.bright_black);
        assert_eq!(indexed[15], theme.palette.ansi.bright_white);
    }
}
