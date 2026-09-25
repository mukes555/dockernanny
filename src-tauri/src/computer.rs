//! Is this computer ready to use other machines? The same shape of answer as
//! the doctor gives for a machine: one row per thing it needs, with the fix.
//! Also the SSH key: which one to offer, and making one when there is none.

use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::doctor::DoctorRow;
use crate::store;

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

/// One row each for what the controller role needs here. The tools run with
/// no side effects: only their version lines are read.
pub async fn readiness(key_path: &Path) -> Vec<DoctorRow> {
    let ssh = version_line("ssh", &["-V"]).await;
    let rsync = version_line("rsync", &["--version"]).await;
    let docker = version_line("docker", &["version", "--format", "{{.Server.Version}}"]).await;
    let key_present = key_path.exists();
    let key_detail = if key_present { key_path.display().to_string() } else { format!("no key at {}", key_path.display()) };
    let docker_detail = docker.clone().map(|v| format!("Docker {v}")).unwrap_or_else(|| "not running or not installed".into());
    vec![
        row("ssh", "SSH client", ssh.clone(), ssh.is_some(), install_hint("an OpenSSH client", "openssh-client")),
        row("rsync", "rsync", rsync.clone(), rsync.is_some(), install_hint("rsync", "rsync")),
        row("key", "SSH key", Some(key_detail), key_present, "Create one with the button below, or choose an existing key in Settings.".into()),
        row(
            "docker",
            "Docker here",
            Some(docker_detail),
            docker.is_some(),
            "Only needed to preview a dropped compose file and to copy this computer's own projects. Install Docker Desktop, OrbStack or Docker Engine and start it.".into(),
        ),
    ]
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
        format!("On Windows, dockerNanny runs {tool} inside WSL; see Prepare another machine.")
    } else {
        format!("Install {tool} with your package manager, for example `sudo apt install {debian_package}`.")
    }
}

/// The first line a tool prints about itself, or None when it does not run.
/// `ssh -V` prints to stderr, the others to stdout.
pub(crate) async fn version_line(program: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(program).args(args).output().await.ok()?;
    if !out.status.success() {
        return None;
    }
    let text = if out.stdout.is_empty() { out.stderr } else { out.stdout };
    let first = String::from_utf8_lossy(&text).lines().next().unwrap_or("").trim().to_string();
    (!first.is_empty()).then_some(first)
}

/// Makes an ed25519 key without a passphrase at `path`, never over an
/// existing one. Returns the path of the public half.
pub async fn generate_key(path: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(!path.exists(), "{} already exists; nothing was changed", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let out = Command::new("ssh-keygen")
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
