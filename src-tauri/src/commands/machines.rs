//! The machines: listing, checking, adding, pairing, removing, and what a
//! terminal needs to reach one the way the app does.

use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::{fail, CmdResult};
use crate::doctor::{self, DoctorEvent, DoctorRow};
use crate::machine::{self, Machine, MachineStats};
use crate::{forward, guide, pairing, stack, AppState};

#[tauri::command]
pub fn list_machines(state: State<'_, AppState>) -> Vec<Machine> {
    state.store.machines()
}

#[tauri::command]
pub fn machine_stats(state: State<'_, AppState>) -> HashMap<String, MachineStats> {
    state.stats.lock().expect("stats lock").clone()
}

#[tauri::command]
pub fn new_machine_id() -> String {
    Machine::new_id()
}

/// What the terminal dialog needs to show working commands: the config as
/// ssh reads it, and on Windows the WSL distribution those commands run in.
#[derive(Debug, Serialize)]
pub struct TerminalInfo {
    pub ssh_config: String,
    pub wsl_distro: Option<String>,
    /// The machine's Host name in the ssh config: `ssh <alias>`.
    pub alias: String,
    /// The Docker context the app makes for it, whether or not it exists yet.
    pub context_name: String,
}

/// What a terminal needs to reach one machine the way the app does.
#[tauri::command]
pub fn terminal_info(state: State<'_, AppState>, machine_id: String) -> CmdResult<TerminalInfo> {
    let machine = state.store.machine(&machine_id).ok_or("That machine is no longer known.")?;
    Ok(TerminalInfo {
        ssh_config: state.ssh.config_path(),
        wsl_distro: crate::tools::wsl_distro(),
        alias: machine.alias(),
        context_name: machine::context_name(&machine),
    })
}

/// Checks a machine that may not be saved yet. Its Host block is written so
/// ssh can find it; the next add or remove regenerates the file anyway.
#[tauri::command]
pub async fn doctor(app: AppHandle, state: State<'_, AppState>, machine: Machine) -> CmdResult<Vec<DoctorRow>> {
    validate(&machine)?;
    let mut machines = state.store.machines();
    machines.retain(|m| m.id != machine.id);
    machines.push(machine.clone());
    state.ssh.write_config(&machines).map_err(fail)?;
    // A check is asked for after fixing something on the machine; the
    // shared connection may predate the fix, so the check logs in afresh.
    state.ssh.retire_master(&machine.alias());

    let machine_id = machine.id.clone();
    let rows = doctor::doctor(&state.ssh, &machine, |row| {
        let event = DoctorEvent { machine_id: machine_id.clone(), row: row.clone() };
        let _ = app.emit(doctor::DOCTOR_EVENT, event);
    })
    .await;
    Ok(rows)
}

#[tauri::command]
pub async fn add_machine(app: AppHandle, state: State<'_, AppState>, machine: Machine) -> CmdResult<Vec<Machine>> {
    validate(&machine)?;
    let mut machines = state.store.machines();
    machines.retain(|m| m.id != machine.id);
    machines.push(machine.clone());
    state.ssh.write_config(&machines).map_err(fail)?;
    state.store.save_machines(machines.clone()).map_err(fail)?;

    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn(async move {
        let stats = machine::poll(&ssh, &machine).await;
        machine::publish(&app, machine.id, stats);
    });
    Ok(machines)
}

#[tauri::command]
pub async fn remove_machine(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<Vec<Machine>> {
    let mut machines = state.store.machines();
    let Some(machine) = machines.iter().find(|m| m.id == id).cloned() else {
        return Err("unknown machine".into());
    };
    // Its stacks go with it here: their bridges, log streams and live-sync
    // watchers would otherwise keep pointing at a machine that is gone. What
    // runs on the machine stays as it is; it may be off, or added again.
    let mut stacks = state.store.stacks();
    for stack in stacks.iter().filter(|s| s.machine_id == id) {
        forward::stop(&app, &stack.id);
        stack::logs_stop(&app, &stack.id);
        stack::stop_watcher(&app, &stack.id);
        state.statuses.lock().expect("statuses lock").remove(&stack.id);
    }
    stacks.retain(|s| s.machine_id != id);
    state.store.save_stacks(stacks).map_err(fail)?;
    state.ssh.exit_master(&machine.alias(), None);
    if machine.docker_context {
        let _ = machine::set_docker_context(&machine, false).await;
    }
    machines.retain(|m| m.id != id);
    state.ssh.write_config(&machines).map_err(fail)?;
    state.store.save_machines(machines.clone()).map_err(fail)?;
    state.stats.lock().expect("stats lock").remove(&id);
    Ok(machines)
}

#[tauri::command]
pub async fn poll_machine(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<MachineStats> {
    let Some(machine) = state.store.machine(&id) else {
        return Err("unknown machine".into());
    };
    let stats = machine::poll(&state.ssh, &machine).await;
    machine::publish(&app, id, stats.clone());
    Ok(stats)
}

/// A machine saved by pairing, with the fingerprints the machine's screen
/// shows too: the host key pinned here, and this computer's key it installed.
#[derive(Serialize)]
pub struct PairedMachine {
    pub machine: Machine,
    pub host_fingerprint: Option<String>,
    pub key_fingerprint: Option<String>,
}

/// Pairs with a machine that shows a pairing code and saves it. The doctor
/// runs from the UI afterwards, like a manual add.
#[tauri::command]
pub async fn pair_machine(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    code: String,
    key_path: String,
    name: String,
) -> CmdResult<PairedMachine> {
    let address = address.trim().to_string();
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if address.is_empty() || address.contains(char::is_whitespace) {
        return Err("Enter the address shown on the machine.".into());
    }
    if code.len() != 6 {
        return Err("The code is six digits.".into());
    }
    if !guide::key_exists(&key_path) {
        return Err(format!("No key file at {key_path}."));
    }
    let pubkey = guide::public_key(&key_path).await.map_err(fail)?;
    let port = state.store.settings().unwrap_or_default().pairing_port;
    let answer = pairing::pair(&address, port, &code, &pubkey, &pairing::computer_name().await).await.map_err(fail)?;
    if !answer.ok {
        return Err(if answer.error.is_empty() { "The machine refused the pairing.".into() } else { answer.error });
    }
    // The answer is the first thing the machine ever tells us: check every
    // field before it can reach the ssh config.
    if !pairing::valid_user(&answer.user) || answer.port == 0 || !pairing::valid_host(&address) {
        return Err("The machine sent an answer this app does not accept.".into());
    }
    let host_key = if answer.host_key.is_empty() { None } else { pairing::valid_host_key(&answer.host_key) };
    let hostname = if pairing::valid_host(&answer.hostname) { answer.hostname.clone() } else { address.clone() };

    let machine = Machine {
        id: Machine::new_id(),
        name: if name.trim().is_empty() { hostname } else { name.trim().to_string() },
        user: answer.user,
        host: address,
        port: answer.port,
        key_path,
        docker_context: false,
        pinned: host_key.is_some(),
    };
    validate(&machine)?;
    if let Some((key_type, blob)) = &host_key {
        state.ssh.pin_host_key(&machine, key_type, blob).map_err(fail)?;
    }
    let mut machines = state.store.machines();
    machines.retain(|m| !(m.host == machine.host && m.port == machine.port));
    machines.push(machine.clone());
    state.ssh.write_config(&machines).map_err(fail)?;
    state.store.save_machines(machines).map_err(fail)?;

    let ssh = state.ssh.clone();
    let polled = machine.clone();
    tauri::async_runtime::spawn(async move {
        let stats = machine::poll(&ssh, &polled).await;
        machine::publish(&app, polled.id, stats);
    });
    let host_fingerprint = host_key.and_then(|(key_type, blob)| pairing::fingerprint(&format!("{key_type} {blob}")));
    Ok(PairedMachine { machine, host_fingerprint, key_fingerprint: pairing::fingerprint(&pubkey) })
}

#[tauri::command]
pub async fn set_docker_context(state: State<'_, AppState>, id: String, enabled: bool) -> CmdResult<Vec<Machine>> {
    let mut machines = state.store.machines();
    let Some(machine) = machines.iter_mut().find(|m| m.id == id) else {
        return Err("unknown machine".into());
    };
    machine::set_docker_context(machine, enabled).await.map_err(fail)?;
    machine.docker_context = enabled;
    state.store.save_machines(machines.clone()).map_err(fail)?;
    Ok(machines)
}

fn validate(machine: &Machine) -> CmdResult<()> {
    let name_ok = !machine.name.trim().is_empty();
    let host_ok = pairing::valid_host(machine.host.trim());
    let user_ok = pairing::valid_user(machine.user.trim());
    let key_ok = std::path::Path::new(&machine.key_path).is_file() && !machine.key_path.contains(['\n', '"']);
    if !name_ok {
        return Err("Give the machine a name.".into());
    }
    if !host_ok {
        return Err("The host must be an address without spaces.".into());
    }
    if !user_ok {
        return Err("The user must be a plain lowercase account name.".into());
    }
    if !key_ok {
        return Err(format!("No key file at {}.", machine.key_path));
    }
    Ok(())
}
