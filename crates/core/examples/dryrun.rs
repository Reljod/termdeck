//! Shows what TermDeck would find and change, without writing anything.
//!
//! Run with `cargo run -p termdeck-core --example dryrun -- <theme-id>`.
//! Useful for checking the adapters against a real machine before letting them
//! near a real dotfile.

use termdeck_core::edit;
use termdeck_core::targets::{claude, iterm2, nvim, tmux, zsh};
use termdeck_core::theme;

fn main() -> anyhow::Result<()> {
    let wanted = std::env::args().nth(1).unwrap_or_else(|| "catppuccin-mocha".to_string());
    let Some(theme) = theme::find(&wanted) else {
        eprintln!("No theme called `{wanted}`. Available:");
        for candidate in theme::built_in_themes() {
            eprintln!("  {:<22} {}", candidate.id, candidate.name);
        }
        std::process::exit(1);
    };

    println!("== Detected ==");
    let state = termdeck_core::state();
    for target in &state.targets {
        println!(
            "{:<20} installed={:<5} {}",
            target.name, target.installed, target.detail
        );
        for path in &target.paths {
            println!("{:<20}   {path}", "");
        }
        if let Some(current) = &target.current_theme {
            println!("{:<20}   currently: {current}", "");
        }
    }
    println!("\ndetected theme: {:?}\n", state.detected_theme);

    println!("== Would apply: {} ({}) ==\n", theme.name, theme.id);

    // tmux: show only the managed blocks, not the whole rewritten file.
    let tmux_path = tmux::config_path();
    let tmux_before = edit::read_or_empty(&tmux_path)?;
    let tmux_after = tmux::rewrite(&tmux_before, &theme);
    println!("--- ~/.tmux.conf ---");
    println!("lines before: {}", tmux_before.lines().count());
    println!("lines after:  {}", tmux_after.lines().count());
    for block in ["tmux-flavour", "tmux-style"] {
        if let Some(body) = edit::read_block(&tmux_after, block) {
            println!("\n[{block}]\n{body}");
        }
    }

    // Confirm nothing the user wrote is dropped.
    let lost: Vec<&str> = tmux_before
        .lines()
        .filter(|line| !line.trim().is_empty() && !tmux_after.contains(*line))
        .collect();
    println!("\nlines that would be lost: {}", lost.len());
    for line in &lost {
        println!("  LOST: {line}");
    }

    println!("\n--- ~/.config/termdeck/zsh-theme.zsh ---");
    println!("{}", zsh::theme_file(&theme));

    println!("--- ~/.zshrc ---");
    let zshrc_before = edit::read_or_empty(&zsh::zshrc_path())?;
    let zshrc_after = zsh::rewrite_zshrc(&zshrc_before);
    println!(
        "lines before: {}, after: {}",
        zshrc_before.lines().count(),
        zshrc_after.lines().count()
    );
    if let Some(body) = edit::read_block(&zshrc_after, "zsh") {
        println!("[zsh]\n{body}");
    }

    println!("\n--- Neovim ---");
    let plugins = nvim::installed_plugins();
    let (colorscheme, exact) = nvim::resolve(&theme, &plugins);
    println!("colourscheme: {colorscheme}{}", if exact { "" } else { "  (substituted — the theme's own plugin is not installed)" });
    println!("open editors that would recolour now: {}", nvim::running_sockets().len());
    let init_before = edit::read_or_empty(&nvim::init_path())?;
    let init_after = nvim::rewrite_init(&init_before);
    println!(
        "init.lua lines before: {}, after: {}",
        init_before.lines().count(),
        init_after.lines().count()
    );
    if let Some(body) = edit::read_block(&init_after, "nvim") {
        println!("[nvim]\n{body}");
    }
    println!("\n{}", nvim::theme_file(&theme, &colorscheme));

    println!("--- Claude Code ---");
    for file in claude::settings_files() {
        let text = edit::read_or_empty(&file)?;
        let before = claude::read_theme(&text);
        let after = claude::theme_value(theme.mode, claude::Scheme::Standard);
        println!("{}: {:?} -> {after}", file.display(), before);
    }

    println!("\n--- iTerm2 ---");
    println!("prefs: {}", iterm2::prefs_path().display());
    println!("managed profile: {}", iterm2::dynamic_profile_path().display());
    println!(
        "managed profile is the default: {}",
        iterm2::managed_profile_is_default()
    );
    if std::env::args().any(|arg| arg == "--write-profile") {
        let root = plist::Value::from_file(iterm2::prefs_path())
            .ok()
            .and_then(|value| value.into_dictionary());
        println!(
            "copying settings from: {:?}",
            root.as_ref().and_then(iterm2::default_profile_name)
        );
        let base = root.as_ref().and_then(iterm2::default_profile);
        let json = serde_json::to_string_pretty(&iterm2::dynamic_profile(&theme, base.as_ref()))?;
        let path = iterm2::dynamic_profile_path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(&path, format!("{json}\n"))?;
        println!("wrote {}", path.display());
    }
    println!("preset that would be registered: {}", iterm2::preset_name(&theme));
    println!("colour keys written per profile: {}", iterm2::color_entries(&theme).len());

    Ok(())
}
