//! A machine is an SSH target that has Docker: how it is saved, the stats
//! poll that keeps its card honest, and the Docker context that lets a
//! terminal use it. The doctor lives in `doctor.rs`, the probe script and
//! its parsers in `probe.rs`.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::probe::{self, Probe};
use crate::ssh::Ssh;
use crate::AppState;

pub const STATS_EVENT: &str = "machine:stats";
const POLL_EVERY: Duration = Duration::from_secs(10);
const POLL_TIMEOUT: Duration = Duration::from_secs(20);
/// An unreachable machine is retried every third tick, so every 30s, not 10s.
const BACKOFF_TICKS: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Machine {
    pub id: String,
    pub name: String,
    pub user: String,
    pub host: String,
    pub port: u16,
    pub key_path: String,
    #[serde(default)]
    pub docker_context: bool,
    /// The sshd host key was pinned at pairing time; connections then trust
    /// only that key. Machines added by hand accept the first key they see.
    #[serde(default)]
    pub pinned: bool,
}

impl Machine {
    pub fn new_id() -> String {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
    }

    /// The Host alias in the generated ssh config.
    pub fn alias(&self) -> String {
        format!("dn-{}", self.id)
    }

    pub fn address(&self) -> String {
        format!("{}@{}:{}", self.user, self.host, self.port)
    }

    /// A record that points back at this computer: a test leftover, or a
    /// stack someone runs "remotely" on their own Docker. Not a machine.
    pub fn points_at_this_computer(&self, own_hostname: &str) -> bool {
        let host = self.host.trim().to_ascii_lowercase();
        let loopback = matches!(host.as_str(), "localhost" | "127.0.0.1" | "::1" | "0.0.0.0");
        let own_lower = own_hostname.to_ascii_lowercase();
        let own = !own_lower.is_empty() && (host == own_lower || host == format!("{own_lower}.local"));
        loopback || own
    }
}

/// What the card and the page show: online plus everything the probe found.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MachineStats {
    pub online: bool,
    #[serde(flatten)]
    pub probe: Probe,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StatsEvent {
    pub machine_id: String,
    pub stats: MachineStats,
}

pub(crate) fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").to_string()
}

/// The Docker context name offered for the terminal: `dn-<machine name>`.
pub fn context_name(machine: &Machine) -> String {
    format!("dn-{}", crate::compose::sanitize_name(&machine.name))
}

/// Creates or removes the Docker context that points at this machine through
/// the generated ssh config. Docker itself resolves the alias, so the user's
/// `~/.ssh/config` must `Include` that file for the context to work.
pub async fn set_docker_context(machine: &Machine, enabled: bool) -> anyhow::Result<()> {
    let name = context_name(machine);
    let _ = tokio::process::Command::new("docker").args(["context", "rm", "-f", &name]).output().await;
    if !enabled {
        return Ok(());
    }
    let out = tokio::process::Command::new("docker")
        .args(["context", "create", &name, "--docker"])
        .arg(format!("host=ssh://{}", machine.alias()))
        .arg("--description")
        .arg(format!("dockerNanny: {}", machine.name))
        .output()
        .await
        .map_err(|err| anyhow::anyhow!("run docker context create (is Docker installed on this computer?): {err}"))?;
    anyhow::ensure!(out.status.success(), "docker context create failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(())
}

pub async fn poll(ssh: &Ssh, machine: &Machine) -> MachineStats {
    let offline = |error: String| MachineStats {
        error: Some(error),
        ..Default::default()
    };
    match timeout(POLL_TIMEOUT, ssh.run(&machine.alias(), probe::SCRIPT)).await {
        Ok(Ok(out)) if out.ok() => MachineStats {
            online: true,
            probe: probe::parse(&out.stdout),
            error: None,
        },
        Ok(Ok(out)) => offline(first_line(&out.stderr)),
        Ok(Err(err)) => offline(format!("{err:#}")),
        Err(_) => offline("timed out".into()),
    }
}

/// The same probe on this computer, through `sh`.
pub async fn probe_this_computer() -> Probe {
    use tokio::io::AsyncWriteExt;
    let spawned = tokio::process::Command::new("sh")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else { return Probe::default() };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(probe::SCRIPT.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    match timeout(POLL_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => probe::parse(&String::from_utf8_lossy(&out.stdout)),
        _ => Probe::default(),
    }
}

/// Polls every machine on a fixed beat and publishes the result. Machines
/// that failed last time are skipped on most ticks so a machine that is off
/// does not cost a connection attempt every ten seconds.
pub fn spawn_stats_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut failures: HashMap<String, u32> = HashMap::new();
        let mut tick: u32 = 0;
        loop {
            let state = app.state::<AppState>();
            let due: Vec<Machine> = state
                .store
                .machines()
                .into_iter()
                .filter(|machine| {
                    let failed_before = failures.get(&machine.id).copied().unwrap_or(0) > 0;
                    !failed_before || tick.is_multiple_of(BACKOFF_TICKS)
                })
                .collect();

            let mut polls = JoinSet::new();
            for machine in due {
                let ssh = state.ssh.clone();
                polls.spawn(async move {
                    let stats = poll(&ssh, &machine).await;
                    (machine.id, stats)
                });
            }
            while let Some(Ok((machine_id, stats))) = polls.join_next().await {
                let count = failures.entry(machine_id.clone()).or_default();
                *count = if stats.online { 0 } else { *count + 1 };
                publish(&app, machine_id, stats);
            }

            tick = tick.wrapping_add(1);
            tokio::time::sleep(POLL_EVERY).await;
        }
    });
}

pub fn publish(app: &AppHandle, machine_id: String, stats: MachineStats) {
    let state = app.state::<AppState>();
    state.stats.lock().expect("stats lock").insert(machine_id.clone(), stats.clone());
    let _ = app.emit(STATS_EVENT, StatsEvent { machine_id, stats });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_and_own_names_point_at_this_computer() {
        let machine = |host: &str| Machine { id: "x".into(), name: "x".into(), user: "alex".into(), host: host.into(), port: 22, key_path: String::new(), docker_context: false, pinned: false };
        assert!(machine("localhost").points_at_this_computer("studio"));
        assert!(machine("127.0.0.1").points_at_this_computer(""));
        assert!(machine("Studio.local").points_at_this_computer("studio"));
        assert!(!machine("192.0.2.10").points_at_this_computer("studio"));
        assert!(!machine("workshop").points_at_this_computer("studio"));
    }

    #[test]
    fn stats_flatten_the_probe_on_the_wire() {
        let stats = MachineStats { online: true, probe: Probe { cpus: 4, ..Default::default() }, error: None };
        let json = serde_json::to_value(&stats).unwrap();
        assert_eq!(json["online"], true);
        assert_eq!(json["cpus"], 4);
        assert!(json["probe"].is_null());
    }
}
