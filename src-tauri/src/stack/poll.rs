//! One `ps` round trip per online machine every few seconds, all of that
//! machine's stacks in a single script separated by markers. The result
//! updates the services on every card and re-checks the port forwards.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::task::JoinSet;

use super::{derive_phase, now_ms, set_status, Stack};
use crate::compose::{self, ServiceState};
use crate::ssh::Ssh;
use crate::{forward, AppState};

const POLL_EVERY: Duration = Duration::from_secs(3);
/// While nobody can see the window; the bridges keep themselves up meanwhile.
const POLL_EVERY_HIDDEN: Duration = Duration::from_secs(15);
pub const POLL_TIMEOUT: Duration = Duration::from_secs(15);

pub fn spawn_status_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            let state = app.state::<AppState>();
            let online: HashSet<String> =
                state.stats.lock().expect("stats lock").iter().filter(|(_, stats)| stats.online).map(|(id, _)| id.clone()).collect();
            let mut by_machine: HashMap<String, Vec<Stack>> = HashMap::new();
            for stack in state.store.stacks() {
                if online.contains(&stack.machine_id) {
                    by_machine.entry(stack.machine_id.clone()).or_default().push(stack);
                }
            }

            let mut polls = JoinSet::new();
            for (machine_id, stacks) in by_machine {
                let Some(machine) = state.store.machine(&machine_id) else { continue };
                let ssh: Ssh = state.ssh.clone();
                polls.spawn(async move {
                    let started_ms = now_ms();
                    let out = ssh.run_poll(&machine.alias(), &ps_script(&stacks), POLL_TIMEOUT).await;
                    (stacks, out, started_ms)
                });
            }
            while let Some(joined) = polls.join_next().await {
                // A poll task that died must not drop the other machines' answers.
                let Ok((stacks, out, started_ms)) = joined else { continue };
                // Unreachable right now: keep the last known picture.
                let Ok(out) = out else { continue };
                let sections = split_by_marker(&out.stdout);
                for stack in stacks {
                    let section = sections.get(&stack.id).map(String::as_str).unwrap_or("");
                    let missing = folder_is_missing(section);
                    // Docker down or a broken compose file: `ps` failed, which is not "stopped".
                    if !missing && !ps_answered(section) {
                        continue;
                    }
                    apply_ps(&app, &stack.id, compose::parse_ps(section), missing, started_ms);
                }
            }
            crate::tray::until_next_poll(&app, POLL_EVERY, POLL_EVERY_HIDDEN).await;
        }
    });
}

/// What one `compose ps` said about a stack, applied the same way by the
/// poll and after an operation. A stack removed meanwhile is not brought
/// back, while an operation holds the stack its phase is left to it, and a
/// reading that started before the last operation ended is dropped.
pub fn apply_ps(app: &AppHandle, stack_id: &str, services: Vec<ServiceState>, folder_missing: bool, started_ms: u64) {
    let state = app.state::<AppState>();
    let Some(stack) = state.store.stack(stack_id) else { return };
    if state.operations.ended_since(stack_id, started_ms) {
        return;
    }
    forward::reconcile(app, &stack, &services);
    let held = state.operations.holds(stack_id);
    set_status(app, stack_id, |status| {
        status.services = services;
        status.known = true;
        status.folder_missing = folder_missing;
        if !held {
            status.phase = derive_phase(&status.services);
        }
    });
}

/// A stack whose folder is gone from the machine says so, instead of
/// looking merely stopped.
const NO_FOLDER: &str = "dockernanny-no-folder";
/// After each `ps`, its exit code, so a failed one is not read as no containers.
const PS_EXIT: &str = "dockernanny-ps-exit";

fn ps_script(stacks: &[Stack]) -> String {
    let mut script = String::new();
    for stack in stacks {
        let dir = crate::stack::shell_quote(&stack.remote_dir());
        script.push_str(&format!(
            "echo '=== {}'; [ -d {dir} ] || echo {NO_FOLDER}; ( {} 2>/dev/null ); echo \"{PS_EXIT} $?\"; ",
            stack.id,
            stack.compose_cmd("ps --all --format json")
        ));
    }
    script.push_str("true");
    script
}

fn folder_is_missing(section: &str) -> bool {
    section.lines().any(|line| line.trim() == NO_FOLDER)
}

fn ps_answered(section: &str) -> bool {
    section.lines().any(|line| line.trim() == format!("{PS_EXIT} 0"))
}

fn split_by_marker(text: &str) -> HashMap<String, String> {
    let mut sections: HashMap<String, String> = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(id) = line.strip_prefix("=== ") {
            let id = id.trim().to_string();
            // A stack with no containers still gets an (empty) section.
            sections.entry(id.clone()).or_default();
            current = Some(id);
            continue;
        }
        if let Some(id) = &current {
            let section = sections.entry(id.clone()).or_default();
            section.push_str(line);
            section.push('\n');
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markers_split_per_stack() {
        let text = "=== aaa\n{\"Service\":\"web\"}\ndockernanny-ps-exit 0\n=== bbb\ndockernanny-ps-exit 1\n=== ccc\ndockernanny-no-folder\ndockernanny-ps-exit 1\n";
        let sections = split_by_marker(text);
        assert_eq!(compose::parse_ps(&sections["aaa"]).len(), 1, "the exit line is not a service");
        assert!(ps_answered(&sections["aaa"]));
        assert!(!ps_answered(&sections["bbb"]), "a failed ps keeps the last picture");
        assert!(!folder_is_missing(&sections["aaa"]));
        assert!(folder_is_missing(&sections["ccc"]), "a stack whose folder is gone says so");
        assert!(compose::parse_ps(&sections["ccc"]).is_empty(), "the marker is not a service");
    }

    #[test]
    fn the_script_checks_the_folder_before_asking_compose() {
        let stack = Stack {
            id: "s1".into(),
            name: "shop".into(),
            machine_id: "m".into(),
            project_dir: "/p".into(),
            compose_rel: "compose.yaml".into(),
            excludes: vec![],
            forward_ports: true,
            live_sync: false,
            port_overrides: HashMap::new(),
        };
        let script = ps_script(std::slice::from_ref(&stack));
        assert!(
            script.starts_with(
                "echo '=== s1'; [ -d '.dockernanny/shop' ] || echo dockernanny-no-folder; ( cd '.dockernanny/shop' && docker compose"
            ),
            "{script}"
        );
        assert!(script.contains("echo \"dockernanny-ps-exit $?\""));
    }
}
