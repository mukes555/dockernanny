//! One `ps` round trip per online machine every few seconds, all of that
//! machine's stacks in a single script separated by markers. The result
//! updates the services on every card and re-checks the port forwards.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use tauri::{AppHandle, Manager};
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::{derive_phase, set_status, Stack};
use crate::compose;
use crate::ssh::Ssh;
use crate::{forward, AppState};

const POLL_EVERY: Duration = Duration::from_secs(3);
/// While nobody can see the window; the bridges keep themselves up meanwhile.
const POLL_EVERY_HIDDEN: Duration = Duration::from_secs(15);
const POLL_TIMEOUT: Duration = Duration::from_secs(15);

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
                    let out = timeout(POLL_TIMEOUT, ssh.run(&machine.alias(), &ps_script(&stacks))).await;
                    (stacks, out)
                });
            }
            while let Some(Ok((stacks, out))) = polls.join_next().await {
                // Unreachable right now: keep the last known picture.
                let Ok(Ok(out)) = out else { continue };
                let sections = split_by_marker(&out.stdout);
                for stack in stacks {
                    let section = sections.get(&stack.id).map(String::as_str).unwrap_or("");
                    let services = compose::parse_ps(section);
                    let missing = folder_is_missing(section);
                    forward::reconcile(&app, &stack, &services);
                    set_status(&app, &stack.id, |status| {
                        status.services = services;
                        status.known = true;
                        status.folder_missing = missing;
                        if !status.phase.busy() {
                            status.phase = derive_phase(&status.services);
                        }
                    });
                }
            }
            crate::tray::until_next_poll(&app, POLL_EVERY, POLL_EVERY_HIDDEN).await;
        }
    });
}

/// A stack whose folder is gone from the machine says so, instead of
/// looking merely stopped.
const NO_FOLDER: &str = "dockernanny-no-folder";

fn ps_script(stacks: &[Stack]) -> String {
    let mut script = String::new();
    for stack in stacks {
        let dir = crate::stack::shell_quote(&stack.remote_dir());
        script.push_str(&format!(
            "echo '=== {}'; [ -d {dir} ] || echo {NO_FOLDER}; ( {} 2>/dev/null ); ",
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
        let text = "=== aaa\n{\"Service\":\"web\"}\n=== bbb\n=== ccc\ndockernanny-no-folder\n";
        let sections = split_by_marker(text);
        assert_eq!(sections["aaa"].trim(), "{\"Service\":\"web\"}");
        assert_eq!(sections["bbb"].trim(), "");
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
    }
}
