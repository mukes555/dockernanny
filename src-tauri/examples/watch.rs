//! Headless run of the live watcher: every burst of changes in the project
//! folder triggers one rsync to the machine. Runs until killed.
//!
//!   DOCKERNANNY_HOME=~/.dockernanny-test cargo run --example watch -- <user> <host> <port> ~/.ssh/id_ed25519 ../examples/sample-stack

use std::collections::HashMap;
use std::path::PathBuf;

use dockernanny_lib::machine::Machine;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::stack::Stack;
use dockernanny_lib::{store, sync};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key, project] = args.as_slice() else {
        eprintln!("usage: watch <user> <host> <port> <key_path> <project_dir>");
        std::process::exit(2);
    };
    let home = store::home_dir();
    std::fs::create_dir_all(&home)?;
    let ssh = Ssh::new(&home)?;
    let machine = Machine {
        id: "example".into(),
        name: "example".into(),
        user: user.clone(),
        host: host.clone(),
        port: port.parse()?,
        key_path: key.clone(),
        docker_context: false,
        pinned: false,
    };
    ssh.prepare(std::slice::from_ref(&machine))?;
    let project_dir = std::fs::canonicalize(project)?;
    let stack = Stack {
        id: "watch001".into(),
        name: "dn-watch-example".into(),
        machine_id: machine.id.clone(),
        project_dir: project_dir.display().to_string(),
        compose_rel: "docker-compose.yml".into(),
        excludes: sync::default_excludes(),
        forward_ports: false,
        live_sync: true,
        port_overrides: HashMap::new(),
    };

    let alias = machine.alias();
    let watcher_ssh = ssh.clone();
    let watcher_stack = stack.clone();
    let _watcher = sync::watch(PathBuf::from(&stack.project_dir), stack.excludes.clone(), move || {
        let result = tauri::async_runtime::block_on(sync::run(&watcher_ssh, &alias, &watcher_stack, |_| {}));
        match result {
            Ok(synced) => println!("synced {} files", synced.files),
            Err(err) => println!("sync failed: {err:#}"),
        }
    })?;
    println!("watching {} (touch a file)", stack.project_dir);
    tokio::signal::ctrl_c().await?;
    Ok(())
}
