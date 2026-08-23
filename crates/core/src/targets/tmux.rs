//! tmux.
//!
//! Two managed blocks, because tmux config is order-dependent. The flavour has
//! to be set *before* the line that runs the plugin manager, since that is when
//! the catppuccin plugin reads it. Anything we want to win over the plugin has
//! to come *after* that line.

use std::path::PathBuf;

use anyhow::Result;

use crate::edit::{self, Placement};
use crate::which;
use crate::theme::{Theme, TmuxStyle};

use super::{Detection, Outcome, Target, TMUX};

/// The block that configures the plugin, placed before tpm runs.
const FLAVOUR_BLOCK: &str = "tmux-flavour";
/// The block that overrides the plugin, placed at the end of the file.
const STYLE_BLOCK: &str = "tmux-style";
/// Matched loosely so a moved or requoted tpm line still anchors correctly.
const TPM_ANCHOR: &str = "plugins/tpm/tpm";

pub struct Tmux;

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_default()
        .join(".tmux.conf")
}

/// Whether a tmux server is running, which decides if we can apply live.
fn server_running() -> bool {
    let Some(mut tmux) = which::command("tmux") else {
        return false;
    };
    tmux.arg("list-sessions")
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false)
}

fn tmux_installed() -> bool {
    which::exists("tmux")
}

/// The contents of the flavour block.
pub fn flavour_block(theme: &Theme) -> String {
    format!(
        "set -g @catppuccin_flavour '{}'\nset -g @termdeck_theme '{}'",
        theme.nearest_catppuccin_flavour(),
        theme.id
    )
}

/// The contents of the style block.
///
/// Empty for catppuccin themes: the plugin already draws a status line built
/// for that flavour, and overriding it would only make it worse.
pub fn style_block(theme: &Theme) -> String {
    if matches!(theme.tmux, TmuxStyle::CatppuccinFlavour(_)) {
        return String::from("# Catppuccin's own plugin draws this flavour's status line.");
    }

    let p = &theme.palette;
    let (bg, fg) = (p.background.hex(), p.foreground.hex());
    let (surface, accent) = (p.surface.hex(), p.accent.hex());
    let subtle = p.subtle.hex();

    [
        format!("set -g status-style \"bg={surface},fg={fg}\""),
        format!("set -g status-left-length 40"),
        format!(
            "set -g status-left \"#[bg={accent},fg={bg},bold] #S #[bg={surface},fg={surface}] \""
        ),
        format!("set -g status-right \"#[fg={subtle}] %Y-%m-%d  %H:%M \""),
        format!("set -g window-status-format \"#[bg={surface},fg={subtle}] #I:#W \""),
        format!(
            "set -g window-status-current-format \"#[bg={accent},fg={bg},bold] #I:#W \""
        ),
        format!("set -g pane-border-style \"fg={surface}\""),
        format!("set -g pane-active-border-style \"fg={accent}\""),
        format!("set -g message-style \"bg={accent},fg={bg}\""),
        format!("set -g message-command-style \"bg={accent},fg={bg}\""),
        format!(
            "set -g mode-style \"bg={},fg={}\"",
            p.selection_background.hex(),
            p.selection_foreground.hex()
        ),
        format!("set -g status-justify left"),
    ]
    .join("\n")
}

/// The whole rewrite, as a pure function so it can be tested without a home
/// directory or a running tmux.
pub fn rewrite(existing: &str, theme: &Theme) -> String {
    let with_flavour = edit::upsert_block(
        existing,
        FLAVOUR_BLOCK,
        &flavour_block(theme),
        &Placement::BeforeLineContaining(TPM_ANCHOR.to_string()),
        edit::HASH,
    );
    edit::upsert_block(
        &with_flavour,
        STYLE_BLOCK,
        &style_block(theme),
        &Placement::End,
        edit::HASH,
    )
}

/// Reads back the theme id we recorded in the flavour block.
pub fn current_theme(existing: &str) -> Option<String> {
    let block = edit::read_block(existing, FLAVOUR_BLOCK)?;
    let line = block
        .lines()
        .find(|line| line.contains("@termdeck_theme"))?;
    let id = line.split('\'').nth(1)?;
    Some(id.to_string())
}

impl Target for Tmux {
    fn id(&self) -> &'static str {
        TMUX
    }

    fn name(&self) -> &'static str {
        "tmux"
    }

    fn detect(&self) -> Detection {
        let path = config_path();
        let installed = tmux_installed();
        let existing = edit::read_or_empty(&path).unwrap_or_default();

        let detail = if !installed {
            "tmux is not installed".to_string()
        } else if !path.exists() {
            "no ~/.tmux.conf yet — TermDeck will create one".to_string()
        } else if server_running() {
            "running, so changes apply to open sessions".to_string()
        } else {
            "configured; no server running right now".to_string()
        };

        Detection {
            id: TMUX.to_string(),
            name: "tmux".to_string(),
            installed,
            paths: vec![super::display_path(&path)],
            detail,
            current_theme: current_theme(&existing),
        }
    }

    fn apply(&self, theme: &Theme, options: &super::ApplyOptions) -> Result<Outcome> {
        if !tmux_installed() {
            return Ok(Outcome::skipped(TMUX, "tmux", "tmux is not installed"));
        }

        let path = config_path();
        let existing = edit::read_or_empty(&path)?;
        let updated = rewrite(&existing, theme);

        if updated != existing {
            edit::write_with_backup(&path, &updated)?;
        }

        let changed = vec![path.clone()];

        if !options.live || !server_running() {
            return Ok(Outcome::pending(
                TMUX,
                "tmux",
                format!("~/.tmux.conf now sets {}", theme.name),
                changed,
                "Starts themed with your next tmux session.",
            ));
        }

        let Some(mut tmux) = which::command("tmux") else {
            return Ok(Outcome::skipped(TMUX, "tmux", "tmux is not installed"));
        };
        let sourced = tmux.arg("source-file").arg(&path).output()?;

        if sourced.status.success() {
            Ok(Outcome::applied(
                TMUX,
                "tmux",
                "Reloaded open sessions.",
                changed,
            ))
        } else {
            let stderr = String::from_utf8_lossy(&sourced.stderr).trim().to_string();
            Ok(Outcome::pending(
                TMUX,
                "tmux",
                format!("Config written, but reloading it failed: {stderr}"),
                changed,
                "Run `tmux source-file ~/.tmux.conf` by hand to see what it objects to.",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::find;

    /// A trimmed copy of the shape the real config has: settings, then the tpm
    /// line, then more settings after it.
    const SAMPLE: &str = "\
set -g mouse on
set -g @plugin \"catppuccin/tmux\"
set -g @catppuccin_flavour 'latte'
run '~/.tmux/plugins/tpm/tpm'

bind-key -n 'C-h' select-pane -L
";

    #[test]
    fn flavour_is_set_before_tpm_runs() {
        let mocha = find("catppuccin-mocha").unwrap();
        let out = rewrite(SAMPLE, &mocha);

        let block = out.find("# >>> termdeck:tmux-flavour").unwrap();
        let tpm = out.find("run '~/.tmux/plugins/tpm/tpm'").unwrap();
        assert!(block < tpm, "the plugin reads the flavour when tpm runs");
    }

    #[test]
    fn style_overrides_land_after_tpm() {
        let dracula = find("dracula").unwrap();
        let out = rewrite(SAMPLE, &dracula);

        let tpm = out.find("run '~/.tmux/plugins/tpm/tpm'").unwrap();
        let style = out.find("# >>> termdeck:tmux-style").unwrap();
        assert!(style > tpm, "overrides only win if they come after tpm");
    }

    #[test]
    fn the_users_own_settings_survive() {
        let mocha = find("catppuccin-mocha").unwrap();
        let out = rewrite(SAMPLE, &mocha);
        assert!(out.contains("set -g mouse on"));
        assert!(out.contains("bind-key -n 'C-h' select-pane -L"));
        assert!(out.contains("set -g @plugin \"catppuccin/tmux\""));
    }

    #[test]
    fn catppuccin_themes_name_their_own_flavour() {
        for (id, flavour) in [
            ("catppuccin-latte", "latte"),
            ("catppuccin-frappe", "frappe"),
            ("catppuccin-macchiato", "macchiato"),
            ("catppuccin-mocha", "mocha"),
        ] {
            let theme = find(id).unwrap();
            let block = flavour_block(&theme);
            assert!(
                block.contains(&format!("@catppuccin_flavour '{flavour}'")),
                "{id} should select {flavour}"
            );
        }
    }

    #[test]
    fn catppuccin_themes_leave_the_status_line_to_the_plugin() {
        let latte = find("catppuccin-latte").unwrap();
        let style = style_block(&latte);
        assert!(!style.contains("set -g status-style"));
    }

    #[test]
    fn other_themes_style_the_status_line_themselves() {
        let dracula = find("dracula").unwrap();
        let style = style_block(&dracula);
        assert!(style.contains("set -g status-style"));
        assert!(style.contains("#bd93f9"), "should use the dracula accent");
        assert!(style.contains("set -g pane-active-border-style"));
    }

    #[test]
    fn non_catppuccin_themes_still_pin_a_same_mode_flavour() {
        // Whatever the plugin draws underneath our overrides should at least
        // not be a light status bar behind a dark theme.
        let dracula = find("dracula").unwrap();
        assert!(flavour_block(&dracula).contains("'mocha'"));

        let dawn = find("rose-pine-dawn").unwrap();
        assert!(flavour_block(&dawn).contains("'latte'"));
    }

    #[test]
    fn switching_themes_replaces_rather_than_stacks() {
        let latte = find("catppuccin-latte").unwrap();
        let mocha = find("catppuccin-mocha").unwrap();

        let once = rewrite(SAMPLE, &latte);
        let twice = rewrite(&once, &mocha);
        let thrice = rewrite(&twice, &latte);

        assert_eq!(twice.matches("# >>> termdeck:tmux-flavour").count(), 1);
        assert_eq!(twice.matches("# >>> termdeck:tmux-style").count(), 1);
        assert!(thrice.contains("@catppuccin_flavour 'latte'"));
        // Back where we started, byte for byte.
        assert_eq!(once, thrice);
    }

    #[test]
    fn the_applied_theme_can_be_read_back() {
        let mocha = find("catppuccin-mocha").unwrap();
        let out = rewrite(SAMPLE, &mocha);
        assert_eq!(current_theme(&out).as_deref(), Some("catppuccin-mocha"));
    }

    #[test]
    fn an_untouched_config_reports_no_theme() {
        assert_eq!(current_theme(SAMPLE), None);
    }

    #[test]
    fn a_config_without_tpm_still_gets_both_blocks() {
        let bare = "set -g mouse on\n";
        let out = rewrite(bare, &find("dracula").unwrap());
        assert!(out.contains("# >>> termdeck:tmux-flavour"));
        assert!(out.contains("# >>> termdeck:tmux-style"));
        assert!(out.contains("set -g mouse on"));
    }

    #[test]
    fn an_empty_config_is_handled() {
        let out = rewrite("", &find("catppuccin-mocha").unwrap());
        assert!(out.contains("@catppuccin_flavour 'mocha'"));
    }
}
