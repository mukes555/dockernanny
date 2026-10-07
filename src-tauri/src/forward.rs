//! Keeps `localhost:<port>` on this computer pointing at a stack's published
//! ports on its machine: one `ssh -N` per stack holding every `-L`, restarted
//! with backoff when it dies. The process is its own control master, and
//! ssh's control commands do the rest: `-O check` says when it is up,
//! `-O forward` and `-O cancel` change its ports in place (connections
//! through the other ports stay open), and `-O exit` ends one left behind.

use std::io::ErrorKind;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, ChildStderr};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::compose::ServiceState;
use crate::ssh::Ssh;
use crate::stack::Stack;
use crate::{tools, AppState};

pub const FORWARD_EVENT: &str = "forward:state";
/// ConnectTimeout is 5 s; a login that has not finished well after that will not.
const READY_TIMEOUT: Duration = Duration::from_secs(12);
const CHECK_EVERY: Duration = Duration::from_millis(250);
/// One control request; the master answers at once or not at all.
const CONTROL_LIMIT: Duration = Duration::from_secs(10);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// Each ssh gets a socket of its own, so a request meant for an old one
/// (its final `-O exit`) can never reach its successor.
static NEXT_SOCKET: AtomicU64 = AtomicU64::new(1);

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

/// A stack's bridge as the app holds it: the ports it should carry, which
/// its task follows without a restart. Dropping it ends the bridge.
pub struct Forwarder {
    wanted: watch::Sender<Vec<ForwardPort>>,
}

/// Ports that something on this computer already listens on. Both address families
/// are tried: Node and browsers resolve `localhost` to `::1` first.
pub fn busy_ports(ports: &[u16]) -> Vec<u16> {
    ports.iter().copied().filter(|port| !can_bind(*port)).collect()
}

fn can_bind(port: u16) -> bool {
    let v4 = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let v6 = SocketAddr::from((Ipv6Addr::LOCALHOST, port));
    free(TcpListener::bind(v4)) && free(TcpListener::bind(v6))
}

/// Only "in use" and "not allowed" say the port is taken. Any other failure,
/// such as a computer without IPv6, says nothing about the port.
fn free(bind: std::io::Result<TcpListener>) -> bool {
    match bind {
        Ok(_) => true,
        Err(err) => !matches!(err.kind(), ErrorKind::AddrInUse | ErrorKind::PermissionDenied),
    }
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

/// Brings the bridge in line with what the stack currently publishes. A
/// bridge that runs changes its ports itself; one is started only for a
/// stack that has none yet.
pub fn reconcile(app: &AppHandle, stack: &Stack, services: &[ServiceState]) {
    let state = app.state::<AppState>();
    let mut forwarders = state.forwarders.lock().expect("forwarders lock");
    let current = forwarders.get(&stack.id).map(|f| f.wanted.borrow().clone()).unwrap_or_default();
    let desired = desired_ports(stack, services, &current);
    if current == desired {
        return;
    }
    if let Some(forwarder) = forwarders.get(&stack.id) {
        forwarder.wanted.send_replace(desired);
        return;
    }
    let Some(machine) = state.store.machine(&stack.machine_id) else { return };
    let report = {
        let app = app.clone();
        let stack_id = stack.id.clone();
        move |forward: ForwardState| publish(&app, &stack_id, forward)
    };
    let wanted = spawn_bridge(state.ssh.clone(), machine.alias(), stack.id.clone(), desired, report);
    forwarders.insert(stack.id.clone(), Forwarder { wanted });
}

/// Ends a stack's bridge: its task sees the sender go and stops its ssh.
pub fn stop(app: &AppHandle, stack_id: &str) {
    let state = app.state::<AppState>();
    state.forwarders.lock().expect("forwarders lock").remove(stack_id);
    state.forward_states.lock().expect("forward states lock").remove(stack_id);
}

/// Tells every bridge whose socket is still in the folder to exit, whoever
/// left it there: this instance at quit, a crashed one, a stack removed
/// since. One shell loop, so Windows starts one wsl.exe, not one per stack.
pub fn exit_all(ssh: &Ssh) {
    let script = r#"for s in "$1"/*; do [ -S "$s" ] && ssh -F "$2" -S "$s" -O exit bridge; rm -f "$s"; done"#;
    let _ = tools::unix_std("sh")
        .args(["-c", script, "sh"])
        .arg(ssh.forward_dir())
        .arg(ssh.config_path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// Starts a bridge that carries `ports` to the machine behind `alias` and
/// tells `report` how it stands. Other ports go in through the returned
/// sender; dropping the sender ends the bridge. `name` names its sockets.
pub fn spawn_bridge(
    ssh: Ssh,
    alias: String,
    name: String,
    ports: Vec<ForwardPort>,
    report: impl Fn(ForwardState) + Send + Sync + 'static,
) -> watch::Sender<Vec<ForwardPort>> {
    let (wanted, watching) = watch::channel(ports);
    let command = SshCommand { program: "ssh".into(), config: ssh.config_path(), sockets: ssh.forward_dir() };
    tauri::async_runtime::spawn(run(command, alias, name, watching, report));
    wanted
}

/// How a bridge runs ssh: the program, the app's ssh config, and the folder
/// its control sockets go in. The app runs the real ssh; the tests put a
/// stand-in in its place.
struct SshCommand {
    program: String,
    config: String,
    sockets: String,
}

/// How one ssh process ended.
enum Ended {
    /// The bridge was ended; nothing follows.
    Closed,
    /// The ports changed in a way it could not follow; the next one starts at once.
    Changed,
    /// It died or never came up; the next one waits a little.
    Died,
}

async fn run(ssh: SshCommand, alias: String, name: String, mut wanted: watch::Receiver<Vec<ForwardPort>>, report: impl Fn(ForwardState)) {
    let mut attempts: u32 = 0;
    loop {
        let ports = wanted.borrow_and_update().clone();
        if ports.is_empty() {
            // Nothing to carry (the stack stopped, the bridge was turned off) until ports come back.
            report(ForwardState::default());
            if wanted.changed().await.is_err() {
                return;
            }
            continue;
        }
        attempts += 1;
        let socket = format!("{}/{name}-{}", ssh.sockets, NEXT_SOCKET.fetch_add(1, Ordering::Relaxed));
        let tunnel = Tunnel { ssh: &ssh, alias: &alias, socket };
        let ended = carry(&tunnel, ports, &mut wanted, &mut attempts, &report).await;
        let closed = match ended {
            Ended::Closed => true,
            Ended::Changed => false,
            Ended::Died => wait_before_retry(attempts, &mut wanted).await == Wait::Closed,
        };
        if closed {
            report(ForwardState::default());
            return;
        }
    }
}

/// One ssh carrying the ports until it dies, the bridge is ended, or the
/// ports change in a way it cannot follow in place.
async fn carry(
    tunnel: &Tunnel<'_>,
    mut ports: Vec<ForwardPort>,
    wanted: &mut watch::Receiver<Vec<ForwardPort>>,
    attempts: &mut u32,
    report: &impl Fn(ForwardState),
) -> Ended {
    // A start that fails (WSL not up yet, ssh missing for a moment) is
    // retried like a bridge that died; only the end of the bridge stops that.
    let (mut child, _tracked) = match tunnel.spawn(&ports) {
        Ok(spawned) => spawned,
        Err(err) => {
            report(down(&ports, format!("{err:#}"), *attempts));
            return Ended::Died;
        }
    };
    let said = last_line_of(child.stderr.take());

    let ready = tokio::select! {
        ready = tunnel.wait_until_ready(&mut child) => ready,
        changed = wanted.changed() => {
            tunnel.stop(&mut child).await;
            return if changed.is_ok() { Ended::Changed } else { Ended::Closed };
        }
    };
    if !ready {
        tunnel.stop(&mut child).await;
        let why = what_ssh_said(said, &format!("the bridge did not come up within {} s", READY_TIMEOUT.as_secs())).await;
        report(down(&ports, why, *attempts));
        return Ended::Died;
    }
    *attempts = 0;
    let since_ms = Some(crate::stack::now_ms());
    report(ForwardState { up: true, ports: ports.clone(), error: None, attempts: 0, since_ms });

    loop {
        tokio::select! {
            _ = child.wait() => {
                let why = what_ssh_said(said, "ssh exited").await;
                report(down(&ports, why, *attempts));
                return Ended::Died;
            }
            changed = wanted.changed() => {
                if changed.is_err() {
                    tunnel.stop(&mut child).await;
                    return Ended::Closed;
                }
                let next = wanted.borrow_and_update().clone();
                let followed = !next.is_empty() && tunnel.adjust(&ports, &next).await.is_ok();
                if !followed {
                    tunnel.stop(&mut child).await;
                    return Ended::Changed;
                }
                ports = next;
                report(ForwardState { up: true, ports: ports.clone(), error: None, attempts: 0, since_ms });
            }
        }
    }
}

fn down(ports: &[ForwardPort], error: String, attempts: u32) -> ForwardState {
    ForwardState { up: false, ports: ports.to_vec(), error: Some(error), attempts, since_ms: None }
}

#[derive(PartialEq, Eq)]
enum Wait {
    Over,
    Closed,
}

/// Exponential backoff so a machine that went to sleep is not hammered.
/// New ports end the wait at once.
async fn wait_before_retry(attempts: u32, wanted: &mut watch::Receiver<Vec<ForwardPort>>) -> Wait {
    let delay = Duration::from_secs(2u64.saturating_pow(attempts.min(5))).min(MAX_BACKOFF);
    tokio::select! {
        _ = tokio::time::sleep(delay) => Wait::Over,
        changed = wanted.changed() => if changed.is_ok() { Wait::Over } else { Wait::Closed },
    }
}

/// One ssh process of a bridge and the control requests sent to it.
struct Tunnel<'a> {
    ssh: &'a SshCommand,
    alias: &'a str,
    /// Its own control socket, as ssh sees the path.
    socket: String,
}

impl Tunnel<'_> {
    fn spawn(&self, ports: &[ForwardPort]) -> anyhow::Result<(Child, tools::Tracked)> {
        let mut cmd = tools::unix(&self.ssh.program);
        cmd.arg("-F").arg(&self.ssh.config);
        // -N: no remote command. -M/-S: be a control master on our own socket,
        // which the control requests below use. ExitOnForwardFailure turns a
        // taken port into a clean exit instead of a half-working session.
        // ControlPersist must be off here: with it, ssh forks a second
        // background master that would keep the forwards alive after this
        // child is killed.
        cmd.args(["-N", "-M", "-S"]).arg(&self.socket).args(["-o", "ExitOnForwardFailure=yes", "-o", "ControlPersist=no"]);
        for port in ports {
            cmd.arg("-L").arg(local_forward(port));
        }
        cmd.arg(self.alias).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped()).kill_on_drop(true);
        let child = cmd.spawn()?;
        let tracked = tools::track(&child);
        Ok((child, tracked))
    }

    /// True once the master answers `-O check`. ssh opens its control socket
    /// only after it has logged in and bound every port, so no test
    /// connection has to go through to the service behind the bridge.
    async fn wait_until_ready(&self, child: &mut Child) -> bool {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                return false;
            }
            if self.control("check", None).await.is_ok() {
                return true;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
        false
    }

    /// Moves the running ssh from `carried` to `next`. Cancels come first, so
    /// a local port that now leads to another remote port is free again.
    async fn adjust(&self, carried: &[ForwardPort], next: &[ForwardPort]) -> anyhow::Result<()> {
        for port in carried.iter().filter(|port| !next.contains(port)) {
            self.control("cancel", Some(port)).await?;
        }
        for port in next.iter().filter(|port| !carried.contains(port)) {
            self.control("forward", Some(port)).await?;
        }
        Ok(())
    }

    /// One control request (`check`, `forward`, `cancel`, `exit`) to this
    /// ssh's master; an error when it refuses or does not answer.
    async fn control(&self, request: &str, port: Option<&ForwardPort>) -> anyhow::Result<()> {
        let mut cmd = tools::unix(&self.ssh.program);
        cmd.arg("-F").arg(&self.ssh.config).arg("-S").arg(&self.socket).args(["-O", request]);
        if let Some(port) = port {
            cmd.arg("-L").arg(local_forward(port));
        }
        cmd.arg(self.alias).stdin(Stdio::null()).kill_on_drop(true);
        let out = tokio::time::timeout(CONTROL_LIMIT, cmd.output()).await.context("ssh did not answer")??;
        let said = String::from_utf8_lossy(&out.stderr);
        anyhow::ensure!(out.status.success(), "ssh -O {request}: {}", said.trim());
        Ok(())
    }

    /// Kills the ssh and asks whatever still answers on its socket to exit.
    /// On Windows the kill reaches only wsl.exe, and the request is what ends
    /// the ssh inside WSL; elsewhere it finds nothing, which is harmless.
    async fn stop(&self, child: &mut Child) {
        let _ = child.start_kill();
        let _ = child.wait().await;
        let _ = self.control("exit", None).await;
        tools::remove_file(&self.socket);
    }
}

/// `localhost` binds both 127.0.0.1 and ::1, which browsers and Node need.
fn local_forward(port: &ForwardPort) -> String {
    format!("localhost:{}:127.0.0.1:{}", port.local, port.remote)
}

/// Reads ssh's stderr to the end and keeps the last line, its reason for exiting.
fn last_line_of(stderr: Option<ChildStderr>) -> JoinHandle<String> {
    tokio::spawn(async move {
        let mut last = String::new();
        let Some(stderr) = stderr else { return last };
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                last = trimmed.to_string();
            }
        }
        last
    })
}

/// ssh's last line once the process has ended, or `otherwise`. The pipe
/// closes with the process, so the wait is short; a second is the most.
async fn what_ssh_said(reader: JoinHandle<String>, otherwise: &str) -> String {
    let said = tokio::time::timeout(Duration::from_secs(1), reader).await.ok().and_then(Result::ok).unwrap_or_default();
    if said.is_empty() {
        otherwise.to_string()
    } else {
        said
    }
}

fn publish(app: &AppHandle, stack_id: &str, state: ForwardState) {
    let app_state = app.state::<AppState>();
    app_state.forward_states.lock().expect("forward states lock").insert(stack_id.to_string(), state.clone());
    let _ = app.emit(FORWARD_EVENT, ForwardEvent { stack_id: stack_id.to_string(), state });
}

#[cfg(test)]
mod tests;
