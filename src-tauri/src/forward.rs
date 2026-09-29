//! Keeps `localhost:<port>` on this computer pointing at a stack's published ports
//! on its machine: one `ssh -N` per stack holding every `-L`, restarted with
//! backoff when it dies and replaced when the port set changes. The process
//! is its own control master, so a forwarder left behind by a crash can be
//! told to exit at the next start.

use std::collections::HashMap;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Child;
use tokio::sync::watch;

use crate::compose::ServiceState;
use crate::ssh::Ssh;
use crate::stack::Stack;
use crate::{tools, AppState};

pub const FORWARD_EVENT: &str = "forward:state";
const LISTEN_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct ForwardPort {
    pub local: u16,
    pub remote: u16,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct ForwardState {
    pub up: bool,
    pub ports: Vec<ForwardPort>,
    pub error: Option<String>,
    /// Consecutive failed attempts; zero once the forwards are up.
    pub attempts: u32,
    /// When the forwards came up; None while they are down.
    pub since_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ForwardEvent {
    pub stack_id: String,
    pub state: ForwardState,
}

/// A running forwarder: what it forwards, how to stop it, and its task, so a
/// replacement can wait until it has fully stopped.
pub struct Forwarder {
    pub ports: Vec<ForwardPort>,
    cancel: watch::Sender<bool>,
    task: tauri::async_runtime::JoinHandle<()>,
}

/// Ports that something on this computer already listens on. Both address families
/// are tried: Node and browsers resolve `localhost` to `::1` first.
pub fn busy_ports(ports: &[u16]) -> Vec<u16> {
    ports.iter().copied().filter(|port| !can_bind(*port)).collect()
}

fn can_bind(port: u16) -> bool {
    let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    TcpListener::bind(v4).is_ok() && TcpListener::bind(v6).is_ok()
}

/// The forwards a stack should have right now: every TCP port of a running
/// service, mapped through the stack's overrides. A service that is
/// restarting publishes nothing for a moment; its ports stay bridged (as
/// `kept`, what is bridged now) so localhost does not flap while a
/// container comes back.
pub fn desired_ports(stack: &Stack, services: &[ServiceState], kept: &[ForwardPort]) -> Vec<ForwardPort> {
    if !stack.forward_ports {
        return Vec::new();
    }
    let mut ports = Vec::new();
    for service in services.iter().filter(|s| s.state == "running") {
        for port in service.ports.iter().filter(|p| p.protocol == "tcp") {
            let local = stack.port_overrides.get(&port.published).copied().unwrap_or(port.published);
            let forward = ForwardPort { local, remote: port.published };
            if !ports.contains(&forward) {
                ports.push(forward);
            }
        }
    }
    let restarting = services.iter().any(|s| s.state == "restarting");
    if restarting {
        let still_bridged: Vec<ForwardPort> = kept.iter().filter(|port| !ports.contains(port)).cloned().collect();
        ports.extend(still_bridged);
    }
    ports.sort();
    ports
}

/// Brings the forwarder in line with what the stack currently publishes.
pub fn reconcile(app: &AppHandle, stack: &Stack, services: &[ServiceState]) {
    let state = app.state::<AppState>();
    let mut forwarders = state.forwarders.lock().expect("forwarders lock");
    let current = forwarders.get(&stack.id).map(|f| f.ports.clone()).unwrap_or_default();
    let desired = desired_ports(stack, services, &current);
    if current == desired {
        return;
    }
    let previous = forwarders.remove(&stack.id).map(|old| {
        let _ = old.cancel.send(true);
        old.task
    });
    if desired.is_empty() {
        publish(app, &stack.id, ForwardState::default());
        return;
    }
    let Some(machine) = state.store.machine(&stack.machine_id) else { return };
    let (cancel, cancelled) = watch::channel(false);
    let task = tauri::async_runtime::spawn(run(app.clone(), stack.id.clone(), machine.alias(), desired.clone(), cancelled, previous));
    forwarders.insert(stack.id.clone(), Forwarder { ports: desired, cancel, task });
}

pub fn stop(app: &AppHandle, stack_id: &str) {
    let state = app.state::<AppState>();
    let removed = state.forwarders.lock().expect("forwarders lock").remove(stack_id);
    if let Some(forwarder) = removed {
        let _ = forwarder.cancel.send(true);
    }
    state.forward_states.lock().expect("forward states lock").remove(stack_id);
}

/// Tells every forwarder that may still be running from an earlier instance
/// to exit. Harmless when there is none. Each is its own process (a wsl.exe
/// start on Windows), and the window waits for them at start, so they run
/// side by side rather than one after another.
pub fn exit_all(ssh: &Ssh, stack_ids: impl Iterator<Item = String>, aliases: &HashMap<String, String>) {
    let stack_ids: Vec<String> = stack_ids.collect();
    std::thread::scope(|scope| {
        for stack_id in &stack_ids {
            scope.spawn(move || {
                let socket = ssh.forward_socket(stack_id);
                if let Some(alias) = aliases.get(stack_id) {
                    ssh.exit_master(alias, Some(&socket));
                }
                tools::remove_file(&socket);
            });
        }
    });
}

async fn run(
    app: AppHandle,
    stack_id: String,
    alias: String,
    ports: Vec<ForwardPort>,
    mut cancelled: watch::Receiver<bool>,
    previous: Option<tauri::async_runtime::JoinHandle<()>>,
) {
    // Both use the same control socket: the old forwarder's final "exit"
    // must not reach this one, so it finishes stopping first.
    if let Some(previous) = previous {
        let _ = previous.await;
    }
    let ssh = app.state::<AppState>().ssh.clone();
    let socket = ssh.forward_socket(&stack_id);
    let mut attempts: u32 = 0;
    loop {
        attempts += 1;
        // A start that fails (WSL not up yet, ssh missing for a moment) is
        // retried like a bridge that died; only a cancel ends the loop.
        let mut child = match spawn_ssh(&ssh, &alias, &socket, &ports) {
            Ok(child) => child,
            Err(err) => {
                publish(
                    &app,
                    &stack_id,
                    ForwardState { up: false, ports: ports.clone(), error: Some(format!("{err:#}")), attempts, since_ms: None },
                );
                if wait_before_retry(attempts, &mut cancelled).await == Wait::Cancelled {
                    publish(&app, &stack_id, ForwardState::default());
                    return;
                }
                continue;
            }
        };
        let last_error = capture_last_line(child.stderr.take());

        let listening = tokio::select! {
            listening = wait_until_listening(&mut child, &ports) => listening,
            _ = cancelled.changed() => { stop_child(&mut child, &ssh, &alias, &socket).await; return; }
        };
        if listening {
            attempts = 0;
            publish(
                &app,
                &stack_id,
                ForwardState { up: true, ports: ports.clone(), error: None, attempts: 0, since_ms: Some(crate::stack::now_ms()) },
            );
        }

        tokio::select! {
            _ = child.wait() => {
                let error = last_error.lock().expect("stderr lock").clone();
                let error = if error.is_empty() { "ssh exited".to_string() } else { error };
                publish(&app, &stack_id, ForwardState { up: false, ports: ports.clone(), error: Some(error), attempts, since_ms: None });
            }
            _ = cancelled.changed() => {
                stop_child(&mut child, &ssh, &alias, &socket).await;
                publish(&app, &stack_id, ForwardState::default());
                return;
            }
        }

        if wait_before_retry(attempts, &mut cancelled).await == Wait::Cancelled {
            publish(&app, &stack_id, ForwardState::default());
            return;
        }
    }
}

#[derive(PartialEq, Eq)]
enum Wait {
    Over,
    Cancelled,
}

/// Exponential backoff so a machine that went to sleep is not hammered.
async fn wait_before_retry(attempts: u32, cancelled: &mut watch::Receiver<bool>) -> Wait {
    let delay = Duration::from_secs(2u64.saturating_pow(attempts.min(5))).min(MAX_BACKOFF);
    tokio::select! {
        _ = tokio::time::sleep(delay) => Wait::Over,
        _ = cancelled.changed() => Wait::Cancelled,
    }
}

fn spawn_ssh(ssh: &Ssh, alias: &str, socket: &str, ports: &[ForwardPort]) -> anyhow::Result<Child> {
    let mut cmd = tools::unix("ssh");
    cmd.arg("-F").arg(ssh.config_path());
    // -N: no remote command. -M/-S: be a control master on our own socket so a
    // stale copy can be asked to exit. ExitOnForwardFailure turns a taken
    // port into a clean exit instead of a half-working session. ControlPersist
    // must be off here: with it, ssh forks a second background master that
    // would keep the forwards alive after this child is killed.
    cmd.args(["-N", "-M", "-S"]).arg(socket).args(["-o", "ExitOnForwardFailure=yes", "-o", "ControlPersist=no"]);
    for port in ports {
        // `localhost` binds both 127.0.0.1 and ::1, which browsers and Node need.
        cmd.arg("-L").arg(format!("localhost:{}:127.0.0.1:{}", port.local, port.remote));
    }
    cmd.arg(alias).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
    let child = cmd.spawn()?;
    tools::track(&child);
    Ok(child)
}

/// True once the first local port accepts a connection, which only happens
/// after ssh has authenticated and set up its listeners.
async fn wait_until_listening(child: &mut Child, ports: &[ForwardPort]) -> bool {
    let Some(first) = ports.first() else { return false };
    let deadline = tokio::time::Instant::now() + LISTEN_TIMEOUT;
    while tokio::time::Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            return false;
        }
        if tokio::net::TcpStream::connect((Ipv4Addr::LOCALHOST, first.local)).await.is_ok() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    false
}

fn capture_last_line(stderr: Option<tokio::process::ChildStderr>) -> Arc<Mutex<String>> {
    let last = Arc::new(Mutex::new(String::new()));
    let Some(stderr) = stderr else { return last };
    let sink = last.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                *sink.lock().expect("stderr lock") = trimmed.to_string();
            }
        }
    });
    last
}

/// Kills the forwarder and, should anything still answer on its socket,
/// asks that to exit too, so the ports are free for the replacement. On
/// Windows the kill only reaches wsl.exe, so the exit request is what ends
/// the ssh inside WSL; it is always sent, and is harmless when nothing answers.
async fn stop_child(child: &mut Child, ssh: &Ssh, alias: &str, socket: &str) {
    let _ = child.start_kill();
    let _ = child.wait().await;
    let _ = tools::unix("ssh")
        .arg("-F")
        .arg(ssh.config_path())
        .arg("-S")
        .arg(socket)
        .args(["-O", "exit", alias])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .await;
    tools::remove_file(socket);
}

fn publish(app: &AppHandle, stack_id: &str, state: ForwardState) {
    let app_state = app.state::<AppState>();
    app_state.forward_states.lock().expect("forward states lock").insert(stack_id.to_string(), state.clone());
    let _ = app.emit(FORWARD_EVENT, ForwardEvent { stack_id: stack_id.to_string(), state });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::Port;

    #[test]
    fn a_held_port_is_busy_and_a_free_one_is_not() {
        let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = holder.local_addr().unwrap().port();
        assert_eq!(busy_ports(&[port]), vec![port]);
        // Port 0 always binds (the OS picks a free one), so this checks the
        // free branch without racing other tests for a just-released port.
        assert!(busy_ports(&[0]).is_empty());
    }

    #[test]
    fn desired_ports_follow_running_tcp_services_and_overrides() {
        let mut stack = Stack {
            id: "s".into(),
            name: "s".into(),
            machine_id: "m".into(),
            project_dir: "/p".into(),
            compose_rel: "docker-compose.yml".into(),
            excludes: vec![],
            forward_ports: true,
            live_sync: false,
            port_overrides: HashMap::from([(5432, 6432)]),
        };
        let service = |state: &str, ports: Vec<(u16, &str)>| ServiceState {
            service: "x".into(),
            container: String::new(),
            state: state.into(),
            health: String::new(),
            exit_code: 0,
            ports: ports.into_iter().map(|(p, proto)| Port { target: p, published: p, protocol: proto.into() }).collect(),
        };
        let services = vec![service("running", vec![(3000, "tcp"), (5432, "tcp"), (9099, "udp")]), service("exited", vec![(4000, "tcp")])];
        let bridged = vec![ForwardPort { local: 3000, remote: 3000 }, ForwardPort { local: 6432, remote: 5432 }];
        assert_eq!(desired_ports(&stack, &services, &[]), bridged);
        stack.forward_ports = false;
        assert!(desired_ports(&stack, &services, &bridged).is_empty());
    }

    #[test]
    fn a_restarting_service_keeps_its_bridge() {
        let stack = Stack {
            id: "s".into(),
            name: "s".into(),
            machine_id: "m".into(),
            project_dir: "/p".into(),
            compose_rel: "compose.yaml".into(),
            excludes: vec![],
            forward_ports: true,
            live_sync: false,
            port_overrides: HashMap::new(),
        };
        let service = |name: &str, state: &str, ports: Vec<u16>| ServiceState {
            service: name.into(),
            container: String::new(),
            state: state.into(),
            health: String::new(),
            exit_code: 0,
            ports: ports.into_iter().map(|p| Port { target: p, published: p, protocol: "tcp".into() }).collect(),
        };
        let bridged = vec![ForwardPort { local: 3000, remote: 3000 }, ForwardPort { local: 5432, remote: 5432 }];
        // The database restarts: it publishes nothing for a moment, and its port stays bridged.
        let mid_restart = vec![service("api", "running", vec![3000]), service("db", "restarting", vec![])];
        assert_eq!(desired_ports(&stack, &mid_restart, &bridged), bridged);
        // Stopped for real: the bridge follows.
        let stopped = vec![service("api", "running", vec![3000]), service("db", "exited", vec![])];
        assert_eq!(desired_ports(&stack, &stopped, &bridged), vec![ForwardPort { local: 3000, remote: 3000 }]);
    }
}
