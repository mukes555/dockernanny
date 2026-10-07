//! Headless run of the stack path against one machine: sync the folder, compose
//! up, read ps, compose down, delete the remote folder.
//!
//!   DOCKERNANNY_HOME=~/.dockernanny-test cargo run --example stack -- <user> <host> <port> ~/.ssh/id_ed25519 ../examples/sample-stack

use std::collections::HashMap;

use dockernanny_lib::compose;
use dockernanny_lib::job::Line;
use dockernanny_lib::machine::Machine;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::stack::{down_args, shell_quote, up_args, Stack};
use dockernanny_lib::{store, sync};

fn print(line: Line) {
    println!("  | {}", line.text);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key, project] = args.as_slice() else {
        eprintln!("usage: stack <user> <host> <port> <key_path> <project_dir>");
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
    let alias = machine.alias();

    let project_dir = std::fs::canonicalize(project)?;
    let (dir, compose_rel) = compose::locate(&project_dir)?;
    let stack = Stack {
        id: "example1".into(),
        // smoke-*, like everything scripts/smoke-local.sh creates, so its cleanup finds it.
        name: "smoke-stack".into(),
        machine_id: machine.id.clone(),
        project_dir: dir.display().to_string(),
        compose_rel,
        excludes: sync::default_excludes(),
        forward_ports: true,
        live_sync: false,
        port_overrides: HashMap::new(),
    };

    println!("== preview");
    let preview = compose::preview(&project_dir).await?;
    for service in &preview.services {
        println!("  {} {:?} ports {:?}", service.name, service.image, service.ports);
    }
    for warning in &preview.warnings {
        println!("  note: {warning}");
    }

    println!("== sync");
    let synced = sync::run(&ssh, &alias, &stack, print).await?;
    println!("  {} files, warning {:?}", synced.files, synced.warning);
    let listing = ssh.run(&alias, &format!("ls -a {}", shell_quote(&stack.remote_dir()))).await?;
    println!("  remote folder: {}", listing.stdout.replace('\n', " "));

    println!("== up");
    let code = ssh.job(&alias, &stack.compose_cmd(up_args(true)), print)?.wait().await?;
    println!("  exit {code:?}");

    println!("== ps");
    let out = ssh.run(&alias, &stack.compose_cmd("ps --all --format json")).await?;
    for service in compose::parse_ps(&out.stdout) {
        println!("  {} {} {:?} {:?}", service.service, service.state, service.readiness, service.ports);
    }

    println!("== down + remove");
    let code = ssh.job(&alias, &stack.compose_cmd(down_args(false)), print)?.wait().await?;
    println!("  exit {code:?}");
    let removed = ssh.run(&alias, &format!("rm -rf {}", shell_quote(&stack.remote_dir()))).await?;
    println!("  rm exit {:?}", removed.code);
    ssh.exit_master(&alias, None);
    Ok(())
}
