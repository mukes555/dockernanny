//! The Linux side of preparing a Windows computer, as one shell script shared
//! by Set up in the app and by the setup script the guide hands out, so the
//! two can never drift apart. The script checks before it changes anything
//! (`wsl_setup.sh`), merges settings files instead of replacing them
//! (`wsl_ini.sh`), and says what it changed; from that the caller decides
//! whether WSL needs a restart at all. Plain functions, tested everywhere.

use crate::stack::shell_quote;

pub const INI_FUNCTIONS: &str = include_str!("wsl_ini.sh");
const SETUP: &str = include_str!("wsl_setup.sh");
const CHANGED: &str = "dockernanny-changed: ";

/// What the script needs to know.
pub struct LinuxSetup<'a> {
    pub user: &'a str,
    pub ssh_port: u16,
    /// The Windows user's `.wslconfig` as Windows names it (`C:\Users\...`);
    /// the script turns it into a WSL path with `wslpath`, the stock tool.
    pub wslconfig: Option<&'a str>,
    /// Memory the user chose for Docker, in GB; None leaves theirs alone.
    pub memory_gb: Option<u32>,
    /// Written only when `.wslconfig` has no memory line yet.
    pub default_memory_gb: u32,
}

/// The whole script: the values, the INI functions, then the steps.
pub fn linux_script(setup: &LinuxSetup) -> String {
    let wslconfig = match setup.wslconfig {
        Some(path) => format!("DN_WSLCONFIG=$(wslpath -u {})", shell_quote(path)),
        None => "DN_WSLCONFIG=".to_string(),
    };
    let memory = setup.memory_gb.map(|gb| format!("{gb}GB")).unwrap_or_default();
    format!(
        "DN_USER={user}\nDN_PORT={port}\n{wslconfig}\nDN_MEMORY={memory}\nDN_MEMORY_DEFAULT={default}GB\n{INI_FUNCTIONS}\n{SETUP}",
        user = shell_quote(setup.user),
        port = setup.ssh_port,
        memory = shell_quote(&memory),
        default = setup.default_memory_gb.max(2),
    )
}

/// The functions and the steps without the values, for the guide's setup
/// script, which learns the user and the paths on the computer itself.
pub fn shared_body() -> String {
    format!("{INI_FUNCTIONS}\n{SETUP}")
}

/// Only the default user in `/etc/wsl.conf`, for a user Set up just created.
pub fn default_user_script(user: &str) -> String {
    format!("set -e\n{INI_FUNCTIONS}\nset_ini /etc/wsl.conf user default {} || true\n", shell_quote(user))
}

/// The script's "changed" lines, in order.
pub fn changes(output: &str) -> Vec<String> {
    output.lines().filter_map(|line| line.trim().strip_prefix(CHANGED)).map(str::to_string).collect()
}

/// How much of WSL has to restart for the changes to take effect, from
/// least to most: `max` of two needs is the one that covers both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum Restart {
    /// Everything took effect at once (or nothing changed).
    #[default]
    None,
    /// `/etc/wsl.conf` is read when the distribution starts: `wsl --terminate <distro>`.
    Distro,
    /// `.wslconfig` is read when the WSL VM starts: `wsl --shutdown`, which
    /// stops every distribution, so it is done only when that file changed.
    Wsl,
}

pub fn restart_for(changes: &[String]) -> Restart {
    let wslconfig_changed = changes.iter().any(|change| change == ".wslconfig");
    if wslconfig_changed {
        return Restart::Wsl;
    }
    let wsl_conf_changed = changes.iter().any(|change| change.starts_with("wsl.conf"));
    if wsl_conf_changed {
        return Restart::Distro;
    }
    Restart::None
}

/// The WSL version from `wsl --version`, whatever the language: the first
/// line is the WSL version in every translation ("WSL version: 2.3.26.0",
/// "WSL-Version: 2.3.26.0", "Version de WSL : 2.3.26.0"), so the first
/// dotted number there is it.
pub fn wsl_version(output: &str) -> Option<String> {
    let first_line = output.lines().map(str::trim).find(|line| !line.is_empty())?;
    first_line
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_ascii_digit()))
        .find(|word| is_dotted_number(word))
        .map(str::to_string)
}

fn is_dotted_number(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() >= 2 && parts.iter().all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// `ss` inside the distribution, asked for the TCP sockets listening on
/// `port` alone: a line for each, nothing when there is none.
pub fn listening_query(port: u16) -> String {
    format!("ss -Hltn 'sport = :{port}' 2>/dev/null")
}

/// Mirrored networking and the rest need WSL 2.0 or newer.
pub fn is_wsl_two(version: &str) -> bool {
    version.split('.').next().and_then(|major| major.parse::<u32>().ok()).is_some_and(|major| major >= 2)
}

/// A value from an INI text, read the way WSL and `set_ini` read it: names
/// without regard to case, spaces around `=` ignored.
pub fn ini_value(text: &str, section: &str, key: &str) -> Option<String> {
    let wanted = format!("[{}]", section.to_ascii_lowercase());
    let mut inside = false;
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            inside = line.to_ascii_lowercase() == wanted;
            continue;
        }
        let Some((name, value)) = line.split_once('=') else { continue };
        if inside && name.trim().eq_ignore_ascii_case(key) {
            return Some(value.trim().to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_read_in_any_language() {
        let english = "WSL version: 2.3.26.0\nKernel version: 5.15.167.4-1\nWSLg version: 1.0.65\n";
        let german = "WSL-Version: 2.3.26.0\nKernelversion: 5.15.167.4-1\n";
        let french = "Version de WSL : 2.3.26.0\nVersion du noyau : 5.15.167.4-1\n";
        for text in [english, german, french] {
            assert_eq!(wsl_version(text).as_deref(), Some("2.3.26.0"), "{text}");
        }
        assert_eq!(wsl_version("WSL version: 1.2.5.0").as_deref(), Some("1.2.5.0"));
        assert!(is_wsl_two("2.3.26.0"));
        assert!(!is_wsl_two("1.2.5.0"));
        assert_eq!(wsl_version("Invalid command line option: --version"), None, "an old inbox WSL has no version");
    }

    #[test]
    fn only_the_changes_that_need_it_restart_wsl() {
        let output = "hello\ndockernanny-changed: installed rsync\ndockernanny-changed: wsl.conf: systemd on\ndockernanny-linux-ok\n";
        let found = changes(output);
        assert_eq!(found, vec!["installed rsync".to_string(), "wsl.conf: systemd on".into()]);
        assert_eq!(restart_for(&found), Restart::Distro);
        assert_eq!(restart_for(&["installed rsync".to_string()]), Restart::None);
        assert_eq!(restart_for(&["wsl.conf: systemd on".to_string(), ".wslconfig".into()]), Restart::Wsl);
        assert_eq!(restart_for(&[]), Restart::None, "a second Set up restarts nothing");
    }

    #[test]
    fn the_script_carries_its_values_quoted() {
        let script = linux_script(&LinuxSetup {
            user: "nanny",
            ssh_port: 2222,
            wslconfig: Some(r"C:\Users\Alex O'Neil\.wslconfig"),
            memory_gb: None,
            default_memory_gb: 8,
        });
        assert!(script.starts_with("DN_USER='nanny'\nDN_PORT=2222\n"), "{script}");
        assert!(script.contains(r"DN_WSLCONFIG=$(wslpath -u 'C:\Users\Alex O'\''Neil\.wslconfig')"), "{script}");
        assert!(script.contains("DN_MEMORY=''\nDN_MEMORY_DEFAULT=8GB\n"));
        assert!(script.contains("set_ini()") && script.contains("echo dockernanny-linux-ok"));
        let chosen = linux_script(&LinuxSetup { user: "a", ssh_port: 22, wslconfig: None, memory_gb: Some(12), default_memory_gb: 1 });
        assert!(chosen.contains("DN_WSLCONFIG=\nDN_MEMORY='12GB'\nDN_MEMORY_DEFAULT=2GB\n"), "{chosen}");
    }

    #[test]
    fn ini_values_are_read_like_wsl_reads_them() {
        let text = "# mine\r\n[WSL2]\r\nmemory = 12GB\r\nnetworkingMode=mirrored\r\n[general]\r\ninstanceIdleTimeout=-1\r\n";
        assert_eq!(ini_value(text, "wsl2", "Memory").as_deref(), Some("12GB"));
        assert_eq!(ini_value(text, "wsl2", "networkingMode").as_deref(), Some("mirrored"));
        assert_eq!(ini_value(text, "general", "instanceIdleTimeout").as_deref(), Some("-1"));
        assert_eq!(ini_value(text, "general", "memory"), None, "a key only counts in its own section");
    }

    /// The shell half, run for real: the same `sh` and `awk` a distribution has.
    #[cfg(unix)]
    #[test]
    fn set_ini_keeps_everything_it_does_not_set() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("ini-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("wslconfig");
        let theirs = "# my settings\r\n[wsl2]\r\nmemory = 12GB\r\nprocessors=4\r\n\r\n[experimental]\r\nautoMemoryReclaim=gradual\r\n";
        std::fs::write(&file, theirs).unwrap();

        let run = |script: &str| {
            let out = std::process::Command::new("sh").arg("-c").arg(format!("{INI_FUNCTIONS}\n{script}")).output().unwrap();
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        };
        let path = shell_quote(&file.display().to_string());
        let first = run(&format!(
            "set_ini {path} wsl2 networkingMode mirrored && echo changed; ini_missing {path} wsl2 memory || echo has-memory; set_ini {path} general instanceIdleTimeout -1 && echo changed"
        ));
        assert_eq!(first, "changed\nhas-memory\nchanged");
        let merged = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            merged,
            "# my settings\r\n[wsl2]\r\nmemory = 12GB\r\nprocessors=4\r\nnetworkingMode=mirrored\r\n\r\n[experimental]\r\nautoMemoryReclaim=gradual\r\n\r\n[general]\r\ninstanceIdleTimeout=-1\r\n"
        );
        let again = run(&format!("set_ini {path} wsl2 networkingMode mirrored && echo changed || echo same"));
        assert_eq!(again, "same", "a second run changes nothing");
        let kept = std::fs::read_to_string(dir.join("wslconfig.before-dockernanny")).unwrap();
        assert_eq!(kept, theirs, "the original is kept once, before the first change");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
