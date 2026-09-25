//! What the user chose for this computer, kept in `~/.dockernanny/settings.json`.
//! Missing fields take their defaults, so an older file still loads; a missing
//! file means the first launch, and the role chooser is shown.

use serde::{Deserialize, Serialize};

use crate::{guide, pairing, sync};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    System,
    Dark,
    Light,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    /// The controller role: send stacks to other machines.
    pub use_machines: bool,
    /// The host role: run stacks for other computers.
    pub share_this_computer: bool,
    /// The private key offered when adding a machine; empty picks the first
    /// key found in `~/.ssh`.
    pub key_path: String,
    /// Paths a new stack does not sync.
    pub excludes: Vec<String>,
    pub theme: Theme,
    /// Memory Docker may use when this computer is shared; 0 means half of it.
    pub host_memory_gb: u32,
    pub start_at_login: bool,
    /// Where a shared machine listens for pairing, and where this computer
    /// connects to pair. Both sides must agree.
    pub pairing_port: u16,
    /// Where the Windows setup script is served from.
    pub script_port: u16,
    /// The small image that reads and writes volumes during a copy; it must
    /// be pullable on both ends (or already there, for offline machines).
    pub helper_image: String,
    /// Windows only: the WSL distribution that runs ssh and rsync for this
    /// computer, and Docker and sshd when it is shared.
    pub wsl_distro: String,
    /// Windows only: where sshd inside WSL listens when this computer is
    /// shared, away from a Windows OpenSSH server on 22.
    pub wsl_ssh_port: u16,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            use_machines: true,
            share_this_computer: false,
            key_path: String::new(),
            excludes: sync::default_excludes(),
            theme: Theme::System,
            host_memory_gb: 0,
            start_at_login: false,
            pairing_port: pairing::PORT,
            script_port: guide::SERVE_PORT,
            helper_image: DEFAULT_HELPER_IMAGE.into(),
            wsl_distro: DEFAULT_WSL_DISTRO.into(),
            wsl_ssh_port: DEFAULT_WSL_SSH_PORT,
        }
    }
}

pub const DEFAULT_HELPER_IMAGE: &str = "alpine:3";
pub const DEFAULT_WSL_DISTRO: &str = "Ubuntu";
pub const DEFAULT_WSL_SSH_PORT: u16 = 2222;

impl Settings {
    /// Trims what the user typed and puts a default in place of a port of 0,
    /// which no listener could use.
    pub fn normalised(mut self) -> Self {
        self.key_path = self.key_path.trim().to_string();
        let mut excludes: Vec<String> = Vec::new();
        for exclude in self.excludes.iter().map(|e| e.trim()).filter(|e| !e.is_empty()) {
            if !excludes.iter().any(|seen| seen == exclude) {
                excludes.push(exclude.to_string());
            }
        }
        self.excludes = excludes;
        if self.pairing_port == 0 {
            self.pairing_port = pairing::PORT;
        }
        if self.script_port == 0 {
            self.script_port = guide::SERVE_PORT;
        }
        // Only a plain image reference reaches the docker command line.
        let image = self.helper_image.trim();
        let usable = !image.is_empty() && crate::copy::transfer::safe_image(image);
        self.helper_image = if usable { image.to_string() } else { DEFAULT_HELPER_IMAGE.into() };
        // The name reaches `wsl.exe -d` and the setup script; only what WSL itself allows.
        let distro = self.wsl_distro.trim();
        self.wsl_distro = if safe_distro(distro) { distro.to_string() } else { DEFAULT_WSL_DISTRO.into() };
        if self.wsl_ssh_port == 0 {
            self.wsl_ssh_port = DEFAULT_WSL_SSH_PORT;
        }
        self
    }
}

/// WSL distribution names are letters, digits, dots, dashes and underscores.
pub fn safe_distro(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    !name.is_empty() && name.len() <= 64 && name.chars().all(allowed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_older_file_loads_with_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"theme":"light","use_machines":false}"#).unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert!(!settings.use_machines);
        assert_eq!(settings.pairing_port, pairing::PORT);
        assert_eq!(settings.excludes, sync::default_excludes());
    }

    #[test]
    fn normalising_cleans_the_typed_values() {
        let settings = Settings {
            key_path: "  /home/alex/.ssh/id_ed25519 ".into(),
            excludes: vec![" .git ".into(), "".into(), ".git".into(), "dist".into()],
            pairing_port: 0,
            script_port: 0,
            ..Default::default()
        }
        .normalised();
        assert_eq!(settings.key_path, "/home/alex/.ssh/id_ed25519");
        assert_eq!(settings.excludes, vec![".git".to_string(), "dist".to_string()]);
        assert_eq!((settings.pairing_port, settings.script_port), (pairing::PORT, guide::SERVE_PORT));
    }

    #[test]
    fn the_helper_image_is_pinned_by_default_and_kept_plain() {
        assert_eq!(Settings::default().helper_image, "alpine:3");
        let typed = |image: &str| Settings { helper_image: image.into(), ..Default::default() }.normalised().helper_image;
        assert_eq!(typed(" registry.example.com/tools/alpine:3.20 "), "registry.example.com/tools/alpine:3.20");
        assert_eq!(typed(""), "alpine:3", "empty falls back to the default");
        assert_eq!(typed("alpine; rm -rf /"), "alpine:3", "anything unsafe falls back too");
    }

    #[test]
    fn the_wsl_distribution_is_a_plain_name() {
        let typed = |distro: &str| Settings { wsl_distro: distro.into(), wsl_ssh_port: 0, ..Default::default() }.normalised();
        assert_eq!(typed(" Ubuntu-24.04 ").wsl_distro, "Ubuntu-24.04");
        assert_eq!(typed("Debian").wsl_distro, "Debian");
        assert_eq!(typed("").wsl_distro, "Ubuntu");
        assert_eq!(typed("Ubuntu; calc").wsl_distro, "Ubuntu", "anything else falls back");
        assert_eq!(typed("Debian").wsl_ssh_port, 2222);
    }

    #[test]
    fn theme_is_snake_case_on_the_wire() {
        assert_eq!(serde_json::to_string(&Theme::System).unwrap(), "\"system\"");
    }
}
