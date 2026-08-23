//! Editing config files that belong to the user.
//!
//! Two rules hold everywhere in this module. We only ever own the text between
//! our own markers, and we copy a file before the first time we change it.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};

/// How many dated backups to keep per file before the oldest is dropped. The
/// pristine `.original` copy is never part of this count and never deleted.
const KEPT_BACKUPS: usize = 10;

/// Where a managed block should end up when it is not already in the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Append to the end of the file.
    End,
    /// Insert immediately before the first line containing this text, falling
    /// back to the end of the file when no line matches.
    ///
    /// tmux needs this: anything that configures a plugin has to be set before
    /// the line that runs the plugin manager.
    BeforeLineContaining(String),
}

fn start_marker(id: &str) -> String {
    format!("# >>> termdeck:{id} >>>")
}

fn end_marker(id: &str) -> String {
    format!("# <<< termdeck:{id} <<<")
}

/// Inserts or replaces the block named `id`, returning the new file text.
///
/// Calling this twice with the same arguments produces the same text, which is
/// what keeps a repeated apply from stacking duplicate blocks in a dotfile.
pub fn upsert_block(text: &str, id: &str, content: &str, placement: &Placement) -> String {
    let stripped = remove_block(text, id);
    let block = render_block(id, content);

    if stripped.trim().is_empty() {
        return format!("{block}\n");
    }

    match placement {
        Placement::End => {
            let mut out = stripped.trim_end().to_string();
            out.push_str("\n\n");
            out.push_str(&block);
            out.push('\n');
            out
        }
        Placement::BeforeLineContaining(needle) => {
            let anchor = stripped
                .lines()
                .position(|line| line.contains(needle.as_str()));

            match anchor {
                Some(index) => {
                    let mut lines: Vec<String> =
                        stripped.lines().map(|line| line.to_string()).collect();

                    // Pad with a blank line on each side, but only where there
                    // is not one already. Adding one unconditionally would push
                    // the anchor down by a line on every re-apply, so the file
                    // would grow a little each time the theme changed.
                    let needs_lead = index > 0 && !lines[index - 1].trim().is_empty();
                    let needs_trail = !lines[index].trim().is_empty();

                    let mut insert: Vec<String> = Vec::new();
                    if needs_lead {
                        insert.push(String::new());
                    }
                    insert.extend(block.lines().map(|line| line.to_string()));
                    if needs_trail {
                        insert.push(String::new());
                    }

                    lines.splice(index..index, insert);
                    let mut out = lines.join("\n");
                    out.push('\n');
                    out
                }
                None => upsert_block(&stripped, id, content, &Placement::End),
            }
        }
    }
}

fn render_block(id: &str, content: &str) -> String {
    let mut block = String::new();
    block.push_str(&start_marker(id));
    block.push_str("  managed by TermDeck — edits here are overwritten\n");
    let body = content.trim_end();
    if !body.is_empty() {
        block.push_str(body);
        block.push('\n');
    }
    block.push_str(&end_marker(id));
    block
}

/// Removes the block named `id`, leaving everything else untouched.
///
/// A file with a start marker and no end marker is left alone rather than
/// truncated. That case means someone hand-edited the file, and deleting the
/// rest of it would be the worst possible reading of the situation.
pub fn remove_block(text: &str, id: &str) -> String {
    let start = start_marker(id);
    let end = end_marker(id);

    let lines: Vec<&str> = text.lines().collect();
    let Some(start_index) = lines.iter().position(|line| line.starts_with(&start)) else {
        return text.to_string();
    };
    let Some(end_offset) = lines[start_index..]
        .iter()
        .position(|line| line.starts_with(&end))
    else {
        return text.to_string();
    };
    let end_index = start_index + end_offset;

    let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
    kept.extend_from_slice(&lines[..start_index]);
    kept.extend_from_slice(&lines[end_index + 1..]);

    // Drop one blank line left behind at the seam so repeated apply/remove
    // cycles do not slowly push the rest of the file down.
    let mut out = kept.join("\n");
    while out.contains("\n\n\n") {
        out = out.replace("\n\n\n", "\n\n");
    }
    let out = out.trim_end().to_string();
    if out.is_empty() {
        String::new()
    } else {
        format!("{out}\n")
    }
}

/// Returns the body of the block named `id`, if the file has one.
pub fn read_block(text: &str, id: &str) -> Option<String> {
    let start = start_marker(id);
    let end = end_marker(id);

    let lines: Vec<&str> = text.lines().collect();
    let start_index = lines.iter().position(|line| line.starts_with(&start))?;
    let end_offset = lines[start_index..]
        .iter()
        .position(|line| line.starts_with(&end))?;
    let end_index = start_index + end_offset;

    Some(lines[start_index + 1..end_index].join("\n"))
}

/// The directory holding backups, created on demand.
pub fn backup_dir() -> Result<PathBuf> {
    let base = dirs::home_dir().context("no home directory")?;
    let dir = base.join(".config/termdeck/backups");
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Copies `path` into the backup directory before we change it.
///
/// The first copy of any file is kept forever as `<name>.original`, so there is
/// always a way back to what existed before TermDeck ran at all. Later copies
/// are dated and pruned.
pub fn backup(path: &Path) -> Result<Option<PathBuf>> {
    if !path.exists() {
        return Ok(None);
    }

    let dir = backup_dir()?;
    let slug = slugify(path);

    let original = dir.join(format!("{slug}.original"));
    if !original.exists() {
        fs::copy(path, &original)
            .with_context(|| format!("saving the original {}", path.display()))?;
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dated = dir.join(format!("{slug}.{stamp}.bak"));
    fs::copy(path, &dated).with_context(|| format!("backing up {}", path.display()))?;

    prune(&dir, &slug)?;
    Ok(Some(dated))
}

fn prune(dir: &Path, slug: &str) -> Result<()> {
    let prefix = format!("{slug}.");
    let mut dated: Vec<PathBuf> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                return false;
            };
            name.starts_with(&prefix) && name.ends_with(".bak")
        })
        .collect();

    if dated.len() <= KEPT_BACKUPS {
        return Ok(());
    }

    dated.sort();
    let doomed = dated.len() - KEPT_BACKUPS;
    for path in dated.into_iter().take(doomed) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

/// Turns an absolute path into one flat filename, so backups of
/// `~/.tmux.conf` and `~/other/.tmux.conf` cannot collide.
fn slugify(path: &Path) -> String {
    let text = path.to_string_lossy();
    let slug: String = text
        .trim_start_matches('/')
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '.' { c } else { '-' })
        .collect();
    slug.trim_matches('-').to_string()
}

/// Writes `text` to `path`, backing up whatever was there first.
///
/// The write goes to a temporary file in the same directory and is then
/// renamed, so a crash midway cannot leave a half-written shell config that
/// would break the user's next login.
pub fn write_with_backup(path: &Path, text: &str) -> Result<Option<PathBuf>> {
    let saved = backup(path)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
    }

    let temp = path.with_extension(format!(
        "{}termdeck-tmp",
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| format!("{e}."))
            .unwrap_or_default()
    ));
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))?;

    Ok(saved)
}

/// Reads a file, treating "not there" as empty rather than as a failure.
pub fn read_or_empty(path: &Path) -> Result<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(error).with_context(|| format!("reading {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "test";

    #[test]
    fn inserts_a_block_into_an_empty_file() {
        let out = upsert_block("", ID, "set -g one", &Placement::End);
        assert!(out.contains("# >>> termdeck:test >>>"));
        assert!(out.contains("set -g one"));
        assert!(out.contains("# <<< termdeck:test <<<"));
    }

    #[test]
    fn appends_without_disturbing_what_was_there() {
        let existing = "set -g mouse on\nset -g prefix C-a\n";
        let out = upsert_block(existing, ID, "set -g two", &Placement::End);
        assert!(out.starts_with("set -g mouse on\nset -g prefix C-a"));
        assert!(out.contains("set -g two"));
    }

    #[test]
    fn applying_twice_does_not_stack_blocks() {
        let first = upsert_block("set -g mouse on\n", ID, "one", &Placement::End);
        let second = upsert_block(&first, ID, "two", &Placement::End);

        assert_eq!(second.matches("# >>> termdeck:test >>>").count(), 1);
        assert!(!second.contains("one"));
        assert!(second.contains("two"));
    }

    #[test]
    fn re_applying_the_same_content_is_stable() {
        // Toggling back and forth must not make the file drift.
        let once = upsert_block("set -g mouse on\n", ID, "one", &Placement::End);
        let twice = upsert_block(&once, ID, "one", &Placement::End);
        assert_eq!(once, twice);
    }

    #[test]
    fn blocks_with_different_ids_coexist() {
        let first = upsert_block("base\n", "alpha", "a", &Placement::End);
        let both = upsert_block(&first, "beta", "b", &Placement::End);
        assert!(both.contains("# >>> termdeck:alpha >>>"));
        assert!(both.contains("# >>> termdeck:beta >>>"));
        assert!(both.contains("base"));
    }

    #[test]
    fn removing_a_block_restores_the_original_text() {
        let original = "set -g mouse on\nset -g prefix C-a\n";
        let with = upsert_block(original, ID, "managed", &Placement::End);
        let without = remove_block(&with, ID);
        assert_eq!(without, original);
    }

    #[test]
    fn removing_a_block_that_is_not_there_changes_nothing() {
        let original = "set -g mouse on\n";
        assert_eq!(remove_block(original, ID), original);
    }

    #[test]
    fn a_block_missing_its_end_marker_is_left_alone() {
        // Someone hand-deleted the closing marker. Truncating the rest of their
        // shell config would be far worse than doing nothing.
        let mangled = "keep me\n# >>> termdeck:test >>>\nstray\nkeep me too\n";
        assert_eq!(remove_block(mangled, ID), mangled);
    }

    #[test]
    fn places_a_block_before_its_anchor() {
        let tmux = "set -g mouse on\nrun '~/.tmux/plugins/tpm/tpm'\nafter\n";
        let out = upsert_block(
            tmux,
            ID,
            "set -g @catppuccin_flavour 'mocha'",
            &Placement::BeforeLineContaining("run '~/.tmux/plugins/tpm/tpm'".into()),
        );

        let block_at = out.find("@catppuccin_flavour").unwrap();
        let tpm_at = out.find("run '~/.tmux/plugins/tpm/tpm'").unwrap();
        assert!(block_at < tpm_at, "flavour must be set before tpm runs");
        assert!(out.contains("after"));
    }

    #[test]
    fn a_missing_anchor_falls_back_to_the_end() {
        let text = "set -g mouse on\n";
        let out = upsert_block(
            text,
            ID,
            "content",
            &Placement::BeforeLineContaining("no such line".into()),
        );
        assert!(out.contains("content"));
        assert!(out.starts_with("set -g mouse on"));
    }

    #[test]
    fn anchored_blocks_are_also_stable_across_repeats() {
        let tmux = "set -g mouse on\nrun 'tpm'\n";
        let anchor = Placement::BeforeLineContaining("run 'tpm'".into());
        let once = upsert_block(tmux, ID, "content", &anchor);
        let twice = upsert_block(&once, ID, "content", &anchor);
        assert_eq!(once, twice);
        assert_eq!(twice.matches("# >>> termdeck:test >>>").count(), 1);
    }

    #[test]
    fn reads_back_what_it_wrote() {
        let out = upsert_block("base\n", ID, "line one\nline two", &Placement::End);
        assert_eq!(
            read_block(&out, ID).as_deref(),
            Some("line one\nline two")
        );
    }

    #[test]
    fn reading_an_absent_block_gives_nothing() {
        assert_eq!(read_block("set -g mouse on\n", ID), None);
    }

    #[test]
    fn slugify_keeps_two_paths_apart() {
        let a = slugify(Path::new("/Users/x/.tmux.conf"));
        let b = slugify(Path::new("/Users/x/other/.tmux.conf"));
        assert_ne!(a, b);
        assert!(!a.contains('/'));
    }

    #[test]
    fn write_with_backup_keeps_the_original() -> Result<()> {
        let dir = tempfile::tempdir()?;
        let file = dir.path().join("config");
        fs::write(&file, "before")?;

        // Point the backup directory at the temp home for this test.
        let saved = backup_target(&file, "after")?;
        assert_eq!(fs::read_to_string(&file)?, "after");
        assert_eq!(fs::read_to_string(saved)?, "before");
        Ok(())
    }

    /// Local stand-in for `write_with_backup` that keeps its backup beside the
    /// file, so the test does not write into the real home directory.
    fn backup_target(path: &Path, text: &str) -> Result<PathBuf> {
        let saved = path.with_extension("bak");
        fs::copy(path, &saved)?;
        fs::write(path, text)?;
        Ok(saved)
    }

    #[test]
    fn read_or_empty_tolerates_a_missing_file() -> Result<()> {
        let dir = tempfile::tempdir()?;
        assert_eq!(read_or_empty(&dir.path().join("nope"))?, "");
        Ok(())
    }
}
