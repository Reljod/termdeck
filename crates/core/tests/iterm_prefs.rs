//! Checks the iTerm2 plist handling against the real preferences file.
//!
//! This lives in its own test binary on purpose. The end-to-end test
//! rewrites `HOME` for its whole process, and if these two shared one, this
//! test could find itself pointed at an empty sandbox and quietly skip.

use termdeck_core::theme;

/// Reads the real iTerm2 preferences, applies a theme in memory, and checks the
/// result still parses as the same document.
///
/// A real preferences file is far messier than anything worth writing by hand —
/// this one is over a hundred kilobytes with several profiles and custom
/// presets already in it. If the plist handling is going to break, it will
/// break here rather than on the small fixture the unit tests use.
///
/// Skipped when there is no iTerm2 on the machine, so the suite still passes
/// elsewhere.
#[test]
fn a_real_iterm_preferences_file_survives_a_theme() {
    use plist::Value;
    use termdeck_core::targets::iterm2;

    let path = iterm2::prefs_path();
    if !path.exists() {
        eprintln!("skipping: no iTerm2 preferences at {}", path.display());
        return;
    }

    let Ok(value) = Value::from_file(&path) else {
        eprintln!("skipping: could not parse {}", path.display());
        return;
    };
    let Some(original) = value.into_dictionary() else {
        eprintln!("skipping: preferences are not a dictionary");
        return;
    };

    let profiles_before = original
        .get("New Bookmarks")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    let presets_before = original
        .get("Custom Color Presets")
        .and_then(|v| v.as_dictionary())
        .map(|d| d.len())
        .unwrap_or(0);

    let theme = theme::find("catppuccin-mocha").unwrap();
    let mut updated = original.clone();
    let touched = iterm2::apply_to_prefs(&mut updated, &theme, true);
    assert_eq!(touched, profiles_before, "every profile should be coloured");

    // Nothing is lost: same profiles, and every preset that was there still is.
    assert_eq!(
        updated
            .get("New Bookmarks")
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0),
        profiles_before
    );
    let presets_after = updated
        .get("Custom Color Presets")
        .and_then(|v| v.as_dictionary())
        .map(|d| d.len())
        .unwrap_or(0);
    assert!(
        presets_after >= presets_before,
        "existing colour presets were dropped"
    );

    // Every key the file had, it still has.
    for key in original.keys() {
        assert!(updated.contains_key(key), "lost top-level key `{key}`");
    }

    // And the result is a document that can actually be written and read back.
    let temp = tempfile::NamedTempFile::new().unwrap();
    Value::Dictionary(updated.clone())
        .to_file_xml(temp.path())
        .expect("writing the updated preferences");
    let reloaded = Value::from_file(temp.path())
        .expect("reading them back")
        .into_dictionary()
        .expect("still a dictionary");
    assert_eq!(reloaded, updated, "the file did not round-trip");

    // The theme we wrote is the theme we read back.
    assert_eq!(
        iterm2::theme_from_prefs(&reloaded).as_deref(),
        Some("catppuccin-mocha")
    );

    // Applying a second time is stable.
    let mut twice = updated.clone();
    iterm2::apply_to_prefs(&mut twice, &theme, true);
    assert_eq!(twice, updated, "a second apply changed the document");
}
