//! Everything dockerNanny remembers lives in one folder, `~/.dockernanny` by
//! default: machines.json, stacks.json, settings.json, the generated ssh
//! config and the control sockets. The same folder name holds synced projects on every
//! remote, so a path is easy to recognise on either side.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::Context;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::machine::Machine;
use crate::settings::Settings;
use crate::stack::Stack;

const MACHINES_FILE: &str = "machines.json";
const STACKS_FILE: &str = "stacks.json";
const SETTINGS_FILE: &str = "settings.json";

/// `DOCKERNANNY_HOME` lets tests and a second instance keep their own folder.
pub fn home_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("DOCKERNANNY_HOME") {
        return PathBuf::from(dir);
    }
    user_home().join(".dockernanny")
}

/// The user's own home folder. Windows sets `USERPROFILE`, not `HOME`; without
/// it the app would keep its files wherever it happened to be started from.
pub fn user_home() -> PathBuf {
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")).unwrap_or_else(|_| ".".into());
    PathBuf::from(home)
}

pub struct Store {
    home: PathBuf,
    machines: Mutex<Vec<Machine>>,
    stacks: Mutex<Vec<Stack>>,
    /// None until the user has been through the first launch.
    settings: Mutex<Option<Settings>>,
}

impl Store {
    pub fn load(home: &Path) -> anyhow::Result<Self> {
        fs::create_dir_all(home).with_context(|| format!("create {}", home.display()))?;
        let machines = read_json(&home.join(MACHINES_FILE));
        let stacks = read_json(&home.join(STACKS_FILE));
        let settings = read_json(&home.join(SETTINGS_FILE));
        Ok(Self {
            home: home.to_path_buf(),
            machines: Mutex::new(machines),
            stacks: Mutex::new(stacks),
            settings: Mutex::new(settings),
        })
    }

    pub fn stacks(&self) -> Vec<Stack> {
        self.stacks.lock().expect("stacks lock").clone()
    }

    pub fn stack(&self, id: &str) -> Option<Stack> {
        self.stacks().into_iter().find(|s| s.id == id)
    }

    pub fn save_stacks(&self, stacks: Vec<Stack>) -> anyhow::Result<()> {
        write_json(&self.home.join(STACKS_FILE), &stacks)?;
        *self.stacks.lock().expect("stacks lock") = stacks;
        Ok(())
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn machines(&self) -> Vec<Machine> {
        self.machines.lock().expect("machines lock").clone()
    }

    pub fn machine(&self, id: &str) -> Option<Machine> {
        self.machines().into_iter().find(|m| m.id == id)
    }

    pub fn save_machines(&self, machines: Vec<Machine>) -> anyhow::Result<()> {
        write_json(&self.home.join(MACHINES_FILE), &machines)?;
        *self.machines.lock().expect("machines lock") = machines;
        Ok(())
    }

    pub fn settings(&self) -> Option<Settings> {
        self.settings.lock().expect("settings lock").clone()
    }

    pub fn save_settings(&self, settings: Settings) -> anyhow::Result<()> {
        write_json(&self.home.join(SETTINGS_FILE), &settings)?;
        *self.settings.lock().expect("settings lock") = Some(settings);
        Ok(())
    }
}

/// A missing or corrupt file comes back as the default: the app must never
/// refuse to start over one bad json file.
fn read_json<T: DeserializeOwned + Default>(path: &Path) -> T {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Written to a sibling temp file and renamed into place, so a crash mid-write
/// cannot leave a half file behind.
fn write_json<T: Serialize>(path: &Path, value: &T) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(value)?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text).with_context(|| format!("write {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("replace {}", path.display()))?;
    Ok(())
}
