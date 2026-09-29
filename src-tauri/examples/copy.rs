//! Headless copy of a stack between this computer and one machine, in
//! either direction (this computer itself works as the machine, with its own
//! sshd on): the same steps the app runs, printed instead of shown.
//!
//!   DOCKERNANNY_HOME=~/.dockernanny-test cargo run --example copy -- <user> <host> <port> <key_path> \
//!       local:<project name> machine:<name there> [config|data|both] [stop|leave]
//!   DOCKERNANNY_HOME=~/.dockernanny-test cargo run --example copy -- <user> <host> <port> <key_path> \
//!       machine:<stack name there> local:<name here>:<folder here> [config|data|both] [stop|leave]
//!
//! Every anonymous volume and every changed folder that looks like data
//! travels, the same defaults the sheet ticks. "stop" stops the source for
//! the data copy and starts it again afterwards; "leave" keeps it stopped.
//! The steps are printed as they change, with a rate line while bytes move.

use std::io::Write;

use dockernanny_lib::copy::endpoint::Site;
use dockernanny_lib::copy::progress::{megabytes, CopyProgress, Publish, StepState, Tracker};
use dockernanny_lib::copy::{self, CopyRequest, DataSelection, EndpointRef, Report, Sides, Sink};
use dockernanny_lib::job::Line;
use dockernanny_lib::machine::Machine;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::stack::Phase;
use dockernanny_lib::{store, sync};

fn print(line: Line) {
    println!("  | {}", line.text);
}

/// Prints the step list when it changes and one rate line, in place, while
/// a transfer runs: what the window's panel shows, for a terminal.
fn printer() -> Publish {
    let mut last_steps = String::new();
    Box::new(move |p: &CopyProgress| {
        let marks: Vec<String> = p
            .steps
            .iter()
            .map(|s| {
                let mark = match s.state {
                    StepState::Pending => " ",
                    StepState::Running => ">",
                    StepState::Done => "x",
                    StepState::Failed => "!",
                    StepState::Skipped => "-",
                };
                format!("[{mark}] {}", s.name)
            })
            .collect();
        let steps = marks.join("\n");
        if steps != last_steps {
            println!("\n{steps}");
            last_steps = steps;
        }
        if let Some(current) = &p.current {
            let total = current.total_bytes.map(|t| format!(" of about {}", megabytes(t))).unwrap_or_default();
            print!("\r  {}: {}{}, {}/s   ", current.label, megabytes(current.bytes), total, megabytes(current.per_second));
            let _ = std::io::stdout().flush();
        }
    })
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 6 {
        eprintln!("usage: copy <user> <host> <port> <key_path> <from> <to> [config|data|both] [stop|leave]");
        eprintln!("  from: local:<project name>  or  machine:<stack name>");
        eprintln!("  to:   machine:<name>        or  local:<name>:<folder>");
        std::process::exit(2);
    }
    let (user, host, port, key, from_arg, to_arg) = (&args[0], &args[1], &args[2], &args[3], &args[4], &args[5]);
    let what = args.get(6).map(String::as_str).unwrap_or("both");
    let stop = args.iter().skip(6).any(|a| a == "stop");
    let leave = args.iter().skip(6).any(|a| a == "leave");

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

    let (from, project) = match from_arg.split_once(':') {
        Some(("local", name)) => {
            let projects = copy::local::local_projects(&ssh).await?;
            let project =
                projects.into_iter().find(|p| p.name == name).ok_or_else(|| anyhow::anyhow!("no local compose project named {name}"))?;
            (Site::local(&project.name, &project.project_dir, &project.compose_rel), Some(project))
        }
        Some(("machine", name)) => (Site::machine(name, "docker-compose.yml", &alias, "the machine"), None),
        _ => anyhow::bail!("from must be local:<project> or machine:<stack>"),
    };
    let to = match to_arg.splitn(3, ':').collect::<Vec<_>>().as_slice() {
        ["machine", name] => Site::machine(name, &from.compose_rel, &alias, "the machine"),
        ["local", name, folder] => Site::local(name, folder, &from.compose_rel),
        _ => anyhow::bail!("to must be machine:<name> or local:<name>:<folder>"),
    };
    println!("== {} on {} -> {} on {}", from.name, from.label, to.name, to.label);

    let mut request = CopyRequest {
        source: if from.is_local() { EndpointRef::ThisComputer } else { EndpointRef::Machine { machine_id: machine.id.clone() } },
        project,
        stack_id: None,
        destination: if to.is_local() { EndpointRef::ThisComputer } else { EndpointRef::Machine { machine_id: machine.id.clone() } },
        folder: if to.is_local() { to.dir.clone() } else { String::new() },
        name: to.name.clone(),
        config: what != "data",
        data: what != "config",
        data_selection: Vec::new(),
        stop_source: stop,
        keep_source_stopped: leave,
        port_overrides: Default::default(),
        excludes: sync::default_excludes(),
        forward_ports: false,
    };
    let sides = Sides { from: from.clone(), to: to.clone(), excludes: sync::default_excludes(), helper_image: "alpine:3".into() };

    println!("== plan");
    let plan = copy::plan(&ssh, &sides, &request).await?;
    for note in &plan.notes {
        println!("  {note}");
    }
    for warning in &plan.warnings {
        println!("  note: {warning}");
    }
    println!("  destination has the stack: {}; source running: {}", plan.destination_exists, plan.source_running);
    for volume in &plan.volumes {
        println!("  volume {} ({}) -> {}", volume.name, volume.size, volume.destination_name);
    }
    for container in &plan.containers {
        for volume in &container.anonymous_volumes {
            println!("  {}: anonymous volume at {} ({})", container.service, volume.destination, volume.size);
            request.data_selection.push(DataSelection { service: container.service.clone(), path: volume.destination.clone() });
        }
        for changed in &container.changed_paths {
            println!(
                "  {}: changed {} ({} entries){}",
                container.service,
                changed.path,
                changed.entries,
                if changed.suggested { ", looks like data" } else { "" }
            );
            if changed.suggested {
                request.data_selection.push(DataSelection { service: container.service.clone(), path: changed.path.clone() });
            }
        }
    }

    println!("== copy");
    let make_sink = || -> Sink { Box::new(print) };
    let status = |phase: Phase, message: &str| println!("  [{phase:?}] {message}");
    let tracker = Tracker::new("example", &to.name, &from.label, &to.label, request.destination.clone(), printer());
    let report = Report { make_sink: &make_sink, status: &status, progress: tracker.shared() };
    let outcome = copy::run(&ssh, &home, &sides, &request, &report).await?;
    if let Some(mirrored) = outcome.mirrored {
        println!("  folder mirrored, {} files", mirrored.files);
    }
    println!("== {}", outcome.summary.text(&to));
    ssh.exit_master(&alias, None);
    Ok(())
}
