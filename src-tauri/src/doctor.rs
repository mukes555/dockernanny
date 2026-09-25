//! The checks that tell the user what a machine is missing before a stack is
//! sent there, each with the command that fixes it.

use std::time::Duration;

use serde::Serialize;
use tokio::time::timeout;

use crate::machine::{first_line, Machine};
use crate::ssh::{Output, Ssh};

pub const DOCTOR_EVENT: &str = "machine:doctor";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Serialize)]
pub struct DoctorRow {
    pub key: &'static str,
    pub label: &'static str,
    pub ok: bool,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorEvent {
    pub machine_id: String,
    pub row: DoctorRow,
}

/// Runs the checks in order and reports each one as it finishes. Stops after
/// the SSH check when the machine cannot be reached; the rest would only
/// repeat the same error.
pub async fn doctor(ssh: &Ssh, machine: &Machine, mut report: impl FnMut(&DoctorRow)) -> Vec<DoctorRow> {
    let mut rows = Vec::new();
    let mut add = |row: DoctorRow| {
        report(&row);
        let ok = row.ok;
        rows.push(row);
        ok
    };
    let reachable = add(check_ssh(ssh, machine).await);
    if reachable {
        add(check_docker(ssh, machine).await);
        add(check_compose(ssh, machine).await);
        add(check_rsync(ssh, machine).await);
        add(check_host(ssh, machine).await);
    }
    rows
}

async fn check(ssh: &Ssh, machine: &Machine, script: &str) -> Result<Output, String> {
    match timeout(CHECK_TIMEOUT, ssh.run(&machine.alias(), script)).await {
        Ok(Ok(out)) => Ok(out),
        Ok(Err(err)) => Err(format!("{err:#}")),
        Err(_) => Err("timed out".into()),
    }
}

fn row(key: &'static str, label: &'static str, ok: bool, detail: impl Into<String>, fix: Option<String>) -> DoctorRow {
    DoctorRow {
        key,
        label,
        ok,
        detail: detail.into(),
        fix,
    }
}

async fn check_ssh(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let out = match check(ssh, machine, "echo dockernanny-ok").await {
        Ok(out) => out,
        Err(err) => return row("ssh", "SSH", false, err, Some("Is `ssh` installed on this computer?".into())),
    };
    if out.ok() && out.stdout.contains("dockernanny-ok") {
        return row("ssh", "SSH", true, format!("{} answers", machine.address()), None);
    }
    let (detail, fix) = explain_ssh_failure(machine, &out);
    row("ssh", "SSH", false, detail, Some(fix))
}

fn explain_ssh_failure(machine: &Machine, out: &Output) -> (String, String) {
    let err = &out.stderr;
    let key_rejected = err.contains("Permission denied");
    let host_key_changed = err.contains("Host key verification failed") || err.contains("IDENTIFICATION HAS CHANGED");
    let refused = err.contains("Connection refused");
    let unreachable = err.contains("timed out") || err.contains("No route") || err.contains("Could not resolve") || err.contains("unreachable");

    if key_rejected {
        // Only macOS's ssh-add knows the keychain flag; elsewhere it is an unknown option.
        let keychain = if cfg!(target_os = "macos") { " --apple-use-keychain" } else { "" };
        let fix = format!(
            "ssh-copy-id -i {key}.pub -p {port} {user}@{host}\n# key with a passphrase? load it first:\nssh-add{keychain} {key}",
            key = machine.key_path,
            port = machine.port,
            user = machine.user,
            host = machine.host
        );
        return ("The machine did not accept the key.".into(), fix);
    }
    if host_key_changed {
        let known_hosts_name = if machine.port == 22 { machine.host.clone() } else { format!("[{}]:{}", machine.host, machine.port) };
        return (
            "The machine's host key changed since it was last seen.".into(),
            format!("# only if you reinstalled the machine:\nssh-keygen -R '{known_hosts_name}'"),
        );
    }
    if refused {
        return (
            format!("Nothing answers on port {}.", machine.port),
            "Start sshd on the machine (Remote Login on macOS). A Windows machine runs it inside WSL 2: see Prepare another machine.".into(),
        );
    }
    if unreachable {
        return (
            format!("This computer cannot reach {}.", machine.host),
            "Check the address, and that both machines are on the same network.".into(),
        );
    }
    let detail = if err.is_empty() { format!("ssh exited with code {:?}", out.code) } else { err.clone() };
    (detail, format!("# try it by hand:\nssh -F ~/.dockernanny/ssh_config {}", machine.alias()))
}

async fn check_docker(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let out = match check(ssh, machine, "docker version --format '{{.Server.Version}}'").await {
        Ok(out) => out,
        Err(err) => return row("docker", "Docker", false, err, None),
    };
    if out.ok() {
        return row("docker", "Docker", true, format!("Docker Engine {}", out.stdout), None);
    }
    let err = &out.stderr;
    let not_allowed = err.contains("permission denied");
    let missing = err.contains("not found");
    let not_running = err.contains("Cannot connect to the Docker daemon");
    let fix = if not_allowed {
        "sudo usermod -aG docker $USER\n# then log out of the machine and back in"
    } else if missing {
        "curl -fsSL https://get.docker.com | sudo sh\nsudo usermod -aG docker $USER"
    } else if not_running {
        "sudo systemctl enable --now docker\n# WSL2: enable systemd first (see the guide)"
    } else {
        "# see what docker says:\ndocker version"
    };
    row("docker", "Docker", false, first_line(err), Some(fix.into()))
}

async fn check_compose(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let out = match check(ssh, machine, "docker compose version --short").await {
        Ok(out) => out,
        Err(err) => return row("compose", "Compose", false, err, None),
    };
    if out.ok() {
        return row("compose", "Compose", true, format!("Compose {}", out.stdout), None);
    }
    // The machine's OS is not known yet at this point, so the fix names the common ones.
    let fix = "# Debian or Ubuntu:\nsudo apt-get install -y docker-compose-plugin\n# Fedora: sudo dnf install -y docker-compose-plugin\n# macOS: comes with Docker Desktop and OrbStack";
    row("compose", "Compose", false, first_line(&out.stderr), Some(fix.into()))
}

async fn check_rsync(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let out = match check(ssh, machine, "rsync --version").await {
        Ok(out) => out,
        Err(err) => return row("rsync", "rsync", false, err, None),
    };
    if out.ok() {
        return row("rsync", "rsync", true, first_line(&out.stdout), None);
    }
    let fix = "# Debian or Ubuntu:\nsudo apt-get install -y rsync\n# Fedora: sudo dnf install -y rsync\n# macOS: brew install rsync";
    row("rsync", "rsync", false, first_line(&out.stderr), Some(fix.into()))
}

async fn check_host(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let script = "uname -sr; nproc 2>/dev/null || sysctl -n hw.ncpu";
    let out = match check(ssh, machine, script).await {
        Ok(out) => out,
        Err(err) => return row("host", "Host", false, err, None),
    };
    let mut lines = out.stdout.lines();
    let os = lines.next().unwrap_or("unknown os");
    let cpus = lines.next().unwrap_or("?");
    row("host", "Host", out.ok(), format!("{os}, {cpus} cpus"), None)
}
