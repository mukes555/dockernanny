//! Linux: whether this login can use Docker, and if not, why. Behind the
//! Docker rows on the sharing page and on this computer's page, and what
//! Set up acts on. Everything here only reads.
//!
//! Only the standard Docker Engine (Docker's packages, its system service,
//! its socket at /var/run/docker.sock, its `docker` group) gets a specific
//! answer that Set up may act on. Docker installed any other way (snap,
//! Docker Desktop, rootless, Podman) is reported in Docker's own words and
//! left as it is.

use crate::host::platform::run;

const STANDARD_SOCKET: &str = "/var/run/docker.sock";

/// What stands between this login and Docker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerAccess {
    /// Docker answers; its version.
    Ready(String),
    NotInstalled,
    /// Docker's service is installed and stopped.
    NotRunning,
    /// The `docker` group exists and the user is not in it.
    NotInGroup,
    /// The user is in the `docker` group, but this login started before
    /// that. Only a new login picks it up, and a restart is the sure one: a
    /// log out keeps the old session while anything else (an ssh session
    /// from another computer) is still logged in.
    RestartNeeded,
    /// Anything else, in Docker's own words.
    Other(String),
}

/// The read-only answers the verdict comes from.
#[derive(Debug, Clone)]
pub struct Facts {
    /// `docker version`: the server's version, or the first line of the error.
    pub answer: Result<String, String>,
    pub installed: bool,
    pub service_exists: bool,
    pub group_exists: bool,
    /// From the user database: what the next login will have.
    pub user_in_group: bool,
    /// This process: what the current login has.
    pub login_in_group: bool,
}

impl DockerAccess {
    pub fn is_ready(&self) -> bool {
        matches!(self, DockerAccess::Ready(_))
    }

    /// One line for a status row.
    pub fn detail(&self) -> String {
        match self {
            DockerAccess::Ready(version) => version.clone(),
            DockerAccess::NotInstalled => "not installed".into(),
            DockerAccess::NotRunning => "installed, not running".into(),
            DockerAccess::NotInGroup => "you are not in the docker group yet".into(),
            DockerAccess::RestartNeeded => "running; restart this computer once so your login can use it".into(),
            DockerAccess::Other(error) => error.clone(),
        }
    }
}

/// Checks for the account this app runs as.
pub fn check() -> DockerAccess {
    let version = run("docker", &["version", "--format", "{{.Server.Version}}"], None, &[]);
    let answered = version.ok && !version.text().is_empty();
    if answered {
        return DockerAccess::Ready(version.text());
    }
    let user = run("id", &["-un"], None, &[]).text();
    let user = user.as_str();
    let facts = Facts {
        answer: Err(first_line(&version.stderr)),
        installed: run("sh", &["-c", "command -v docker"], None, &[]).ok,
        service_exists: run("systemctl", &["cat", "docker.service"], None, &[]).ok,
        group_exists: run("getent", &["group", "docker"], None, &[]).ok,
        user_in_group: has_docker_group(&run("id", &["-nG", user], None, &[]).stdout),
        login_in_group: has_docker_group(&run("id", &["-nG"], None, &[]).stdout),
    };
    classify(&facts)
}

pub fn classify(facts: &Facts) -> DockerAccess {
    let error = match &facts.answer {
        Ok(version) => return DockerAccess::Ready(version.clone()),
        Err(error) => error,
    };
    if !facts.installed {
        return DockerAccess::NotInstalled;
    }
    // Docker names the socket it tried; any other socket is a Docker
    // dockerNanny did not install, and it is not touched.
    let standard = error.contains(STANDARD_SOCKET);
    let denied = standard && error.contains("permission denied");
    let daemon_down = standard && error.contains("Cannot connect to the Docker daemon");
    let only_this_login_is_behind = facts.user_in_group && !facts.login_in_group;
    let can_join_group = facts.group_exists && !facts.user_in_group;

    if denied && only_this_login_is_behind {
        return DockerAccess::RestartNeeded;
    }
    if denied && can_join_group {
        return DockerAccess::NotInGroup;
    }
    if daemon_down && facts.service_exists {
        return DockerAccess::NotRunning;
    }
    DockerAccess::Other(error.clone())
}

fn has_docker_group(groups: &str) -> bool {
    groups.split_whitespace().any(|group| group == "docker")
}

fn first_line(text: &str) -> String {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty()).unwrap_or("");
    if line.is_empty() {
        "Docker did not answer".into()
    } else {
        line.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The exact lines Docker 29 printed on an Ubuntu 26.04 computer.
    const DENIED: &str = "permission denied while trying to connect to the docker API at unix:///var/run/docker.sock";
    const DOWN: &str = "Cannot connect to the Docker daemon at unix:///var/run/docker.sock. Is the docker daemon running?";

    fn facts(error: &str) -> Facts {
        Facts {
            answer: Err(error.into()),
            installed: true,
            service_exists: true,
            group_exists: true,
            user_in_group: true,
            login_in_group: true,
        }
    }

    #[test]
    fn a_login_older_than_the_group_needs_a_restart_not_a_reinstall() {
        // What that computer showed: `mk` in the group, the app's process not.
        let stale = Facts { login_in_group: false, ..facts(DENIED) };
        assert_eq!(classify(&stale), DockerAccess::RestartNeeded);
    }

    #[test]
    fn each_standard_gap_gets_its_own_answer() {
        let ready = Facts { answer: Ok("29.8.1".into()), ..facts("") };
        assert_eq!(classify(&ready), DockerAccess::Ready("29.8.1".into()));
        assert_eq!(classify(&Facts { installed: false, ..facts("docker: not found") }), DockerAccess::NotInstalled);
        assert_eq!(classify(&facts(DOWN)), DockerAccess::NotRunning);
        let outside = Facts { user_in_group: false, login_in_group: false, ..facts(DENIED) };
        assert_eq!(classify(&outside), DockerAccess::NotInGroup);
    }

    #[test]
    fn docker_installed_another_way_is_left_alone() {
        // Docker Desktop's socket: starting docker.service would not help.
        let desktop = "Cannot connect to the Docker daemon at unix:///home/alex/.docker/desktop/docker.sock. Is the docker daemon running?";
        assert_eq!(classify(&facts(desktop)), DockerAccess::Other(desktop.into()));
        // The snap has no docker.service and no docker group.
        let snap = Facts { service_exists: false, group_exists: false, user_in_group: false, login_in_group: false, ..facts(DOWN) };
        assert_eq!(classify(&snap), DockerAccess::Other(DOWN.into()));
        let no_group = Facts { group_exists: false, user_in_group: false, login_in_group: false, ..facts(DENIED) };
        assert_eq!(classify(&no_group), DockerAccess::Other(DENIED.into()));
    }

    #[test]
    fn groups_are_matched_whole() {
        assert!(has_docker_group("mk adm sudo docker"));
        assert!(!has_docker_group("mk dockerusers"));
        assert_eq!(first_line("\n  permission denied\nmore"), "permission denied");
        assert_eq!(first_line(""), "Docker did not answer");
    }
}
