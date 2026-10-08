//! What keeps the sharing role alive when the window is out of the way: a
//! tray icon with Show, Pairing and Quit, and a close button that hides the
//! window instead of ending the app while this computer is shared.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::host::engine::ToEngine;
use crate::stack::now_ms;
use crate::AppState;

const MAIN_WINDOW: &str = "main";
const TRAY: &str = "main";
/// How long the window still counts as looked at after it loses focus: a
/// glance at another app should not slow the cards, a window left behind
/// others all afternoon should.
const STILL_IN_VIEW_FOR: Duration = Duration::from_secs(120);

/// Whether someone is looking at the window, kept from its focus events.
/// The polls ask every few seconds, and asking the window itself would be a
/// round trip to the main thread each time.
#[derive(Default)]
pub struct Attention {
    focused: AtomicBool,
    /// Hidden in the tray: out of view at once, even when the focus event
    /// that follows the hiding arrives later.
    hidden: AtomicBool,
    /// When the window last lost focus, in ms since the epoch; 0 when never.
    left_ms: AtomicU64,
}

impl Attention {
    fn in_view_at(&self, now_ms: u64) -> bool {
        if self.focused.load(Ordering::Relaxed) {
            return true;
        }
        let left = self.left_ms.load(Ordering::Relaxed);
        let left_a_moment_ago = left > 0 && now_ms.saturating_sub(left) < STILL_IN_VIEW_FOR.as_millis() as u64;
        left_a_moment_ago && !self.hidden.load(Ordering::Relaxed)
    }

    /// True when the window comes back into view, the moment to refresh what it shows.
    fn focus(&self, now_ms: u64) -> bool {
        let was_in_view = self.in_view_at(now_ms);
        self.hidden.store(false, Ordering::Relaxed);
        self.focused.store(true, Ordering::Relaxed);
        !was_in_view
    }

    fn blur(&self, now_ms: u64) {
        self.focused.store(false, Ordering::Relaxed);
        self.left_ms.store(now_ms, Ordering::Relaxed);
    }

    fn hide(&self) {
        self.focused.store(false, Ordering::Relaxed);
        self.hidden.store(true, Ordering::Relaxed);
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    focused(app);
}

/// Whether someone is looking at the window: it has focus, or had it within
/// the last two minutes. The polls slow down while nobody is, and catch up
/// when the window comes back.
pub fn window_in_view(app: &AppHandle) -> bool {
    app.state::<AppState>().attention.in_view_at(now_ms())
}

/// The window is in front of someone, at start or after a focus event.
/// Only when it comes back into view do the polls and the update check run
/// at once; a click between two windows on the screen changes nothing.
pub fn focused(app: &AppHandle) {
    let came_back = app.state::<AppState>().attention.focus(now_ms());
    if came_back {
        came_into_view(app);
        crate::updates::window_came_back(app);
    }
}

/// Waits for a poll's next turn: `in_view` while the window can be seen,
/// `hidden` while not. The window coming back ends the wait at once, so
/// what it shows is fresh.
pub async fn until_next_poll(app: &AppHandle, in_view: Duration, hidden: Duration) {
    let every = if window_in_view(app) { in_view } else { hidden };
    let window_back = &app.state::<AppState>().window_back;
    tokio::select! {
        _ = tokio::time::sleep(every) => {}
        _ = window_back.notified() => {}
    }
}

fn came_into_view(app: &AppHandle) {
    app.state::<AppState>().window_back.notify_waiters();
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let icon = app.default_window_icon().cloned().ok_or(tauri::Error::WindowNotFound)?;
    TrayIconBuilder::with_id(TRAY)
        .icon(icon)
        .tooltip("dockerNanny")
        .menu(&menu(app, None)?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "pairing" => {
                let state = app.state::<AppState>();
                let _ = state.host.send(ToEngine::ArmPairing);
                show_main_window(app);
            }
            "update" => {
                show_main_window(app);
                let _ = app.emit(crate::updates::OPEN_EVENT, ());
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// The app often lives in the tray, so a new version is offered there too.
pub fn show_update(app: &AppHandle, version: Option<&str>) {
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    if let Ok(menu) = menu(app, version) {
        let _ = tray.set_menu(Some(menu));
    }
}

fn menu(app: &AppHandle, update: Option<&str>) -> tauri::Result<Menu<tauri::Wry>> {
    let show = MenuItem::with_id(app, "show", "Show dockerNanny", true, None::<&str>)?;
    let pairing = MenuItem::with_id(app, "pairing", "Turn pairing on for 10 minutes", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit dockerNanny", true, None::<&str>)?;
    let Some(version) = update else {
        return Menu::with_items(app, &[&show, &pairing, &quit]);
    };
    let update = MenuItem::with_id(app, "update", format!("Update to {version}…"), true, None::<&str>)?;
    Menu::with_items(app, &[&show, &update, &pairing, &quit])
}

/// Closing the window only hides it while this computer is shared: other
/// computers depend on the sharing role staying up. Quit lives in the tray.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    match event {
        WindowEvent::Focused(true) => focused(window.app_handle()),
        WindowEvent::Focused(false) => window.state::<AppState>().attention.blur(now_ms()),
        WindowEvent::CloseRequested { api, .. } => {
            let shared = window.state::<AppState>().store.settings().map(|s| s.share_this_computer).unwrap_or(false);
            if shared {
                api.prevent_close();
                let _ = window.hide();
                window.state::<AppState>().attention.hide();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE_MS: u64 = 60_000;

    #[test]
    fn a_window_stays_in_view_a_little_after_it_loses_focus() {
        let attention = Attention::default();
        assert!(!attention.in_view_at(0), "not in view before it was ever focused");
        assert!(attention.focus(10 * MINUTE_MS), "the first focus brings it into view");
        assert!(!attention.focus(10 * MINUTE_MS), "focused again while in view: nothing to refresh");

        attention.blur(11 * MINUTE_MS);
        assert!(attention.in_view_at(12 * MINUTE_MS), "a glance at another app keeps it in view");
        assert!(!attention.focus(12 * MINUTE_MS), "back within two minutes: still fresh");

        attention.blur(13 * MINUTE_MS);
        assert!(!attention.in_view_at(16 * MINUTE_MS), "left behind other windows for three minutes");
        assert!(attention.focus(16 * MINUTE_MS), "coming back then refreshes");
    }

    #[test]
    fn a_window_hidden_in_the_tray_is_out_of_view_at_once() {
        let attention = Attention::default();
        attention.focus(MINUTE_MS);
        attention.hide();
        // The focus loss that follows the hiding must not count as a glance away.
        attention.blur(MINUTE_MS);
        assert!(!attention.in_view_at(MINUTE_MS));
        assert!(attention.focus(2 * MINUTE_MS), "shown again from the tray: refresh");
    }
}
