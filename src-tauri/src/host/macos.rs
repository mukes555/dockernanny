//! A Mac as the shared computer: no WSL, just Remote Login (sshd on 22),
//! Docker Desktop or OrbStack, and rsync, which ships with macOS. The one
//! admin step is a single command through the system password prompt.

use std::process::Child;
use std::sync::Mutex;

use super::platform::{
    host_key_from_pub, listening_here, row, run, Installed, Outcome, Output, Picture, Platform, Say, SetupOptions, CHECK_LIMIT, SETUP_LIMIT,
};

const SSH_PORT: u16 = 22;

/// What does not change while the app runs, read once instead of at every
/// probe (every ten seconds while the page is open). Set up and the page's
/// Refresh read it again.
#[derive(Clone)]
struct Kept {
    memory_gb: u32,
    version: String,
    rsync: bool,
}

#[derive(Default)]
pub struct MacOs {
    kept: Mutex<Option<Kept>>,
}

impl MacOs {
    fn kept(&self) -> Kept {
        if let Some(kept) = self.kept.lock().expect("kept lock").clone() {
            return kept;
        }
        let bytes: u64 = self.sh("sysctl -n hw.memsize").text().parse().unwrap_or(0);
        let kept = Kept {
            memory_gb: (bytes / (1024 * 1024 * 1024)) as u32,
            version: self.sh("sw_vers -productVersion").text(),
            rsync: self.sh("command -v rsync").ok,
        };
        *self.kept.lock().expect("kept lock") = Some(kept.clone());
        kept
    }

    /// With the app's PATH, which finds Docker wherever its installer put it
    /// (`tools::add_docker_to_path`).
    fn sh(&self, script: &str) -> Output {
        run("/bin/sh", &["-c", script], None, &[], CHECK_LIMIT)
    }

    fn user(&self) -> String {
        std::env::var("USER").unwrap_or_else(|_| self.sh("whoami").text())
    }

    fn sshd_listening(&self) -> bool {
        listening_here(SSH_PORT)
    }

    fn docker_version(&self) -> Option<String> {
        let out = self.sh("docker version --format '{{.Server.Version}}' 2>/dev/null");
        (out.ok && !out.text().is_empty()).then(|| out.text())
    }
}

impl Platform for MacOs {
    fn os_name(&self) -> &'static str {
        "macOS"
    }

    fn probe(&self) -> Picture {
        let kept = self.kept();
        let mut picture = Picture {
            ssh_port: SSH_PORT,
            total_memory_gb: kept.memory_gb,
            user: Some(self.user()),
            ready_for_pairing: true,
            ..Default::default()
        };
        let version = kept.version;
        picture.rows.push(row("macOS", !version.is_empty(), if version.is_empty() { "unknown".into() } else { version }));

        let docker = self.docker_version();
        let docker_detail = docker.clone().unwrap_or_else(|| "not running: install Docker Desktop or OrbStack and open it".to_string());
        picture.rows.push(row("Docker", docker.is_some(), docker_detail));

        let listening = self.sshd_listening();
        picture.sshd_listening = listening;
        picture.rows.push(row("Remote Login (SSH)", listening, if listening { "on, port 22" } else { "off (Set up turns it on)" }));

        picture.rows.push(row("rsync", kept.rsync, if kept.rsync { "installed" } else { "missing" }));
        picture
    }

    fn forget_kept(&self) {
        *self.kept.lock().expect("kept lock") = None;
    }

    fn setup(&self, _options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        self.forget_kept();
        let mut results = Vec::new();

        say("==> Docker");
        let docker = match self.docker_version() {
            Some(version) => Outcome::Done(format!("Docker {version}")),
            None => Outcome::NeedsUser(
                "Install Docker Desktop (docker.com) or OrbStack (orbstack.dev), open it once, then click Set up again.".into(),
            ),
        };
        let stop = matches!(docker, Outcome::NeedsUser(_));
        results.push(("Docker", docker));
        if stop {
            return results;
        }

        say("==> Remote Login");
        let remote_login = if self.sshd_listening() {
            Outcome::Done("already on".into())
        } else {
            say("    macOS asks for your password to turn Remote Login on");
            // The password prompt waits for the user, so the long limit.
            let turn_on = "osascript -e 'do shell script \"systemsetup -setremotelogin on\" with administrator privileges' 2>&1";
            let out = run("/bin/sh", &["-c", turn_on], None, &[], SETUP_LIMIT);
            std::thread::sleep(std::time::Duration::from_secs(2));
            if self.sshd_listening() {
                Outcome::Changed("Remote Login is on".into())
            } else {
                Outcome::Failed(format!("could not turn Remote Login on ({}); use System Settings, General, Sharing", out.stderr.trim()))
            }
        };
        results.push(("Remote Login", remote_login));

        say("==> Key folder");
        let out = self.sh("install -d -m 700 \"$HOME/.ssh\" && touch \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\" && echo ok");
        results.push((
            "Key folder",
            if out.ok { Outcome::Done("~/.ssh ready".into()) } else { Outcome::Failed(out.stderr.trim().to_string()) },
        ));
        results
    }

    fn host_key(&self) -> String {
        host_key_from_pub(&std::fs::read_to_string("/etc/ssh/ssh_host_ed25519_key.pub").unwrap_or_default())
    }

    fn install_key(&self, key: &str, mark: &str) -> Result<Installed, String> {
        let user = super::platform::checked_user(self.user())?;
        let out = run("/bin/sh", &[], Some(&super::platform::authorized_keys_script(&user, key, mark)), &[], CHECK_LIMIT);
        if !out.ok || !out.stdout.contains("dockernanny-key-ok") {
            return Err(format!("could not write authorized_keys: {}", out.stderr.trim()));
        }
        Ok(Installed { user, port: SSH_PORT, hostname: self.hostname(), host_key: self.host_key() })
    }

    fn remove_key(&self, mark: &str) -> Result<(), String> {
        let user = super::platform::checked_user(self.user())?;
        let out = run("/bin/sh", &[], Some(&super::platform::forget_key_script(&user, mark)), &[], CHECK_LIMIT);
        if !out.ok || !out.stdout.contains("dockernanny-key-gone") {
            return Err(format!("could not change authorized_keys: {}", out.stderr.trim()));
        }
        Ok(())
    }

    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        // Nothing idles out on a Mac; sleep is the user's power setting.
        Ok(None)
    }

    fn established_peers(&self, port: u16) -> Vec<String> {
        super::paired::parse_established(&self.sh("netstat -an -p tcp").stdout, port)
    }

    fn hostname(&self) -> String {
        let name = self.sh("scutil --get ComputerName").text();
        if name.is_empty() {
            "mac".into()
        } else {
            name
        }
    }
}
