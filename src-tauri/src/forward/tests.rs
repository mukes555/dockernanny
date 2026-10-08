//! The bridge's pure parts, and its life run against a stand-in for ssh.

use std::collections::HashMap;

use super::*;
use crate::compose::Port;

#[test]
fn a_held_port_is_busy_and_a_free_one_is_not() {
    let holder = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = holder.local_addr().unwrap().port();
    assert_eq!(busy_ports(&[port]), vec![port]);
    // Port 0 always binds (the OS picks a free one), so this checks the
    // free branch without racing other tests for a just-released port.
    assert!(busy_ports(&[0]).is_empty());
}

#[test]
fn only_in_use_and_not_allowed_mean_taken() {
    let failed = |kind: ErrorKind| Err(std::io::Error::from(kind));
    assert!(!free(failed(ErrorKind::AddrInUse)));
    assert!(!free(failed(ErrorKind::PermissionDenied)));
    assert!(free(failed(ErrorKind::AddrNotAvailable)), "no IPv6 on this computer says nothing about the port");
}

#[test]
fn forwards_bind_localhost_and_reach_the_machine_s_loopback() {
    assert_eq!(local_forward(&ForwardPort { local: 6432, remote: 5432 }), "localhost:6432:127.0.0.1:5432");
}

#[test]
fn desired_ports_follow_running_tcp_services_and_overrides() {
    let mut stack = Stack {
        id: "s".into(),
        name: "s".into(),
        machine_id: "m".into(),
        project_dir: "/p".into(),
        compose_rel: "docker-compose.yml".into(),
        excludes: vec![],
        forward_ports: true,
        live_sync: false,
        port_overrides: HashMap::from([(5432, 6432)]),
    };
    let service = |state: &str, ports: Vec<(u16, &str)>| ServiceState {
        service: "x".into(),
        state: state.into(),
        ports: ports.into_iter().map(|(p, proto)| Port { target: p, published: p, protocol: proto.into() }).collect(),
        ..ServiceState::default()
    };
    let services = vec![service("running", vec![(3000, "tcp"), (5432, "tcp"), (9099, "udp")]), service("exited", vec![(4000, "tcp")])];
    let bridged = vec![ForwardPort { local: 3000, remote: 3000 }, ForwardPort { local: 6432, remote: 5432 }];
    assert_eq!(desired_ports(&stack, &services, &[]), bridged);
    stack.forward_ports = false;
    assert!(desired_ports(&stack, &services, &bridged).is_empty());
}

#[test]
fn a_restarting_service_keeps_its_bridge() {
    let stack = Stack {
        id: "s".into(),
        name: "s".into(),
        machine_id: "m".into(),
        project_dir: "/p".into(),
        compose_rel: "compose.yaml".into(),
        excludes: vec![],
        forward_ports: true,
        live_sync: false,
        port_overrides: HashMap::new(),
    };
    let service = |name: &str, state: &str, ports: Vec<u16>| ServiceState {
        service: name.into(),
        state: state.into(),
        ports: ports.into_iter().map(|p| Port { target: p, published: p, protocol: "tcp".into() }).collect(),
        ..ServiceState::default()
    };
    let bridged = vec![ForwardPort { local: 3000, remote: 3000 }, ForwardPort { local: 5432, remote: 5432 }];
    // The database restarts: it publishes nothing for a moment, and its port stays bridged.
    let mid_restart = vec![service("api", "running", vec![3000]), service("db", "restarting", vec![])];
    assert_eq!(desired_ports(&stack, &mid_restart, &bridged), bridged);
    // Stopped for real: the bridge follows.
    let stopped = vec![service("api", "running", vec![3000]), service("db", "exited", vec![])];
    assert_eq!(desired_ports(&stack, &stopped, &bridged), vec![ForwardPort { local: 3000, remote: 3000 }]);
}

/// The bridge's life, run against a stand-in for ssh.
#[cfg(unix)]
mod bridge {
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver};

    use super::super::*;

    /// Answers the way ssh does, in a folder of its own: `-N` is the
    /// master, which makes its socket and waits to be killed; `-O` is a
    /// control request. A `refuse-master` file there makes the master
    /// fail like a taken port; `refuse-control` makes it turn down new
    /// forwards. `log` lists the masters started and the forwards changed.
    const STAND_IN: &str = r#"#!/bin/sh
socket= request= master= forward=
while [ $# -gt 0 ]; do
  case "$1" in
-S) socket=$2; shift ;;
-O) request=$2; shift ;;
-L) forward=$2; shift ;;
-N) master=1 ;;
  esac
  shift
done
if [ -n "$master" ]; then
  if [ -e "$dir/refuse-master" ]; then echo "bind [127.0.0.1]:8080: Address already in use" >&2; exit 255; fi
  echo master >> "$dir/log"
  : > "$socket"
  exec sleep 60
fi
case "$request" in
  check) [ -e "$socket" ] ;;
  exit) rm -f "$socket" ;;
  *) if [ -e "$dir/refuse-control" ]; then echo "forwarding request failed" >&2; exit 255; fi
 echo "$request $forward" >> "$dir/log" ;;
esac
"#;

    fn stand_in_ssh(test: &str) -> (PathBuf, SshCommand) {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("bridge-{test}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("ssh");
        std::fs::write(&program, STAND_IN.replacen("#!/bin/sh", &format!("#!/bin/sh\ndir='{}'", dir.display()), 1)).unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let command = SshCommand { program: program.display().to_string(), config: "unused".into(), sockets: dir.display().to_string() };
        (dir, command)
    }

    fn port(local: u16, remote: u16) -> ForwardPort {
        ForwardPort { local, remote }
    }

    /// Starts a bridge on the stand-in; what it reports arrives on the receiver.
    fn start(
        ssh: SshCommand,
        ports: Vec<ForwardPort>,
    ) -> (watch::Sender<Vec<ForwardPort>>, UnboundedReceiver<ForwardState>, JoinHandle<()>) {
        start_with(ssh, ports, Arc::new(AtomicBool::new(true)))
    }

    /// `online` stands for what the stats poll found about the machine.
    fn start_with(
        ssh: SshCommand,
        ports: Vec<ForwardPort>,
        online: Arc<AtomicBool>,
    ) -> (watch::Sender<Vec<ForwardPort>>, UnboundedReceiver<ForwardState>, JoinHandle<()>) {
        let (states, said) = unbounded_channel();
        let (wanted, watching) = watch::channel(ports);
        let report = move |state| {
            let _ = states.send(state);
        };
        let machine_online = move || online.load(Ordering::Relaxed);
        let bridge = tokio::spawn(run(ssh, "box".into(), "shop".into(), watching, report, machine_online));
        (wanted, said, bridge)
    }

    async fn next(said: &mut UnboundedReceiver<ForwardState>) -> ForwardState {
        let waited = tokio::time::timeout(Duration::from_secs(20), said.recv()).await;
        waited.expect("the bridge said nothing").expect("the bridge ended without a word")
    }

    fn log(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("log")).unwrap_or_default()
    }

    #[tokio::test]
    async fn a_bridge_follows_new_ports_in_place_and_ends_when_dropped() {
        let (dir, ssh) = stand_in_ssh("follows");
        let (wanted, mut said, bridge) = start(ssh, vec![port(8080, 80)]);
        let up = next(&mut said).await;
        assert!(up.up, "{up:?}");
        assert_eq!(up.ports, vec![port(8080, 80)]);

        wanted.send_replace(vec![port(8080, 80), port(9090, 90)]);
        let moved = next(&mut said).await;
        assert!(moved.up && moved.since_ms == up.since_ms, "the same ssh carries the new port: {moved:?}");
        assert_eq!(moved.ports, vec![port(8080, 80), port(9090, 90)]);

        drop(wanted);
        assert_eq!(next(&mut said).await, ForwardState::default());
        bridge.await.unwrap();
        assert_eq!(log(&dir), "master\nforward localhost:9090:127.0.0.1:90\n");
    }

    #[tokio::test]
    async fn a_bridge_that_cannot_come_up_says_why_in_ssh_s_words() {
        let (dir, ssh) = stand_in_ssh("refused");
        std::fs::write(dir.join("refuse-master"), "").unwrap();
        let (wanted, mut said, bridge) = start(ssh, vec![port(8080, 80)]);
        let down = next(&mut said).await;
        assert!(!down.up);
        assert_eq!(down.error.as_deref(), Some("bind [127.0.0.1]:8080: Address already in use"));
        assert_eq!(down.attempts, 1);

        // Ended while it waits to try again.
        drop(wanted);
        assert_eq!(next(&mut said).await, ForwardState::default());
        bridge.await.unwrap();
    }

    #[tokio::test]
    async fn a_bridge_to_a_machine_that_went_away_waits_for_it_to_answer() {
        let (dir, ssh) = stand_in_ssh("away");
        std::fs::write(dir.join("refuse-master"), "").unwrap();
        let online = Arc::new(AtomicBool::new(false));
        let (wanted, mut said, bridge) = start_with(ssh, vec![port(8080, 80)], online.clone());
        assert!(!next(&mut said).await.up, "the first try fails");

        // The backoff after one failure is 2 s; while the poll says the machine
        // is away no second try follows, though it would now succeed.
        std::fs::remove_file(dir.join("refuse-master")).unwrap();
        let quiet = tokio::time::timeout(Duration::from_secs(6), said.recv()).await;
        assert!(quiet.is_err(), "the bridge dialled a machine the poll says is away");

        online.store(true, Ordering::Relaxed);
        assert!(next(&mut said).await.up, "the machine answers again: the bridge comes up");
        assert_eq!(log(&dir), "master\n");

        drop(wanted);
        assert_eq!(next(&mut said).await, ForwardState::default());
        bridge.await.unwrap();
    }

    #[tokio::test]
    async fn ports_the_running_ssh_turns_down_get_a_new_ssh() {
        let (dir, ssh) = stand_in_ssh("restart");
        let (wanted, mut said, bridge) = start(ssh, vec![port(8080, 80)]);
        assert!(next(&mut said).await.up);

        std::fs::write(dir.join("refuse-control"), "").unwrap();
        wanted.send_replace(vec![port(8080, 80), port(9090, 90)]);
        let again = next(&mut said).await;
        assert!(again.up, "{again:?}");
        assert_eq!(again.ports, vec![port(8080, 80), port(9090, 90)]);
        assert_eq!(log(&dir), "master\nmaster\n");

        drop(wanted);
        assert_eq!(next(&mut said).await, ForwardState::default());
        bridge.await.unwrap();
    }

    #[tokio::test]
    async fn a_stack_that_publishes_nothing_has_no_ssh_until_ports_return() {
        let (dir, ssh) = stand_in_ssh("idle");
        let (wanted, mut said, bridge) = start(ssh, vec![port(8080, 80)]);
        assert!(next(&mut said).await.up);

        wanted.send_replace(Vec::new());
        assert_eq!(next(&mut said).await, ForwardState::default());
        wanted.send_replace(vec![port(8080, 80)]);
        assert!(next(&mut said).await.up);
        assert_eq!(log(&dir), "master\nmaster\n");

        drop(wanted);
        assert_eq!(next(&mut said).await, ForwardState::default());
        bridge.await.unwrap();
    }
}
