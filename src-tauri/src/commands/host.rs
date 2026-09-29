//! The sharing page: what this computer has, Set up, and the pairing switch.

use tauri::State;

use crate::host::engine::ToEngine;
use crate::host::platform::SetupOptions;
use crate::host::HostSnapshot;
use crate::AppState;

/// None while the sharing role is off.
#[tauri::command]
pub fn host_snapshot(state: State<'_, AppState>) -> Option<HostSnapshot> {
    state.host.snapshot()
}

#[tauri::command]
pub fn host_log(state: State<'_, AppState>) -> Vec<String> {
    state.host.log()
}

#[tauri::command]
pub fn host_setup(state: State<'_, AppState>, mut options: SetupOptions) -> Result<(), String> {
    options.pairing_port = state.store.settings().unwrap_or_default().pairing_port;
    state.host.setup(options)
}

#[tauri::command]
pub fn host_arm_pairing(state: State<'_, AppState>) -> Result<(), String> {
    state.host.send(ToEngine::ArmPairing)
}

#[tauri::command]
pub fn host_disarm_pairing(state: State<'_, AppState>) -> Result<(), String> {
    state.host.send(ToEngine::DisarmPairing)
}

#[tauri::command]
pub fn host_probe(state: State<'_, AppState>) -> Result<(), String> {
    state.host.send(ToEngine::Probe)
}

/// Takes a paired computer's access away (its key and its entry).
#[tauri::command]
pub fn host_forget(state: State<'_, AppState>, address: String) -> Result<(), String> {
    state.host.send(ToEngine::Forget(address))
}
