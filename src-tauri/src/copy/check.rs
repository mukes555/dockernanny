//! What the destination looks like before the copy (room for the data, a
//! compose plugin) and after it (how ready each service is).

use super::endpoint::{Endpoint, Site};
use crate::compose::{self, Readiness};
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

/// How the copied stack stands right after it was started, in Compose's own
/// terms (`compose::ps`): a health check that has not passed yet is still
/// starting, not a failure, and a setup job that finished is done.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Summary {
    /// Ready, and one-shot jobs that finished.
    pub ready: usize,
    pub total: usize,
    /// One phrase per service that is not simply ready: "clamav still starting".
    pub notes: Vec<String>,
}

impl Summary {
    /// One sentence for the copy's panel, with the reminder that matters most after a move.
    pub fn text(&self, destination: &Site) -> String {
        let mut text = format!("{} of {} services ready on {}", self.ready, self.total, destination.label);
        if !self.notes.is_empty() {
            text.push_str(&format!(" ({})", self.notes.join("; ")));
        }
        text.push('.');
        if !destination.is_local() {
            text.push_str(" Restart local programs that talk to these ports.");
        }
        text
    }
}

/// `compose ps --all` at the destination. Never fails: a check that cannot
/// run reports zero services. The card keeps following the stack after this.
pub async fn post_copy(ssh: &Ssh, to: &Site) -> Summary {
    summarize(&to.services(ssh).await.unwrap_or_default())
}

fn summarize(services: &[compose::ServiceState]) -> Summary {
    let ready = services.iter().filter(|s| matches!(s.readiness, Readiness::Ready | Readiness::Done)).count();
    let notes = services
        .iter()
        .filter_map(|s| {
            let how = match s.readiness {
                Readiness::Ready => return None,
                Readiness::Done => "finished its job",
                Readiness::Starting => "still starting",
                Readiness::Unhealthy => "unhealthy",
                Readiness::Stopped => "not running",
            };
            Some(format!("{} {how}", s.service))
        })
        .collect();
    Summary { ready, total: services.len(), notes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compose::ServiceState;

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

    fn service(name: &str, readiness: Readiness) -> ServiceState {
        ServiceState { service: name.into(), readiness, ..ServiceState::default() }
    }

    /// The case that started this: right after `up -d`, clamav's first
    /// health check has not run, and keycloak-db is a finished setup job.
    #[test]
    fn a_health_check_not_passed_yet_is_starting_not_a_failure() {
        let services = vec![
            service("clamav", Readiness::Starting),
            service("keycloak", Readiness::Ready),
            service("keycloak-db", Readiness::Done),
            service("postgres", Readiness::Ready),
        ];
        let summary = summarize(&services);
        assert_eq!((summary.ready, summary.total), (3, 4));
        let machine = Site::machine("x", "c.yml", "dn-1", "studio");
        assert_eq!(
            summary.text(&machine),
            "3 of 4 services ready on studio (clamav still starting; keycloak-db finished its job). Restart local programs that talk to these ports."
        );
        let local = Site::local("x", "/tmp/x", "c.yml");
        let failing = summarize(&[service("db", Readiness::Unhealthy), service("worker", Readiness::Stopped)]);
        assert_eq!(failing.text(&local), "0 of 2 services ready on this computer (db unhealthy; worker not running).");
    }
}
