//! A pretend Windows computer for `DOCKERNANNY_FAKE_HOST=1`: starts half
//! prepared, and every setup step flips its flag, so the whole sharing flow
//! can be watched in the real app without touching the system.

use std::process::Child;
use std::sync::Mutex;

use super::platform::{row, Installed, NetworkProfile, Outcome, Picture, Platform, Say, SetupOptions};

#[derive(Default)]
pub struct Fake {
    state: Mutex<FakeState>,
}

#[derive(Default)]
struct FakeState {
    docker: bool,
    sshd: bool,
    firewall: bool,
    network_private: bool,
}

impl Platform for Fake {
    fn os_name(&self) -> &'static str {
        "fake Windows"
    }

    fn probe(&self) -> Picture {
        let state = self.state.lock().expect("fake lock");
        let mut picture = Picture {
            ssh_port: 2222,
            total_memory_gb: 15,
            user: Some("alex".into()),
            sshd_listening: state.sshd,
            ready_for_pairing: state.firewall,
            network: Some(NetworkProfile {
                name: "Home Wi-Fi".into(),
                public: !state.network_private,
            }),
            ..Default::default()
        };
        picture.rows.push(row("Windows", true, "build 22631"));
        picture.rows.push(row("WSL 2", true, "2.6.1.0"));
        picture.rows.push(row("Ubuntu", true, "installed"));
        picture.rows.push(row("Linux user", true, "alex"));
        picture.rows.push(row("Docker Engine", state.docker, if state.docker { "29.8.1" } else { "not running or not installed" }));
        picture.rows.push(row("SSH server", state.sshd, if state.sshd { "listening on 2222" } else { "not listening on 2222" }));
        picture.rows.push(row("rsync", true, "installed"));
        picture.rows.push(row("Firewall", state.firewall, if state.firewall { "ports open" } else { "ports closed (Set up opens them)" }));
        picture.rows.push(row("Network", state.network_private, if state.network_private { "Home Wi-Fi (Private)" } else { "Home Wi-Fi is marked Public, which blocks the firewall rules" }));
        picture
    }

    fn setup(&self, options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        let mut results = Vec::new();
        let network = match &options.make_network_private {
            Some(name) => format!("firewall open, {name} marked Private"),
            None => "firewall open".to_string(),
        };
        for (name, seconds, outcome) in [
            ("Windows version", 1, Outcome::Done("build 22631".into())),
            ("Docker, sshd, rsync inside Ubuntu", 2, Outcome::Changed("Docker, sshd and rsync ready".into())),
            ("WSL settings", 1, Outcome::Changed(format!("Docker may use {} GB", options.memory_gb))),
            ("Firewall and power (administrator)", 1, Outcome::Changed(network)),
        ] {
            say(&format!("==> {name}"));
            std::thread::sleep(std::time::Duration::from_secs(seconds));
            say(&format!("    done: {}", match &outcome { Outcome::Done(t) | Outcome::Changed(t) => t.clone(), _ => String::new() }));
            results.push((name, outcome));
        }
        let mut state = self.state.lock().expect("fake lock");
        state.docker = true;
        state.sshd = true;
        state.firewall = true;
        state.network_private = state.network_private || options.make_network_private.is_some();
        results
    }

    fn host_key(&self) -> String {
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFakeHostKeyFakeHostKeyFakeHostKeyFakeHo".into()
    }

    fn install_key(&self, key: &str) -> Result<Installed, String> {
        let path = crate::store::home_dir().join("fake-authorized_keys");
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut f| {
                use std::io::Write;
                writeln!(f, "{key}")
            })
            .map_err(|e| e.to_string())?;
        Ok(Installed {
            user: "alex".into(),
            port: 2222,
            hostname: "fake-machine".into(),
            host_key: self.host_key(),
        })
    }

    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        Ok(None)
    }

    fn established_peers(&self, _port: u16) -> Vec<String> {
        // Someone is always "connected" once sshd is up, so the list can be seen.
        if self.state.lock().expect("fake lock").sshd { vec!["192.0.2.20".into()] } else { Vec::new() }
    }

    fn lan_ipv4(&self) -> Vec<String> {
        vec!["192.0.2.10".into()]
    }

    fn hostname(&self) -> String {
        "fake-machine".into()
    }
}
