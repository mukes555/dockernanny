//! Proves the port bridge end to end, with the machine's own sshd as the
//! service behind it (it greets every connection with its banner). The
//! bridge comes up, gains a port and loses one in place while a connection
//! through the port that stays is kept open, and ends without leaving a
//! listener or a socket behind. Exits with an error at the first broken
//! promise, and prints "bridge ok" when all is well.
//!
//!   DOCKERNANNY_HOME=../.tmp/home cargo run --example bridge -- <user> <host> <port> ~/.ssh/id_ed25519

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::Duration;

use anyhow::Context;
use dockernanny_lib::forward::{self, ForwardPort, ForwardState};
use dockernanny_lib::machine::Machine;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::store;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [user, host, port, key] = args.as_slice() else {
        eprintln!("usage: bridge <user> <host> <port> <key_path>");
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
    let to_sshd = |local: u16| ForwardPort { local, remote: machine.port };
    let (first, second) = (free_port()?, free_port()?);

    let (reports, mut updates) = mpsc::unbounded_channel();
    let report = move |state: ForwardState| {
        let _ = reports.send(state);
    };
    let wanted = forward::spawn_bridge(ssh.clone(), alias.clone(), "example".into(), vec![to_sshd(first)], report);

    println!("== up");
    wait_for(&mut updates, |s| s.up && s.ports == [to_sshd(first)]).await?;
    let mut kept = TcpStream::connect((Ipv4Addr::LOCALHOST, first))?;
    println!("  localhost:{first} answers: {}", banner(&mut kept)?);

    println!("== a port added in place");
    wanted.send_replace(vec![to_sshd(first), to_sshd(second)]);
    wait_for(&mut updates, |s| s.up && s.ports.len() == 2).await?;
    let mut added = TcpStream::connect((Ipv4Addr::LOCALHOST, second))?;
    println!("  localhost:{second} answers: {}", banner(&mut added)?);

    println!("== the first port dropped in place");
    wanted.send_replace(vec![to_sshd(second)]);
    wait_for(&mut updates, |s| s.up && s.ports == [to_sshd(second)]).await?;
    anyhow::ensure!(TcpStream::connect((Ipv4Addr::LOCALHOST, first)).is_err(), "localhost:{first} still listens");
    println!("  localhost:{first} no longer listens");
    anyhow::ensure!(still_open(&mut kept), "the connection opened before the changes was cut");
    println!("  the connection opened through it before is still open");

    println!("== ended");
    drop(wanted);
    wait_for(&mut updates, |s| !s.up && s.ports.is_empty()).await?;
    anyhow::ensure!(TcpStream::connect((Ipv4Addr::LOCALHOST, second)).is_err(), "localhost:{second} still listens");
    let sockets_left = std::fs::read_dir(home.join("fwd"))?.count();
    anyhow::ensure!(sockets_left == 0, "{sockets_left} control socket(s) left behind");
    println!("  nothing listens, no socket is left");

    ssh.exit_master(&alias, None);
    println!("bridge ok");
    Ok(())
}

/// A port nothing listens on right now; the OS picks it.
fn free_port() -> anyhow::Result<u16> {
    Ok(TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?.local_addr()?.port())
}

/// Reports until one satisfies `wanted`, printing each; at most 20 s.
async fn wait_for(updates: &mut mpsc::UnboundedReceiver<ForwardState>, wanted: impl Fn(&ForwardState) -> bool) -> anyhow::Result<()> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let state = tokio::time::timeout_at(deadline, updates.recv())
            .await
            .context("the bridge did not report that within 20 s")?
            .context("the bridge stopped reporting")?;
        let locals: Vec<u16> = state.ports.iter().map(|port| port.local).collect();
        println!("  report: up {} ports {locals:?} error {:?}", state.up, state.error);
        if wanted(&state) {
            return Ok(());
        }
    }
}

/// sshd's greeting, "SSH-2.0-...", read through the bridge.
fn banner(stream: &mut TcpStream) -> anyhow::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut buffer = [0u8; 128];
    let read = stream.read(&mut buffer)?;
    let text = String::from_utf8_lossy(&buffer[..read]).trim().to_string();
    anyhow::ensure!(text.starts_with("SSH-"), "expected an ssh banner, got {text:?}");
    Ok(text)
}

/// A connection that still carries data both ways: sshd answers a client's
/// greeting with its key exchange offer. A cut one reads nothing.
fn still_open(stream: &mut TcpStream) -> bool {
    let greeted = stream.write_all(b"SSH-2.0-dockernanny-bridge-check\r\n").is_ok();
    let mut buffer = [0u8; 64];
    greeted && stream.read(&mut buffer).is_ok_and(|read| read > 0)
}
