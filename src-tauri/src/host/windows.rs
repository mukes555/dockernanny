//! Windows: Docker lives inside WSL2 Ubuntu, so everything the other
//! computer needs is reached through `wsl.exe`. Probes here, steps in
//! `windows_steps.rs`.

use std::path::PathBuf;
use std::process::Child;

use super::platform::{host_key_from_pub, row, run, Installed, NetworkProfile, Outcome, Output, Picture, Platform, Row, Say, SetupOptions, State};
use super::{windows_steps, DISTRO, MIN_WINDOWS_BUILD, WSL_SSH_PORT};

pub struct Windows;

impl Windows {
    pub fn wsl(&self, args: &[&str]) -> Output {
        run("wsl.exe", args, None, &[("WSL_UTF8", "1")])
    }

    /// A POSIX shell script inside the distro as root, delivered on stdin.
    pub fn in_distro_as_root(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", DISTRO, "-u", "root", "--", "sh"], Some(script), &[("WSL_UTF8", "1")])
    }

    /// The same as the distro's default user, with a login shell for PATH.
    pub fn in_distro(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", DISTRO, "--", "sh", "-l"], Some(script), &[("WSL_UTF8", "1")])
    }

    pub fn powershell(&self, script: &str) -> Output {
        run("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script], None, &[])
    }

    pub fn windows_build(&self) -> u32 {
        self.powershell("[System.Environment]::OSVersion.Version.Build").text().parse().unwrap_or(0)
    }

    pub fn user_profile(&self) -> PathBuf {
        PathBuf::from(std::env::var("USERPROFILE").unwrap_or_else(|_| r"C:\Users\Default".into()))
    }

    pub fn distro_user(&self) -> Option<String> {
        let user = self.wsl(&["-d", DISTRO, "--", "whoami"]).text();
        (!user.is_empty() && user != "root").then_some(user)
    }

    pub fn firewall_rule_exists(&self, name: &str) -> bool {
        run("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={name}")], None, &[]).ok
    }

    fn total_memory_gb(&self) -> u32 {
        let bytes: u64 = self.powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory").text().parse().unwrap_or(0);
        (bytes / (1024 * 1024 * 1024)) as u32
    }

    /// The connected network and its category, as `Name|Public` or `Name|Private`.
    fn network_profile(&self) -> Option<NetworkProfile> {
        let out = self.powershell("Get-NetConnectionProfile | Select-Object -First 1 | ForEach-Object { \"$($_.Name)|$($_.NetworkCategory)\" }");
        let text = out.text();
        let (name, category) = text.split_once('|')?;
        Some(NetworkProfile {
            name: name.trim().to_string(),
            public: category.trim().eq_ignore_ascii_case("public"),
        })
    }
}

impl Platform for Windows {
    fn os_name(&self) -> &'static str {
        "Windows"
    }

    fn probe(&self) -> Picture {
        let mut picture = Picture {
            ssh_port: WSL_SSH_PORT,
            total_memory_gb: self.total_memory_gb(),
            ..Default::default()
        };
        let build = self.windows_build();
        picture.rows.push(row("Windows", build >= MIN_WINDOWS_BUILD, if build == 0 { "version unknown".into() } else { format!("build {build}") }));

        let wsl = self.wsl(&["--version"]);
        let version = wsl.stdout.lines().find(|l| l.starts_with("WSL version")).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string());
        let wsl_ok = version.as_deref().map(|v| !v.starts_with("1.") && !v.starts_with("0.")).unwrap_or(false);
        picture.rows.push(row("WSL 2", wsl_ok, version.unwrap_or_else(|| "not installed (Set up installs it)".into())));
        if !wsl_ok {
            return picture;
        }

        let has_distro = self.wsl(&["-l", "-q"]).stdout.lines().any(|l| l.trim() == DISTRO);
        picture.rows.push(row("Ubuntu", has_distro, if has_distro { "installed" } else { "not installed (Set up installs it)" }));
        if !has_distro {
            return picture;
        }

        let user = self.distro_user();
        picture.rows.push(row("Linux user", user.is_some(), user.clone().unwrap_or_else(|| "none yet (Set up creates one)".into())));
        picture.user = user;

        let docker = self.in_distro("docker version --format '{{.Server.Version}}' 2>/dev/null");
        let docker_ok = docker.ok && !docker.text().is_empty();
        picture.rows.push(row("Docker Engine", docker_ok, if docker_ok { docker.text() } else { "not running or not installed".into() }));

        let listening = self.in_distro("ss -ltn 2>/dev/null").stdout.contains(&format!(":{WSL_SSH_PORT} "));
        picture.sshd_listening = listening;
        picture.rows.push(row("SSH server", listening, if listening { format!("listening on {WSL_SSH_PORT}") } else { format!("not listening on {WSL_SSH_PORT}") }));

        let rsync = self.in_distro("command -v rsync");
        picture.rows.push(row("rsync", rsync.ok && !rsync.text().is_empty(), if rsync.ok { "installed" } else { "missing" }));

        let firewall_open = self.firewall_rule_exists("dockerNanny SSH") && self.firewall_rule_exists("dockerNanny Pair");
        picture.ready_for_pairing = firewall_open;
        picture.rows.push(row("Firewall", firewall_open, if firewall_open { "ports open" } else { "ports closed (Set up opens them)" }));

        picture.network = self.network_profile();
        if let Some(network) = &picture.network {
            let detail = if network.public { format!("{} is marked Public, which blocks the firewall rules", network.name) } else { format!("{} (Private)", network.name) };
            picture.rows.push(row("Network", !network.public, detail));
        }

        let wslconfig = std::fs::read_to_string(self.user_profile().join(".wslconfig")).unwrap_or_default();
        let keeps_running = wslconfig.contains("instanceIdleTimeout=-1") && wslconfig.contains("networkingMode=mirrored");
        picture.rows.push(Row {
            name: "WSL stays up",
            state: if keeps_running { State::Ok } else { State::Unknown },
            detail: if keeps_running { "configured".into() } else { "not configured yet".into() },
        });
        picture
    }

    fn setup(&self, options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        windows_steps::run_all(self, options, say)
    }

    fn host_key(&self) -> String {
        host_key_from_pub(&self.in_distro_as_root("cat /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null").stdout)
    }

    fn install_key(&self, key: &str) -> Result<Installed, String> {
        let user = self.distro_user().ok_or("Ubuntu has no user yet; run Set up first")?;
        let out = self.in_distro_as_root(&super::platform::authorized_keys_script(&user, key));
        if !out.ok || !out.stdout.contains("dockernanny-key-ok") {
            return Err(format!("could not write authorized_keys: {}", out.stderr.trim()));
        }
        Ok(Installed {
            user,
            port: WSL_SSH_PORT,
            hostname: self.hostname(),
            host_key: self.host_key(),
        })
    }

    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        use std::os::windows::process::CommandExt;
        let child = std::process::Command::new("wsl.exe")
            .args(["-d", DISTRO, "-e", "sleep", "infinity"])
            .creation_flags(0x0800_0000)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Some(child))
    }

    fn established_peers(&self, port: u16) -> Vec<String> {
        // sshd lives inside the distro, so its connection table is there too.
        super::paired::parse_established(&self.wsl(&["-d", DISTRO, "--", "ss", "-tn"]).stdout, port)
    }

    fn lan_ipv4(&self) -> Vec<String> {
        parse_ipconfig(&run("ipconfig", &[], None, &[]).stdout)
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".into())
    }
}

/// `   IPv4 Address. . . . . . . . . . . : 192.0.2.15` lines from ipconfig,
/// skipping the virtual adapters that only matter to WSL itself.
pub fn parse_ipconfig(text: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    let mut in_virtual_adapter = false;
    for line in text.lines() {
        let is_adapter_header = !line.starts_with(' ') && line.contains("adapter");
        if is_adapter_header {
            in_virtual_adapter = line.contains("vEthernet") || line.contains("Hyper-V") || line.contains("VirtualBox") || line.contains("VMware");
            continue;
        }
        if in_virtual_adapter || !line.contains("IPv4") {
            continue;
        }
        let Some(address) = line.rsplit(':').next().map(str::trim) else { continue };
        let address = address.trim_end_matches("(Preferred)").trim();
        let usable = address.contains('.') && !address.starts_with("127.") && !address.starts_with("169.254.");
        if usable && !addresses.iter().any(|a| a == address) {
            addresses.push(address.to_string());
        }
    }
    addresses
}
