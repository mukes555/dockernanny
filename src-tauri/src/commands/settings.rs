//! The settings screen: read, save, and the WSL distributions to pick from.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::settings::Settings;
use crate::AppState;

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
    SettingsView { first_run: saved.is_none(), settings: saved.unwrap_or_default(), os: std::env::consts::OS }
}

/// Saves, then starts or stops the sharing role and the start-at-login entry
/// to match, so a toggle takes effect at once. Applying runs off the main
/// thread: a changed WSL distribution means several wsl.exe calls, and the
/// window must not freeze while they run.
#[tauri::command]
pub async fn save_settings(app: AppHandle, state: State<'_, AppState>, settings: Settings) -> Result<Settings, String> {
    let settings = settings.normalised();
    state.store.save_settings(settings.clone()).map_err(|err| format!("{err:#}"))?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || crate::apply_settings(&handle)).await.map_err(|err| format!("{err}"))?;
    Ok(settings)
}

/// The WSL distributions installed on this computer, for the WSL card.
/// Empty off Windows and when WSL is missing.
#[tauri::command]
pub async fn wsl_distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let out = crate::tools::native("wsl.exe").args(["-l", "-q"]).env("WSL_UTF8", "1").output().await;
    let text = out.map(|o| crate::host::platform::decode(&o.stdout)).unwrap_or_default();
    text.lines().map(str::trim).filter(|name| crate::settings::safe_distro(name)).map(str::to_string).collect()
}
