//! The operations on a stack: sync, up, down, stop, restart, live sync, logs,
//! remove. Each compose operation holds a `Ticket`, so one runs at a time and
//! a newer one ends it; long ones stream their output as events. An
//! operation that fails returns its error, which the command writes on the
//! card (`commands::spawn_operation`).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use tauri::{AppHandle, Manager};

use super::poll::{self, POLL_TIMEOUT};
use super::{down_args, lines_to_window, now_ms, output_sink, set_status, shell_quote, up_args, Phase, Stack, Ticket, LOG_EVENT};
use crate::compose;
use crate::job::LastError;
use crate::{forward, sync, AppState};

/// Sync the folder, then `up -d` (`up_args`). A newer operation started
/// while the folder synced (Stop clicked meanwhile) wins: up is not run.
pub async fn up(app: AppHandle, stack: Stack, rebuild: bool) -> anyhow::Result<()> {
    let ticket = Ticket::begin(&app, &stack.id)?;
    let result = sync_and_up(&app, &ticket, &stack, rebuild).await;
    finish(&app, ticket, &stack).await;
    result
}

async fn sync_and_up(app: &AppHandle, ticket: &Ticket, stack: &Stack, rebuild: bool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let alias = machine_alias(app, stack)?;
    ticket.set_status(|status| {
        status.phase = Phase::Syncing;
        status.error = None;
    });
    let synced = sync::run(&state.ssh, &alias, stack, output_sink(app, &stack.id)).await.context("sync failed")?;
    ticket.set_status(|status| {
        status.phase = Phase::Starting;
        status.synced_at_ms = Some(now_ms());
        status.synced_files = synced.files;
        status.sync_warning = synced.warning;
    });
    if !ticket.is_current() {
        return Ok(());
    }
    run_compose(app, ticket, stack, &alias, &stack.compose_cmd(up_args(rebuild)), "compose up").await
}

/// Remove containers (`down_args`), keeping the data unless `remove_volumes`.
pub async fn down(app: AppHandle, stack: Stack, remove_volumes: bool) -> anyhow::Result<()> {
    compose_operation(app, stack, Phase::Stopping, down_args(remove_volumes), "compose down").await
}

/// `compose stop`: the containers stay, so Start brings them back quickly.
pub async fn stop(app: AppHandle, stack: Stack) -> anyhow::Result<()> {
    compose_operation(app, stack, Phase::Stopping, "stop", "compose stop").await
}

pub async fn restart(app: AppHandle, stack: Stack) -> anyhow::Result<()> {
    compose_operation(app, stack, Phase::Starting, "restart", "compose restart").await
}

/// One compose command as an operation: the phase while it runs, the
/// command, then the card as `ps` sees it.
async fn compose_operation(app: AppHandle, stack: Stack, phase: Phase, args: &str, label: &str) -> anyhow::Result<()> {
    let ticket = Ticket::begin(&app, &stack.id)?;
    let result = async {
        let alias = machine_alias(&app, &stack)?;
        ticket.set_status(|status| {
            status.phase = phase;
            status.error = None;
        });
        run_compose(&app, &ticket, &stack, &alias, &stack.compose_cmd(args), label).await
    }
    .await;
    finish(&app, ticket, &stack).await;
    result
}

/// Mirrors the folder again without touching the containers. Compose picks up
/// bind-mounted files by itself; built images need a Rebuild. Skipped while an
/// operation holds the stack: its own sync mirrors the folder, and two rsyncs
/// into one folder with --delete would remove each other's temporary files.
pub async fn resync(app: AppHandle, stack: Stack) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    if state.operations.holds(&stack.id) {
        return Ok(());
    }
    let alias = machine_alias(&app, &stack)?;
    let result = sync::run(&state.ssh, &alias, &stack, |_| {}).await;
    match result {
        Ok(synced) => set_status(&app, &stack.id, |status| {
            status.synced_at_ms = Some(now_ms());
            status.synced_files = synced.files;
            status.sync_warning = synced.warning;
        }),
        Err(err) => set_status(&app, &stack.id, |status| status.sync_warning = Some(format!("sync failed: {err:#}"))),
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
        Err(err) => set_status(app, &stack.id, |status| status.sync_warning = Some(format!("could not watch the folder: {err:#}"))),
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
    let alias = machine_alias(app, stack)?;
    let sink = lines_to_window(app, LOG_EVENT, &stack.id);
    let job = state.ssh.job(&alias, &stack.compose_cmd("logs -f --tail 200 --no-color"), sink)?;
    let handle = job.handle();
    let key = logs_key(&stack.id);
    state.jobs.lock().expect("jobs lock").insert(key.clone(), handle.clone());
    // A stream that ends by itself takes its entry with it, but never a newer stream's.
    let app = app.clone();
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

pub fn logs_stop(app: &AppHandle, stack_id: &str) {
    let state = app.state::<AppState>();
    let running = state.jobs.lock().expect("jobs lock").remove(&logs_key(stack_id));
    if let Some(job) = running {
        job.cancel();
    }
}

fn logs_key(stack_id: &str) -> String {
    format!("{stack_id}:logs")
}

/// Stops the stack, deletes its folder on the machine, forgets it here. Each
/// remote step is best effort: the machine may be switched off. Refused
/// while a copy runs, which cannot be stopped half way.
pub async fn remove(app: AppHandle, stack: Stack, remove_volumes: bool) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    anyhow::ensure!(!state.operations.copying(&stack.id), super::COPY_RUNNING);
    logs_stop(&app, &stack.id);
    stop_watcher(&app, &stack.id);
    if let Some(machine) = state.store.machine(&stack.machine_id) {
        let _ = down(app.clone(), stack.clone(), remove_volumes).await;
        // A project folder with node_modules can take a while to delete.
        let remove_folder = format!("rm -rf {}", shell_quote(&stack.remote_dir()));
        let _ = state.ssh.run_within(&machine.alias(), &remove_folder, std::time::Duration::from_secs(600)).await;
    }
    forward::stop(&app, &stack.id);
    let mut stacks = state.store.stacks();
    stacks.retain(|s| s.id != stack.id);
    state.store.save_stacks(stacks)?;
    state.statuses.lock().expect("statuses lock").remove(&stack.id);
    Ok(())
}

fn machine_alias(app: &AppHandle, stack: &Stack) -> anyhow::Result<String> {
    let machine = app.state::<AppState>().store.machine(&stack.machine_id).context("the machine no longer exists")?;
    Ok(machine.alias())
}

/// Runs one compose command with streamed output. A newer operation ends it
/// and takes over the card, so this one then reports nothing. A failure
/// carries the last error line compose printed, so the card says why
/// without a look at the output.
async fn run_compose(app: &AppHandle, ticket: &Ticket, stack: &Stack, alias: &str, script: &str, label: &str) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let last_error = Arc::new(Mutex::new(LastError::default()));
    let noted = last_error.clone();
    let mut sink = output_sink(app, &stack.id);
    let job = state.ssh.job(alias, script, move |line| {
        noted.lock().expect("last error lock").note(&line);
        sink(line);
    })?;
    ticket.attach(job.handle());
    let code = job.wait().await;
    if !ticket.is_current() {
        return Ok(());
    }
    let code = code?;
    if code != Some(0) {
        let why = last_error.lock().expect("last error lock").explain(code);
        anyhow::bail!("{label} failed: {why}");
    }
    Ok(())
}

/// The operation is over. Unless a newer one took the stack meanwhile, the
/// card shows what `ps` says now; when the machine does not answer, the
/// phase comes from the services last seen, so it never stays on "starting".
async fn finish(app: &AppHandle, ticket: Ticket, stack: &Stack) {
    let superseded = !ticket.is_current();
    drop(ticket);
    if superseded {
        return;
    }
    let answered = refresh_now(app, stack).await;
    if !answered && !app.state::<AppState>().operations.holds(&stack.id) {
        set_status(app, &stack.id, |status| status.phase = super::derive_phase(&status.services));
    }
}

/// The card catches up with `ps` right away instead of at the next poll.
/// False when the machine did not answer.
pub async fn refresh_now(app: &AppHandle, stack: &Stack) -> bool {
    let state = app.state::<AppState>();
    let Some(machine) = state.store.machine(&stack.machine_id) else { return false };
    let script = stack.compose_cmd("ps --all --format json");
    let started_ms = now_ms();
    let services = match state.ssh.run_poll(&machine.alias(), &script, POLL_TIMEOUT).await {
        Ok(out) if out.ok() => compose::parse_ps(&out.stdout),
        _ => return false,
    };
    poll::apply_ps(app, &stack.id, services, false, started_ms);
    true
}
