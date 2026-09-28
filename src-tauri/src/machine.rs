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
/// While nobody can see the window, the numbers need not be fresh.
const POLL_EVERY_HIDDEN: Duration = Duration::from_secs(60);
const POLL_TIMEOUT: Duration = Duration::from_secs(20);
/// An unreachable machine is retried every third tick: every 30 s, or every
/// 3 minutes while the window is hidden.
const BACKOFF_TICKS: u32 = 3;
/// Every this many readings is a full one (about once a minute).
const FULL_EVERY: u32 = 6;

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
    // Docker on Windows would reach the machine with Windows' own ssh, which
    // knows neither the alias nor the key; that is left for a later version.
    let supported = !cfg!(windows);
    anyhow::ensure!(!enabled || supported, "a Docker context for a machine is not available on Windows yet; use the ssh line from this dialog inside WSL");
    let name = context_name(machine);
    let _ = crate::tools::native("docker").args(["context", "rm", "-f", &name]).output().await;
    if !enabled {
        return Ok(());
    }
    let out = crate::tools::native("docker")
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

/// A full reading of the machine.
pub async fn poll(ssh: &Ssh, machine: &Machine) -> MachineStats {
    poll_with(ssh, machine, None).await
}

/// Given the last full reading, only what changes is read again and the
/// rest is taken from it.
async fn poll_with(ssh: &Ssh, machine: &Machine, full: Option<&Probe>) -> MachineStats {
    let offline = |error: String| MachineStats {
        error: Some(error),
        ..Default::default()
    };
    let script = probe::script(full.is_none());
    match timeout(POLL_TIMEOUT, ssh.run(&machine.alias(), &script)).await {
        Ok(Ok(out)) if out.ok() => {
            let read = probe::parse(&out.stdout);
            let probe = match full {
                Some(full) => probe::with_fixed_facts(read, full),
                None => read,
            };
            MachineStats { online: true, probe, error: None }
        }
        Ok(Ok(out)) => offline(first_line(&out.stderr)),
        Ok(Err(err)) => offline(format!("{err:#}")),
        Err(_) => offline("timed out".into()),
    }
}

/// The same probe on this computer, through `sh`. On Windows that `sh` is
/// inside WSL, which answers the way a Windows machine does over ssh (the
/// parsers already read its battery and Windows version lines); memory and
/// disk are then the WSL VM's.
pub async fn probe_this_computer() -> Probe {
    use tokio::io::AsyncWriteExt;
    let spawned = crate::tools::unix("sh")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = spawned else { return Probe::default() };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(probe::script(true).as_bytes()).await;
        let _ = stdin.shutdown().await;
    }
    match timeout(POLL_TIMEOUT, child.wait_with_output()).await {
        Ok(Ok(out)) => probe::parse(&String::from_utf8_lossy(&out.stdout)),
        _ => Probe::default(),
    }
}

/// Polls every machine on a beat and publishes the result: every ten
/// seconds while the window can be seen, every minute while not. Machines
/// that failed last time are skipped on most ticks so a machine that is off
/// does not cost a connection attempt each time. Most readings are light,
/// see `probe::script`.
pub fn spawn_stats_loop(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut failures: HashMap<String, u32> = HashMap::new();
        let mut full_readings: HashMap<String, FullReading> = HashMap::new();
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
                let full = full_readings.get(&machine.id).filter(|kept| kept.light_since < FULL_EVERY).map(|kept| kept.probe.clone());
                polls.spawn(async move {
                    let stats = poll_with(&ssh, &machine, full.as_ref()).await;
                    (machine.id, stats, full.is_none())
                });
            }
            while let Some(Ok((machine_id, stats, was_full))) = polls.join_next().await {
                let count = failures.entry(machine_id.clone()).or_default();
                *count = if stats.online { 0 } else { *count + 1 };
                remember(&mut full_readings, &machine_id, &stats, was_full);
                publish(&app, machine_id, stats);
            }

            tick = tick.wrapping_add(1);
            crate::tray::until_next_poll(&app, POLL_EVERY, POLL_EVERY_HIDDEN).await;
        }
    });
}

/// A machine's last full reading and how many light ones came after it.
struct FullReading {
    probe: Probe,
    light_since: u32,
}

/// A machine that stops answering is read in full when it answers again:
/// it may have been changed meanwhile.
fn remember(readings: &mut HashMap<String, FullReading>, machine_id: &str, stats: &MachineStats, was_full: bool) {
    if !stats.online {
        readings.remove(machine_id);
        return;
    }
    if was_full {
        readings.insert(machine_id.to_string(), FullReading { probe: stats.probe.clone(), light_since: 0 });
        return;
    }
    if let Some(kept) = readings.get_mut(machine_id) {
        kept.light_since += 1;
    }
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
