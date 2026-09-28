//! A stack is one compose project that lives on one machine. This module
//! holds what a stack is and the status the cards show; `lifecycle` runs the
//! operations (sync, up, down, logs, remove) and `poll` keeps every card
//! honest between operations.

mod lifecycle;
mod poll;

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::compose::ServiceState;
use crate::job::Line;
use crate::AppState;

pub use lifecycle::{down, logs_start, logs_stop, refresh_now, remove, restart, resync, start_watcher, stop, stop_watcher, up};
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

/// The compose command for a machine's login shell: `cd` into the stack's
/// folder, then `docker compose -f <file> -p <name> <args>`.
pub fn compose_script(dir: &str, compose_rel: &str, name: &str, args: &str) -> String {
    format!("cd {} && docker compose -f {} -p {} {}", shell_quote(dir), shell_quote(compose_rel), shell_quote(name), args)
}

pub fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Idle,
    Syncing,
    Migrating,
    Starting,
    Running,
    Partial,
    Stopped,
    Stopping,
    Error,
}

impl Phase {
    /// While an operation runs, the poll must not overwrite the phase.
    fn busy(self) -> bool {
        matches!(self, Phase::Syncing | Phase::Migrating | Phase::Starting | Phase::Stopping)
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct StackStatus {
    pub phase: Phase,
    pub services: Vec<ServiceState>,
    pub message: Option<String>,
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

fn derive_phase(services: &[ServiceState]) -> Phase {
    let running = services.iter().filter(|s| s.state == "running").count();
    if services.is_empty() || running == 0 {
        return Phase::Stopped;
    }
    if running == services.len() {
        return Phase::Running;
    }
    Phase::Partial
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
        let _ = app.emit(
            STATUS_EVENT,
            StatusEvent {
                stack_id: stack_id.to_string(),
                status,
            },
        );
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

    #[test]
    fn phase_follows_service_states() {
        let running = |s: &str| ServiceState { service: s.into(), container: String::new(), state: "running".into(), health: String::new(), exit_code: 0, ports: vec![] };
        let exited = |s: &str| ServiceState { state: "exited".into(), ..running(s) };
        assert_eq!(derive_phase(&[]), Phase::Stopped);
        assert_eq!(derive_phase(&[running("a"), running("b")]), Phase::Running);
        assert_eq!(derive_phase(&[running("a"), exited("b")]), Phase::Partial);
        assert_eq!(derive_phase(&[exited("a")]), Phase::Stopped);
    }
}
