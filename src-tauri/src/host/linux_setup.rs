//! What Set up on Linux shows and runs, decided from what the checks found.
//! The root script holds only the parts something is missing for, so a
//! second Set up changes nothing, and every part is the distribution's or
//! Docker's own step. Kept apart from `linux.rs` so it builds and is tested
//! on every platform.

use super::platform::{Row, State};
use crate::docker_access::DockerAccess;
use crate::stack::shell_quote;

/// The Docker line on the sharing page, saying what Set up will do about it.
pub fn docker_row(docker: &DockerAccess) -> Row {
    let detail = match docker {
        DockerAccess::NotInstalled => "not installed; Set up installs it".to_string(),
        DockerAccess::NotRunning => "installed, not running; Set up starts it".into(),
        DockerAccess::NotInGroup => "you are not in the docker group; Set up adds you".into(),
        other => other.detail(),
    };
    let state = match docker {
        DockerAccess::Ready(_) => State::Ok,
        DockerAccess::RestartNeeded => State::Restart,
        _ => State::Missing,
    };
    Row { name: "Docker Engine", state, detail }
}

/// What is left for the user once the root script has nothing to do for
/// Docker: a restart, or a Docker that Set up does not manage.
pub fn docker_action_for_user(docker: &DockerAccess) -> Option<String> {
    match docker {
        DockerAccess::RestartNeeded => Some(
            "Docker is running and you are in the docker group. Restart this computer once so your login can use it (Docker's own post-install step), then open dockerNanny again."
                .into(),
        ),
        DockerAccess::Other(error) => Some(format!(
            "Docker says: {error}\nSet up only sets up the standard Docker Engine, so it leaves this Docker as it is. Once `docker version` works without sudo, click Set up again."
        )),
        _ => None,
    }
}

/// The changes Set up makes as root.
#[derive(Debug, Default)]
pub struct RootPlan {
    packages: Vec<&'static str>,
    commands: Vec<String>,
    /// What the user reads afterwards, one entry per change.
    changes: Vec<&'static str>,
    /// Joining the docker group reaches only a new login.
    restart_after: bool,
}

impl RootPlan {
    pub fn for_missing(rsync_missing: bool, sshd_missing: bool, docker: &DockerAccess, user: &str) -> RootPlan {
        let mut plan = RootPlan::default();
        if rsync_missing {
            plan.packages.push("rsync");
            plan.changes.push("installed rsync");
        }
        if sshd_missing {
            plan.packages.push("openssh-server");
            plan.commands.push("systemctl enable --now ssh 2>/dev/null || systemctl enable --now sshd".into());
            plan.changes.push("turned the SSH server on");
        }
        let join_group = format!("usermod -aG docker {}", shell_quote(user));
        match docker {
            DockerAccess::NotInstalled => {
                plan.packages.extend(["curl", "ca-certificates"]);
                // Docker's official install script, which also creates the
                // docker group and service. Never run over an existing Docker.
                plan.commands.push("if ! command -v docker >/dev/null 2>&1; then curl -fsSL https://get.docker.com | sh; fi".into());
                plan.commands.push("systemctl enable --now docker".into());
                plan.commands.push(join_group);
                plan.changes.push("installed Docker Engine");
                plan.restart_after = true;
            }
            DockerAccess::NotRunning => {
                plan.commands.push("systemctl enable --now docker".into());
                plan.changes.push("started Docker");
            }
            DockerAccess::NotInGroup => {
                plan.commands.push(join_group);
                plan.changes.push("added you to the docker group");
                plan.restart_after = true;
            }
            DockerAccess::Ready(_) | DockerAccess::RestartNeeded | DockerAccess::Other(_) => {}
        }
        plan
    }

    pub fn is_empty(&self) -> bool {
        self.packages.is_empty() && self.commands.is_empty()
    }

    pub fn script(&self) -> String {
        let mut lines = vec!["set -e".to_string(), "export DEBIAN_FRONTEND=noninteractive".into()];
        if !self.packages.is_empty() {
            let list = self.packages.join(" ");
            lines.push(format!(
                "if command -v apt-get >/dev/null 2>&1; then apt-get update -qq && apt-get install -y -qq {list}; elif command -v dnf >/dev/null 2>&1; then dnf install -y -q {list}; fi"
            ));
        }
        lines.extend(self.commands.iter().cloned());
        lines.push("echo dockernanny-linux-ok".into());
        lines.join("\n") + "\n"
    }

    pub fn summary(&self) -> String {
        let done = format!("Done: {}.", self.changes.join(", "));
        if self.restart_after {
            format!("{done} Restart this computer once so your login can use Docker.")
        } else {
            done
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_restart_or_a_docker_set_up_elsewhere_needs_no_root() {
        assert!(RootPlan::for_missing(false, false, &DockerAccess::RestartNeeded, "mk").is_empty());
        assert!(RootPlan::for_missing(false, false, &DockerAccess::Other("snap".into()), "mk").is_empty());
        assert!(RootPlan::for_missing(false, false, &DockerAccess::Ready("29.8.1".into()), "mk").is_empty());
        assert!(docker_action_for_user(&DockerAccess::RestartNeeded).unwrap().contains("Restart this computer once"));
        assert_eq!(docker_action_for_user(&DockerAccess::NotRunning), None);
    }

    #[test]
    fn the_script_holds_only_what_is_missing() {
        let start = RootPlan::for_missing(false, false, &DockerAccess::NotRunning, "mk");
        let script = start.script();
        assert!(script.contains("systemctl enable --now docker"));
        assert!(!script.contains("apt-get"), "nothing to install: {script}");
        assert!(!script.contains("usermod"));
        assert_eq!(start.summary(), "Done: started Docker.");

        let join = RootPlan::for_missing(false, false, &DockerAccess::NotInGroup, "mk");
        assert!(join.script().contains("usermod -aG docker 'mk'"));
        assert!(join.summary().ends_with("Restart this computer once so your login can use Docker."));

        let rsync = RootPlan::for_missing(true, false, &DockerAccess::Ready("29.8.1".into()), "mk");
        assert!(rsync.script().contains("apt-get install -y -qq rsync;"));
        let docker_commands = ["get.docker.com", "systemctl", "usermod"];
        assert!(!docker_commands.iter().any(|c| rsync.script().contains(c)), "Docker is fine, so no Docker step: {}", rsync.script());
    }

    #[test]
    fn a_fresh_computer_gets_the_whole_official_setup() {
        let plan = RootPlan::for_missing(true, true, &DockerAccess::NotInstalled, "o'neil");
        let script = plan.script();
        assert!(script.starts_with("set -e\n"));
        assert!(script.contains("install -y -qq rsync openssh-server curl ca-certificates"));
        assert!(script.contains("then curl -fsSL https://get.docker.com | sh; fi"));
        assert!(script.contains("usermod -aG docker 'o'\\''neil'"));
        assert!(script.ends_with("echo dockernanny-linux-ok\n"));
        assert_eq!(plan.summary(), "Done: installed rsync, turned the SSH server on, installed Docker Engine. Restart this computer once so your login can use Docker.");
    }

    #[test]
    fn the_docker_row_says_what_set_up_does() {
        assert_eq!(docker_row(&DockerAccess::RestartNeeded).state, State::Restart);
        assert_eq!(docker_row(&DockerAccess::Ready("29.8.1".into())).state, State::Ok);
        let missing = docker_row(&DockerAccess::NotInstalled);
        assert_eq!((missing.state, missing.detail.as_str()), (State::Missing, "not installed; Set up installs it"));
    }
}
