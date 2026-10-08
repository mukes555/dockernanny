//! Windows: Docker lives inside a WSL 2 distribution (the one chosen in
//! Settings), so everything the other computer needs is reached through
//! `wsl.exe`. Probes here, steps in `windows_steps.rs`.

use std::path::PathBuf;
use std::process::Child;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::platform::{
    host_key_from_pub, parse_rule, row, run, FirewallRules, Installed, NetworkProfile, Outcome, Output, Picture, Platform, Row, Rule, Say,
    SetupOptions, State, CHECK_LIMIT, SETUP_LIMIT,
};
use super::{windows_steps, wsl_script, MIN_WINDOWS_BUILD};

/// PowerShell and wsl.exe are the dearest things this app starts, and the
/// sharing page probes every ten seconds. What rarely changes is kept this
/// long once all of it is fine, and read again after Set up.
const STEADY_FOR: Duration = Duration::from_secs(10 * 60);
/// The firewall rules and the network's category, once both are fine. Ten
/// minutes, like the rest: at 60 s a hidden window, which probes every
/// minute, read them afresh each time with two PowerShell starts.
const NETWORK_FOR: Duration = Duration::from_secs(10 * 60);

pub struct Windows {
    /// The WSL distribution that runs Docker and sshd.
    pub distro: String,
    /// Where sshd inside it listens.
    pub ssh_port: u16,
    /// How long one wsl.exe or PowerShell call may take: short for probes,
    /// long for Set up, which installs WSL, packages and Docker.
    limit: Duration,
    steady: Mutex<Option<Steady>>,
    network: Mutex<Option<Network>>,
}

/// What rarely changes: the Windows build, WSL, the distribution and its
/// user, rsync inside it, the memory.
#[derive(Clone)]
struct Steady {
    read_at: Instant,
    total_memory_gb: u32,
    user: Option<String>,
    /// Windows, WSL 2, the distribution and its user, in the page's order.
    rows: Vec<Row>,
    /// Shown later on the page, after the SSH server.
    rsync: Option<Row>,
    /// WSL and the distribution answer, so the rest can be asked.
    distro_ready: bool,
}

#[derive(Clone)]
struct Network {
    read_at: Instant,
    rules: FirewallRules,
    profile: Option<NetworkProfile>,
}

/// Kept only when every row is fine: while something is missing the user
/// is likely fixing it, and the page must see the change at once.
fn steady_worth_keeping(steady: &Steady) -> bool {
    let rows_fine = steady.rows.iter().chain(steady.rsync.iter()).all(|row| row.state == State::Ok);
    steady.distro_ready && rows_fine
}

fn network_worth_keeping(rules: &FirewallRules, profile: Option<&NetworkProfile>) -> bool {
    let rules_there = rules.ssh != Rule::Missing && rules.pairing != Rule::Missing;
    let network_private = profile.map(|p| !p.public).unwrap_or(true);
    rules_there && network_private
}

impl Windows {
    pub fn new(distro: String, ssh_port: u16) -> Self {
        Self { distro, ssh_port, limit: CHECK_LIMIT, steady: Mutex::new(None), network: Mutex::new(None) }
    }

    /// The same computer, patient enough for Set up's installs.
    pub fn for_setup(distro: String, ssh_port: u16) -> Self {
        Self { limit: SETUP_LIMIT, ..Self::new(distro, ssh_port) }
    }

    fn steady(&self) -> Steady {
        let kept = self.steady.lock().expect("steady lock").clone().filter(|s| s.read_at.elapsed() < STEADY_FOR);
        if let Some(kept) = kept {
            return kept;
        }
        let fresh = self.read_steady();
        *self.steady.lock().expect("steady lock") = steady_worth_keeping(&fresh).then(|| fresh.clone());
        fresh
    }

    fn read_steady(&self) -> Steady {
        let mut steady = Steady {
            read_at: Instant::now(),
            total_memory_gb: self.total_memory_gb(),
            user: None,
            rows: Vec::new(),
            rsync: None,
            distro_ready: false,
        };
        let build = self.windows_build();
        steady.rows.push(row(
            "Windows",
            build >= MIN_WINDOWS_BUILD,
            if build == 0 { "version unknown".into() } else { format!("build {build}") },
        ));

        // Read by its number, not its label: `wsl --version` is translated.
        let version = wsl_script::wsl_version(&self.wsl(&["--version"]).stdout);
        let wsl_ok = version.as_deref().is_some_and(wsl_script::is_wsl_two);
        let wsl_detail = match &version {
            Some(version) if wsl_ok => version.clone(),
            Some(version) => format!("{version} is older than 2.0 (Set up updates it)"),
            None => "not installed (Set up installs it)".into(),
        };
        steady.rows.push(row("WSL 2", wsl_ok, wsl_detail));
        if !wsl_ok {
            return steady;
        }

        let has_distro = self.has_distro();
        let distro_detail =
            if has_distro { format!("{} installed", self.distro) } else { format!("{} not installed (Set up installs it)", self.distro) };
        steady.rows.push(row("Linux distribution", has_distro, distro_detail));
        if !has_distro {
            return steady;
        }

        let user = self.distro_user();
        steady.rows.push(row("Linux user", user.is_some(), user.clone().unwrap_or_else(|| "none yet (Set up creates one)".into())));
        steady.user = user;

        let rsync = self.in_distro("command -v rsync");
        steady.rsync = Some(row("rsync", rsync.ok && !rsync.text().is_empty(), if rsync.ok { "installed" } else { "missing" }));
        steady.distro_ready = true;
        steady
    }

    fn firewall_and_network(&self) -> (FirewallRules, Option<NetworkProfile>) {
        let kept = self.network.lock().expect("network lock").clone().filter(|n| n.read_at.elapsed() < NETWORK_FOR);
        if let Some(kept) = kept {
            return (kept.rules, kept.profile);
        }
        let rules = self.firewall_rules();
        let profile = self.network_profile();
        let keep = network_worth_keeping(&rules, profile.as_ref());
        *self.network.lock().expect("network lock") = keep.then(|| Network { read_at: Instant::now(), rules, profile: profile.clone() });
        (rules, profile)
    }

    /// Set up may have changed any of it.
    fn forget_what_was_read(&self) {
        *self.steady.lock().expect("steady lock") = None;
        *self.network.lock().expect("network lock") = None;
    }

    pub fn wsl(&self, args: &[&str]) -> Output {
        run("wsl.exe", args, None, &[("WSL_UTF8", "1")], self.limit)
    }

    /// A POSIX shell script inside the distro as root, delivered on stdin.
    pub fn in_distro_as_root(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", &self.distro, "-u", "root", "--", "sh"], Some(script), &[("WSL_UTF8", "1")], self.limit)
    }

    /// The same as the distro's default user, with a login shell for PATH.
    pub fn in_distro(&self, script: &str) -> Output {
        run("wsl.exe", &["-d", &self.distro, "--", "sh", "-l"], Some(script), &[("WSL_UTF8", "1")], self.limit)
    }

    /// Whether `wsl -l -q` lists the chosen distribution.
    /// WSL matches distribution names without regard to case, and so does this.
    pub fn has_distro(&self) -> bool {
        self.wsl(&["-l", "-q"]).stdout.lines().any(|l| l.trim().eq_ignore_ascii_case(&self.distro))
    }

    pub fn powershell(&self, script: &str) -> Output {
        run("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script], None, &[], self.limit)
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
        run("netsh", &["advfirewall", "firewall", "show", "rule", &format!("name={name}")], None, &[], self.limit).ok
    }

    /// This app's two firewall rules, read in one PowerShell call without
    /// admin rights (PowerShell objects, because netsh's text is translated).
    /// When PowerShell cannot answer, netsh still tells whether each exists.
    pub fn firewall_rules(&self) -> FirewallRules {
        let script = "foreach ($name in 'dockerNanny SSH', 'dockerNanny Pair') { $rule = Get-NetFirewallRule -DisplayName $name -ErrorAction SilentlyContinue; if ($rule) { $port = ($rule | Get-NetFirewallPortFilter | Select-Object -First 1).LocalPort; \"$name=$port\" } else { \"$name=\" } }";
        let out = self.powershell(script);
        let answered = out.ok && out.stdout.contains("dockerNanny SSH=");
        if !answered {
            let fallback = |name: &str| if self.firewall_rule_exists(name) { Rule::Open(None) } else { Rule::Missing };
            return FirewallRules { ssh: fallback("dockerNanny SSH"), pairing: fallback("dockerNanny Pair") };
        }
        FirewallRules { ssh: parse_rule(&out.stdout, "dockerNanny SSH"), pairing: parse_rule(&out.stdout, "dockerNanny Pair") }
    }

    /// Both rules there and, as far as can be read, on the ports asked for.
    pub fn firewall_matches(&self, pairing_port: u16) -> bool {
        let rules = self.firewall_rules();
        rules.ssh.opens(self.ssh_port) && rules.pairing.opens(pairing_port)
    }

    pub(super) fn total_memory_gb(&self) -> u32 {
        let bytes: u64 = self.powershell("(Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory").text().parse().unwrap_or(0);
        (bytes / (1024 * 1024 * 1024)) as u32
    }

    /// The connected network and its category, as `Name|Public` or `Name|Private`.
    fn network_profile(&self) -> Option<NetworkProfile> {
        let out =
            self.powershell("Get-NetConnectionProfile | Select-Object -First 1 | ForEach-Object { \"$($_.Name)|$($_.NetworkCategory)\" }");
        let text = out.text();
        let (name, category) = text.split_once('|')?;
        Some(NetworkProfile { name: name.trim().to_string(), public: category.trim().eq_ignore_ascii_case("public") })
    }
}

impl Platform for Windows {
    fn os_name(&self) -> &'static str {
        "Windows"
    }

    fn probe(&self) -> Picture {
        let steady = self.steady();
        let mut picture = Picture {
            ssh_port: self.ssh_port,
            total_memory_gb: steady.total_memory_gb,
            user: steady.user.clone(),
            rows: steady.rows.clone(),
            ..Default::default()
        };
        if !steady.distro_ready {
            return picture;
        }
        let port = self.ssh_port;

        // One wsl.exe start for both answers: the Docker version on the first
        // line, then an empty one, then the sockets listening on the ssh port.
        let script =
            format!("docker version --format '{{{{.Server.Version}}}}' 2>/dev/null | head -1; echo; {}", wsl_script::listening_query(port));
        let answer = self.in_distro(&script);
        let mut lines = answer.stdout.lines();
        let docker_version = lines.next().unwrap_or_default().trim().to_string();
        let docker_ok = !docker_version.is_empty();
        picture.rows.push(row("Docker Engine", docker_ok, if docker_ok { docker_version } else { "not running or not installed".into() }));

        let listening = lines.any(|line| !line.trim().is_empty());
        picture.sshd_listening = listening;
        picture.rows.push(row(
            "SSH server",
            listening,
            if listening { format!("listening on {port}") } else { format!("not listening on {port}") },
        ));
        picture.rows.extend(steady.rsync.clone());

        // Pairing may listen once both rules exist; a port changed since only needs Set up again.
        let (rules, network) = self.firewall_and_network();
        let firewall_open = rules.ssh != Rule::Missing && rules.pairing != Rule::Missing;
        picture.ready_for_pairing = firewall_open;
        let firewall_detail = match (&rules.ssh, firewall_open) {
            (_, false) => "ports closed (Set up opens them)".to_string(),
            (Rule::Open(Some(open)), true) if *open != port => format!("open for {open}, not {port} (Set up again updates it)"),
            _ => "ports open".to_string(),
        };
        picture.rows.push(row("Firewall", firewall_open && rules.ssh.opens(port), firewall_detail));

        picture.network = network;
        if let Some(network) = &picture.network {
            let detail = if network.public {
                format!("{} is marked Public, which blocks the firewall rules", network.name)
            } else {
                format!("{} (Private)", network.name)
            };
            picture.rows.push(row("Network", !network.public, detail));
        }

        let wslconfig = std::fs::read_to_string(self.user_profile().join(".wslconfig")).unwrap_or_default();
        let setting = |section: &str, key: &str| wsl_script::ini_value(&wslconfig, section, key).unwrap_or_default().to_ascii_lowercase();
        let keeps_running = setting("general", "instanceIdleTimeout") == "-1"
            && setting("wsl2", "vmIdleTimeout") == "-1"
            && setting("wsl2", "networkingMode") == "mirrored";
        picture.rows.push(Row {
            name: "WSL stays up",
            state: if keeps_running { State::Ok } else { State::Unknown },
            detail: if keeps_running { "configured".into() } else { "not configured yet".into() },
        });
        picture
    }

    fn setup(&self, options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
        let patient = Windows::for_setup(self.distro.clone(), self.ssh_port);
        let results = windows_steps::run_all(&patient, options, say);
        self.forget_what_was_read();
        results
    }

    fn host_key(&self) -> String {
        host_key_from_pub(&self.in_distro_as_root("cat /etc/ssh/ssh_host_ed25519_key.pub 2>/dev/null").stdout)
    }

    fn install_key(&self, key: &str, mark: &str) -> Result<Installed, String> {
        let user = self.distro_user().ok_or_else(|| format!("{} has no user yet; run Set up first", self.distro))?;
        let user = super::platform::checked_user(user)?;
        let out = self.in_distro_as_root(&super::platform::authorized_keys_script(&user, key, mark));
        if !out.ok || !out.stdout.contains("dockernanny-key-ok") {
            return Err(format!("could not write authorized_keys: {}", out.stderr.trim()));
        }
        Ok(Installed { user, port: self.ssh_port, hostname: self.hostname(), host_key: self.host_key() })
    }

    fn remove_key(&self, mark: &str) -> Result<(), String> {
        let user = self.distro_user().ok_or_else(|| format!("{} has no user", self.distro))?;
        let user = super::platform::checked_user(user)?;
        let out = self.in_distro_as_root(&super::platform::forget_key_script(&user, mark));
        if !out.ok || !out.stdout.contains("dockernanny-key-gone") {
            return Err(format!("could not change authorized_keys: {}", out.stderr.trim()));
        }
        Ok(())
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

    fn forget_kept(&self) {
        self.forget_what_was_read();
    }

    fn hostname(&self) -> String {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "windows".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steady(rows: Vec<Row>, rsync: bool, distro_ready: bool) -> Steady {
        Steady {
            read_at: Instant::now(),
            total_memory_gb: 16,
            user: Some("alex".into()),
            rows,
            rsync: Some(row("rsync", rsync, "")),
            distro_ready,
        }
    }

    #[test]
    fn only_a_fully_fine_computer_is_remembered() {
        let fine = vec![row("Windows", true, ""), row("WSL 2", true, ""), row("Linux distribution", true, ""), row("Linux user", true, "")];
        assert!(steady_worth_keeping(&steady(fine.clone(), true, true)));
        assert!(!steady_worth_keeping(&steady(fine.clone(), false, true)), "rsync missing: the user is about to fix it");
        assert!(!steady_worth_keeping(&steady(vec![row("Windows", true, ""), row("WSL 2", false, "")], true, false)));
    }

    #[test]
    fn the_network_is_remembered_once_rules_exist_and_it_is_private() {
        let open = FirewallRules { ssh: Rule::Open(Some(2222)), pairing: Rule::Open(Some(47433)) };
        let private = NetworkProfile { name: "Home".into(), public: false };
        let public = NetworkProfile { name: "Cafe".into(), public: true };
        assert!(network_worth_keeping(&open, Some(&private)));
        assert!(network_worth_keeping(&open, None));
        assert!(!network_worth_keeping(&open, Some(&public)));
        assert!(!network_worth_keeping(&FirewallRules { ssh: Rule::Missing, pairing: Rule::Open(None) }, Some(&private)));
    }
}
