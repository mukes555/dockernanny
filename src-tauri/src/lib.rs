// Public so `examples/` can drive the backend without the window.
pub mod commands;
pub mod compose;
pub mod computer;
pub mod containers;
pub mod diagnostics;
pub mod docker_access;
pub mod doctor;
pub mod forward;
pub mod guide;
pub mod host;
pub mod job;
pub mod machine;
pub mod copy;
pub mod pairing;
pub mod probe;
pub mod settings;
pub mod ssh;
pub mod stack;
pub mod store;
pub mod sync;
pub mod tools;
pub mod tray;
pub mod updates;

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::Manager;
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tracing_subscriber::EnvFilter;

use forward::{ForwardState, Forwarder};
use job::JobHandle;
use machine::MachineStats;
use ssh::Ssh;
use stack::StackStatus;
use store::Store;

pub struct AppState {
    pub store: Store,
    pub ssh: Ssh,
    /// The sharing role, running only while it is on in the settings.
    pub host: host::Host,
    /// Latest poll per machine id, so a freshly opened window has numbers
    /// before the next tick.
    pub stats: Mutex<HashMap<String, MachineStats>>,
    pub statuses: Mutex<HashMap<String, StackStatus>>,
    /// The compose operation running per stack id, so a new one can cancel it.
    pub jobs: Mutex<HashMap<String, JobHandle>>,
    pub forwarders: Mutex<HashMap<String, Forwarder>>,
    pub forward_states: Mutex<HashMap<String, ForwardState>>,
    /// Live-sync watchers per stack id; dropping one stops it.
    pub watchers: Mutex<HashMap<String, sync::Watcher>>,
    /// The machine setup script server, while the guide is handing it out.
    pub script_server: Mutex<Option<guide::ScriptServer>>,
    /// The latest state of every copy started in this run, by the card's stack id.
    pub copies: Mutex<HashMap<String, copy::progress::CopyProgress>>,
    /// The window came back into view: polls waiting out their slow beat
    /// run at once (see `tray::until_next_poll`).
    pub window_back: tokio::sync::Notify,
}

pub fn run() {
    init_tracing();
    // GUI apps on macOS start with a bare PATH; without this, Homebrew's
    // docker, ssh and rsync are invisible when launched from Finder.
    let _ = fix_path_env::fix();
    let state = boot().expect("dockerNanny could not prepare its home folder");
    let start_hidden = std::env::args().any(|a| a == "--minimized");

    tauri::Builder::default()
        // First, so a second launch only brings the running window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--minimized"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .manage(updates::Updates::default())
        .setup(move |app| {
            let state = app.state::<AppState>();
            forget_stale_forwarders(&state);
            machine::spawn_stats_loop(app.handle().clone());
            stack::spawn_status_loop(app.handle().clone());
            let live_stacks: Vec<_> = state.store.stacks().into_iter().filter(|s| s.live_sync).collect();
            for stack in live_stacks {
                stack::start_watcher(app.handle(), &stack);
            }
            tray::build(app.handle())?;
            apply_settings(app.handle());
            updates::start(app.handle());
            if start_hidden {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            tray::on_window_event(window, event);
            updates::on_window_event(window, event);
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_machines,
            commands::machine_stats,
            commands::app_home,
            commands::terminal_info,
            commands::wsl_distros,
            commands::computer::computer_name,
            commands::computer::computer_info,
            commands::computer::computer_readiness,
            commands::computer::generate_key,
            commands::computer::install_wsl_tools,
            commands::computer::diagnostics,
            commands::computer::reveal_app_file,
            commands::new_machine_id,
            commands::computer::default_key_path,
            commands::doctor,
            commands::add_machine,
            commands::remove_machine,
            commands::poll_machine,
            commands::preview_compose,
            commands::default_excludes,
            commands::busy_ports,
            commands::list_stacks,
            commands::stack_statuses,
            commands::create_stack,
            commands::up_stack,
            commands::down_stack,
            commands::restart_stack,
            commands::remove_stack,
            commands::forward_states,
            commands::set_forward_ports,
            commands::set_docker_context,
            commands::containers::list_containers,
            commands::containers::container_action,
            commands::containers::start_container_logs,
            commands::containers::stop_container_logs,
            commands::sync_stack,
            commands::start_logs,
            commands::stop_logs,
            commands::script_preview,
            commands::script_serve,
            commands::script_stop,
            commands::pair_machine,
            commands::copy::local_projects,
            commands::copy::copy_plan,
            commands::copy::copy_stack,
            commands::copy::copy_progress,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::reset_forwards,
            commands::host::host_snapshot,
            commands::host::host_log,
            commands::host::host_setup,
            commands::host::host_arm_pairing,
            commands::host::host_disarm_pairing,
            commands::host::host_probe,
            commands::updates::update_status,
            commands::updates::check_for_update,
            commands::updates::install_update,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                shut_down(app);
            }
        });
}

/// What every way out of the app does first, a restart for an update too:
/// sharing stops, bridges and child processes end, ssh connections close.
pub fn shut_down(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    state.host.stop();
    forget_stale_forwarders(&state);
    tools::end_all_children();
    for machine in state.store.machines() {
        state.ssh.exit_master(&machine.alias(), None);
    }
}

/// Starts or stops what the settings ask for: the WSL distribution the
/// tools use, the sharing role and the start-at-login entry. Called at
/// launch and after every save.
pub fn apply_settings(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let Some(settings) = state.store.settings() else { return };
    let distro_changed = tools::configure(&settings.wsl_distro);
    if distro_changed {
        // The new distribution needs its own config, key copies and socket folders.
        if let Err(err) = state.ssh.prepare(&state.store.machines()) {
            tracing::warn!("the tools could not be prepared in WSL {}: {err:#}", settings.wsl_distro);
        }
    }
    if settings.share_this_computer {
        state.host.start(app, host::HostConfig::from_settings(&settings));
    } else {
        state.host.stop();
    }
    let autostart = app.autolaunch();
    let result = if settings.start_at_login { autostart.enable() } else { autostart.disable() };
    if let Err(err) = result {
        tracing::warn!("start at login could not be changed: {err}");
    }
}

/// Forwarders from a crashed instance would still hold the ports; at start
/// and at exit every stack's forwarder socket is told to exit.
pub fn forget_stale_forwarders(state: &AppState) {
    let aliases: HashMap<String, String> = state
        .store
        .stacks()
        .iter()
        .filter_map(|stack| state.store.machine(&stack.machine_id).map(|m| (stack.id.clone(), m.alias())))
        .collect();
    let ids: Vec<String> = aliases.keys().cloned().collect();
    forward::exit_all(&state.ssh, ids.into_iter(), &aliases);
}

fn boot() -> anyhow::Result<AppState> {
    let home = store::home_dir();
    let store = Store::load(&home)?;
    tools::configure(&store.settings().unwrap_or_default().wsl_distro);
    let ssh = Ssh::new(&home)?;
    // Without WSL on Windows this fails; the app still starts and says what is missing.
    if let Err(err) = ssh.prepare(&store.machines()) {
        tracing::warn!("the ssh config could not be written: {err:#}");
    }
    Ok(AppState {
        store,
        ssh,
        host: host::Host::default(),
        stats: Mutex::new(HashMap::new()),
        statuses: Mutex::new(HashMap::new()),
        jobs: Mutex::new(HashMap::new()),
        forwarders: Mutex::new(HashMap::new()),
        forward_states: Mutex::new(HashMap::new()),
        watchers: Mutex::new(HashMap::new()),
        script_server: Mutex::new(None),
        copies: Mutex::new(HashMap::new()),
        window_back: tokio::sync::Notify::new(),
    })
}

/// Everything the backend says goes to `app.log` in the app folder, because a
/// window launched from the Finder or the Start menu has no terminal to show
/// stderr on; `cargo run` and the examples still print to the terminal.
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let path = store::home_dir().join("app.log");
    let file = std::fs::create_dir_all(store::home_dir()).and_then(|_| std::fs::OpenOptions::new().create(true).append(true).open(&path));
    match file {
        Ok(file) if !std::io::IsTerminal::is_terminal(&std::io::stderr()) => {
            tracing_subscriber::fmt().with_env_filter(filter).with_ansi(false).with_writer(Mutex::new(file)).init();
        }
        _ => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
}
