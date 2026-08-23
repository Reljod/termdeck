//! Finding the programs we need to run.
//!
//! An app launched from the Finder does not get the `PATH` a terminal gets. On
//! this machine a GUI process starts with
//! `/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin`, which does not include
//! Homebrew — so `Command::new("tmux")` fails and the target looks as though it
//! is not installed when it plainly is.
//!
//! Everything that shells out goes through here instead.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// Places package managers install things, checked after `PATH`.
///
/// Homebrew on Apple silicon and Intel first, then MacPorts, Nix and the
/// conventional user-local directory.
const EXTRA_DIRS: [&str; 6] = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
    "/run/current-system/sw/bin",
    "/usr/bin",
    "/bin",
];

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

/// The `PATH` a login shell would have, worked out once and reused.
///
/// This is the backstop for version managers that install somewhere unusual —
/// asdf, mise, nvm and friends. It costs one subprocess, and only when a
/// program was not found anywhere more obvious.
fn login_path() -> &'static [PathBuf] {
    static CACHE: OnceLock<Vec<PathBuf>> = OnceLock::new();
    CACHE.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let Ok(out) = Command::new(&shell).args(["-lc", "printf %s \"$PATH\""]).output() else {
            return Vec::new();
        };
        if !out.status.success() {
            return Vec::new();
        }
        String::from_utf8_lossy(&out.stdout)
            .split(':')
            .filter(|entry| !entry.is_empty())
            .map(PathBuf::from)
            .collect()
    })
}

/// The full path to `program`, or `None` if it is genuinely not installed.
pub fn resolve(program: &str) -> Option<PathBuf> {
    if program.contains('/') {
        let path = PathBuf::from(program);
        return is_executable(&path).then_some(path);
    }

    let from_path = std::env::var("PATH").unwrap_or_default();
    let candidates = from_path
        .split(':')
        .filter(|entry| !entry.is_empty())
        .map(PathBuf::from)
        .chain(EXTRA_DIRS.iter().map(PathBuf::from));

    for dir in candidates {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    // Nothing obvious worked, so ask a login shell where it looks.
    for dir in login_path() {
        let candidate = dir.join(program);
        if is_executable(&candidate) {
            return Some(candidate);
        }
    }

    None
}

/// A `Command` for `program`, resolved to a full path.
///
/// Returns `None` when the program is not installed, which callers report as a
/// skipped target rather than a failure.
pub fn command(program: &str) -> Option<Command> {
    resolve(program).map(Command::new)
}

/// Whether `program` is installed.
pub fn exists(program: &str) -> bool {
    resolve(program).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_something_that_is_always_there() {
        // `sh` is in /bin on every unix, and in the restricted GUI PATH too.
        let found = resolve("sh").expect("sh should be findable");
        assert!(found.is_absolute());
        assert!(is_executable(&found));
    }

    #[test]
    fn reports_a_missing_program_as_missing() {
        assert_eq!(resolve("termdeck-definitely-not-a-real-binary"), None);
        assert!(!exists("termdeck-definitely-not-a-real-binary"));
    }

    #[test]
    fn an_absolute_path_is_checked_rather_than_searched() {
        assert_eq!(resolve("/bin/sh"), Some(PathBuf::from("/bin/sh")));
        assert_eq!(resolve("/bin/definitely-not-here"), None);
    }

    #[test]
    fn a_directory_is_not_an_executable() {
        assert_eq!(resolve("/usr"), None);
    }

    #[test]
    fn finds_a_program_outside_the_gui_path() {
        // The whole point of this module: with the PATH a Finder-launched app
        // gets, Homebrew programs must still be found.
        let real = resolve("sh").expect("sh");
        let restricted = "/usr/bin:/bin:/usr/sbin:/sbin:/usr/local/bin";

        // SAFETY: this test binary runs these serially and restores PATH.
        let original = std::env::var("PATH").unwrap_or_default();
        unsafe {
            std::env::set_var("PATH", restricted);
        }
        let found = resolve("sh");
        unsafe {
            std::env::set_var("PATH", original);
        }

        assert_eq!(found.as_deref(), Some(real.as_path()));
    }

    #[test]
    fn command_is_built_from_the_resolved_path() {
        let command = command("sh").expect("sh");
        let program = command.get_program().to_string_lossy().to_string();
        assert!(program.starts_with('/'), "should be absolute, got {program}");
    }

    #[test]
    fn command_for_a_missing_program_is_none() {
        assert!(command("termdeck-definitely-not-a-real-binary").is_none());
    }
}
