//! Seeing and steering a machine's Docker through its context: the container
//! list, the three lifecycle verbs, and a live log stream. Everything runs
//! over the same ssh connection dockerNanny already holds to the machine, so
//! no ports are opened and the machine's own `docker` is the authority.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::containers::{self, Action, Container};
use crate::job::Line;
use crate::AppState;

use super::CmdResult;

pub const LOG_EVENT: &str = "container:log";

#[derive(Debug, Clone, Serialize)]
pub struct ContainerLogEvent {
    pub id: String,
    pub lines: Vec<Line>,
}

fn alias_of(state: &AppState, machine_id: &str) -> CmdResult<String> {
    let machine = state.store.machine(machine_id).ok_or("That machine is no longer known.")?;
    Ok(machine.alias())
}

/// Every container on the machine, running or not, freshly read.
#[tauri::command]
pub async fn list_containers(state: State<'_, AppState>, machine_id: String) -> CmdResult<Vec<Container>> {
    let alias = alias_of(&state, &machine_id)?;
    let out =
        state.ssh.run(&alias, containers::LIST_SCRIPT).await.map_err(|err| format!("Could not reach the machine's Docker: {err:#}"))?;
    if !out.ok() {
        // A machine that answers ssh but has no docker, or a docker that is down.
        return Err(if out.stderr.is_empty() { "The machine did not answer docker ps.".into() } else { out.stderr });
    }
    Ok(containers::parse(&out.stdout))
}

/// Start, stop or restart one container by id or name.
#[tauri::command]
pub async fn container_action(state: State<'_, AppState>, machine_id: String, id: String, action: Action) -> CmdResult<()> {
    if !containers::safe_ref(&id) {
        return Err("That container id looks unsafe; refusing to run it.".into());
    }
    let alias = alias_of(&state, &machine_id)?;
    let script = format!("docker {} {}", action.verb(), id);
    let out = state.ssh.run(&alias, &script).await.map_err(|err| format!("Could not reach the machine's Docker: {err:#}"))?;
    if !out.ok() {
        let why = if out.stderr.is_empty() { format!("docker {} exited with an error", action.verb()) } else { out.stderr };
        return Err(why);
    }
    Ok(())
}

/// `docker logs -f` for one container, streamed as `container:log` until stopped.
#[tauri::command]
pub async fn start_container_logs(app: AppHandle, state: State<'_, AppState>, machine_id: String, id: String) -> CmdResult<()> {
    if !containers::safe_ref(&id) {
        return Err("That container id looks unsafe; refusing to run it.".into());
    }
    stop_container_logs(app.clone(), id.clone());
    let alias = alias_of(&state, &machine_id)?;
    let sink = {
        let app = app.clone();
        let id = id.clone();
        crate::job::batched(move |lines| {
            let _ = app.emit(LOG_EVENT, ContainerLogEvent { id: id.clone(), lines });
        })
    };
    let script = format!("docker logs -f --tail 200 {id}");
    let job = state.ssh.job(&alias, &script, sink).map_err(|err| format!("{err:#}"))?;
    let handle = job.handle();
    let key = logs_key(&id);
    state.jobs.lock().expect("jobs lock").insert(key.clone(), handle.clone());
    // A stream that ends by itself takes its entry with it, but never a newer stream's.
    tauri::async_runtime::spawn(async move {
        let _ = job.wait().await;
        let state = app.state::<AppState>();
        let mut jobs = state.jobs.lock().expect("jobs lock");
        let still_mine = jobs.get(&key).is_some_and(|current| current.same(&handle));
        if still_mine {
            jobs.remove(&key);
        }
    });
    Ok(())
}

#[tauri::command]
pub fn stop_container_logs(app: AppHandle, id: String) {
    let state = app.state::<AppState>();
    let removed = state.jobs.lock().expect("jobs lock").remove(&logs_key(&id));
    if let Some(handle) = removed {
        handle.cancel();
    }
}

fn logs_key(id: &str) -> String {
    format!("ctr:{id}:logs")
}
