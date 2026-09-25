//! What keeps the sharing role alive when the window is out of the way: a
//! tray icon with Show, Pairing and Quit, and a close button that hides the
//! window instead of ending the app while this computer is shared.

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

use crate::host::engine::ToEngine;
use crate::AppState;

const MAIN_WINDOW: &str = "main";

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn build(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show dockerNanny", true, None::<&str>)?;
    let pairing = MenuItem::with_id(app, "pairing", "Turn pairing on for 10 minutes", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit dockerNanny", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &pairing, &quit])?;
    let icon = app.default_window_icon().cloned().ok_or(tauri::Error::WindowNotFound)?;
    TrayIconBuilder::with_id("main")
        .icon(icon)
        .tooltip("dockerNanny")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "pairing" => {
                let state = app.state::<AppState>();
                let _ = state.host.send(ToEngine::ArmPairing);
                show_main_window(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

/// Closing the window only hides it while this computer is shared: other
/// computers depend on the sharing role staying up. Quit lives in the tray.
pub fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let WindowEvent::CloseRequested { api, .. } = event else { return };
    let shared = window.state::<AppState>().store.settings().map(|s| s.share_this_computer).unwrap_or(false);
    if shared {
        api.prevent_close();
        let _ = window.hide();
    }
}
