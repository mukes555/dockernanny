//! New releases, looked for by the app itself rather than its window: at
//! start, every hour, and when the window comes forward after an hour. A
//! hidden window (start at login) or a crashed one never stops it. Tauri's
//! updater does the work the same way on macOS, Windows and Linux, and only
//! installs what this project's key signed. Installing is always the
//! user's click.

use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
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
    /// What the last check found; None before the first one ends.
    pub outcome: Option<CheckOutcome>,
    /// Why the last check failed, when it did.
    pub error: Option<String>,
    pub checking: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckOutcome {
    UpToDate,
    Found,
    Failed,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<UpdateStatus>,
    found: Mutex<Option<Update>>,
    last_answer: Mutex<Option<SystemTime>>,
    /// When a check last started, answered or not: offline, each focus of
    /// the window would otherwise ask again.
    last_try: Mutex<Option<SystemTime>>,
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
        *self.last_try.lock().expect("last try lock") = Some(SystemTime::now());
        true
    }

    fn finish(&self, answer: Result<Option<Update>, String>) -> UpdateStatus {
        let mut status = self.status.lock().expect("update status lock");
        status.checking = false;
        status.checked_ms = Some(millis(SystemTime::now()));
        match answer {
            Ok(found) => {
                *self.last_answer.lock().expect("last answer lock") = Some(SystemTime::now());
                status.available =
                    found.as_ref().map(|u| AvailableUpdate { version: u.version.clone(), notes: u.body.clone().unwrap_or_default() });
                status.outcome = Some(if found.is_some() { CheckOutcome::Found } else { CheckOutcome::UpToDate });
                status.error = None;
                *self.found.lock().expect("found update lock") = found;
            }
            // An update found earlier stays on offer; only the answer changes.
            Err(error) => {
                status.outcome = Some(CheckOutcome::Failed);
                status.error = Some(error);
            }
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

/// The window came back into view (see `tray::focused`): look again if the
/// last answer is an hour old.
pub fn window_came_back(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { check_if_due(&app).await });
}

async fn check_if_due(app: &AppHandle) {
    let allowed = app.state::<AppState>().store.settings().unwrap_or_default().check_updates;
    let updates = app.state::<Updates>();
    let last_answer = *updates.last_answer.lock().expect("last answer lock");
    let last_try = *updates.last_try.lock().expect("last try lock");
    if is_due(allowed, last_answer, last_try, SystemTime::now()) {
        check_now(app).await;
    }
}

/// Due an hour after the last answer, but no sooner than ten minutes after
/// the last try, so a computer that is offline is not asked again and again.
fn is_due(allowed: bool, last_answer: Option<SystemTime>, last_try: Option<SystemTime>, now: SystemTime) -> bool {
    if !allowed {
        return false;
    }
    // A clock set back makes a time look new; checking then is harmless.
    let answer_is_old = last_answer.is_none_or(|last| now.duration_since(last).map(|age| age >= CHECK_EVERY).unwrap_or(true));
    let tried_a_moment_ago = last_try.is_some_and(|last| now.duration_since(last).map(|age| age < LOOK_EVERY).unwrap_or(false));
    answer_is_old && !tried_a_moment_ago
}

/// Asks the release feed now. The answer goes to the window, the tray and
/// app.log; a failure is logged once, not every ten minutes while offline.
pub async fn check_now(app: &AppHandle) -> UpdateStatus {
    let updates = app.state::<Updates>();
    if !updates.begin() {
        return updates.status();
    }
    let before = updates.status();
    let previous_error = before.error;
    let previous_version = before.available.map(|a| a.version);
    let answer = match updater(app) {
        Ok(updater) => updater.check().await.map_err(|err| err.to_string()),
        Err(err) => Err(err.to_string()),
    };
    let status = updates.finish(answer);
    match (&status.error, &status.available) {
        (Some(error), _) if previous_error.as_ref() != Some(error) => tracing::warn!("update check failed: {error}"),
        (Some(_), _) => {}
        (None, Some(found)) => tracing::info!("update check: {} is available", found.version),
        (None, None) => tracing::info!("update check: up to date"),
    }
    let _ = app.emit(STATUS_EVENT, &status);
    // The tray menu is rebuilt only when what it offers changed.
    let version = status.available.as_ref().map(|a| a.version.clone());
    if version != previous_version {
        crate::tray::show_update(app, version.as_deref());
    }
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

/// The update file comes from GitHub's download servers: one name, several
/// addresses. From some networks one of them never answers, and without a
/// connect timeout each such address costs the operating system's own limit
/// (about 75 s on macOS) before the next one is tried. The HTTP library
/// divides this timeout across a name's addresses, so with four addresses a
/// dead one costs 5 s.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// The whole check, so "Checking…" can never hang; the next look retries.
/// The updater applies it to the check only, never to the download, which
/// may take long on a slow link.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// On Windows the updater ends the app itself to run the installer; the
/// app's normal shutdown runs first there too.
fn updater(app: &AppHandle) -> tauri_plugin_updater::Result<tauri_plugin_updater::Updater> {
    let handle = app.clone();
    app.updater_builder()
        .configure_client(http_client)
        .timeout(CHECK_TIMEOUT)
        .on_before_exit(move || {
            crate::shut_down(&handle);
            handle.cleanup_before_exit();
        })
        .build()
}

/// The HTTP client settings for both the check and the download.
pub fn http_client(client: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    client.connect_timeout(CONNECT_TIMEOUT)
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
        let minutes_ago = |m: u64| Some(now - Duration::from_secs(m * 60));
        assert!(is_due(true, None, None, now), "never checked: check at start");
        assert!(!is_due(false, None, None, now), "the setting is off: never");
        assert!(!is_due(true, minutes_ago(59), minutes_ago(59), now));
        assert!(is_due(true, minutes_ago(61), minutes_ago(61), now));
        // Asleep all night: the first look after waking checks.
        assert!(is_due(true, minutes_ago(9 * 60), minutes_ago(9 * 60), now));
        assert!(is_due(true, Some(now + Duration::from_secs(60)), None, now), "a clock set back does not block checks");
    }

    #[test]
    fn an_offline_computer_is_not_asked_again_at_every_focus() {
        let now = SystemTime::now();
        let minutes_ago = |m: u64| Some(now - Duration::from_secs(m * 60));
        // The last answer is old because the last tries failed.
        assert!(!is_due(true, minutes_ago(90), minutes_ago(2), now), "tried two minutes ago");
        assert!(is_due(true, minutes_ago(90), minutes_ago(11), now), "ten minutes on, try again");
        assert!(!is_due(true, None, minutes_ago(1), now), "the first try at start failed a minute ago");
    }

    /// What one of GitHub's download servers did from one network: the name's
    /// first address never answers. The next address is tried after a share
    /// of the connect timeout (5 s of 20 with four addresses), not after the
    /// operating system gives up. Takes those 5 s.
    #[tokio::test]
    async fn a_dead_address_is_skipped_within_the_connect_timeout() {
        use std::io::{Read, Write};
        use std::net::{SocketAddr, TcpListener};

        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = server.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let Ok((mut stream, _)) = server.accept() else { return };
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });
        // As the updater does before it builds its client.
        let _ = rustls::crypto::ring::default_provider().install_default();
        // A private address nothing answers on, then three times this test's server.
        let dead = SocketAddr::from(([10, 255, 255, 1], port));
        let live = SocketAddr::from(([127, 0, 0, 1], port));
        let client = http_client(reqwest::Client::builder())
            .resolve_to_addrs("updates.dockernanny.test", &[dead, live, live, live])
            .build()
            .unwrap();

        let started = std::time::Instant::now();
        let body = client.get(format!("http://updates.dockernanny.test:{port}/")).send().await.unwrap().text().await.unwrap();
        assert_eq!(body, "ok");
        assert!(started.elapsed() < CONNECT_TIMEOUT / 2, "the dead address held the check for {:?}", started.elapsed());
    }

    #[test]
    fn a_failed_check_keeps_what_was_found_and_retries_soon() {
        let updates = Updates::default();
        assert!(updates.begin());
        assert!(!updates.begin(), "one check at a time");
        let status = updates.finish(Err("offline".into()));
        assert_eq!((status.outcome, status.error.as_deref()), (Some(CheckOutcome::Failed), Some("offline")));
        assert!(!status.checking);
        assert!(updates.last_answer.lock().unwrap().is_none(), "a failure is not an answer, so the next look tries again");

        assert!(updates.begin());
        let status = updates.finish(Ok(None));
        assert_eq!((status.outcome, status.error), (Some(CheckOutcome::UpToDate), None), "a good answer clears the old failure");
        assert!(status.available.is_none());
        assert!(updates.last_answer.lock().unwrap().is_some());
    }
}
