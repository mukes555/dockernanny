//! The stacks: creating one from a compose file, the operations on it
//! (start, stop, remove containers, restart, sync, remove), its logs and
//! its bridge. An operation runs in the background; the command returns at
//! once and progress arrives as events.

use std::collections::HashMap;
use std::path::Path;

use tauri::{AppHandle, Manager, State};

use super::{fail, CmdResult};
use crate::compose::{self, Preview};
use crate::stack::{self, Stack, StackStatus};
use crate::{forward, sync, AppState};

#[tauri::command]
pub fn list_stacks(state: State<'_, AppState>) -> Vec<Stack> {
    state.store.stacks()
}

#[tauri::command]
pub fn stack_statuses(state: State<'_, AppState>) -> HashMap<String, StackStatus> {
    state.statuses.lock().expect("statuses lock").clone()
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

#[tauri::command]
pub async fn sync_stack(app: AppHandle, state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let stack = state.store.stack(&id).ok_or("unknown stack")?;
    spawn_operation(app.clone(), id, stack::resync(app, stack));
    Ok(())
}

#[tauri::command]
pub async fn remove_stack(app: AppHandle, state: State<'_, AppState>, id: String, volumes: bool) -> CmdResult<Vec<Stack>> {
    let stack = state.store.stack(&id).ok_or("unknown stack")?;
    stack::remove(app, stack, volumes).await.map_err(fail)?;
    Ok(state.store.stacks())
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

/// Drops every bridge, including ones left by an earlier instance, and lets
/// the status poll start fresh ones a few seconds later.
#[tauri::command]
pub async fn reset_forwards(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    for stack in state.store.stacks() {
        forward::stop(&app, &stack.id);
    }
    let ssh = state.ssh.clone();
    tauri::async_runtime::spawn_blocking(move || forward::exit_all(&ssh)).await.map_err(|err| format!("{err}"))?;
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
