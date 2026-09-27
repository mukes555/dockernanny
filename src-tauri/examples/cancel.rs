//! Proves that giving up on a remote command ends it on the machine: a
//! cancelled job and a timed-out run, each followed by a count of what the
//! machine still runs. Prints "left running = 0" twice when all is well.
//!
//!   DOCKERNANNY_HOME=../.tmp/home cargo run --example cancel -- <user> <host> <port> ~/.ssh/id_ed25519

use std::time::Duration;

use dockernanny_lib::machine::Machine;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key] = args.as_slice() else {
        eprintln!("usage: cancel <user> <host> <port> <key_path>");
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

    // Unusual durations are the markers: nothing else runs `sleep 3331`.
    println!("== a job that is cancelled after a second");
    let job = ssh.job(&alias, "sleep 3331", |line| println!("  | {}", line.text))?;
    let handle = job.handle();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    handle.cancel();
    println!("cancelled, exit {:?}", job.wait().await?);
    tokio::time::sleep(Duration::from_secs(1)).await;
    let cancelled_left = left_running(&ssh, &alias, "sleep 3331").await?;
    println!("left running = {cancelled_left}");

    println!("== a run that is given up on after a second");
    let attempt = tokio::time::timeout(Duration::from_secs(1), ssh.run(&alias, "sleep 3332")).await;
    let gave_up = attempt.is_err();
    println!("gave up = {gave_up}");
    if let Ok(early) = attempt {
        println!("  returned early: {early:?}");
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    let timed_out_left = left_running(&ssh, &alias, "sleep 3332").await?;
    println!("left running = {timed_out_left}");

    println!("== a run that finishes by itself keeps its exit code and output");
    let out = ssh.run(&alias, "echo done; exit 7").await?;
    println!("stdout = {:?}, exit = {:?}", out.stdout, out.code);

    ssh.exit_master(&alias, None);
    let all_good = cancelled_left == 0 && timed_out_left == 0 && out.code == Some(7) && out.stdout == "done";
    if !all_good {
        std::process::exit(1);
    }
    Ok(())
}

async fn left_running(ssh: &Ssh, alias: &str, command: &str) -> anyhow::Result<u32> {
    let out = ssh.run(alias, &format!("pgrep -fx '{command}' | wc -l")).await?;
    Ok(out.stdout.trim().parse().unwrap_or(0))
}
