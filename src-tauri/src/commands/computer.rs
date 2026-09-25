//! This computer: who it is, whether it is ready to use other machines, its
//! SSH key, and the diagnostics a bug report needs.

use serde::Serialize;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::diagnostics::{self, Facts, Replacement};
use crate::doctor::DoctorRow;
use crate::{computer, machine, pairing, store, tools, AppState};

type CmdResult<T> = Result<T, String>;

/// The name this computer shows to a machine when pairing, and in its own rail.
#[tauri::command]
pub async fn computer_name() -> String {
    pairing::computer_name().await
}

#[derive(Debug, Serialize)]
pub struct ComputerInfo {
    pub name: String,
    pub user: String,
    pub probe: crate::probe::Probe,
}

/// What this computer is and how it is doing, from the same probe the
/// machines answer.
#[tauri::command]
pub async fn computer_info() -> ComputerInfo {
    ComputerInfo {
        name: pairing::computer_name().await,
        user: user_name(),
        probe: machine::probe_this_computer().await,
    }
}

/// The key from the settings, or the first key the user is likely to already
/// have on this computer, or where a new one would go.
#[tauri::command]
pub fn default_key_path(state: State<'_, AppState>) -> String {
    key_path(&state).display().to_string()
}

#[tauri::command]
pub async fn computer_readiness(state: State<'_, AppState>) -> CmdResult<Vec<DoctorRow>> {
    Ok(computer::readiness(&key_path(&state)).await)
}

/// Makes the key `default_key_path` points at, only when it does not exist.
#[tauri::command]
pub async fn generate_key(state: State<'_, AppState>) -> CmdResult<String> {
    let path = key_path(&state);
    let public = computer::generate_key(&path).await.map_err(|err| format!("{err:#}"))?;
    Ok(public.display().to_string())
}

/// Windows: ssh and rsync installed inside WSL, for "Use other machines".
#[tauri::command]
pub async fn install_wsl_tools() -> CmdResult<()> {
    computer::install_wsl_tools().await.map_err(|err| format!("{err:#}"))
}

/// The redacted report for a bug report; see `diagnostics.rs`.
#[tauri::command]
pub async fn diagnostics(state: State<'_, AppState>) -> CmdResult<String> {
    let settings = state.store.settings().unwrap_or_default();
    let machines = state.store.machines();
    let online = {
        let stats = state.stats.lock().expect("stats lock");
        machines.iter().filter(|m| stats.get(&m.id).is_some_and(|s| s.online)).count()
    };
    let versions = computer::versions().await;
    let facts = Facts {
        use_machines: settings.use_machines,
        share_this_computer: settings.share_this_computer,
        machines: machines.len(),
        machines_online: online,
        stacks: state.store.stacks().len(),
        ssh: versions.ssh,
        rsync: versions.rsync,
        docker: versions.docker,
    };

    let mut private: Vec<Replacement> = vec![
        (store::user_home().display().to_string(), "~".into()),
        (user_name(), "<you>".into()),
        (pairing::computer_name().await, "<this-computer>".into()),
    ];
    if let Some(host) = computer::version_line(tools::native("hostname")).await {
        private.push((host, "<this-computer>".into()));
    }
    for (index, machine) in machines.iter().enumerate() {
        let n = index + 1;
        private.push((machine.name.clone(), format!("machine-{n}")));
        private.push((machine.host.clone(), format!("host-{n}")));
        private.push((machine.user.clone(), format!("user-{n}")));
    }

    let home = state.store.home();
    Ok(diagnostics::report(&facts, &home.join("app.log"), &home.join("host.log"), &private))
}

/// Opens the app's data folder, or shows `app.log` in the file manager.
#[tauri::command]
pub fn reveal_app_file(app: AppHandle, state: State<'_, AppState>, which: String) -> CmdResult<()> {
    let home = state.store.home().to_path_buf();
    let opener = app.opener();
    let result = match which.as_str() {
        "log" => opener.reveal_item_in_dir(home.join("app.log")),
        _ => opener.open_path(home.display().to_string(), None::<&str>),
    };
    result.map_err(|err| format!("could not open it: {err}"))
}

fn key_path(state: &AppState) -> std::path::PathBuf {
    let chosen = state.store.settings().map(|s| s.key_path).unwrap_or_default();
    computer::default_key_path(&chosen)
}

fn user_name() -> String {
    std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_default()
}
