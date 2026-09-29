//! The checks that tell the user what a machine is missing before a stack is
//! sent there, each with the command that fixes it. Two round trips: ssh
//! itself, then one script that asks every tool at once and reports each
//! one's exit code and first words, marker-separated like the probe.

use std::collections::HashMap;
use std::time::Duration;

use serde::Serialize;

use crate::job::Output;
use crate::machine::Machine;
use crate::ssh::{Ssh, TimedOut};

pub const DOCTOR_EVENT: &str = "machine:doctor";
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `docker version` may take; a daemon that hangs then gets its own
/// row instead of the whole check running into CHECK_TIMEOUT. Only Linux has
/// `timeout`, and only a Linux daemon is commonly seen hanging.
const DOCKER_LIMIT_S: u64 = 10;
/// The tools' messages in English whatever the machine's language, and a
/// missing tool known by the shell's exit code 127 rather than by a word.
const TOOLS_SCRIPT: &str = r#"export LC_ALL=C
t=; command -v timeout >/dev/null 2>&1 && t="timeout DOCKER_LIMIT_S"
out=$($t docker version --format '{{.Server.Version}}' 2>&1); echo "=== docker $?"; echo "$out"
out=$(docker compose version --short 2>&1); echo "=== compose $?"; echo "$out"
out=$(rsync --version 2>&1); echo "=== rsync $?"; echo "$out" | head -1
echo "=== host 0"; uname -sr; nproc 2>/dev/null || sysctl -n hw.ncpu
"#;
/// What the shell says about a command it cannot find, and `timeout` about one it ended.
const NOT_FOUND: i32 = 127;
const TIMED_OUT: i32 = 124;

#[derive(Debug, Clone, Serialize)]
pub struct DoctorRow {
    pub key: &'static str,
    pub label: &'static str,
    pub ok: bool,
    pub detail: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorEvent {
    pub machine_id: String,
    pub row: DoctorRow,
}

/// Runs the checks in order and reports each one as it finishes. Stops after
/// the SSH check when the machine cannot be reached; the rest would only
/// repeat the same error.
pub async fn doctor(ssh: &Ssh, machine: &Machine, mut report: impl FnMut(&DoctorRow)) -> Vec<DoctorRow> {
    let mut rows = Vec::new();
    let mut add = |row: DoctorRow| {
        report(&row);
        let ok = row.ok;
        rows.push(row);
        ok
    };
    let reachable = add(check_ssh(ssh, machine).await);
    if !reachable {
        return rows;
    }
    let script = TOOLS_SCRIPT.replace("DOCKER_LIMIT_S", &DOCKER_LIMIT_S.to_string());
    let answers = match ssh.run_within(&machine.alias(), &script, CHECK_TIMEOUT).await {
        Ok(out) => parse_answers(&out.stdout),
        Err(err) => {
            let why = format!("{err:#}");
            for (key, label) in TOOLS {
                add(row(key, label, false, why.clone(), None));
            }
            return rows;
        }
    };
    add(docker_row(answers.get("docker")));
    add(compose_row(answers.get("compose")));
    add(rsync_row(answers.get("rsync")));
    add(host_row(answers.get("host")));
    rows
}

/// The rows after SSH, in the order the window lists them.
const TOOLS: [(&str, &str); 4] = [("docker", "Docker"), ("compose", "Compose"), ("rsync", "rsync"), ("host", "Host")];

fn row(key: &'static str, label: &'static str, ok: bool, detail: impl Into<String>, fix: Option<String>) -> DoctorRow {
    DoctorRow { key, label, ok, detail: detail.into(), fix }
}

/// One tool's answer in the script's output: its exit code and what it printed.
#[derive(Debug, PartialEq)]
struct Answer {
    code: i32,
    lines: Vec<String>,
}

impl Answer {
    fn first(&self) -> String {
        self.lines.iter().map(|line| line.trim()).find(|line| !line.is_empty()).unwrap_or("").to_string()
    }
}

/// `=== <tool> <exit code>` starts each answer.
fn parse_answers(text: &str) -> HashMap<String, Answer> {
    let mut answers = HashMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let marker = line.strip_prefix("=== ").and_then(|rest| rest.split_once(' '));
        if let Some((tool, code)) = marker {
            answers.insert(tool.to_string(), Answer { code: code.trim().parse().unwrap_or(-1), lines: Vec::new() });
            current = Some(tool.to_string());
            continue;
        }
        if let Some(answer) = current.as_ref().and_then(|tool| answers.get_mut(tool)) {
            answer.lines.push(line.to_string());
        }
    }
    answers
}

/// A row for a tool whose answer is missing from the output.
fn unanswered(key: &'static str, label: &'static str) -> DoctorRow {
    row(key, label, false, "no answer for this check", None)
}

async fn check_ssh(ssh: &Ssh, machine: &Machine) -> DoctorRow {
    let out = match ssh.run_within(&machine.alias(), "echo dockernanny-ok", CHECK_TIMEOUT).await {
        Ok(out) => out,
        Err(err) if err.downcast_ref::<TimedOut>().is_some() => {
            let fix = "The machine let this computer in but did not answer: it may be busy, going to sleep, or an old connection is stuck. Check again: it starts with a fresh connection.";
            return row("ssh", "SSH", false, format!("{err:#}"), Some(fix.into()));
        }
        Err(err) => return row("ssh", "SSH", false, format!("{err:#}"), Some("Is `ssh` installed on this computer?".into())),
    };
    if out.ok() && out.stdout.contains("dockernanny-ok") {
        return row("ssh", "SSH", true, format!("{} answers", machine.address()), None);
    }
    let (detail, fix) = explain_ssh_failure(machine, &out, &ssh.config_path());
    row("ssh", "SSH", false, detail, Some(fix))
}

/// A command as it is typed in a terminal here: on Windows ssh lives inside WSL.
fn in_terminal(command: &str) -> String {
    match crate::tools::wsl_distro() {
        Some(distro) => format!("wsl -d {distro} {command}"),
        None => command.to_string(),
    }
}

fn explain_ssh_failure(machine: &Machine, out: &Output, config: &str) -> (String, String) {
    let err = &out.stderr;
    let key_rejected = err.contains("Permission denied");
    let host_key_changed = err.contains("Host key verification failed") || err.contains("IDENTIFICATION HAS CHANGED");
    let refused = err.contains("Connection refused");
    let unreachable =
        err.contains("timed out") || err.contains("No route") || err.contains("Could not resolve") || err.contains("unreachable");

    if key_rejected {
        // Only macOS's ssh-add knows the keychain flag; elsewhere it is an unknown option.
        let keychain = if cfg!(target_os = "macos") { " --apple-use-keychain" } else { "" };
        let key = crate::tools::path(std::path::Path::new(&machine.key_path));
        let copy_id = in_terminal(&format!("ssh-copy-id -i {key}.pub -p {} {}@{}", machine.port, machine.user, machine.host));
        let add = in_terminal(&format!("ssh-add{keychain} {key}"));
        return ("The machine did not accept the key.".into(), format!("{copy_id}\n# key with a passphrase? load it first:\n{add}"));
    }
    if host_key_changed {
        // The app keeps what it learned about machines in its own known_hosts, next to the config.
        let known_hosts = config.rsplit_once('/').map(|(dir, _)| format!("{dir}/known_hosts")).unwrap_or_else(|| "known_hosts".into());
        return (
            "The machine's host key changed since it was last seen.".into(),
            format!(
                "# only if you reinstalled the machine:\n{}",
                in_terminal(&format!("ssh-keygen -f {known_hosts} -R '{}'", crate::ssh::known_hosts_name(machine)))
            ),
        );
    }
    if refused {
        return (
            format!("Nothing answers on port {}.", machine.port),
            "Start sshd on the machine (Remote Login on macOS). A Windows machine runs it inside WSL 2: see Prepare another machine."
                .into(),
        );
    }
    if unreachable {
        return (
            format!("This computer cannot reach {}.", machine.host),
            "Check the address, and that both machines are on the same network.".into(),
        );
    }
    let detail = if err.is_empty() { format!("ssh exited with code {:?}", out.code) } else { err.clone() };
    (detail, format!("# try it by hand:\n{}", in_terminal(&format!("ssh -F {config} {}", machine.alias()))))
}

fn docker_row(answer: Option<&Answer>) -> DoctorRow {
    let Some(answer) = answer else { return unanswered("docker", "Docker") };
    let said = answer.first();
    if answer.code == 0 {
        return row("docker", "Docker", true, format!("Docker Engine {said}"), None);
    }
    // Docker's own messages, which it never translates.
    let not_allowed = said.contains("permission denied");
    let not_running = said.contains("Cannot connect to the Docker daemon");
    let (detail, fix) = if answer.code == NOT_FOUND {
        ("Docker is not installed".to_string(), "curl -fsSL https://get.docker.com | sudo sh\nsudo usermod -aG docker $USER")
    } else if answer.code == TIMED_OUT {
        (
            format!("Docker did not answer within {DOCKER_LIMIT_S} s"),
            "sudo systemctl restart docker\n# Docker Desktop: restart it from its menu",
        )
    } else if not_allowed {
        (said, "sudo usermod -aG docker $USER\n# then click Check again: every new ssh login picks up the group")
    } else if not_running {
        (said, "sudo systemctl enable --now docker\n# WSL2: enable systemd first (see the guide)")
    } else {
        (said, "# see what docker says:\ndocker version")
    };
    row("docker", "Docker", false, detail, Some(fix.into()))
}

fn compose_row(answer: Option<&Answer>) -> DoctorRow {
    let Some(answer) = answer else { return unanswered("compose", "Compose") };
    if answer.code == 0 {
        return row("compose", "Compose", true, format!("Compose {}", answer.first()), None);
    }
    // The machine's OS is not known yet at this point, so the fix names the common ones.
    let fix = "# Debian or Ubuntu:\nsudo apt-get install -y docker-compose-plugin\n# Fedora: sudo dnf install -y docker-compose-plugin\n# macOS: comes with Docker Desktop and OrbStack";
    row("compose", "Compose", false, answer.first(), Some(fix.into()))
}

fn rsync_row(answer: Option<&Answer>) -> DoctorRow {
    let Some(answer) = answer else { return unanswered("rsync", "rsync") };
    if answer.code == 0 {
        return row("rsync", "rsync", true, answer.first(), None);
    }
    let detail = if answer.code == NOT_FOUND { "rsync is not installed".to_string() } else { answer.first() };
    let fix = "# Debian or Ubuntu:\nsudo apt-get install -y rsync\n# Fedora: sudo dnf install -y rsync\n# macOS: brew install rsync";
    row("rsync", "rsync", false, detail, Some(fix.into()))
}

fn host_row(answer: Option<&Answer>) -> DoctorRow {
    let Some(answer) = answer else { return unanswered("host", "Host") };
    let mut lines = answer.lines.iter().map(|line| line.trim()).filter(|line| !line.is_empty());
    let os = lines.next().unwrap_or("unknown os");
    let cpus = lines.next().unwrap_or("?");
    row("host", "Host", true, format!("{os}, {cpus} cpus"), None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn failed(stderr: &str) -> Output {
        Output { code: Some(255), stdout: String::new(), stderr: stderr.into() }
    }

    fn machine() -> Machine {
        Machine {
            id: "m1".into(),
            name: "studio".into(),
            user: "alex".into(),
            host: "192.0.2.15".into(),
            port: 2222,
            key_path: "/home/alex/.ssh/id_ed25519".into(),
            docker_context: false,
            pinned: false,
        }
    }

    #[test]
    fn each_ssh_failure_gets_its_own_fix() {
        let (detail, fix) = explain_ssh_failure(&machine(), &failed("alex@192.0.2.15: Permission denied (publickey)."), "/h/ssh_config");
        assert_eq!(detail, "The machine did not accept the key.");
        assert!(fix.contains("ssh-copy-id -i /home/alex/.ssh/id_ed25519.pub -p 2222 alex@192.0.2.15"), "{fix}");

        let (_, fix) = explain_ssh_failure(&machine(), &failed("Host key verification failed."), "/h/ssh_config");
        assert!(fix.contains("ssh-keygen -f /h/known_hosts -R '[192.0.2.15]:2222'"), "{fix}");

        let (detail, _) =
            explain_ssh_failure(&machine(), &failed("ssh: connect to host 192.0.2.15 port 2222: Connection refused"), "/h/ssh_config");
        assert_eq!(detail, "Nothing answers on port 2222.");

        let (_, fix) = explain_ssh_failure(&machine(), &failed("something new"), "/h/ssh_config");
        assert!(fix.ends_with("ssh -F /h/ssh_config dn-m1"), "{fix}");
    }

    fn answer(code: i32, text: &str) -> Answer {
        Answer { code, lines: text.lines().map(str::to_string).collect() }
    }

    #[test]
    fn answers_split_at_their_markers_with_exit_codes() {
        let text = "=== docker 0\n27.3.1\n=== compose 1\ndocker: 'compose' is not a docker command.\n=== rsync 127\nsh: 1: rsync: not found\n=== host 0\nLinux 6.8.0\n8\n";
        let answers = parse_answers(text);
        assert_eq!(answers["docker"], answer(0, "27.3.1"));
        assert_eq!(answers["rsync"].code, 127);
        assert_eq!(answers["host"].lines, vec!["Linux 6.8.0", "8"]);
    }

    #[test]
    fn a_missing_tool_is_known_by_its_exit_code_not_its_words() {
        let missing = docker_row(Some(&answer(127, "sh: 1: docker: not found")));
        assert_eq!(missing.detail, "Docker is not installed");
        assert!(missing.fix.unwrap().contains("get.docker.com"));
        // "not found" inside some other error no longer suggests installing Docker.
        let other = docker_row(Some(&answer(1, "Error response from daemon: context not found")));
        assert!(other.fix.unwrap().contains("docker version"));
        let hung = docker_row(Some(&answer(124, "")));
        assert_eq!(hung.detail, "Docker did not answer within 10 s");
        let denied = docker_row(Some(&answer(1, "permission denied while trying to connect to the Docker daemon socket")));
        assert!(denied.fix.unwrap().contains("usermod"));
        assert_eq!(rsync_row(Some(&answer(127, ""))).detail, "rsync is not installed");
        assert_eq!(host_row(Some(&answer(0, "Linux 6.8.0\n8"))).detail, "Linux 6.8.0, 8 cpus");
        assert!(!compose_row(None).ok, "an answer missing from the output is not a pass");
    }

    /// The real script on the computer running the tests: every tool answers,
    /// installed or not, and the host always does.
    #[cfg(unix)]
    #[test]
    fn the_tools_script_answers_for_every_tool() {
        let script = TOOLS_SCRIPT.replace("DOCKER_LIMIT_S", "10");
        let out = std::process::Command::new("sh").arg("-c").arg(script).output().unwrap();
        let answers = parse_answers(&String::from_utf8_lossy(&out.stdout));
        for (tool, _) in TOOLS {
            assert!(answers.contains_key(tool), "no answer for {tool}");
        }
        assert!(host_row(answers.get("host")).detail.contains(" cpus"));
    }
}
