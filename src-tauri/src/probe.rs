//! One shell script that tells what a computer is and how it is doing: host
//! name and OS, CPU, load, uptime, memory, the disk Docker writes to, the
//! battery, Docker itself. It runs over ssh for a machine and with `sh` for
//! this computer, and every section is marker-separated so a missing tool
//! cannot shift the parse. The parsers are pure functions.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;

// The script's sections. Each survives its tools being absent: Linux, macOS
// and Ubuntu inside WSL2 all fill what they can. Inside WSL the Windows side
// answers too (its version and the laptop's battery) through the interop
// socket, which an ssh session has to pick up by hand.
const HOST: &str = r#"echo HOST; hostname; uname -sr; if [ -r /etc/os-release ]; then . /etc/os-release; echo "$PRETTY_NAME"; elif command -v sw_vers >/dev/null 2>&1; then echo "macOS $(sw_vers -productVersion)"; fi"#;
const CPU: &str = r#"echo CPU; if command -v lscpu >/dev/null 2>&1; then lscpu | sed -n 's/^Model name:[ ]*//p' | head -1; else sysctl -n machdep.cpu.brand_string 2>/dev/null; fi; nproc 2>/dev/null || sysctl -n hw.ncpu"#;
const LOAD: &str = "echo LOAD; uptime";
const UPTIME: &str = "echo UPTIME; cut -d' ' -f1 /proc/uptime 2>/dev/null || sysctl -n kern.boottime 2>/dev/null";
const MEM: &str = "echo MEM; free -m 2>/dev/null || { sysctl -n hw.memsize; vm_stat; }";
// One `docker info` answers DISK and DOCKER both; DOCKER reads `$di` again.
const DISK: &str = r#"echo DISK; di=$(docker info --format '{{.DockerRootDir}}|{{.OperatingSystem}}' 2>/dev/null); d=${di%%|*}; [ -d "$d" ] || d=$HOME; df -Pk "$d" 2>/dev/null | tail -1"#;
const BATTERY: &str = r#"echo BATTERY; if command -v pmset >/dev/null 2>&1; then pmset -g batt; else for b in /sys/class/power_supply/BAT*; do [ -r "$b/capacity" ] && echo "$(cat "$b/capacity") $(cat "$b/status")"; done; fi"#;
const WINDOWS: &str = r#"echo WINDOWS; if grep -qi microsoft /proc/version 2>/dev/null; then [ -z "$WSL_INTEROP" ] && WSL_INTEROP=$(ls -t /run/WSL/*_interop 2>/dev/null | head -1) && export WSL_INTEROP; timeout 8 /mnt/c/Windows/System32/WindowsPowerShell/v1.0/powershell.exe -NoProfile -NonInteractive -Command '$o = Get-CimInstance Win32_OperatingSystem; "$($o.Caption) $($o.Version)"; $b = Get-CimInstance Win32_Battery | Select-Object -First 1; if ($b) { "battery $($b.EstimatedChargeRemaining) $($b.BatteryStatus)" }' 2>/dev/null | tr -d '\r'; fi"#;
// Always three lines (version, what it runs on, containers), even with no
// Docker at all: the parse is positional.
const DOCKER: &str = r#"echo DOCKER; v=$(docker version --format '{{.Server.Version}}' 2>/dev/null); echo "$v"; echo "${di#*|}"; docker ps -q 2>/dev/null | wc -l"#;

/// What never changes on a computer. WINDOWS also carries a WSL machine's
/// battery, which only Windows reports; a full reading every minute or so
/// keeps it fresh enough.
const FIXED: [&str; 3] = [HOST, CPU, WINDOWS];
/// What changes, read every time. DISK comes before DOCKER, see above.
const CHANGING: [&str; 6] = [LOAD, UPTIME, MEM, DISK, BATTERY, DOCKER];

/// The whole reading, or with `full` false only what changes: the fixed
/// facts cost a PowerShell start on a Windows machine and several processes
/// everywhere, every ten seconds, for answers that do not change.
pub fn script(full: bool) -> String {
    let mut sections: Vec<&str> = Vec::new();
    if full {
        sections.extend(FIXED);
    }
    sections.extend(CHANGING);
    sections.push("true");
    sections.join("\n") + "\n"
}

/// A light reading with the facts only a full one has.
pub fn with_fixed_facts(light: Probe, full: &Probe) -> Probe {
    Probe {
        hostname: full.hostname.clone(),
        os: full.os.clone(),
        cpu_model: full.cpu_model.clone(),
        cpus: full.cpus,
        battery: light.battery.or(full.battery),
        ..light
    }
}

const SECTIONS: [&str; 9] = ["HOST", "CPU", "LOAD", "UPTIME", "MEM", "DISK", "BATTERY", "WINDOWS", "DOCKER"];

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// Plugged in, charging or full: the machine is not running down.
    pub charging: bool,
}

/// What the probe found. Everything optional is absent when the tool that
/// reports it is not there.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct Probe {
    pub hostname: Option<String>,
    /// "Ubuntu 24.04 LTS", "macOS 15.1", or "Windows 11 (build 22631) · Ubuntu 24.04 in WSL2".
    pub os: Option<String>,
    pub cpu_model: Option<String>,
    pub cpus: u32,
    pub load1: f32,
    pub uptime_s: u64,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
    pub disk_free_bytes: u64,
    pub disk_total_bytes: u64,
    pub battery: Option<Battery>,
    pub docker_version: Option<String>,
    /// What Docker says it runs on: "Docker Desktop", "Ubuntu 22.04.4 LTS".
    pub flavor: Option<String>,
    pub containers_running: u32,
}

pub fn parse(text: &str) -> Probe {
    parse_at(text, now_s())
}

/// `now_s` is passed in so the macOS boot-time form of uptime is testable.
pub fn parse_at(text: &str, now_s: u64) -> Probe {
    let sections = split_sections(text, &SECTIONS);
    let lines = |name: &str| -> Vec<String> { sections.get(name).cloned().unwrap_or_default().into_iter().filter(|l| !l.trim().is_empty()).map(|l| l.trim().to_string()).collect() };

    let host = lines("HOST");
    let windows = lines("WINDOWS");
    let cpu = lines("CPU");
    let (mem_total_mb, mem_used_mb) = parse_memory(&lines("MEM"));
    let (disk_total_bytes, disk_free_bytes) = lines("DISK").first().map(|l| parse_df(l)).unwrap_or((0, 0));
    // Positional, so an absent Docker leaves two empty lines rather than shifting the count up.
    let docker: Vec<String> = sections.get("DOCKER").cloned().unwrap_or_default().into_iter().map(|l| l.trim().to_string()).collect();
    let docker_field = |i: usize| docker.get(i).cloned().filter(|v| !v.is_empty());

    Probe {
        hostname: host.first().cloned(),
        os: describe_os(host.get(2).map(String::as_str), windows.first().map(String::as_str)),
        cpu_model: cpu.first().cloned().filter(|m| !m.is_empty() && cpu.len() > 1),
        cpus: cpu.last().and_then(|c| c.parse().ok()).unwrap_or(0),
        load1: lines("LOAD").first().and_then(|l| parse_load(l)).unwrap_or(0.0),
        uptime_s: lines("UPTIME").first().map(|l| parse_uptime(l, now_s)).unwrap_or(0),
        mem_used_mb,
        mem_total_mb,
        disk_free_bytes,
        disk_total_bytes,
        battery: parse_battery(&lines("BATTERY"), windows.get(1).map(String::as_str)),
        docker_version: docker_field(0).filter(|v| v.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false)),
        flavor: docker_field(1),
        containers_running: docker_field(2).and_then(|c| c.parse().ok()).unwrap_or(0),
    }
}

fn split_sections(text: &str, markers: &[&str]) -> HashMap<String, Vec<String>> {
    let mut sections: HashMap<String, Vec<String>> = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if markers.contains(&line.trim()) {
            current = Some(line.trim().to_string());
            sections.entry(line.trim().to_string()).or_default();
            continue;
        }
        if let Some(name) = &current {
            sections.entry(name.clone()).or_default().push(line.to_string());
        }
    }
    sections
}

/// The Linux or macOS name, or the Windows edition and build with the
/// distro that runs inside it.
fn describe_os(pretty: Option<&str>, windows: Option<&str>) -> Option<String> {
    let pretty = pretty.map(str::trim).filter(|p| !p.is_empty());
    let Some(windows) = windows.map(str::trim).filter(|w| w.starts_with("Microsoft Windows")) else { return pretty.map(str::to_string) };
    let mut words: Vec<&str> = windows.split_whitespace().collect();
    let version = words.pop().unwrap_or("");
    let build = version.rsplit('.').next().unwrap_or("");
    let edition: Vec<&str> = words.iter().skip(1).take(2).copied().collect();
    let mut text = edition.join(" ");
    if !build.is_empty() {
        text.push_str(&format!(" (build {build})"));
    }
    if let Some(distro) = pretty {
        text.push_str(&format!(" · {distro} in WSL2"));
    }
    Some(text)
}

/// Linux says `load average: 0.52, 0.58, 0.59`, macOS says `load averages: 2.11 2.35 2.40`.
fn parse_load(uptime_line: &str) -> Option<f32> {
    let index = uptime_line.find("load average")?;
    let after = &uptime_line[index..];
    let numbers = after.split_once(':')?.1;
    numbers.split([' ', ',']).find_map(|token| token.parse().ok())
}

/// `/proc/uptime` gives seconds; macOS `kern.boottime` gives the boot epoch.
fn parse_uptime(line: &str, now_s: u64) -> u64 {
    if let Ok(seconds) = line.trim().parse::<f64>() {
        return seconds as u64;
    }
    let boot: Option<u64> = line.split_once("sec = ").and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next()).and_then(|n| n.parse().ok());
    boot.map(|b| now_s.saturating_sub(b)).unwrap_or(0)
}

/// `free -m` on Linux (total then used on the `Mem:` line); on macOS the
/// total in bytes followed by `vm_stat`, where used is the active, wired
/// and compressed pages.
fn parse_memory(lines: &[String]) -> (u64, u64) {
    if let Some(mem) = lines.iter().find(|l| l.starts_with("Mem:")) {
        let mut fields = mem.split_whitespace().skip(1);
        let total = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let used = fields.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        return (total, used);
    }
    let Some(total_bytes) = lines.first().and_then(|l| l.parse::<u64>().ok()) else { return (0, 0) };
    let page_size: u64 = lines.iter().find_map(|l| l.split_once("page size of ").and_then(|(_, r)| r.split_whitespace().next()).and_then(|n| n.parse().ok())).unwrap_or(4096);
    let pages = |label: &str| -> u64 { lines.iter().find(|l| l.starts_with(label)).and_then(|l| l.rsplit(':').next()).and_then(|n| n.trim().trim_end_matches('.').parse().ok()).unwrap_or(0) };
    let used_pages = pages("Pages active") + pages("Pages wired down") + pages("Pages occupied by compressor");
    (total_bytes / (1024 * 1024), used_pages * page_size / (1024 * 1024))
}

/// One `df -Pk` line: total and available kilobytes, as bytes.
fn parse_df(line: &str) -> (u64, u64) {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let kb = |i: usize| fields.get(i).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0) * 1024;
    (kb(1), kb(3))
}

/// Linux sysfs gives `87 Charging`; macOS `pmset -g batt` gives
/// `100%; charged;` under a `drawing from 'AC Power'` line; Windows gives
/// `battery 100 2` where 2, 3, 6 to 9 mean plugged in.
fn parse_battery(lines: &[String], windows_battery: Option<&str>) -> Option<Battery> {
    if let Some(line) = windows_battery.and_then(|l| l.strip_prefix("battery ")) {
        let mut fields = line.split_whitespace();
        let percent: u8 = fields.next()?.parse().ok()?;
        let status: u32 = fields.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        return Some(Battery { percent: percent.min(100), charging: matches!(status, 2 | 3 | 6 | 7 | 8 | 9) });
    }
    let text = lines.join("\n");
    if let Some(percent_end) = text.find('%') {
        let percent: u8 = text[..percent_end].rsplit(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
        let after = text[percent_end..].to_ascii_lowercase();
        let charging = text.contains("AC Power") || after.contains("charged") || after.contains(" charging");
        return Some(Battery { percent: percent.min(100), charging });
    }
    let line = lines.first()?;
    let (percent, status) = line.split_once(' ').unwrap_or((line.as_str(), ""));
    let percent: u8 = percent.trim().parse().ok()?;
    // sysfs says "Not charging" for a full battery on mains: plugged in either way.
    let status = status.trim().to_ascii_lowercase();
    Some(Battery { percent: percent.min(100), charging: status == "charging" || status == "full" || status == "not charging" })
}

fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `up 9 days, 8:22` style text for a card.
pub fn uptime_text(seconds: u64) -> String {
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3600;
    let minutes = (seconds % 3600) / 60;
    if days > 0 {
        format!("up {days}d {hours}h")
    } else if hours > 0 {
        format!("up {hours}h {minutes}m")
    } else {
        format!("up {minutes}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made-up samples in the exact formats the tools print; round numbers so
    // the expected values below can be checked by eye.
    const MAC: &str = "HOST\nstudio\nDarwin 24.1.0\nmacOS 15.1\nCPU\nApple M2\n8\nLOAD\n10:00  up 3 days,  2:00, 2 users, load averages: 1.50 1.20 1.00\nUPTIME\n{ sec = 1700000000, usec = 0 } Tue Nov 14 22:13:20 2023\nMEM\n17179869184\nMach Virtual Memory Statistics: (page size of 16384 bytes)\nPages free:                                    50000.\nPages active:                                 200000.\nPages inactive:                               100000.\nPages wired down:                              80000.\nPages occupied by compressor:                  40000.\nDISK\n/dev/disk3s5   500000000 300000000 200000000    60%    /System/Volumes/Data\nBATTERY\nNow drawing from 'AC Power'\n -InternalBattery-0 (id=1234567)\t100%; charged; 0:00 remaining present: true\nWINDOWS\nDOCKER\n27.3.1\nDocker Desktop\n      5\n";

    const WSL: &str = "HOST\nworkshop\nLinux 5.15.167.4-microsoft-standard-WSL2\nUbuntu 24.04.1 LTS\nCPU\nExample 8-Core Processor\n8\nLOAD\n 10:00:00 up  5:00,  1 user,  load average: 0.10, 0.20, 0.30\nUPTIME\n18000.00\nMEM\n               total        used        free      shared  buff/cache   available\nMem:           16000        4000       12000           0         500       11500\nSwap:           4096           0        4096\nDISK\n/dev/sdd        1000000000 100000000 900000000      10% /var/lib/docker\nBATTERY\nWINDOWS\nMicrosoft Windows 11 Pro 10.0.22631\nbattery 80 1\nDOCKER\n27.3.1\nUbuntu 24.04.1 LTS\n2\n";

    #[test]
    fn a_mac_is_described_with_its_battery_and_memory() {
        let probe = parse_at(MAC, 1700000000 + 3600 * 24 * 3 + 2 * 3600);
        assert_eq!(probe.hostname.as_deref(), Some("studio"));
        assert_eq!(probe.os.as_deref(), Some("macOS 15.1"));
        assert_eq!(probe.cpu_model.as_deref(), Some("Apple M2"));
        assert_eq!(probe.cpus, 8);
        assert_eq!(probe.load1, 1.50);
        assert_eq!(probe.uptime_s, 3600 * 24 * 3 + 2 * 3600);
        assert_eq!(probe.mem_total_mb, 16384);
        // (200000 active + 80000 wired + 40000 compressed) pages of 16 KiB.
        assert_eq!(probe.mem_used_mb, 5000);
        assert_eq!(probe.disk_total_bytes, 500000000 * 1024);
        assert_eq!(probe.disk_free_bytes, 200000000 * 1024);
        assert_eq!(probe.battery, Some(Battery { percent: 100, charging: true }));
        assert_eq!(probe.docker_version.as_deref(), Some("27.3.1"));
        assert_eq!(probe.flavor.as_deref(), Some("Docker Desktop"));
        assert_eq!(probe.containers_running, 5);
    }

    #[test]
    fn a_wsl_machine_reports_windows_and_the_distro() {
        let probe = parse(WSL);
        assert_eq!(probe.os.as_deref(), Some("Windows 11 (build 22631) · Ubuntu 24.04.1 LTS in WSL2"));
        assert_eq!(probe.cpu_model.as_deref(), Some("Example 8-Core Processor"));
        assert_eq!(probe.cpus, 8);
        assert_eq!(probe.load1, 0.10);
        assert_eq!(probe.uptime_s, 18000);
        assert_eq!((probe.mem_total_mb, probe.mem_used_mb), (16000, 4000));
        assert_eq!(probe.disk_free_bytes, 900000000 * 1024);
        assert_eq!(probe.battery, Some(Battery { percent: 80, charging: false }));
        assert_eq!(probe.flavor.as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(probe.containers_running, 2);
    }

    #[test]
    fn linux_battery_and_missing_tools_are_tolerated() {
        let text = "HOST\nbox\nLinux 6.8\nDebian GNU/Linux 12 (bookworm)\nCPU\n4\nLOAD\nload average: 1.00, 0.9, 0.8\nUPTIME\nMEM\nDISK\nBATTERY\n42 Discharging\nWINDOWS\nDOCKER\n\n\n0\n";
        let probe = parse(text);
        assert_eq!(probe.cpu_model, None);
        assert_eq!(probe.cpus, 4);
        assert_eq!(probe.battery, Some(Battery { percent: 42, charging: false }));
        assert_eq!(probe.docker_version, None);
        assert_eq!(probe.mem_total_mb, 0);
        assert_eq!(parse_battery(&["95 Not charging".to_string()], None), Some(Battery { percent: 95, charging: true }));
        assert_eq!(parse_battery(&[], Some("battery 60 2")), Some(Battery { percent: 60, charging: true }));
        assert_eq!(parse_battery(&[], None), None);
    }

    #[test]
    fn a_light_reading_keeps_the_fixed_facts_of_the_full_one() {
        let full = parse(WSL);
        let light_text = "LOAD\n load average: 2.50, 0.20, 0.30\nUPTIME\n18060.00\nMEM\nMem:           16000        6000       10000           0         500       9500\nDISK\n/dev/sdd        1000000000 200000000 800000000      20% /var/lib/docker\nBATTERY\nDOCKER\n27.3.1\nUbuntu 24.04.1 LTS\n3\n";
        let light = with_fixed_facts(parse(light_text), &full);
        assert_eq!(light.hostname.as_deref(), Some("workshop"));
        assert_eq!(light.os, full.os);
        assert_eq!((light.cpu_model.clone(), light.cpus), (full.cpu_model.clone(), 8));
        assert_eq!(light.battery, full.battery, "only Windows reports a WSL machine's battery, in the full reading");
        assert_eq!((light.load1, light.mem_used_mb, light.containers_running), (2.50, 6000, 3), "what changes comes from the light reading");
    }

    #[test]
    fn the_light_script_skips_the_fixed_sections_and_asks_docker_info_once() {
        let full = script(true);
        let light = script(false);
        assert!(full.contains("echo HOST") && full.contains("echo WINDOWS"));
        assert!(!light.contains("echo HOST") && !light.contains("echo CPU") && !light.contains("powershell"));
        assert_eq!(light.matches("docker info").count(), 1);
        assert!(light.find("echo DISK").unwrap() < light.find("echo DOCKER").unwrap(), "DOCKER reads what DISK asked");
    }

    #[test]
    fn a_machine_without_docker_has_no_docker_version() {
        // What the DOCKER section prints then: two empty lines and the count.
        let probe = parse("DOCKER\n\n\n0\n");
        assert_eq!(probe.docker_version, None);
        assert_eq!(probe.flavor, None);
        assert_eq!(probe.containers_running, 0);
    }

    #[test]
    fn uptime_reads_like_a_person_says_it() {
        assert_eq!(uptime_text(45), "up 0m");
        assert_eq!(uptime_text(3600 * 7 + 60), "up 7h 1m");
        assert_eq!(uptime_text(86_400 * 9 + 3600 * 8), "up 9d 8h");
    }
}
