//! Headless check of the volume stream: a local volume with one file is copied
//! to the machine under a new name, then read back there.
//!
//!   DOCKERNANNY_HOME=~/.dockernanny-test cargo run --example volume -- <user> <host> <port> ~/.ssh/id_ed25519

use dockernanny_lib::machine::Machine;
use dockernanny_lib::copy::discover::NamedVolume;
use dockernanny_lib::copy::endpoint::Site;
use dockernanny_lib::copy::progress::Tracker;
use dockernanny_lib::copy::transfer::copy_volume;
use dockernanny_lib::copy::{EndpointRef, Report, Sink};
use dockernanny_lib::ssh::{Line, Ssh};
use dockernanny_lib::stack::Phase;
use dockernanny_lib::store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key] = args.as_slice() else {
        eprintln!("usage: volume <user> <host> <port> <key_path>");
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
    let alias = machine.alias();

    let docker = |args: &[&str]| {
        let mut cmd = std::process::Command::new("docker");
        cmd.args(args).env("DOCKER_CONTEXT", "default");
        cmd.output().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    println!("== local volume with one file");
    docker(&["volume", "rm", "-f", "dn-volume-src"])?;
    docker(&["volume", "create", "dn-volume-src"])?;
    docker(&["run", "--rm", "-v", "dn-volume-src:/from", "alpine", "sh", "-c", "echo hello-from-mac > /from/hello.txt"])?;

    println!("== copy to the machine as dn-example_data");
    let _ = ssh.run(&alias, "docker volume rm -f dn-example_data >/dev/null 2>&1").await;
    let volume = NamedVolume {
        key: "data".into(),
        name: "dn-volume-src".into(),
        size: "small".into(),
        external: false,
    };
    let from = Site::local("local", ".", "docker-compose.yml");
    let to = Site::machine("dn-example", "docker-compose.yml", &alias, "the machine");
    let make_sink = || -> Sink { Box::new(|line: Line| println!("  | {}", line.text)) };
    let status = |_: Phase, _: &str| {};
    let tracker = Tracker::new("example", "dn-example", "this computer", "the machine", EndpointRef::ThisComputer, Box::new(|_| {}));
    let report = Report { make_sink: &make_sink, status: &status, progress: tracker.shared() };
    copy_volume(&ssh, &from, &to, &volume, "alpine:3", &report).await?;

    println!("== read back on the machine");
    let out = ssh.run(&alias, "docker run --rm -v dn-example_data:/x alpine cat /x/hello.txt; docker volume inspect dn-example_data --format '{{json .Labels}}'").await?;
    println!("{}", out.stdout);

    println!("== cleanup");
    let _ = ssh.run(&alias, "docker volume rm -f dn-example_data >/dev/null").await;
    docker(&["volume", "rm", "-f", "dn-volume-src"])?;
    ssh.close_master(&alias);
    Ok(())
}
