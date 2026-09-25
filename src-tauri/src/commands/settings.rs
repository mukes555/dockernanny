//! The settings screen: read, save, and the maintenance actions that live there.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::settings::Settings;
use crate::{forward, AppState};

#[derive(Debug, Serialize)]
pub struct SettingsView {
    pub settings: Settings,
    /// No settings file yet: the app asks which roles this computer plays.
    pub first_run: bool,
    /// "macos", "windows" or "linux", so the window can say what each role needs here.
    pub os: &'static str,
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsView {
    let saved = state.store.settings();
    SettingsView {
        first_run: saved.is_none(),
        settings: saved.unwrap_or_default(),
        os: std::env::consts::OS,
    }
}

/// Saves, then starts or stops the sharing role and the start-at-login entry
/// to match, so a toggle takes effect at once.
#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let settings = settings.normalised();
    state.store.save_settings(settings.clone()).map_err(|err| format!("{err:#}"))?;
    crate::apply_settings(&app);
    Ok(settings)
}

/// Drops every forwarder, including ones left by an earlier instance, and
/// lets the status poll start fresh ones a few seconds later.
#[tauri::command]
pub async fn reset_forwards(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    for stack in state.store.stacks() {
        forward::stop(&app, &stack.id);
    }
    crate::forget_stale_forwarders(&state);
    Ok(())
}
