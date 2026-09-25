//! A Linux box as the shared computer: Docker Engine, sshd on 22, rsync.
//! Everything that needs root is one script; it runs through `pkexec` when
//! a desktop session can show the password prompt, and is shown for the
//! user to paste into a terminal otherwise.

use std::process::Child;

use super::platform::{host_key_from_pub, parse_ifconfig, row, run, Installed, Outcome, Output, Picture, Platform, Say, SetupOptions};

const SSH_PORT: u16 = 22;
const ROOT_SCRIPT: &str = "set -e\n\
export DEBIAN_FRONTEND=noninteractive\n\
if command -v apt-get >/dev/null 2>&1; then apt-get update -qq && apt-get install -y -qq openssh-server rsync curl ca-certificates; fi\n\
if command -v dnf >/dev/null 2>&1; then dnf install -y -q openssh-server rsync curl ca-certificates; fi\n\
if command -v docker >/dev/null 2>&1; then echo \"docker already installed: $(docker --version)\"; else curl -fsSL https://get.docker.com | sh; fi\n\
usermod -aG docker \"$DOCKERNANNY_USER\"\n\
systemctl enable --now ssh 2>/dev/null || systemctl enable --now sshd\n\
systemctl enable --now docker\n\
echo dockernanny-linux-ok\n";

pub struct Linux;

impl Linux {
    fn sh(&self, script: &str) -> Output {
        run("/bin/sh", &["-c", script], None, &[])
    }

    fn user(&self) -> String {
        std::env::var("USER").unwrap_or_else(|_| self.sh("whoami").text())
    }

    fn docker_version(&self) -> Option<String> {
        let out = self.sh("docker version --format '{{.Server.Version}}' 2>/dev/null");
        (out.ok && !out.text().is_empty()).then(|| out.text())
    }

    fn sshd_listening(&self) -> bool {
        let sockets = self.sh("ss -ltn 2>/dev/null || netstat -ltn 2>/dev/null").stdout;
        sockets.lines().any(|line| line.split_whitespace().any(|field| field.ends_with(&format!(":{SSH_PORT}"))))
    }

    fn rsync_present(&self) -> bool {
        self.sh("command -v rsync").ok
    }

    /// A polkit agent answers `pkexec` only inside a desktop session.
    fn can_prompt_for_root(&self) -> bool {
        let desktop = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
        desktop && self.sh("command -v pkexec").ok
    }
}

impl Platform for Linux {
    fn os_name(&self) -> &'static str {
        "Linux"
    }

    fn probe(&self) -> Picture {
        let kb: u64 = self
            .sh("awk '/MemTotal/ {print $2}' /proc/meminfo")
            .text()
            .parse()
            .unwrap_or(0);
        let mut picture = Picture {
            ssh_port: SSH_PORT,
            total_memory_gb: (kb / (1024 * 1024)) as u32,
            user: Some(self.user()),
            ready_for_pairing: true,
            ..Default::default()
        };
        let release = self.sh(". /etc/os-release 2>/dev/null && echo \"$PRETTY_NAME\"").text();
        picture.rows.push(row("Linux", !release.is_empty(), if release.is_empty() { "unknown distribution".into() } else { release }));

        let docker = self.docker_version();
        picture.rows.push(row("Docker Engine", docker.is_some(), docker.unwrap_or_else(|| "not running or not installed".into())));

        let listening = self.sshd_listening();
        picture.sshd_listening = listening;
        picture.rows.push(row("SSH server", listening, if listening { format!("listening on {SSH_PORT}") } else { format!("not listening on {SSH_PORT}") }));

        let rsync = self.rsync_present();
        picture.rows.push(row("rsync", rsync, if rsync { "installed" } else { "missing" }));
        picture
    }

    fn setup(&self, _options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        let mut results = Vec::new();
        let user = self.user();

        say("==> Docker, sshd, rsync");
        let all_present = self.docker_version().is_some() && self.sshd_listening() && self.rsync_present();
        let root_step = if all_present {
            Outcome::Done("Docker, sshd and rsync present".into())
        } else if self.can_prompt_for_root() {
            say("    the system asks for your password (pkexec)");
            let out = run("pkexec", &["env", &format!("DOCKERNANNY_USER={user}"), "sh", "-c", ROOT_SCRIPT], None, &[]);
            for line in out.stdout.lines().chain(out.stderr.lines()).filter(|l| !l.trim().is_empty()).take(40) {
                say(&format!("    | {line}"));
            }
            if out.ok && out.stdout.contains("dockernanny-linux-ok") {
                Outcome::Changed("Docker, sshd and rsync ready; log out and back in once so the docker group applies".into())
            } else {
                Outcome::Failed("the root script did not finish; see the lines above".into())
            }
        } else {
            let script = ROOT_SCRIPT.replace("\"$DOCKERNANNY_USER\"", &user);
            Outcome::NeedsUser(format!("Run this as root in a terminal, then click Set up again:\n\n{script}"))
        };
        let stop = matches!(root_step, Outcome::Failed(_) | Outcome::NeedsUser(_));
        results.push(("Docker, sshd, rsync", root_step));
        if stop {
            return results;
        }

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
        let out = run("/bin/sh", &[], Some(&super::platform::authorized_keys_script(&user, key)), &[]);
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
        Ok(None)
    }

    fn established_peers(&self, port: u16) -> Vec<String> {
        super::paired::parse_established(&self.sh("ss -tn 2>/dev/null || netstat -tn").stdout, port)
    }

    fn lan_ipv4(&self) -> Vec<String> {
        parse_ifconfig(&self.sh("ip -4 -o addr 2>/dev/null || ifconfig").stdout)
    }

    fn hostname(&self) -> String {
        let name = self.sh("hostname").text();
        if name.is_empty() {
            "linux".into()
        } else {
            name
        }
    }
}
