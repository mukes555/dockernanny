//! A Mac as the shared computer: no WSL, just Remote Login (sshd on 22),
//! Docker Desktop or OrbStack, and rsync, which ships with macOS. The one
//! admin step is a single command through the system password prompt.

use std::process::Child;

use super::platform::{host_key_from_pub, parse_ifconfig, row, run, Installed, Outcome, Output, Picture, Platform, Say, SetupOptions};

const SSH_PORT: u16 = 22;
/// Docker Desktop and OrbStack put their CLI here; a GUI app's PATH does not include it.
const PATH: &str = "/usr/local/bin:/opt/homebrew/bin:/opt/orbstack/bin:/usr/bin:/bin:/usr/sbin:/sbin";

pub struct MacOs;

impl MacOs {
    fn sh(&self, script: &str) -> Output {
        run("/bin/sh", &["-c", script], None, &[("PATH", PATH)])
    }

    fn user(&self) -> String {
        std::env::var("USER").unwrap_or_else(|_| self.sh("whoami").text())
    }

    fn sshd_listening(&self) -> bool {
        self.sh("nc -z 127.0.0.1 22 >/dev/null 2>&1 && echo yes").text() == "yes"
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
        let bytes: u64 = self.sh("sysctl -n hw.memsize").text().parse().unwrap_or(0);
        let mut picture = Picture {
            ssh_port: SSH_PORT,
            total_memory_gb: (bytes / (1024 * 1024 * 1024)) as u32,
            user: Some(self.user()),
            ready_for_pairing: true,
            ..Default::default()
        };
        let version = self.sh("sw_vers -productVersion").text();
        picture.rows.push(row("macOS", !version.is_empty(), if version.is_empty() { "unknown".into() } else { version }));

        let docker = self.docker_version();
        let docker_detail = docker.clone().unwrap_or_else(|| "not running: install Docker Desktop or OrbStack and open it".to_string());
        picture.rows.push(row("Docker", docker.is_some(), docker_detail));

        let listening = self.sshd_listening();
        picture.sshd_listening = listening;
        picture.rows.push(row("Remote Login (SSH)", listening, if listening { "on, port 22" } else { "off (Set up turns it on)" }));

        let rsync = self.sh("command -v rsync");
        picture.rows.push(row("rsync", rsync.ok, if rsync.ok { "installed" } else { "missing" }));
        picture
    }

    fn setup(&self, _options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        let mut results = Vec::new();

        say("==> Docker");
        let docker = match self.docker_version() {
            Some(version) => Outcome::Done(format!("Docker {version}")),
            None => Outcome::NeedsUser("Install Docker Desktop (docker.com) or OrbStack (orbstack.dev), open it once, then click Set up again.".into()),
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
            let out = self.sh("osascript -e 'do shell script \"systemsetup -setremotelogin on\" with administrator privileges' 2>&1");
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
        results.push(("Key folder", if out.ok { Outcome::Done("~/.ssh ready".into()) } else { Outcome::Failed(out.stderr.trim().to_string()) }));
        results
    }

    fn host_key(&self) -> String {
        host_key_from_pub(&std::fs::read_to_string("/etc/ssh/ssh_host_ed25519_key.pub").unwrap_or_default())
    }

    fn install_key(&self, key: &str) -> Result<Installed, String> {
        let user = self.user();
        let out = run("/bin/sh", &[], Some(&super::platform::authorized_keys_script(&user, key)), &[("PATH", PATH)]);
        if !out.ok || !out.stdout.contains("dockernanny-key-ok") {
            return Err(format!("could not write authorized_keys: {}", out.stderr.trim()));
        }
        Ok(Installed {
            user,
            port: SSH_PORT,
            hostname: self.hostname(),
            host_key: self.host_key(),
        })
    }

    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        // Nothing idles out on a Mac; sleep is the user's power setting.
        Ok(None)
    }

    fn established_peers(&self, port: u16) -> Vec<String> {
        super::paired::parse_established(&self.sh("netstat -an -p tcp").stdout, port)
    }

    fn lan_ipv4(&self) -> Vec<String> {
        parse_ifconfig(&self.sh("ifconfig").stdout)
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
