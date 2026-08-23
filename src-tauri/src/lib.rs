//! The Tauri layer.
//!
//! Deliberately thin. Every command hands straight off to `termdeck-core`,
//! which is where the logic and the tests live.

use termdeck_core::settings::Settings;
use termdeck_core::targets::{ApplyOptions, Outcome};
use termdeck_core::{apply, state, State};

/// Errors cross into JavaScript as plain strings, since the interface only
/// ever shows them to a person.
type CommandResult<T> = Result<T, String>;

fn stringify(error: anyhow::Error) -> String {
    format!("{error:#}")
}

#[tauri::command]
fn get_state() -> State {
    state()
}

#[tauri::command]
fn apply_theme(
    theme_id: String,
    options: ApplyOptions,
    enabled: Vec<String>,
) -> CommandResult<Vec<Outcome>> {
    apply(&theme_id, &options, &enabled).map_err(stringify)
}

#[tauri::command]
fn toggle_theme(options: ApplyOptions, enabled: Vec<String>) -> CommandResult<Vec<Outcome>> {
    let settings = Settings::load();
    termdeck_core::toggle(
        &settings.light_theme,
        &settings.dark_theme,
        &options,
        &enabled,
    )
    .map_err(stringify)
}

#[tauri::command]
fn save_settings(settings: Settings) -> CommandResult<()> {
    settings.save().map_err(stringify)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            get_state,
            apply_theme,
            toggle_theme,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running TermDeck");
}
