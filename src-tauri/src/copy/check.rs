//! What the destination looks like before the copy (room for the data, a
//! compose plugin) and after it (every service up, every port answering).

use super::endpoint::{Endpoint, Site};
use crate::compose;
use crate::ssh::Ssh;

/// Room for the data plus half again: compose builds and image layers land
/// on the same disk.
const HEADROOM: f64 = 1.5;

/// Fails with a sentence the user can act on; otherwise returns what was found.
pub async fn preflight(ssh: &Ssh, to: &Site, needed_bytes: u64) -> anyhow::Result<Vec<String>> {
    let compose = to.endpoint.docker_output(ssh, &["compose", "version", "--short"]).await?;
    anyhow::ensure!(
        compose.ok() && !compose.stdout.trim().is_empty(),
        "{} has Docker but not the compose plugin (on Debian and Ubuntu: sudo apt-get install docker-compose-plugin)",
        to.label
    );
    let mut notes = vec![format!("compose {} on {}", compose.stdout.trim(), to.label)];

    let df_line = match &to.endpoint {
        Endpoint::Machine { .. } => to
            .endpoint
            .run_script(ssh, "df -Pk \"$(docker info -f '{{.DockerRootDir}}' 2>/dev/null || echo /)\" 2>/dev/null | tail -1")
            .await
            .map(|o| o.stdout)
            .unwrap_or_default(),
        // Docker Desktop keeps its disk image in the home folder; the home disk is a fair proxy.
        Endpoint::Local if cfg!(unix) => {
            to.endpoint.run_script(ssh, "df -Pk \"$HOME\" 2>/dev/null | tail -1").await.map(|o| o.stdout).unwrap_or_default()
        }
        Endpoint::Local => String::new(),
    };
    let free = match &to.endpoint {
        Endpoint::Local if cfg!(windows) => windows_free_bytes(&crate::store::user_home()),
        _ => parse_df_free_kb(&df_line).map(|kb| kb * 1024),
    };
    if let Some(free_bytes) = free {
        let wanted = (needed_bytes as f64 * HEADROOM) as u64;
        anyhow::ensure!(
            free_bytes >= wanted,
            "{} has {} free where Docker keeps its data, but the volumes need about {} with room to build",
            to.label,
            human(free_bytes),
            human(wanted)
        );
        notes.push(format!("{} free on {}, {} needed", human(free_bytes), to.label, human(wanted)));
    }
    Ok(notes)
}

/// Free bytes on the Windows drive holding `path`; Docker Desktop keeps its
/// disk under the user's folder, so that drive is the one that fills up.
#[cfg(windows)]
fn windows_free_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut available: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(available)
}

#[cfg(not(windows))]
fn windows_free_bytes(_path: &std::path::Path) -> Option<u64> {
    None
}

/// The last line of `df -Pk`: available kilobytes are the fourth column.
pub fn parse_df_free_kb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(3)?.parse().ok()
}

/// `1.234GB`, `63B`, `32.8kB`, `?`: what `docker system df` and `docker ps -s`
/// print. Unknown text counts as nothing.
pub fn parse_human_size(text: &str) -> u64 {
    let text = text.trim();
    let digits_end = text.find(|c: char| !(c.is_ascii_digit() || c == '.')).unwrap_or(text.len());
    let number: f64 = text[..digits_end].parse().unwrap_or(0.0);
    let unit = text[digits_end..].trim().to_ascii_lowercase();
    let factor: f64 = match unit.as_str() {
        "" | "b" => 1.0,
        "kb" | "k" | "kib" => 1e3,
        "mb" | "m" | "mib" => 1e6,
        "gb" | "g" | "gib" => 1e9,
        "tb" | "t" | "tib" => 1e12,
        _ => 0.0,
    };
    (number * factor) as u64
}

pub fn human(bytes: u64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub services_up: usize,
    pub services_total: usize,
    pub unhealthy: Vec<String>,
    pub ports_listening: usize,
    pub ports_total: usize,
    pub ports_silent: Vec<u16>,
}

impl Summary {
    /// One sentence for the card, with the reminder that matters most after a move.
    pub fn text(&self, destination: &Site) -> String {
        let mut text = format!("{} of {} services up", self.services_up, self.services_total);
        if !self.unhealthy.is_empty() {
            text.push_str(&format!(" ({} not healthy)", self.unhealthy.join(", ")));
        }
        text.push_str(&format!(", {} of {} ports listening on {}", self.ports_listening, self.ports_total, destination.label));
        if !self.ports_silent.is_empty() {
            let silent: Vec<String> = self.ports_silent.iter().map(|p| p.to_string()).collect();
            text.push_str(&format!(" (silent: {})", silent.join(", ")));
        }
        text.push('.');
        if !destination.is_local() {
            text.push_str(" Restart local programs that talk to these ports.");
        }
        text
    }
}

/// `compose ps` plus the listening sockets. Never fails: a check that cannot
/// run reports zero services.
pub async fn post_copy(ssh: &Ssh, to: &Site) -> Summary {
    let ps = to.compose_output(ssh, "ps --format json").await.map(|o| o.stdout).unwrap_or_default();
    let sockets = match &to.endpoint {
        Endpoint::Machine { .. } => {
            to.endpoint.run_script(ssh, "ss -ltn 2>/dev/null || netstat -an 2>/dev/null").await.map(|o| o.stdout).unwrap_or_default()
        }
        Endpoint::Local if cfg!(windows) => crate::tools::native("netstat")
            .arg("-an")
            .output()
            .await
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default(),
        Endpoint::Local => {
            to.endpoint.run_script(ssh, "ss -ltn 2>/dev/null || netstat -an 2>/dev/null").await.map(|o| o.stdout).unwrap_or_default()
        }
    };
    summarize(&compose::parse_ps(&ps), &sockets)
}

fn summarize(services: &[compose::ServiceState], sockets: &str) -> Summary {
    let services_up = services.iter().filter(|s| s.state == "running").count();
    let unhealthy: Vec<String> = services
        .iter()
        .filter(|s| s.state == "running" && !s.health.is_empty() && s.health != "healthy")
        .map(|s| s.service.clone())
        .collect();
    let mut ports: Vec<u16> = services.iter().flat_map(|s| s.ports.iter().filter(|p| p.protocol == "tcp").map(|p| p.published)).collect();
    ports.sort_unstable();
    ports.dedup();
    let ports_silent: Vec<u16> = ports.iter().copied().filter(|port| !is_listening(sockets, *port)).collect();
    Summary {
        services_up,
        services_total: services.len(),
        unhealthy,
        ports_listening: ports.len() - ports_silent.len(),
        ports_total: ports.len(),
        ports_silent,
    }
}

/// `ss -ltn` prints `0.0.0.0:5432`, macOS `netstat -an` prints `*.5432`,
/// Windows says `LISTENING`.
fn is_listening(sockets: &str, port: u16) -> bool {
    sockets.lines().any(|line| {
        let listening = line.contains("LISTEN");
        listening && line.split_whitespace().any(|field| field.ends_with(&format!(":{port}")) || field.ends_with(&format!(".{port}")))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::{Port, ServiceState};

    #[test]
    fn human_sizes_parse_the_way_docker_prints_them() {
        assert_eq!(parse_human_size("1.5GB"), 1_500_000_000);
        assert_eq!(parse_human_size("32.8kB"), 32_800);
        assert_eq!(parse_human_size("63B"), 63);
        assert_eq!(parse_human_size("?"), 0);
        assert_eq!(parse_human_size(""), 0);
    }

    #[test]
    fn df_free_is_the_fourth_column() {
        assert_eq!(parse_df_free_kb("/dev/sda1 102400000 61440000 40960000 60% /var/lib/docker"), Some(40_960_000));
        assert_eq!(parse_df_free_kb(""), None);
    }

    #[test]
    fn human_rounds_to_one_decimal() {
        assert_eq!(human(999), "999 B");
        assert_eq!(human(40_960_000 * 1024), "41.9 GB");
    }

    fn service(name: &str, state: &str, health: &str, port: u16) -> ServiceState {
        ServiceState {
            service: name.into(),
            container: format!("x-{name}-1"),
            state: state.into(),
            health: health.into(),
            exit_code: 0,
            ports: vec![Port { target: port, published: port, protocol: "tcp".into() }],
        }
    }

    #[test]
    fn summary_counts_services_and_ports_on_every_platform() {
        let services = vec![
            service("db", "running", "healthy", 5432),
            service("api", "running", "starting", 3000),
            service("worker", "exited", "", 9000),
        ];
        let linux = "State  Recv-Q Send-Q Local Address:Port\nLISTEN 0      128    0.0.0.0:5432\nLISTEN 0      128    [::]:3000\n";
        let summary = summarize(&services, linux);
        assert_eq!(summary.services_up, 2);
        assert_eq!(summary.unhealthy, vec!["api".to_string()]);
        assert_eq!((summary.ports_listening, summary.ports_total), (2, 3));
        assert_eq!(summary.ports_silent, vec![9000]);
        let machine = Site::machine("x", "c.yml", "dn-1", "studio");
        assert!(summary.text(&machine).starts_with("2 of 3 services up (api not healthy), 2 of 3 ports listening on studio"));
        assert!(summary.text(&machine).ends_with("Restart local programs that talk to these ports."));
        let local = Site::local("x", "/tmp/x", "c.yml");
        assert!(summary.text(&local).ends_with("(silent: 9000)."));

        let mac = "tcp46      0      0  *.5432                 *.*                    LISTEN\ntcp4       0      0  127.0.0.1.3000         *.*                    LISTEN\n";
        assert_eq!(summarize(&services, mac).ports_listening, 2);
        let windows = "  TCP    0.0.0.0:5432           0.0.0.0:0              LISTENING\n  TCP    [::]:3000              [::]:0                 LISTENING\n";
        assert_eq!(summarize(&services, windows).ports_listening, 2);
    }
}
