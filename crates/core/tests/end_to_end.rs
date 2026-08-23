//! Applies themes for real, against a throwaway home directory.
//!
//! The unit tests cover the text transformations. This covers the part they
//! cannot: that `apply` actually writes the files, that a second apply does not
//! corrupt the first, and that toggling back and forth leaves the dotfiles
//! exactly where they started.
//!
//! Everything runs inside one test because it changes `HOME` for the whole
//! process, and a second test running in parallel would see the wrong one.
//! iTerm2 is deliberately left out: it finds its preferences through macOS's
//! preference daemon rather than through `HOME`, so a sandboxed run would still
//! write to the real file. It is covered by `iterm_prefs.rs`, which is a
//! separate binary and so cannot be caught by the `HOME` change here.

use std::fs;
use std::path::Path;

use termdeck_core::apply;
use termdeck_core::targets::{ApplyOptions, Status};

/// A `.tmux.conf` with the same shape as a real one: settings, the tpm line,
/// then more settings after it.
const TMUX_CONF: &str = "\
set -g mouse on
unbind C-b
set -g prefix C-a

set -g @plugin \"tmux-plugins/tpm\"
set -g @plugin \"catppuccin/tmux\"
set -g @catppuccin_flavour 'latte'
run '~/.tmux/plugins/tpm/tpm'

bind-key -n 'C-h' select-pane -L
bind-key -n 'C-l' select-pane -R
";

const ZSHRC: &str = "\
ZSH_THEME=\"powerlevel10k/powerlevel10k\"
export FZF_DEFAULT_OPTS=\"--color=fg:#4c4f69\"
[[ ! -f ~/.p10k.zsh ]] || source ~/.p10k.zsh
";

const CLAUDE_SETTINGS: &str = r#"{
  "includeCoAuthoredBy": false,
  "model": "opus",
  "theme": "light",
  "permissions": {
    "allow": ["Bash(pnpm build)"]
  }
}
"#;

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

#[test]
fn applying_and_toggling_leaves_a_real_home_intact() {
    let home = tempfile::tempdir().expect("temp home");
    let root = home.path().to_path_buf();

    fs::write(root.join(".tmux.conf"), TMUX_CONF).unwrap();
    fs::write(root.join(".zshrc"), ZSHRC).unwrap();
    fs::create_dir_all(root.join(".claude")).unwrap();
    fs::write(root.join(".claude/settings.json"), CLAUDE_SETTINGS).unwrap();
    fs::create_dir_all(root.join(".claude-work")).unwrap();
    fs::write(root.join(".claude-work/settings.json"), r#"{"theme": "auto"}"#).unwrap();

    // SAFETY: single-threaded within this test binary, and every path the
    // adapters touch is derived from HOME.
    unsafe {
        std::env::set_var("HOME", &root);
    }

    let enabled: Vec<String> = vec!["tmux".into(), "zsh".into(), "claude".into()];
    let options = ApplyOptions {
        // Do not try to reach real running sessions from a test.
        live: false,
        ..ApplyOptions::default()
    };

    let tmux_conf = root.join(".tmux.conf");
    let zshrc = root.join(".zshrc");
    let claude = root.join(".claude/settings.json");
    let claude_work = root.join(".claude-work/settings.json");
    let generated = root.join(".config/termdeck/zsh-theme.zsh");

    // --- Apply a dark theme ------------------------------------------------
    let outcomes = apply("catppuccin-mocha", &options, &enabled).expect("apply mocha");

    for outcome in &outcomes {
        assert_ne!(
            outcome.status,
            Status::Failed,
            "{} failed: {}",
            outcome.id,
            outcome.message
        );
    }
    // iTerm2 was not enabled, so it must report as skipped rather than run.
    let iterm = outcomes.iter().find(|o| o.id == "iterm2").unwrap();
    assert_eq!(iterm.status, Status::Skipped);

    let tmux_after = read(&tmux_conf);
    assert!(tmux_after.contains("@catppuccin_flavour 'mocha'"));
    assert!(tmux_after.contains("set -g mouse on"), "user config survived");
    assert!(tmux_after.contains("bind-key -n 'C-h' select-pane -L"));
    // The flavour must be set before the plugin manager reads it.
    assert!(
        tmux_after.find("termdeck:tmux-flavour").unwrap()
            < tmux_after.find("run '~/.tmux/plugins/tpm/tpm'").unwrap()
    );

    assert!(generated.exists(), "zsh theme file written");
    assert!(read(&generated).contains("TERMDECK_THEME='catppuccin-mocha'"));
    assert!(read(&zshrc).contains("source ~/.config/termdeck/zsh-theme.zsh"));
    assert!(read(&zshrc).contains("ZSH_THEME=\"powerlevel10k/powerlevel10k\""));

    let claude_json: serde_json::Value = serde_json::from_str(&read(&claude)).unwrap();
    assert_eq!(claude_json["theme"], "dark");
    assert_eq!(claude_json["model"], "opus", "other settings survived");
    assert_eq!(claude_json["permissions"]["allow"][0], "Bash(pnpm build)");

    // Both profiles are lined up, which is the point of doing all of them.
    let work_json: serde_json::Value = serde_json::from_str(&read(&claude_work)).unwrap();
    assert_eq!(work_json["theme"], "dark");

    // --- The originals were backed up before the first write ---------------
    let backups = root.join(".config/termdeck/backups");
    let originals: Vec<_> = fs::read_dir(&backups)
        .expect("backup directory")
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".original"))
        .collect();
    assert!(
        originals.len() >= 3,
        "expected an original for each file we changed, found {}",
        originals.len()
    );
    let tmux_original = originals
        .iter()
        .find(|entry| entry.file_name().to_string_lossy().contains("tmux.conf"))
        .expect("tmux backup");
    assert_eq!(
        read(&tmux_original.path()),
        TMUX_CONF,
        "the original tmux.conf is recoverable byte for byte"
    );

    // --- Applying the same theme twice changes nothing ---------------------
    let tmux_once = read(&tmux_conf);
    let zshrc_once = read(&zshrc);
    apply("catppuccin-mocha", &options, &enabled).expect("apply mocha again");
    assert_eq!(read(&tmux_conf), tmux_once, "tmux.conf drifted on re-apply");
    assert_eq!(read(&zshrc), zshrc_once, ".zshrc drifted on re-apply");

    // --- Switch to a theme that needs an explicit tmux status line ---------
    apply("dracula", &options, &enabled).expect("apply dracula");
    let dracula_tmux = read(&tmux_conf);
    assert!(dracula_tmux.contains("set -g status-style"));
    assert!(dracula_tmux.contains("#bd93f9"));
    // Only one of each block, no matter how many times we switch.
    assert_eq!(dracula_tmux.matches("termdeck:tmux-flavour >>>").count(), 1);
    assert_eq!(dracula_tmux.matches("termdeck:tmux-style >>>").count(), 1);

    // --- Back to a light theme --------------------------------------------
    apply("catppuccin-latte", &options, &enabled).expect("apply latte");
    let latte_tmux = read(&tmux_conf);
    assert!(latte_tmux.contains("@catppuccin_flavour 'latte'"));
    assert!(
        !latte_tmux.contains("set -g status-style"),
        "dracula's overrides should be gone, not left behind"
    );

    let claude_json: serde_json::Value = serde_json::from_str(&read(&claude)).unwrap();
    assert_eq!(claude_json["theme"], "light", "Claude followed back to light");

    // --- A full round trip returns the files to their first-applied state --
    apply("catppuccin-mocha", &options, &enabled).expect("round trip");
    assert_eq!(
        read(&tmux_conf),
        tmux_once,
        "toggling away and back should be lossless"
    );
    assert_eq!(read(&zshrc), zshrc_once);

    // --- The user's own settings are still there after all of that ---------
    let final_tmux = read(&tmux_conf);
    for line in TMUX_CONF.lines().filter(|l| {
        // The one line we deliberately take over is the flavour.
        !l.trim().is_empty() && !l.contains("@catppuccin_flavour")
    }) {
        assert!(final_tmux.contains(line), "lost `{line}` from tmux.conf");
    }
    for line in ZSHRC.lines().filter(|l| !l.trim().is_empty()) {
        assert!(read(&zshrc).contains(line), "lost `{line}` from .zshrc");
    }
}
