//! A stack is one compose project that lives on one machine. This module
//! holds what a stack is and the status the cards show; `lifecycle` runs the
//! operations (sync, up, down, logs, remove), `operation` lets one run at a
//! time, and `poll` keeps every card honest between operations.

mod lifecycle;
mod operation;
mod poll;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::compose::{Readiness, ServiceState};
use crate::job::Line;
use crate::AppState;

pub use lifecycle::{down, logs_start, logs_stop, refresh_now, remove, restart, resync, start_watcher, stop, stop_watcher, up};
pub use operation::{Operations, Ticket, COPY_RUNNING};
pub use poll::spawn_status_loop;

pub const STATUS_EVENT: &str = "stack:status";
pub const OUTPUT_EVENT: &str = "stack:output";
pub const LOG_EVENT: &str = "stack:log";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stack {
    pub id: String,
    pub name: String,
    pub machine_id: String,
    pub project_dir: String,
    pub compose_rel: String,
    pub excludes: Vec<String>,
    pub forward_ports: bool,
    pub live_sync: bool,
    /// Published port on the machine to the port used on this computer, when they differ.
    #[serde(default)]
    pub port_overrides: HashMap<u16, u16>,
}

impl Stack {
    pub fn new_id() -> String {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
    }

    /// Relative to the remote home, on purpose: rsync and `cd` both resolve it.
    pub fn remote_dir(&self) -> String {
        format!(".dockernanny/{}", self.name)
    }

    /// Always `-f` and `-p`, so override files and the project name resolve the
    /// same way every time, whoever runs the command.
    pub fn compose_cmd(&self, args: &str) -> String {
        compose_script(&self.remote_dir(), &self.compose_rel, &self.name, args)
    }
}

/// Start: `up -d` builds only missing images, as `docker compose up` does;
/// Rebuild adds `--build`. Shared with the example the smoke test runs, so
/// the test runs what the app runs.
pub fn up_args(rebuild: bool) -> &'static str {
    if rebuild {
        "up -d --build --remove-orphans"
    } else {
        "up -d --remove-orphans"
    }
}

/// Remove containers; with `volumes`, their data goes too.
pub fn down_args(volumes: bool) -> &'static str {
    if volumes {
        "down --remove-orphans --volumes"
    } else {
        "down --remove-orphans"
    }
}

/// The compose command for a machine's login shell: `cd` into the stack's
/// folder, then `docker compose -f <file> -p <name> <args>`.
pub fn compose_script(dir: &str, compose_rel: &str, name: &str, args: &str) -> String {
    format!("cd {} && docker compose -f {} -p {} {}", shell_quote(dir), shell_quote(compose_rel), shell_quote(name), args)
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

/// What the card says about the stack. Syncing, Migrating, Starting and
/// Stopping are set by an operation while it runs; the rest are read from
/// `compose ps` (`derive_phase`).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Syncing,
    Migrating,
    Starting,
    /// Up, while a health check has not passed yet.
    Waiting,
    Running,
    Partial,
    Stopped,
    Stopping,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct StackStatus {
    pub phase: Phase,
    pub services: Vec<ServiceState>,
    /// Why the last operation failed; cleared when the next one starts.
    /// Only operations write it, never the poll.
    pub error: Option<String>,
    /// What the last folder sync could not do (a partial rsync, a folder
    /// that cannot be watched); written by the syncs only.
    pub sync_warning: Option<String>,
    pub synced_at_ms: Option<u64>,
    pub synced_files: u32,
    /// False until `compose ps` has answered at least once.
    pub known: bool,
    /// The project folder is gone from the machine; Start copies it again.
    #[serde(default)]
    pub folder_missing: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatusEvent {
    pub stack_id: String,
    pub status: StackStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutputEvent {
    pub stack_id: String,
    pub lines: Vec<Line>,
}

/// The stack as a whole, by Compose's own readiness rules (`compose::ps`):
/// a one-shot job that finished counts as done, a health check that has not
/// passed yet as waiting, and only an unhealthy service or one stopped
/// while the rest runs makes the stack partly running.
fn derive_phase(services: &[ServiceState]) -> Phase {
    let up = services.iter().filter(|s| matches!(s.readiness, Readiness::Ready | Readiness::Starting | Readiness::Unhealthy)).count();
    if up == 0 {
        return Phase::Stopped;
    }
    let something_wrong = services.iter().any(|s| matches!(s.readiness, Readiness::Unhealthy | Readiness::Stopped));
    if something_wrong {
        return Phase::Partial;
    }
    let getting_ready = services.iter().any(|s| s.readiness == Readiness::Starting);
    if getting_ready {
        return Phase::Waiting;
    }
    Phase::Running
}

pub fn output_sink(app: &AppHandle, stack_id: &str) -> impl FnMut(Line) + Send + 'static {
    lines_to_window(app, OUTPUT_EVENT, stack_id)
}

/// A stack's lines sent as `event`, in batches (see `job::batched`).
pub fn lines_to_window(app: &AppHandle, event: &'static str, stack_id: &str) -> impl FnMut(Line) + Send + 'static {
    let app = app.clone();
    let stack_id = stack_id.to_string();
    crate::job::batched(move |lines| {
        let _ = app.emit(event, OutputEvent { stack_id: stack_id.clone(), lines });
    })
}

/// Applies a change and publishes the result, but only when something changed.
pub fn set_status(app: &AppHandle, stack_id: &str, change: impl FnOnce(&mut StackStatus)) {
    let state = app.state::<AppState>();
    let updated = {
        let mut statuses = state.statuses.lock().expect("statuses lock");
        let status = statuses.entry(stack_id.to_string()).or_default();
        let before = status.clone();
        change(status);
        (before != *status).then(|| status.clone())
    };
    if let Some(status) = updated {
        let _ = app.emit(STATUS_EVENT, StatusEvent { stack_id: stack_id.to_string(), status });
    }
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_survives_single_quotes() {
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }

    fn service(name: &str, readiness: Readiness) -> ServiceState {
        ServiceState { service: name.into(), readiness, ..ServiceState::default() }
    }

    #[test]
    fn the_phase_follows_composes_readiness() {
        use Readiness::*;
        assert_eq!(derive_phase(&[]), Phase::Stopped);
        assert_eq!(derive_phase(&[service("a", Ready), service("b", Ready)]), Phase::Running);
        assert_eq!(derive_phase(&[service("app", Ready), service("setup", Done)]), Phase::Running, "a finished setup job is fine");
        assert_eq!(derive_phase(&[service("app", Ready), service("scanner", Starting)]), Phase::Waiting, "a health check not passed yet");
        assert_eq!(derive_phase(&[service("app", Ready), service("db", Unhealthy)]), Phase::Partial);
        assert_eq!(derive_phase(&[service("app", Ready), service("worker", Stopped)]), Phase::Partial);
        assert_eq!(derive_phase(&[service("a", Stopped), service("setup", Done)]), Phase::Stopped);
    }

    #[test]
    fn start_and_remove_run_the_documented_commands() {
        assert_eq!(up_args(false), "up -d --remove-orphans", "Start builds only what is missing");
        assert_eq!(up_args(true), "up -d --build --remove-orphans");
        assert_eq!(down_args(false), "down --remove-orphans", "the data stays");
        assert_eq!(down_args(true), "down --remove-orphans --volumes");
    }
}
