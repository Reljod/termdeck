# TermDeck

One toggle that puts iTerm2, tmux, zsh and Claude Code on the same theme.

Changing terminal themes by hand means editing four different things in four
different formats, and forgetting one of them is what makes the result look
wrong. TermDeck does all four together and tells you honestly which ones took
effect immediately and which need a nudge.

## What it changes

| Target | File | How it applies |
| --- | --- | --- |
| **iTerm2** | the preferences plist it actually loads, including a custom prefs folder | AppleScript recolours every open session immediately; the palette is written to the plist for new windows |
| **tmux** | `~/.tmux.conf` | the catppuccin plugin's flavour, or an explicit status line for other themes, then `tmux source-file` |
| **zsh** | `~/.config/termdeck/zsh-theme.zsh`, plus one line in `~/.zshrc` | powerlevel10k prompt colours and the fzf palette |
| **Claude Code** | `settings.json` in every `~/.claude*` profile | the theme's light or dark mode |

Claude Code has no palette to configure, so a theme only decides whether it runs
light or dark. Tick **"Make Claude Code use the terminal's own palette"** and it
switches to `light-ansi` / `dark-ansi` instead, which draws with the same
sixteen ANSI colours TermDeck just set — so it matches exactly rather than
approximately.

## Themes

Catppuccin (Latte, Frappé, Macchiato, Mocha), Dracula, Tokyo Night, Nord,
Gruvbox light and dark, Solarized light and dark, and Rosé Pine Dawn. Each one
carries a full sixteen-slot ANSI palette rather than just a background colour.

The toggle switches between one light and one dark theme, both of which you
pick. It defaults to Latte and Mocha.

## Running it

```bash
pnpm install
pnpm app          # development
pnpm app:build    # produces TermDeck.app and a .dmg in target/release/bundle
```

To see what it would do without letting it near a real dotfile:

```bash
cargo run -p termdeck-core --example dryrun -- catppuccin-mocha
```

## How it edits your files

Two rules hold everywhere, and the tests are mostly about enforcing them.

**Every file is backed up before the first write.** The first copy of anything
is kept forever as `~/.config/termdeck/backups/<file>.original`, so there is
always a way back to what existed before TermDeck ran at all. Later copies are
dated, and the ten most recent are kept.

**TermDeck only owns the text between its own markers.** Changes go into blocks
like this, and everything outside them is left exactly as it was:

```
# >>> termdeck:tmux-flavour >>>  managed by TermDeck — edits here are overwritten
set -g @catppuccin_flavour 'mocha'
# <<< termdeck:tmux-flavour <<<
```

Re-applying rewrites the block rather than appending a second one, so switching
themes fifty times leaves the file the same length it was after the first.
Toggling away and back is byte-for-byte lossless, which
`tests/end_to_end.rs` checks by doing exactly that.

Placement matters and is not incidental. The tmux flavour is written *before*
the line that runs the plugin manager, because that is when the catppuccin
plugin reads it. Themes with no catppuccin flavour get a second block *after*
that line, since anything meant to override a plugin has to come after it. The
zsh block goes at the end of `.zshrc`, after `~/.p10k.zsh` is sourced, because
the last assignment to a `POWERLEVEL9K_*` variable is the one the prompt uses.

## What "applied" and "pending" mean

Some of this is visible instantly and some genuinely is not, so the app
distinguishes them rather than saying "done" to everything:

- **iTerm2** recolours open sessions immediately. But iTerm2 writes its
  in-memory preferences over the file when it quits, which can undo what was
  written for *new* windows. To make that stick, either quit iTerm2 before
  applying, or set Settings → General → Preferences → Save changes to
  "Manually". The app says so when iTerm2 is running.
- **tmux** reloads open sessions right away.
- **zsh** cannot be reached from outside a running shell. Open a new one, or run
  `source ~/.config/termdeck/zsh-theme.zsh`.
- **Claude Code** reads its theme at startup, so restart a running session.

## Layout

```
crates/core/     the theme model and the four adapters — no Tauri, fully tested
src-tauri/       a thin Tauri command layer over the above
src/             the React interface
```

The logic lives in `crates/core` precisely so it can be tested without a window.
`cargo test --workspace` runs 111 tests, including one that applies themes to a
throwaway home directory and one that round-trips a real iTerm2 preferences
file.

## Notes

powerlevel10k takes 256-colour palette indices rather than truecolor hex, so
prompt colours are mapped to the nearest palette entry on the way out. fzf gets
exact hex. Indices 0-15 are excluded from that mapping on purpose: those are the
ANSI slots TermDeck is also rewriting, so matching against them would make the
prompt colour depend on the palette it sits on.
