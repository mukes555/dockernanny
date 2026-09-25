//! Headless run of the doctor and the stats poll against one machine:
//!
//!   DOCKERNANNY_HOME=../.tmp/home cargo run --example doctor -- <user> <host> <port> ~/.ssh/id_ed25519
//!
//! Point DOCKERNANNY_HOME somewhere disposable, or the app's own ssh config is
//! replaced with this one machine.

use dockernanny_lib::doctor;
use dockernanny_lib::machine::{self, Machine};
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key] = args.as_slice() else {
        eprintln!("usage: doctor <user> <host> <port> <key_path>");
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
    ssh.write_config(std::slice::from_ref(&machine))?;

    let started = std::time::Instant::now();
    doctor::doctor(&ssh, &machine, |row| {
        let mark = if row.ok { "ok " } else { "ERR" };
        println!("{mark} {:<8} {}  ({:?})", row.label, row.detail, started.elapsed());
        if let Some(fix) = &row.fix {
            println!("      fix: {}", fix.replace('\n', "\n           "));
        }
    })
    .await;

    let stats = machine::poll(&ssh, &machine).await;
    println!("stats: {stats:?}  ({:?})", started.elapsed());
    ssh.close_master(&machine.alias());
    Ok(())
}
