//! New releases: what the app knows, a check on request, and the install.

use tauri::{AppHandle, State};

use crate::updates::{self, UpdateStatus, Updates};

#[tauri::command]
pub fn update_status(updates: State<'_, Updates>) -> UpdateStatus {
    updates.status()
}

/// Help's "Check for updates": asks now, whatever the setting says.
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> UpdateStatus {
    updates::check_now(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    updates::install(&app).await
}
