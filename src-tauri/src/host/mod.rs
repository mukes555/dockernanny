//! The sharing role: this computer runs stacks for other computers. It
//! makes itself ready (Docker, sshd, rsync, the firewall), shows a pairing
//! code, and keeps the Docker host awake. `engine` does the work on its own
//! thread; the platforms know their operating system; the page only shows
//! the snapshot the engine publishes.

pub mod elevate;
pub mod engine;
pub mod fake;
pub mod keepalive;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod linux_setup;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod paired;
pub mod pairing_server;
pub mod platform;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub mod windows_steps;

use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::AppHandle;

use platform::{NetworkProfile, Platform, Row, SetupOptions};

pub const SNAPSHOT_EVENT: &str = "host:snapshot";
pub const LOG_EVENT: &str = "host:log";
/// Windows 11 22H2, the first build with WSL mirrored networking.
pub const MIN_WINDOWS_BUILD: u32 = 22621;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub text: String,
    pub failed: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PairingState {
    /// The port is open; requests are still refused while `armed` is false.
    pub listening: bool,
    pub armed: bool,
    pub locked: bool,
    /// `481 923` while armed.
    pub code: Option<String>,
    pub remaining_s: u64,
    pub note: Option<String>,
}

/// What the page shows; published whole whenever something changes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct HostSnapshot {
    pub os: String,
    /// False until the first look at this computer finished.
    pub probed: bool,
    pub rows: Vec<Row>,
    pub user: Option<String>,
    pub ssh_port: u16,
    pub ready_for_pairing: bool,
    pub total_memory_gb: u32,
    pub network: Option<NetworkProfile>,
    pub addresses: Vec<String>,
    pub setup_running: bool,
    pub notice: Option<Notice>,
    pub pairing: PairingState,
    /// The computers that paired with this one, oldest first.
    pub paired: Vec<paired::PairedComputer>,
    /// The computers with an ssh session open right now.
    pub connected: Vec<paired::Connected>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LogLine {
    pub line: String,
}

/// The settings the sharing role runs with; a change restarts it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostConfig {
    pub pairing_port: u16,
    /// Windows only: the WSL distribution and the sshd port inside it.
    pub wsl_distro: String,
    pub wsl_ssh_port: u16,
}

impl HostConfig {
    pub fn from_settings(settings: &crate::settings::Settings) -> Self {
        Self { pairing_port: settings.pairing_port, wsl_distro: settings.wsl_distro.clone(), wsl_ssh_port: settings.wsl_ssh_port }
    }
}

/// The app's handle on the sharing role: started when the role is on,
/// stopped when it is turned off or the app quits.
#[derive(Default)]
pub struct Host {
    engine: Mutex<Option<(HostConfig, engine::Engine)>>,
}

impl Host {
    pub fn is_running(&self) -> bool {
        self.engine.lock().expect("host lock").is_some()
    }

    /// Starts the role, or restarts it when it runs with other settings.
    pub fn start(&self, app: &AppHandle, config: HostConfig) {
        let mut slot = self.engine.lock().expect("host lock");
        let unchanged = slot.as_ref().map(|(running, _)| *running == config).unwrap_or(false);
        if unchanged {
            return;
        }
        if let Some((_, old)) = slot.take() {
            old.quit();
        }
        let engine = engine::start(app.clone(), make_platform(&config), config.pairing_port);
        *slot = Some((config, engine));
    }

    pub fn stop(&self) {
        if let Some((_, engine)) = self.engine.lock().expect("host lock").take() {
            engine.quit();
        }
    }

    pub fn snapshot(&self) -> Option<HostSnapshot> {
        self.engine.lock().expect("host lock").as_ref().map(|(_, e)| e.snapshot.lock().expect("host snapshot lock").clone())
    }

    pub fn log(&self) -> Vec<String> {
        self.engine.lock().expect("host lock").as_ref().map(|(_, e)| e.log.lock().expect("host log lock").clone()).unwrap_or_default()
    }

    pub fn send(&self, message: engine::ToEngine) -> Result<(), String> {
        let slot = self.engine.lock().expect("host lock");
        let (_, engine) = slot.as_ref().ok_or("sharing is off; turn it on in Settings")?;
        engine.to_engine.send(message).map_err(|_| "the sharing engine is not running".to_string())
    }

    pub fn setup(&self, options: SetupOptions) -> Result<(), String> {
        self.send(engine::ToEngine::Setup(options))
    }
}

/// `DOCKERNANNY_FAKE_HOST=1` swaps in a pretend computer, so the page can be
/// tried without changing anything on this one.
fn make_platform(config: &HostConfig) -> Arc<dyn Platform> {
    let fake = std::env::var("DOCKERNANNY_FAKE_HOST").map(|v| v == "1").unwrap_or(false);
    if fake {
        return Arc::new(fake::Fake::default());
    }
    real_platform(config)
}

#[cfg(windows)]
fn real_platform(config: &HostConfig) -> Arc<dyn Platform> {
    Arc::new(windows::Windows::new(config.wsl_distro.clone(), config.wsl_ssh_port))
}

#[cfg(target_os = "macos")]
fn real_platform(_config: &HostConfig) -> Arc<dyn Platform> {
    Arc::new(macos::MacOs)
}

#[cfg(target_os = "linux")]
fn real_platform(_config: &HostConfig) -> Arc<dyn Platform> {
    Arc::new(linux::Linux)
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
fn real_platform(_config: &HostConfig) -> Arc<dyn Platform> {
    Arc::new(fake::Fake::default())
}

/// One append-only file, `~/.dockernanny/host.log`, for what the sharing
/// role did while nobody was looking.
pub fn log_to_file(message: &str) {
    use std::io::Write;
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let path = crate::store::home_dir().join("host.log");
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{seconds} {message}");
    }
}
