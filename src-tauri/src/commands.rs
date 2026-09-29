//! What the webview can ask the backend to do. Every command is a thin wrapper
//! that turns an anyhow error into the string the UI shows. The settings
//! commands live in `commands/settings.rs`.

pub mod computer;
pub mod containers;
pub mod copy;
pub mod host;
pub mod settings;
pub mod updates;

use std::collections::HashMap;
use std::path::Path;

use tauri::{AppHandle, Emitter, Manager, State};

use serde::{Deserialize, Serialize};

use crate::compose::{self, Preview};
use crate::doctor::{self, DoctorEvent, DoctorRow};
use crate::guide::{self, ScriptOptions};
use crate::machine::{self, Machine, MachineStats};
use crate::stack::{self, Stack, StackStatus};
use crate::{forward, pairing, sync, AppState};

type CmdResult<T> = Result<T, String>;

fn fail(err: anyhow::Error) -> String {
    format!("{err:#}")
}

#[tauri::command]
pub fn list_machines(state: State<'_, AppState>) -> Vec<Machine> {
    state.store.machines()
}

#[tauri::command]
pub fn machine_stats(state: State<'_, AppState>) -> HashMap<String, MachineStats> {
    state.stats.lock().expect("stats lock").clone()
}

#[tauri::command]
pub fn app_home(state: State<'_, AppState>) -> String {
    state.store.home().display().to_string()
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

/// The WSL distributions installed on this computer, for Settings. Empty
/// off Windows and when WSL is missing.
#[tauri::command]
pub async fn wsl_distros() -> Vec<String> {
    if !cfg!(windows) {
        return Vec::new();
    }
    let out = crate::tools::native("wsl.exe").args(["-l", "-q"]).env("WSL_UTF8", "1").output().await;
    let text = out.map(|o| crate::host::platform::decode(&o.stdout)).unwrap_or_default();
    text.lines().map(str::trim).filter(|name| crate::settings::safe_distro(name)).map(str::to_string).collect()
}

#[tauri::command]
pub fn new_machine_id() -> String {
    Machine::new_id()
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

#[tauri::command]
pub async fn sync_stack(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = state.store.stack(&id).ok_or("unknown stack")?;
    spawn_operation(app.clone(), id, stack::resync(app, stack));
    Ok(())
}

/// Async on purpose: a sync command runs on the main thread, where spawning
/// the tokio log process aborts the whole app ("there is no reactor running").
#[tauri::command]
pub async fn start_logs(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = state.store.stack(&id).ok_or("unknown stack")?;
    stack::logs_start(&app, &stack).map_err(fail)
}

#[tauri::command]
pub fn stop_logs(app: AppHandle, id: String) {
    stack::logs_stop(&app, &id);
}

#[tauri::command]
pub async fn preview_compose(path: String) -> CmdResult<Preview> {
    compose::preview(Path::new(&path)).await.map_err(fail)
}

#[tauri::command]
pub fn default_excludes(state: State<'_, AppState>) -> Vec<String> {
    state.store.settings().map(|s| s.excludes).unwrap_or_else(sync::default_excludes)
}

#[tauri::command]
pub fn busy_ports(ports: Vec<u16>) -> Vec<u16> {
    forward::busy_ports(&ports)
}

#[tauri::command]
pub fn list_stacks(state: State<'_, AppState>) -> Vec<Stack> {
    state.store.stacks()
}

#[tauri::command]
pub fn stack_statuses(state: State<'_, AppState>) -> HashMap<String, StackStatus> {
    state.statuses.lock().expect("statuses lock").clone()
}

/// Saves the stack and starts it. The command returns as soon as the stack is
/// saved; progress arrives as `stack:status` and `stack:output` events.
#[tauri::command]
pub async fn create_stack(app: AppHandle, state: State<'_, AppState>, mut stack: Stack) -> CmdResult<Vec<Stack>> {
    // Ids end up in shell scripts, so they are minted here, never taken from the webview.
    stack.id = Stack::new_id();
    stack.name = compose::sanitize_name(&stack.name);
    if state.store.machine(&stack.machine_id).is_none() {
        return Err("Pick a machine first.".into());
    }
    let is_file = Path::new(&stack.project_dir).join(&stack.compose_rel).is_file();
    if !is_file || !stays_inside_folder(&stack.compose_rel) {
        return Err(format!("{} is not a file inside {}.", stack.compose_rel, stack.project_dir));
    }
    // The id was just minted, so the name is the only thing to check.
    let mut stacks = state.store.stacks();
    let name_taken = stacks.iter().any(|s| s.machine_id == stack.machine_id && s.name == stack.name);
    if name_taken {
        return Err(format!("A stack named {} already runs on that machine.", stack.name));
    }
    stacks.push(stack.clone());
    state.store.save_stacks(stacks.clone()).map_err(fail)?;
    if stack.live_sync {
        stack::start_watcher(&app, &stack);
    }
    spawn_operation(app.clone(), stack.id.clone(), stack::up(app, stack, false));
    Ok(stacks)
}

/// Start (`up -d`) or, with `rebuild`, Rebuild (`up -d --build`).
#[tauri::command]
pub async fn up_stack(app: AppHandle, state: State<'_, AppState>, id: String, rebuild: bool) -> CmdResult<()> {
    let stack = stack_free_to_change(&state, &id)?;
    spawn_operation(app.clone(), id, stack::up(app, stack, rebuild));
    Ok(())
}

/// Stop keeps the containers (`compose stop`); `down_stack` removes them.
#[tauri::command]
pub async fn stop_stack(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = stack_free_to_change(&state, &id)?;
    spawn_operation(app.clone(), id, stack::stop(app, stack));
    Ok(())
}

#[tauri::command]
pub async fn down_stack(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = stack_free_to_change(&state, &id)?;
    spawn_operation(app.clone(), id, stack::down(app, stack, false));
    Ok(())
}

#[tauri::command]
pub async fn restart_stack(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = stack_free_to_change(&state, &id)?;
    spawn_operation(app.clone(), id, stack::restart(app, stack));
    Ok(())
}

/// The stack, unless a copy of it runs: a copy cannot be stopped half way,
/// so the button answers at once instead of racing it.
fn stack_free_to_change(state: &AppState, id: &str) -> CmdResult<Stack> {
    let stack = state.store.stack(id).ok_or("unknown stack")?;
    if state.operations.copying(id) {
        return Err(stack::COPY_RUNNING.into());
    }
    Ok(stack)
}

#[tauri::command]
pub async fn remove_stack(app: AppHandle, state: State<'_, AppState>, id: String, volumes: bool) -> CmdResult<Vec<Stack>> {
    let stack = state.store.stack(&id).ok_or("unknown stack")?;
    stack::remove(app, stack, volumes).await.map_err(fail)?;
    Ok(state.store.stacks())
}

/// Turns a stack's bridge (its localhost ports) on or off, and acts on it at
/// once instead of waiting for the next poll.
#[tauri::command]
pub async fn set_forward_ports(app: AppHandle, state: State<'_, AppState>, id: String, on: bool) -> CmdResult<Vec<Stack>> {
    let mut stacks = state.store.stacks();
    let stack = stacks.iter_mut().find(|s| s.id == id).ok_or("That stack is no longer known.")?;
    stack.forward_ports = on;
    let stack = stack.clone();
    state.store.save_stacks(stacks.clone()).map_err(fail)?;
    let services = state.statuses.lock().expect("statuses lock").get(&id).map(|s| s.services.clone()).unwrap_or_default();
    forward::reconcile(&app, &stack, &services);
    Ok(stacks)
}

#[tauri::command]
pub fn forward_states(state: State<'_, AppState>) -> HashMap<String, forward::ForwardState> {
    state.forward_states.lock().expect("forward states lock").clone()
}

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

/// Runs a stack operation in the background; the command returns at once and
/// progress arrives as events. A failure is written on the card, unless the
/// stack was removed meanwhile or a newer operation took it over.
fn spawn_operation(app: AppHandle, stack_id: String, task: impl std::future::Future<Output = anyhow::Result<()>> + Send + 'static) {
    tauri::async_runtime::spawn(async move {
        let Err(err) = task.await else { return };
        tracing::warn!("{err:#}");
        let state = app.state::<AppState>();
        let stack_gone = state.store.stack(&stack_id).is_none();
        let newer_operation = state.operations.holds(&stack_id);
        if stack_gone || newer_operation {
            return;
        }
        stack::set_status(&app, &stack_id, |status| status.error = Some(format!("{err:#}")));
    });
}

/// Only the project folder is synced, so a compose file reached with `..`
/// or an absolute path would never exist on the machine.
fn stays_inside_folder(compose_rel: &str) -> bool {
    let absolute = Path::new(compose_rel).is_absolute() || compose_rel.starts_with(['/', '\\']);
    let climbs = compose_rel.split(['/', '\\']).any(|part| part == "..");
    !absolute && !climbs
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compose_file_must_sit_inside_the_synced_folder() {
        assert!(stays_inside_folder("compose.yaml"));
        assert!(stays_inside_folder("deploy/docker-compose.yml"));
        assert!(!stays_inside_folder("../other/compose.yaml"));
        assert!(!stays_inside_folder("deploy/../../compose.yaml"));
        assert!(!stays_inside_folder("/etc/compose.yaml"));
        assert!(!stays_inside_folder("\\shared\\compose.yaml"));
    }
}
