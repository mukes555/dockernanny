//! The operations on a stack: sync, up, down, restart, live sync, logs,
//! remove. Long operations stream their output as events and leave the
//! status to say how they ended.

use std::path::PathBuf;

use anyhow::Context;
use tauri::{AppHandle, Emitter, Manager};

use super::{derive_phase, now_ms, output_sink, set_status, shell_quote, OutputEvent, Phase, Stack, LOG_EVENT};
use crate::compose;
use crate::{forward, sync, AppState};

/// Sync the folder, then `up -d`, which builds only images that are missing,
/// as `docker compose up` does; `rebuild` adds `--build`, so every built
/// image is made again from the synced folder. A previous up or down still
/// running is cancelled first.
pub async fn up(app: AppHandle, stack: Stack, rebuild: bool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let machine = state.store.machine(&stack.machine_id).context("the machine no longer exists")?;
    let alias = machine.alias();
    cancel_job(&app, &stack.id);

    set_status(&app, &stack.id, |status| {
        status.phase = Phase::Syncing;
        status.message = None;
    });
    let synced = sync::run(&state.ssh, &alias, &stack, output_sink(&app, &stack.id)).await;
    let synced = match synced {
        Ok(result) => result,
        Err(err) => {
            set_status(&app, &stack.id, |status| {
                status.phase = Phase::Error;
                status.message = Some(format!("sync failed: {err:#}"));
            });
            return Err(err);
        }
    };
    set_status(&app, &stack.id, |status| {
        status.phase = Phase::Starting;
        status.synced_at_ms = Some(now_ms());
        status.synced_files = synced.files;
        status.message = synced.warning;
    });

    let args = if rebuild { "up -d --build --remove-orphans" } else { "up -d --remove-orphans" };
    run_compose(&app, &stack, &alias, &stack.compose_cmd(args), "compose up").await
}

pub async fn down(app: AppHandle, stack: Stack, remove_volumes: bool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let machine = state.store.machine(&stack.machine_id).context("the machine no longer exists")?;
    cancel_job(&app, &stack.id);
    set_status(&app, &stack.id, |status| {
        status.phase = Phase::Stopping;
        status.message = None;
    });
    let args = if remove_volumes { "down --remove-orphans --volumes" } else { "down --remove-orphans" };
    run_compose(&app, &stack, &machine.alias(), &stack.compose_cmd(args), "compose down").await
}

pub async fn restart(app: AppHandle, stack: Stack) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let machine = state.store.machine(&stack.machine_id).context("the machine no longer exists")?;
    cancel_job(&app, &stack.id);
    set_status(&app, &stack.id, |status| status.phase = Phase::Starting);
    run_compose(&app, &stack, &machine.alias(), &stack.compose_cmd("restart"), "compose restart").await
}

/// Mirrors the folder again without touching the containers. Compose picks up
/// bind-mounted files by itself; built images need a Rebuild.
pub async fn resync(app: AppHandle, stack: Stack) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let machine = state.store.machine(&stack.machine_id).context("the machine no longer exists")?;
    let result = sync::run(&state.ssh, &machine.alias(), &stack, |_| {}).await;
    match result {
        Ok(synced) => set_status(&app, &stack.id, |status| {
            status.synced_at_ms = Some(now_ms());
            status.synced_files = synced.files;
            status.message = synced.warning;
        }),
        Err(err) => set_status(&app, &stack.id, |status| status.message = Some(format!("sync failed: {err:#}"))),
    }
    Ok(())
}

/// Re-syncs after every burst of changes in the project folder.
pub fn start_watcher(app: &AppHandle, stack: &Stack) {
    let state = app.state::<AppState>();
    stop_watcher(app, &stack.id);
    let app_for_changes = app.clone();
    let stack_for_changes = stack.clone();
    let on_change = move || {
        let _ = tauri::async_runtime::block_on(resync(app_for_changes.clone(), stack_for_changes.clone()));
    };
    match sync::watch(PathBuf::from(&stack.project_dir), stack.excludes.clone(), on_change) {
        Ok(watcher) => {
            state.watchers.lock().expect("watchers lock").insert(stack.id.clone(), watcher);
        }
        Err(err) => set_status(app, &stack.id, |status| status.message = Some(format!("could not watch the folder: {err:#}"))),
    }
}

pub fn stop_watcher(app: &AppHandle, stack_id: &str) {
    let state = app.state::<AppState>();
    state.watchers.lock().expect("watchers lock").remove(stack_id);
}

/// `compose logs -f`, streamed as `stack:log` events until `logs_stop`.
pub fn logs_start(app: &AppHandle, stack: &Stack) -> anyhow::Result<()> {
    logs_stop(app, &stack.id);
    let state = app.state::<AppState>();
    let machine = state.store.machine(&stack.machine_id).context("the machine no longer exists")?;
    let sink = {
        let app = app.clone();
        let stack_id = stack.id.clone();
        move |line| {
            let _ = app.emit(
                LOG_EVENT,
                OutputEvent {
                    stack_id: stack_id.clone(),
                    line,
                },
            );
        }
    };
    let job = state.ssh.job(&machine.alias(), &stack.compose_cmd("logs -f --tail 200 --no-color"), sink)?;
    let handle = job.handle();
    let key = logs_key(&stack.id);
    state.jobs.lock().expect("jobs lock").insert(key.clone(), handle.clone());
    // Reap the entry when the stream ends by itself; a cancelled one was
    // already removed by whoever cancelled it.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _ = job.wait().await;
        if !handle.is_cancelled() {
            app.state::<AppState>().jobs.lock().expect("jobs lock").remove(&key);
        }
    });
    Ok(())
}

pub fn logs_stop(app: &AppHandle, stack_id: &str) {
    cancel_job(app, &logs_key(stack_id));
}

fn logs_key(stack_id: &str) -> String {
    format!("{stack_id}:logs")
}

/// Stops the stack, deletes its folder on the machine, forgets it here. Each
/// remote step is best effort: the machine may be switched off.
pub async fn remove(app: AppHandle, stack: Stack, remove_volumes: bool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    logs_stop(&app, &stack.id);
    stop_watcher(&app, &stack.id);
    if let Some(machine) = state.store.machine(&stack.machine_id) {
        let _ = down(app.clone(), stack.clone(), remove_volumes).await;
        let _ = state.ssh.run(&machine.alias(), &format!("rm -rf {}", shell_quote(&stack.remote_dir()))).await;
    }
    forward::stop(&app, &stack.id);
    let mut stacks = state.store.stacks();
    stacks.retain(|s| s.id != stack.id);
    state.store.save_stacks(stacks)?;
    state.statuses.lock().expect("statuses lock").remove(&stack.id);
    Ok(())
}

/// Runs one compose command with streamed output, then refreshes the phase
/// from `ps`. A cancelled job leaves the status to whoever cancelled it.
async fn run_compose(app: &AppHandle, stack: &Stack, alias: &str, script: &str, label: &str) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let job = state.ssh.job(alias, script, output_sink(app, &stack.id))?;
    let handle = job.handle();
    state.jobs.lock().expect("jobs lock").insert(stack.id.clone(), handle.clone());
    let code = job.wait().await;
    state.jobs.lock().expect("jobs lock").remove(&stack.id);
    if handle.is_cancelled() {
        return Ok(());
    }
    let code = code?;
    if code != Some(0) {
        set_status(app, &stack.id, |status| {
            status.phase = Phase::Error;
            status.message = Some(format!("{label} exited with code {}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())));
        });
        anyhow::bail!("{label} failed");
    }
    refresh(app, stack, alias).await;
    Ok(())
}

/// The card catches up with `ps` right away instead of at the next poll.
pub async fn refresh_now(app: &AppHandle, stack: &Stack) {
    let machine = app.state::<AppState>().store.machine(&stack.machine_id);
    if let Some(machine) = machine {
        refresh(app, stack, &machine.alias()).await;
    }
}

async fn refresh(app: &AppHandle, stack: &Stack, alias: &str) {
    let state = app.state::<AppState>();
    let script = stack.compose_cmd("ps --all --format json");
    let services = match state.ssh.run(alias, &script).await {
        Ok(out) if out.ok() => compose::parse_ps(&out.stdout),
        _ => return,
    };
    forward::reconcile(app, stack, &services);
    set_status(app, &stack.id, |status| {
        status.phase = derive_phase(&services);
        status.services = services;
        status.known = true;
    });
}

fn cancel_job(app: &AppHandle, stack_id: &str) {
    let state = app.state::<AppState>();
    let running = state.jobs.lock().expect("jobs lock").remove(stack_id);
    if let Some(job) = running {
        job.cancel();
    }
}
