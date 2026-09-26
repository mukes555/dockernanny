//! Is this computer ready to use other machines? The same shape of answer as
//! the doctor gives for a machine: one row per thing it needs, with the fix.
//! Also the SSH key: which one to offer, and making one when there is none.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::doctor::DoctorRow;
use crate::{store, tools};

/// The key the user chose in Settings, or the first common one in `~/.ssh`,
/// or where a new one would go.
pub fn default_key_path(chosen: &str) -> PathBuf {
    if !chosen.trim().is_empty() {
        return PathBuf::from(chosen.trim());
    }
    let ssh_dir = store::user_home().join(".ssh");
    for name in ["id_ed25519", "id_rsa", "id_ecdsa"] {
        let candidate = ssh_dir.join(name);
        if candidate.exists() {
            return candidate;
        }
    }
    ssh_dir.join("id_ed25519")
}

/// The version lines of the tools this computer uses, None for what does
/// not run. ssh and rsync are asked where they run (inside WSL on Windows).
pub struct Versions {
    pub ssh: Option<String>,
    pub rsync: Option<String>,
    pub docker: Option<String>,
}

pub async fn versions() -> Versions {
    let mut ssh = tools::unix("ssh");
    ssh.arg("-V");
    let mut rsync = tools::unix("rsync");
    rsync.arg("--version");
    let mut docker = tools::native("docker");
    docker.args(["version", "--format", "{{.Server.Version}}"]);
    Versions {
        ssh: version_line(ssh).await,
        rsync: version_line(rsync).await,
        docker: version_line(docker).await,
    }
}

/// One row each for what the controller role needs here. The tools run with
/// no side effects: only their version lines are read.
pub async fn readiness(key_path: &Path) -> Vec<DoctorRow> {
    let Versions { ssh, rsync, docker } = versions().await;
    let key_present = key_path.exists();
    let key_detail = if key_present { key_path.display().to_string() } else { format!("no key at {}", key_path.display()) };
    let mut rows = Vec::new();
    if let Some(distro) = tools::wsl_distro() {
        rows.push(wsl_row(&distro).await);
    }
    rows.push(row("ssh", "SSH client", ssh.clone(), ssh.is_some(), install_hint("an OpenSSH client", "openssh-client")));
    rows.push(row("rsync", "rsync", rsync.clone(), rsync.is_some(), install_hint("rsync", "rsync")));
    rows.push(row("key", "SSH key", Some(key_detail), key_present, "Create one with the button below, or choose an existing key in Settings.".into()));
    rows.push(docker_here_row(docker).await);
    rows
}

const DOCKER_NEEDED_FOR: &str = "Only needed to preview a dropped compose file and to copy this computer's own projects.";

/// Docker on this computer. On Linux, when it does not answer, the same
/// check as the sharing page says why, with Docker's own next step.
async fn docker_here_row(version: Option<String>) -> DoctorRow {
    if let Some(version) = version {
        return row("docker", "Docker here", Some(format!("Docker {version}")), true, String::new());
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(access) = tauri::async_runtime::spawn_blocking(crate::docker_access::check).await {
            return row("docker", "Docker here", Some(access.detail()), access.is_ready(), linux_docker_fix(&access));
        }
    }
    row(
        "docker",
        "Docker here",
        Some("not running or not installed".into()),
        false,
        format!("{DOCKER_NEEDED_FOR} Install Docker Desktop, OrbStack or Docker Engine and start it."),
    )
}

#[cfg(target_os = "linux")]
fn linux_docker_fix(access: &crate::docker_access::DockerAccess) -> String {
    use crate::docker_access::DockerAccess;
    match access {
        DockerAccess::NotInstalled => format!("{DOCKER_NEEDED_FOR} Install Docker Engine (docs.docker.com/engine/install) and start it."),
        DockerAccess::NotRunning => "Start it: sudo systemctl enable --now docker".into(),
        DockerAccess::NotInGroup => "Docker's post-install step: sudo usermod -aG docker $USER, then restart this computer once.".into(),
        DockerAccess::RestartNeeded => "Restart this computer once so your login can use Docker.".into(),
        DockerAccess::Ready(_) | DockerAccess::Other(_) => format!("{DOCKER_NEEDED_FOR} Make `docker version` work without sudo."),
    }
}

/// Windows: whether the chosen WSL distribution answers at all.
async fn wsl_row(distro: &str) -> DoctorRow {
    let mut uname = tools::unix("uname");
    uname.arg("-sr");
    let answer = version_line(uname).await;
    let detail = answer.as_ref().map(|kernel| format!("{distro}, {kernel}")).unwrap_or_else(|| format!("{distro} does not answer"));
    let fix = format!("Install WSL 2 with a distribution: in PowerShell run `wsl --install -d {distro}`, restart, open it once to create a user. Another distribution can be chosen in Settings.");
    row("wsl", "WSL", Some(detail), answer.is_some(), fix)
}

/// A readiness row; the fix is only shown for what is missing.
fn row(key: &'static str, label: &'static str, detail: Option<String>, ok: bool, fix: String) -> DoctorRow {
    DoctorRow {
        key,
        label,
        ok,
        detail: detail.unwrap_or_else(|| "not found".into()),
        fix: (!ok).then_some(fix),
    }
}

/// How to install a tool on this operating system, in one sentence.
fn install_hint(tool: &str, debian_package: &str) -> String {
    if cfg!(target_os = "macos") {
        format!("{tool} ships with macOS; if it is missing, run `xcode-select --install`.")
    } else if cfg!(windows) {
        format!("dockerNanny runs {tool} inside WSL: open the distribution and run `sudo apt install {debian_package}`.")
    } else {
        format!("Install {tool} with your package manager, for example `sudo apt install {debian_package}`.")
    }
}

/// The first line a tool prints about itself, or None when it does not run.
/// `ssh -V` prints to stderr, the others to stdout.
pub(crate) async fn version_line(mut cmd: Command) -> Option<String> {
    let out = cmd.stdin(std::process::Stdio::null()).output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    let first = String::from_utf8_lossy(&text).lines().next().unwrap_or("").trim().to_string();
    (!first.is_empty()).then_some(first)
}

/// Windows: the OpenSSH client and rsync inside the WSL distribution,
/// installed with apt-get as its root user; Windows itself is not changed.
/// A distribution without apt-get gets a sentence saying what to install.
pub async fn install_wsl_tools() -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        use anyhow::Context;
        const SCRIPT: &str = "command -v apt-get >/dev/null 2>&1 || { echo 'This distribution has no apt-get; install openssh-client and rsync with its package manager.' >&2; exit 3; }\n\
                              export DEBIAN_FRONTEND=noninteractive\n\
                              apt-get update -qq && apt-get install -y -qq openssh-client rsync\n";
        let out = tools::unix_as_root("sh").args(["-c", SCRIPT]).stdin(std::process::Stdio::null()).output().await.context("start wsl.exe")?;
        let stderr = crate::host::platform::decode(&out.stderr);
        let last_lines: Vec<&str> = stderr.lines().filter(|l| !l.trim().is_empty()).rev().take(3).collect();
        anyhow::ensure!(out.status.success(), "{}", last_lines.into_iter().rev().collect::<Vec<_>>().join("\n"));
        Ok(())
    }
    #[cfg(not(windows))]
    anyhow::bail!("only Windows keeps ssh and rsync inside WSL")
}

/// Makes an ed25519 key without a passphrase at `path`, never over an
/// existing one. Returns the path of the public half.
pub async fn generate_key(path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(!path.exists(), "{} already exists; nothing was changed", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    // Native on every OS: Windows ships ssh-keygen and gives the key the right ACL.
    let out = tools::native("ssh-keygen")
        .args(["-t", "ed25519", "-N", "", "-C", "dockernanny", "-f"])
        .arg(path)
        .output()
        .await
        .map_err(|err| anyhow::anyhow!("could not run ssh-keygen (is an OpenSSH client installed?): {err}"))?;
    anyhow::ensure!(out.status.success(), "ssh-keygen failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    Ok(PathBuf::from(format!("{}.pub", path.display())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chosen_key_wins_over_the_usual_places() {
        assert_eq!(default_key_path("  /keys/work  "), PathBuf::from("/keys/work"));
        assert!(default_key_path("").ends_with(".ssh/id_ed25519") || default_key_path("").ends_with(".ssh/id_rsa") || default_key_path("").ends_with(".ssh/id_ecdsa"));
    }

    #[tokio::test]
    async fn a_key_is_made_once_and_never_overwritten() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("keygen-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let key = dir.join("id_ed25519");
        let public = generate_key(&key).await.expect("ssh-keygen is available in CI and on developer machines");
        assert!(key.exists() && public.exists());
        let again = generate_key(&key).await;
        assert!(again.is_err(), "a second call must refuse, not replace the key");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
