//! The steps that prepare a Windows computer. Every step is safe to run
//! again and says whether it changed anything. Steps that need admin rights
//! run in a second, elevated copy of this exe (`--privileged firewall`),
//! see `elevated_batch`.

use super::elevate;
use super::platform::{run, Outcome, Output, Say, SetupOptions};
use super::windows::Windows;
use super::{DISTRO, MIN_WINDOWS_BUILD, WSL_SSH_PORT};

pub const ELEVATED_LOG: &str = "elevated.log";
const HYPERV_WSL_VM: &str = "{40E0AC32-46A5-438A-A0B2-2B479E8F2E90}";
const NEW_USER: &str = "nanny";

type Step = fn(&Windows, &SetupOptions, &mut Say) -> Outcome;

pub fn run_all(win: &Windows, options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)> {
    let steps: [(&'static str, Step); 8] = [
        ("Windows version", windows_version),
        ("WSL 2", wsl_present),
        ("Ubuntu", distro_present),
        ("Linux user", linux_user),
        ("Docker, sshd, rsync inside Ubuntu", inside_ubuntu),
        ("WSL settings", wslconfig),
        ("Firewall and power (administrator)", admin_batch),
        ("Restart WSL", restart_wsl),
    ];
    let mut results = Vec::new();
    for (name, step) in steps {
        say(&format!("==> {name}"));
        let outcome = step(win, options, say);
        match &outcome {
            Outcome::Done(text) => say(&format!("    already done: {text}")),
            Outcome::Changed(text) => say(&format!("    done: {text}")),
            Outcome::Failed(text) => say(&format!("    FAILED: {text}")),
            Outcome::NeedsUser(text) => say(&format!("    ACTION NEEDED: {text}")),
        }
        let stop = matches!(outcome, Outcome::Failed(_) | Outcome::NeedsUser(_));
        results.push((name, outcome));
        if stop {
            break;
        }
    }
    results
}

fn windows_version(win: &Windows, _options: &SetupOptions, _say: &mut Say) -> Outcome {
    let build = win.windows_build();
    if build >= MIN_WINDOWS_BUILD {
        return Outcome::Done(format!("build {build}"));
    }
    Outcome::Failed(format!("Windows 11 22H2 or newer is needed (this is build {build}); WSL mirrored networking does not exist before that"))
}

fn wsl_present(win: &Windows, _options: &SetupOptions, say: &mut Say) -> Outcome {
    let out = win.wsl(&["--version"]);
    let version = out.stdout.lines().find(|l| l.starts_with("WSL version")).map(|l| l.trim().to_string());
    if let Some(version) = version {
        if !version.contains(": 1.") && !version.contains(": 0.") {
            say("    checking for a newer WSL build");
            let _ = win.wsl(&["--update"]);
            return Outcome::Done(version);
        }
    }
    say("    turning on the Windows Subsystem for Linux (Windows shows an administrator prompt)");
    let install = win.wsl(&["--install", "--no-distribution"]);
    if !install.ok {
        return Outcome::Failed(format!("wsl --install failed: {}", install.stderr.trim()));
    }
    Outcome::NeedsUser("WSL 2 is now enabled. Reboot, open dockerNanny again and click Set up once more.".into())
}

fn distro_present(win: &Windows, _options: &SetupOptions, say: &mut Say) -> Outcome {
    if win.wsl(&["-l", "-q"]).stdout.lines().any(|l| l.trim() == DISTRO) {
        return Outcome::Done("installed".into());
    }
    say(&format!("    installing {DISTRO} (a few minutes, no questions asked)"));
    let install = win.wsl(&["--install", "-d", DISTRO, "--no-launch"]);
    if !install.ok {
        return Outcome::Failed(format!("could not install {DISTRO}: {}", install.stderr.trim()));
    }
    Outcome::Changed(format!("{DISTRO} installed"))
}

/// A non-root default user. One is created when the distro has none, so
/// nobody has to open Ubuntu and answer its questions.
fn linux_user(win: &Windows, _options: &SetupOptions, say: &mut Say) -> Outcome {
    if let Some(user) = win.distro_user() {
        return Outcome::Done(user);
    }
    say(&format!("    creating Linux user {NEW_USER}"));
    let script = format!("set -e\nid -u {NEW_USER} >/dev/null 2>&1 || useradd -m -s /bin/bash -G sudo {NEW_USER}\n{}", wsl_conf_script(NEW_USER));
    let out = win.in_distro_as_root(&script);
    if !out.ok {
        return Outcome::Failed(format!("useradd failed: {}", out.stderr.trim()));
    }
    let _ = win.wsl(&["--terminate", DISTRO]);
    Outcome::Changed(format!("user {NEW_USER} created and made the default"))
}

/// `/etc/wsl.conf` written whole, both sections at once: writing them in two
/// places would have one overwrite the other.
fn wsl_conf_script(user: &str) -> String {
    format!("printf '[boot]\\nsystemd=true\\n\\n[user]\\ndefault={user}\\n' > /etc/wsl.conf\n")
}

fn inside_ubuntu(win: &Windows, _options: &SetupOptions, say: &mut Say) -> Outcome {
    let Some(user) = win.distro_user() else { return Outcome::Failed("no Linux user".into()) };
    say("    packages, Docker Engine if missing, sshd port, systemd (as root inside Ubuntu)");
    let script = format!(
        "set -e\n\
         export DEBIAN_FRONTEND=noninteractive\n\
         apt-get update -qq\n\
         apt-get install -y -qq openssh-server rsync curl ca-certificates\n\
         if command -v docker >/dev/null 2>&1; then echo \"docker already installed: $(docker --version)\"; else curl -fsSL https://get.docker.com | sh; fi\n\
         usermod -aG docker '{user}'\n\
         printf 'Port {WSL_SSH_PORT}\\n' > /etc/ssh/sshd_config.d/dockernanny.conf\n\
         {wsl_conf}\
         systemctl disable --now ssh.socket >/dev/null 2>&1 || true\n\
         systemctl enable ssh.service >/dev/null 2>&1 || true\n\
         systemctl enable docker >/dev/null 2>&1 || true\n\
         echo dockernanny-linux-ok\n",
        wsl_conf = wsl_conf_script(&user)
    );
    let out = win.in_distro_as_root(&script);
    for line in out.stdout.lines().chain(out.stderr.lines()).filter(|l| !l.trim().is_empty()).take(40) {
        say(&format!("    | {line}"));
    }
    if !out.ok || !out.stdout.contains("dockernanny-linux-ok") {
        return Outcome::Failed("the Linux part did not finish; see the lines above".into());
    }
    Outcome::Changed("Docker, sshd and rsync ready inside Ubuntu".into())
}

/// `.wslconfig` in the Windows user profile: mirrored networking so the
/// computer's own address reaches WSL, and no idle shutdown of the distro or VM.
fn wslconfig(win: &Windows, options: &SetupOptions, say: &mut Say) -> Outcome {
    let memory_gb = options.memory_gb.max(2);
    let path = win.user_profile().join(".wslconfig");
    let wanted = format!("[general]\ninstanceIdleTimeout=-1\n\n[wsl2]\nnetworkingMode=mirrored\nvmIdleTimeout=-1\nmemory={memory_gb}GB\n");
    let current = std::fs::read_to_string(&path).unwrap_or_default();
    if current == wanted {
        return Outcome::Done("already set".into());
    }
    if !current.is_empty() {
        let backup = path.with_extension("before-dockernanny");
        let _ = std::fs::copy(&path, &backup);
        say(&format!("    previous file kept as {}", backup.display()));
    }
    match std::fs::write(&path, wanted) {
        Ok(()) => Outcome::Changed(format!("{} written, Docker may use {memory_gb} GB", path.display())),
        Err(err) => Outcome::Failed(format!("could not write {}: {err}", path.display())),
    }
}

/// Firewall rules, network profile, power settings: run by the elevated copy.
fn admin_batch(win: &Windows, options: &SetupOptions, say: &mut Say) -> Outcome {
    let rules_present = win.firewall_rule_exists("dockerNanny SSH") && win.firewall_rule_exists("dockerNanny Pair");
    let network = options.make_network_private.as_deref();
    if rules_present && network.is_none() {
        return Outcome::Done("firewall rules present".into());
    }
    say("    Windows asks for administrator permission now");
    let log_path = elevate::log_path();
    let _ = std::fs::remove_file(&log_path);
    let task = elevate::Task::Firewall {
        pairing_port: options.pairing_port,
        private_network: network.map(str::to_string),
    };
    let code = match elevate::run_elevated(&task) {
        Ok(code) => code,
        Err(err) => return Outcome::Failed(err),
    };
    for line in std::fs::read_to_string(&log_path).unwrap_or_default().lines() {
        say(&format!("    | {line}"));
    }
    if code != 0 {
        return Outcome::Failed(format!("the administrator step exited with code {code}"));
    }
    Outcome::Changed("firewall open, computer stays awake when plugged in".into())
}

/// What the elevated copy does. Output goes to a file the parent shows.
/// `private_network` is the one network the user agreed to mark Private.
pub fn elevated_batch(win: &Windows, pairing_port: u16, private_network: Option<&str>, log_path: &std::path::Path) -> i32 {
    let mut report = Report::default();

    for (name, port) in [("dockerNanny SSH", WSL_SSH_PORT), ("dockerNanny Pair", pairing_port)] {
        if win.firewall_rule_exists(name) {
            report.lines.push(format!("firewall rule {name}: already there"));
            continue;
        }
        let out = run(
            "netsh",
            &["advfirewall", "firewall", "add", "rule", &format!("name={name}"), "dir=in", "action=allow", "protocol=TCP", &format!("localport={port}"), "profile=private,domain"],
            None,
            &[],
        );
        report.record(&format!("firewall rule {name}"), out);
    }

    let hyperv = format!(
        "if (-not (Get-NetFirewallHyperVRule -Name dockerNannySsh -ErrorAction SilentlyContinue)) {{ New-NetFirewallHyperVRule -Name dockerNannySsh -DisplayName 'dockerNanny SSH' -Direction Inbound -VMCreatorId '{HYPERV_WSL_VM}' -Protocol TCP -LocalPorts {WSL_SSH_PORT} | Out-Null }}; \
         schtasks.exe /Delete /TN 'dockerNanny WSL' /F 2>$null | Out-Null; 'done'"
    );
    let out = win.powershell(&hyperv);
    // Older Windows builds lack the Hyper-V firewall cmdlets; the rest still ran.
    report.lines.push(format!("Hyper-V firewall rule: {}", if out.ok { "ok" } else { "skipped (cmdlets unavailable)" }));

    if let Some(name) = private_network {
        // The name reaches PowerShell through the environment, never inside the script text.
        let script = "Get-NetConnectionProfile | Where-Object { $_.Name -eq $env:DOCKERNANNY_NETWORK -and $_.NetworkCategory -eq 'Public' } | Set-NetConnectionProfile -NetworkCategory Private; 'done'";
        let out = run("powershell.exe", &["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", script], None, &[("DOCKERNANNY_NETWORK", name)]);
        report.record(&format!("network {name} marked Private"), out);
    }

    report.record("lid closed does nothing (plugged in)", run("powercfg", &["/setacvalueindex", "SCHEME_CURRENT", "SUB_BUTTONS", "LIDACTION", "0"], None, &[]));
    report.record("apply power scheme", run("powercfg", &["/setactive", "SCHEME_CURRENT"], None, &[]));
    report.record("no sleep while plugged in", run("powercfg", &["/change", "standby-timeout-ac", "0"], None, &[]));

    let _ = std::fs::write(log_path, report.lines.join("\n"));
    if report.failed {
        1
    } else {
        0
    }
}

#[derive(Default)]
struct Report {
    lines: Vec<String>,
    failed: bool,
}

impl Report {
    fn record(&mut self, label: &str, out: Output) {
        let status = if out.ok { "ok" } else { "failed" };
        self.lines.push(format!("{label}: {status} {}", out.stderr.trim()));
        if !out.ok {
            self.failed = true;
        }
    }
}

fn restart_wsl(win: &Windows, _options: &SetupOptions, say: &mut Say) -> Outcome {
    say("    wsl --shutdown, then waiting for sshd and Docker to come back");
    let _ = win.wsl(&["--shutdown"]);
    std::thread::sleep(std::time::Duration::from_secs(3));
    for attempt in 0..20 {
        let sshd_ok = win.in_distro("ss -ltn 2>/dev/null").stdout.contains(&format!(":{WSL_SSH_PORT} "));
        let docker = win.in_distro("docker version --format '{{.Server.Version}}' 2>/dev/null");
        let docker_ok = docker.ok && !docker.text().is_empty();
        if sshd_ok && docker_ok {
            return Outcome::Changed(format!("sshd on {WSL_SSH_PORT}, Docker {}", docker.text()));
        }
        if attempt == 19 {
            let what = if !sshd_ok { "sshd is not listening" } else { "Docker is not answering" };
            return Outcome::Failed(format!("{what} after the restart; reboot once and open the app again"));
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
    Outcome::Failed("unreachable".into())
}
