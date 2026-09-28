//! What keeps the sharing role alive when the window is out of the way: a
//! tray icon with Show, Pairing and Quit, and a close button that hides the
//! window instead of ending the app while this computer is shared.

use std::time::Duration;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, WindowEvent};

use crate::host::engine::ToEngine;
use crate::AppState;

const MAIN_WINDOW: &str = "main";
const TRAY: &str = "main";

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
    came_into_view(app);
}

/// Whether someone can see the window: shown and not minimized. The polls
/// slow down while nobody can, and catch up when the window comes back.
pub fn window_in_view(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(MAIN_WINDOW) else { return false };
    let shown = window.is_visible().unwrap_or(true);
    let minimized = window.is_minimized().unwrap_or(false);
    shown && !minimized
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
    if matches!(event, WindowEvent::Focused(true)) {
        came_into_view(window.app_handle());
        return;
    }
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    let shared = window.state::<AppState>().store.settings().map(|s| s.share_this_computer).unwrap_or(false);
    if shared {
        api.prevent_close();
        let _ = window.hide();
    }
}
