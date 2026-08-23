//! The five things TermDeck themes, and the shape they all share.

pub mod claude;
pub mod iterm2;
pub mod nvim;
pub mod tmux;
pub mod zsh;

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::theme::Theme;

/// The stable ids used in settings and in the UI.
pub const ITERM2: &str = "iterm2";
pub const TMUX: &str = "tmux";
pub const ZSH: &str = "zsh";
pub const NVIM: &str = "nvim";
pub const CLAUDE: &str = "claude";

/// Every target in the order the UI shows them.
pub const ALL: [&str; 5] = [ITERM2, TMUX, ZSH, NVIM, CLAUDE];

/// What we found on this machine for one target.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub id: String,
    pub name: String,
    /// Whether the thing is installed at all.
    pub installed: bool,
    /// The files this target would write to.
    pub paths: Vec<String>,
    /// Human-readable state, shown under the target's toggle.
    pub detail: String,
    /// The theme id TermDeck last applied here, when it can be worked out.
    pub current_theme: Option<String>,
}

/// How a target should be applied.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApplyOptions {
    /// Also push the change into already-running sessions, not just the
    /// config files that new sessions read.
    pub live: bool,
    /// Make Claude Code use the terminal's own ANSI palette rather than its
    /// built-in light and dark schemes.
    pub claude_use_ansi: bool,
    /// Write the palette into every iTerm2 profile rather than only the
    /// default one.
    pub iterm_all_profiles: bool,
}

impl Default for ApplyOptions {
    fn default() -> Self {
        Self {
            live: true,
            claude_use_ansi: false,
            iterm_all_profiles: true,
        }
    }
}

/// How one target's apply went.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Outcome {
    pub id: String,
    pub name: String,
    pub status: Status,
    /// One sentence describing what happened, shown next to the target.
    pub message: String,
    /// Files that were changed, so the user can see what we touched.
    pub changed: Vec<String>,
    /// Set when the change needs something from the user before it shows up,
    /// such as opening a new shell.
    pub follow_up: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    /// Applied, and visible now.
    Applied,
    /// Written, but something has to happen before it shows up.
    Pending,
    /// Deliberately not touched — target absent or switched off.
    Skipped,
    /// Tried and failed.
    Failed,
}

impl Outcome {
    pub fn applied(id: &str, name: &str, message: impl Into<String>, changed: Vec<PathBuf>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: Status::Applied,
            message: message.into(),
            changed: changed.iter().map(display_path).collect(),
            follow_up: None,
        }
    }

    pub fn pending(
        id: &str,
        name: &str,
        message: impl Into<String>,
        changed: Vec<PathBuf>,
        follow_up: impl Into<String>,
    ) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: Status::Pending,
            message: message.into(),
            changed: changed.iter().map(display_path).collect(),
            follow_up: Some(follow_up.into()),
        }
    }

    pub fn skipped(id: &str, name: &str, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: Status::Skipped,
            message: message.into(),
            changed: Vec::new(),
            follow_up: None,
        }
    }

    pub fn failed(id: &str, name: &str, message: impl Into<String>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            status: Status::Failed,
            message: message.into(),
            changed: Vec::new(),
            follow_up: None,
        }
    }
}

/// Shortens a path for display, so the UI shows `~/.tmux.conf` rather than the
/// full home directory every time.
pub fn display_path(path: &PathBuf) -> String {
    let text = path.to_string_lossy().to_string();
    match dirs::home_dir() {
        Some(home) => {
            let home = home.to_string_lossy().to_string();
            match text.strip_prefix(&home) {
                Some(rest) => format!("~{rest}"),
                None => text,
            }
        }
        None => text,
    }
}

/// One thing TermDeck can theme.
pub trait Target {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;

    /// What exists on this machine right now.
    fn detect(&self) -> Detection;

    /// Point this target at `theme`.
    ///
    /// Returning `Err` is reserved for genuine failures. A target that is not
    /// installed reports `Status::Skipped` through `Ok`, because "you do not
    /// have tmux" is an answer rather than a fault.
    fn apply(&self, theme: &Theme, options: &ApplyOptions) -> anyhow::Result<Outcome>;
}

/// Every target, ready to use.
pub fn all_targets() -> Vec<Box<dyn Target>> {
    vec![
        Box::new(iterm2::Iterm2),
        Box::new(tmux::Tmux),
        Box::new(zsh::Zsh),
        Box::new(nvim::Neovim),
        Box::new(claude::ClaudeCode),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_id_has_exactly_one_target() {
        let targets = all_targets();
        assert_eq!(targets.len(), ALL.len());
        for id in ALL {
            assert_eq!(
                targets.iter().filter(|t| t.id() == id).count(),
                1,
                "{id} should map to one target"
            );
        }
    }

    #[test]
    fn targets_report_a_name_for_the_ui() {
        for target in all_targets() {
            assert!(!target.name().is_empty());
        }
    }

    #[test]
    fn defaults_apply_live_to_running_sessions() {
        let options = ApplyOptions::default();
        assert!(options.live, "a toggle that needs a restart is not a toggle");
    }

    #[test]
    fn outcomes_shorten_home_paths() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let outcome = Outcome::applied("x", "X", "done", vec![home.join(".tmux.conf")]);
        assert_eq!(outcome.changed, vec!["~/.tmux.conf".to_string()]);
    }

    #[test]
    fn a_pending_outcome_always_says_what_to_do_next() {
        let outcome = Outcome::pending("x", "X", "written", vec![], "open a new shell");
        assert_eq!(outcome.status, Status::Pending);
        assert!(outcome.follow_up.is_some());
    }
}
