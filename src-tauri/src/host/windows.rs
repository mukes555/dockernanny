//! Windows: Docker lives inside a WSL 2 distribution (the one chosen in
//! Settings), so everything the other computer needs is reached through
//! `wsl.exe`. Probes here, steps in `windows_steps.rs`.

use std::path::PathBuf;
use std::process::Child;

use super::platform::{host_key_from_pub, parse_ipconfig, row, run, Installed, NetworkProfile, Outcome, Output, Picture, Platform, Row, Say, SetupOptions, State};
use super::{windows_steps, MIN_WINDOWS_BUILD};

pub struct Windows {
    /// The WSL distribution that runs Docker and sshd.
    pub distro: String,
    /// Where sshd inside it listens.
    pub ssh_port: u16,
}

impl Windows {
    pub fn wsl(&self, args: &[&str]) -> Output {
        run("wsl.exe", args, None, &[("WSL_UTF8", "1")])
    }

    /// A POSIX shell script inside the distro as root, delivered on stdin.
    pub fn in_distro_as_root(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", &self.distro, "-u", "root", "--", "sh"], Some(script), &[("WSL_UTF8", "1")])
    }

    /// The same as the distro's default user, with a login shell for PATH.
    pub fn in_distro(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", &self.distro, "--", "sh", "-l"], Some(script), &[("WSL_UTF8", "1")])
    }

    /// Whether `wsl -l -q` lists the chosen distribution.
    pub fn has_distro(&self) -> bool {
        self.wsl(&["-l", "-q"]).stdout.lines().any(|l| l.trim() == self.distro)
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
        let user = self.wsl(&["-d", &self.distro, "--", "whoami"]).text();
        (!user.is_empty() && user != "root").then_some(user)
    }

    pub fn firewall_rule_exists(&self, name: &str) -> bool {
        run("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={name}")], None, &[]).ok
    }

    /// The port a firewall rule opens, None when there is no such rule. Read
    /// without admin rights, so Set up knows whether a changed port needs
    /// it; through PowerShell objects because netsh's text is translated.
    /// `name` is one of this app's fixed rule names.
    pub fn firewall_rule_port(&self, name: &str) -> Option<u16> {
        let script = format!("Get-NetFirewallRule -DisplayName '{name}' -ErrorAction SilentlyContinue | Get-NetFirewallPortFilter | Select-Object -First 1 -ExpandProperty LocalPort");
        self.powershell(&script).text().parse().ok()
    }

    /// Both rules there and on the ports asked for.
    pub fn firewall_matches(&self, pairing_port: u16) -> bool {
        let ssh_ok = self.firewall_rule_port("dockerNanny SSH") == Some(self.ssh_port);
        let pair_ok = self.firewall_rule_port("dockerNanny Pair") == Some(pairing_port);
        ssh_ok && pair_ok
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
            ssh_port: self.ssh_port,
            total_memory_gb: self.total_memory_gb(),
            ..Default::default()
        };
        let port = self.ssh_port;
        let build = self.windows_build();
        picture.rows.push(row("Windows", build >= MIN_WINDOWS_BUILD, if build == 0 { "version unknown".into() } else { format!("build {build}") }));

        let wsl = self.wsl(&["--version"]);
        let version = wsl.stdout.lines().find(|l| l.starts_with("WSL version")).and_then(|l| l.split(':').nth(1)).map(|v| v.trim().to_string());
        let wsl_ok = version.as_deref().map(|v| !v.starts_with("1.") && !v.starts_with("0.")).unwrap_or(false);
        picture.rows.push(row("WSL 2", wsl_ok, version.unwrap_or_else(|| "not installed (Set up installs it)".into())));
        if !wsl_ok {
            return picture;
        }

        let has_distro = self.has_distro();
        let distro_detail = if has_distro { format!("{} installed", self.distro) } else { format!("{} not installed (Set up installs it)", self.distro) };
        picture.rows.push(row("Linux distribution", has_distro, distro_detail));
        if !has_distro {
            return picture;
        }

        let user = self.distro_user();
        picture.rows.push(row("Linux user", user.is_some(), user.clone().unwrap_or_else(|| "none yet (Set up creates one)".into())));
        picture.user = user;

        let docker = self.in_distro("docker version --format '{{.Server.Version}}' 2>/dev/null");
        let docker_ok = docker.ok && !docker.text().is_empty();
        picture.rows.push(row("Docker Engine", docker_ok, if docker_ok { docker.text() } else { "not running or not installed".into() }));

        let listening = self.in_distro("ss -ltn 2>/dev/null").stdout.contains(&format!(":{port} "));
        picture.sshd_listening = listening;
        picture.rows.push(row("SSH server", listening, if listening { format!("listening on {port}") } else { format!("not listening on {port}") }));

        let rsync = self.in_distro("command -v rsync");
        picture.rows.push(row("rsync", rsync.ok && !rsync.text().is_empty(), if rsync.ok { "installed" } else { "missing" }));

        // Pairing may listen once both rules exist; a port changed since only needs Set up again.
        let firewall_open = self.firewall_rule_exists("dockerNanny SSH") && self.firewall_rule_exists("dockerNanny Pair");
        picture.ready_for_pairing = firewall_open;
        let ssh_rule_port = self.firewall_rule_port("dockerNanny SSH");
        let firewall_detail = match ssh_rule_port {
            Some(open) if open == port => "ports open".to_string(),
            Some(open) => format!("open for {open}, not {port} (Set up again updates it)"),
            None => "ports closed (Set up opens them)".to_string(),
        };
        picture.rows.push(row("Firewall", firewall_open && ssh_rule_port == Some(port), firewall_detail));

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
        let user = self.distro_user().ok_or_else(|| format!("{} has no user yet; run Set up first", self.distro))?;
        let out = self.in_distro_as_root(&super::platform::authorized_keys_script(&user, key));
        if !out.ok || !out.stdout.contains("dockernanny-key-ok") {
            return Err(format!("could not write authorized_keys: {}", out.stderr.trim()));
        }
        Ok(Installed {
            user,
            port: self.ssh_port,
            hostname: self.hostname(),
            host_key: self.host_key(),
        })
    }

    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        use std::os::windows::process::CommandExt;
        let child = std::process::Command::new("wsl.exe")
            .args(["-d", &self.distro, "-e", "sleep", "infinity"])
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
        super::paired::parse_established(&self.wsl(&["-d", &self.distro, "--", "ss", "-tn"]).stdout, port)
    }

    fn lan_ipv4(&self) -> Vec<String> {
        parse_ipconfig(&run("ipconfig", &[], None, &[]).stdout)
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".into())
    }
}
