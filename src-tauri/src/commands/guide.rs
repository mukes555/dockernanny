//! The guide to prepare a machine: the Windows setup script, shown or
//! handed to the machine over the network with a line that checks it.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use super::{fail, CmdResult};
use crate::guide::{self, ScriptOptions};
use crate::AppState;

#[derive(Debug, Deserialize)]
pub struct ScriptRequest {
    pub key_path: String,
    pub port: u16,
    pub memory_gb: u32,
    pub distro: String,
    #[serde(default)]
    pub keep_awake: bool,
    #[serde(default)]
    pub make_private: bool,
}

#[derive(Debug, Serialize)]
pub struct ServeInfo {
    pub addresses: Vec<String>,
    pub port: u16,
    /// The line to type on the machine, one per address (`guide::fetch_command`).
    pub commands: Vec<String>,
    pub expires_ms: u64,
}

#[tauri::command]
pub async fn script_preview(request: ScriptRequest) -> CmdResult<String> {
    script_for(&request).await
}

/// Hands the setup script to the machine over the LAN until `script_stop`.
#[tauri::command]
pub async fn script_serve(app: AppHandle, state: State<'_, AppState>, request: ScriptRequest) -> CmdResult<ServeInfo> {
    let script = script_for(&request).await?;
    let port = state.store.settings().unwrap_or_default().script_port;
    let server = guide::serve(port, script, move |fetched| {
        let _ = app.emit(guide::FETCHED_EVENT, fetched);
    })
    .await
    .map_err(fail)?;
    let addresses = guide::lan_addresses().await;
    let commands = addresses.iter().map(|address| guide::fetch_command(address, server.port, &server.path, &server.sha256)).collect();
    let expires_ms = server.expires_ms;
    *state.script_server.lock().expect("script server lock") = Some(server);
    Ok(ServeInfo { addresses, port, commands, expires_ms })
}

#[tauri::command]
pub fn script_stop(state: State<'_, AppState>) {
    state.script_server.lock().expect("script server lock").take();
}

async fn script_for(request: &ScriptRequest) -> CmdResult<String> {
    if !guide::key_exists(&request.key_path) {
        return Err(format!("No key file at {}.", request.key_path));
    }
    // The name lands inside a quoted string of a script run as Administrator.
    if !crate::settings::safe_distro(request.distro.trim()) {
        return Err("A WSL distribution name is letters, digits, dots, dashes and underscores.".into());
    }
    if request.port == 0 {
        return Err("Choose the port sshd should listen on.".into());
    }
    // Only `type base64` goes in: a comment could hold a quote and break out of the string.
    let public_key = crate::host::platform::host_key_from_pub(&guide::public_key(&request.key_path).await.map_err(fail)?);
    if public_key.is_empty() || public_key.contains('\'') {
        return Err(format!("{}.pub does not look like an ssh public key.", request.key_path));
    }
    Ok(guide::build_script(&ScriptOptions {
        public_key,
        port: request.port,
        memory_gb: request.memory_gb,
        distro: request.distro.clone(),
        keep_awake: request.keep_awake,
        make_private: request.make_private,
    }))
}
