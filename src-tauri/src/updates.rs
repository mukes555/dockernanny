//! New releases, looked for by the app itself rather than its window: at
//! start, every hour, and when the window comes forward after an hour. A
//! hidden window (start at login) or a crashed one never stops it. Tauri's
//! updater does the work the same way on macOS, Windows and Linux, and only
//! installs what this project's key signed. Installing is always the
//! user's click.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::AppState;

const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
/// How often the loop asks whether a check is due. Wall-clock time decides,
/// so a computer that slept overnight checks soon after it wakes.
const LOOK_EVERY: Duration = Duration::from_secs(10 * 60);
pub const STATUS_EVENT: &str = "update:status";
pub const PROGRESS_EVENT: &str = "update:progress";
pub const OPEN_EVENT: &str = "update:open";

#[derive(Debug, Clone, Serialize)]
pub struct AvailableUpdate {
    pub version: String,
    pub notes: String,
}

/// What the window, Help and the tray show.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateStatus {
    pub available: Option<AvailableUpdate>,
    /// When the last check ended, in milliseconds since 1970.
    pub checked_ms: Option<u64>,
    /// "up to date", "0.3.3 is available" or why the check failed.
    pub result: Option<String>,
    pub checking: bool,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    found: Mutex<Option<Update>>,
    last_answer: Mutex<Option<SystemTime>>,
}

impl Updates {
    pub fn status(&self) -> UpdateStatus {
        self.status.lock().expect("update status lock").clone()
    }

    /// False when a check is already running; one at a time is enough.
    fn begin(&self) -> bool {
        let mut status = self.status.lock().expect("update status lock");
        if status.checking {
            return false;
        }
        status.checking = true;
        true
    }

    fn finish(&self, answer: Result<Option<Update>, String>) -> UpdateStatus {
        let mut status = self.status.lock().expect("update status lock");
        status.checking = false;
        status.checked_ms = Some(millis(SystemTime::now()));
        match answer {
            Ok(found) => {
                *self.last_answer.lock().expect("last answer lock") = Some(SystemTime::now());
                status.available = found.as_ref().map(|u| AvailableUpdate { version: u.version.clone(), notes: u.body.clone().unwrap_or_default() });
                status.result = Some(match &found {
                    Some(update) => format!("{} is available", update.version),
                    None => "up to date".into(),
                });
                *self.found.lock().expect("found update lock") = found;
            }
            // An update found earlier stays on offer; only the answer changes.
            Err(error) => status.result = Some(format!("the check failed: {error}")),
        }
        status.clone()
    }
}

/// The loop behind "at start and every hour".
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            check_if_due(&app).await;
            tokio::time::sleep(LOOK_EVERY).await;
        }
    });
}

/// The window came forward: look again if the last answer is an hour old.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    if matches!(event, WindowEvent::Focused(true)) {
        let app = window.app_handle().clone();
        tauri::async_runtime::spawn(async move { check_if_due(&app).await });
    }
}

async fn check_if_due(app: &AppHandle) {
    let allowed = app.state::<AppState>().store.settings().unwrap_or_default().check_updates;
    let last_answer = *app.state::<Updates>().last_answer.lock().expect("last answer lock");
    if is_due(allowed, last_answer, SystemTime::now()) {
        check_now(app).await;
    }
}

fn is_due(allowed: bool, last_answer: Option<SystemTime>, now: SystemTime) -> bool {
    if !allowed {
        return false;
    }
    let Some(last) = last_answer else { return true };
    // A clock set back makes the answer look new; checking then is harmless.
    now.duration_since(last).map(|age| age >= CHECK_EVERY).unwrap_or(true)
}

/// Asks the release feed now. The answer goes to the window, the tray and
/// app.log; a failure is logged once, not every ten minutes while offline.
pub async fn check_now(app: &AppHandle) -> UpdateStatus {
    let updates = app.state::<Updates>();
    if !updates.begin() {
        return updates.status();
    }
    let previous = updates.status().result;
    let answer = match updater(app) {
        Ok(updater) => updater.check().await.map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    let status = updates.finish(answer);
    let result = status.result.clone().unwrap_or_default();
    if status.available.is_some() || result == "up to date" {
        tracing::info!("update check: {result}");
    } else if previous.as_deref() != Some(result.as_str()) {
        tracing::warn!("update check: {result}");
    }
    let _ = app.emit(STATUS_EVENT, &status);
    crate::tray::show_update(app, status.available.as_ref().map(|a| a.version.as_str()));
    status
}

/// Downloads what the last check found, verifies its signature, installs it
/// and restarts through the normal exit, so bridges and ssh connections
/// close first. On Windows the installer takes over and restarts the app.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let found = app.state::<Updates>().found.lock().expect("found update lock").clone();
    let update = found.ok_or("No update to install; check again.")?;
    tracing::info!("installing update {}", update.version);
    let progress = app.clone();
    let mut received: u64 = 0;
    let on_chunk = move |chunk: usize, total: Option<u64>| {
        received += chunk as u64;
        let fraction = total.filter(|t| *t > 0).map(|t| received as f64 / t as f64);
        let _ = progress.emit(PROGRESS_EVENT, fraction);
    };
    update.download_and_install(on_chunk, || {}).await.map_err(|err| format!("the update could not be installed: {err}"))?;
    tracing::info!("update {} installed; restarting", update.version);
    app.request_restart();
    Ok(())
}

/// On Windows the updater ends the app itself to run the installer; the
/// app's normal shutdown runs first there too.
fn updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    let handle = app.clone();
    app.updater_builder()
        .on_before_exit(move || {
            crate::shut_down(&handle);
            handle.cleanup_before_exit();
        })
        .build()
}

fn millis(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_check_is_due_at_start_and_an_hour_after_the_last_answer() {
        let now = SystemTime::now();
        assert!(is_due(true, None, now), "never checked: check at start");
        assert!(!is_due(false, None, now), "the setting is off: never");
        assert!(!is_due(true, Some(now - Duration::from_secs(59 * 60)), now));
        assert!(is_due(true, Some(now - Duration::from_secs(61 * 60)), now));
        // Asleep all night: the first look after waking checks.
        assert!(is_due(true, Some(now - Duration::from_secs(9 * 60 * 60)), now));
        assert!(is_due(true, Some(now + Duration::from_secs(60)), now), "a clock set back does not block checks");
    }

    #[test]
    fn a_failed_check_keeps_what_was_found_and_retries_soon() {
        let updates = Updates::default();
        assert!(updates.begin());
        assert!(!updates.begin(), "one check at a time");
        let status = updates.finish(Err("offline".into()));
        assert_eq!(status.result.as_deref(), Some("the check failed: offline"));
        assert!(!status.checking);
        assert!(updates.last_answer.lock().unwrap().is_none(), "a failure is not an answer, so the next look tries again");

        assert!(updates.begin());
        let status = updates.finish(Ok(None));
        assert_eq!(status.result.as_deref(), Some("up to date"));
        assert!(status.available.is_none());
        assert!(updates.last_answer.lock().unwrap().is_some());
    }
}
