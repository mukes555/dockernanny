// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // The elevated helper never opens a window: it runs one fixed task and exits.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((task, log)) = dockernanny_lib::host::elevate::task_from_args(&args) {
        std::process::exit(dockernanny_lib::host::elevate::run_task(&task, &log));
    }
    dockernanny_lib::run()
}
