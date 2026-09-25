//! Every container on a machine, not only the compose stacks dockerNanny
//! runs: what `docker ps -a` shows, reached over the same ssh connection the
//! machine's Docker context uses. Parsing lives here; the commands that run
//! it and the actions on a container live in `commands/containers`.

use serde::{Deserialize, Serialize};

/// `docker ps -a --no-trunc --format '{{json .}}'`: one JSON object per line.
pub const LIST_SCRIPT: &str = "docker ps -a --no-trunc --format '{{json .}}'";

/// The three verbs the UI offers; a fixed set so nothing else reaches the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Start,
    Stop,
    Restart,
}

impl Action {
    pub fn verb(self) -> &'static str {
        match self {
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
        }
    }
}

/// One row of the container table, already shaped for the window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Container {
    pub id: String,
    pub name: String,
    pub image: String,
    /// running, exited, created, paused, restarting, dead.
    pub state: String,
    /// The human line: "Up 6 minutes", "Exited (0) 2 hours ago".
    pub status: String,
    /// Published ports on the machine, as docker prints them.
    pub ports: String,
    pub created: String,
    /// The compose project this container belongs to, when it has one.
    pub project: Option<String>,
}

/// What docker prints per line; only the fields the table needs. Docker's
/// keys are capitalised, and `Labels` is one comma-joined string.
#[derive(Debug, Deserialize)]
struct Raw {
    #[serde(rename = "ID", default)]
    id: String,
    #[serde(rename = "Names", default)]
    names: String,
    #[serde(rename = "Image", default)]
    image: String,
    #[serde(rename = "State", default)]
    state: String,
    #[serde(rename = "Status", default)]
    status: String,
    #[serde(rename = "Ports", default)]
    ports: String,
    #[serde(rename = "CreatedAt", default)]
    created_at: String,
    #[serde(rename = "Labels", default)]
    labels: String,
}

pub fn parse(text: &str) -> Vec<Container> {
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<Raw>(line).ok())
        .map(|raw| Container {
            project: project_label(&raw.labels),
            // A container can carry several names on one network; the first is enough.
            name: raw.names.split(',').next().unwrap_or("").trim().to_string(),
            id: short_id(&raw.id),
            image: raw.image,
            state: raw.state,
            status: raw.status,
            ports: tidy_ports(&raw.ports),
            created: raw.created_at,
        })
        .collect()
}

fn project_label(labels: &str) -> Option<String> {
    labels.split(',').find_map(|pair| pair.trim().strip_prefix("com.docker.compose.project=").map(|v| v.to_string())).filter(|v| !v.is_empty())
}

/// The 64-hex id shortened to the 12 chars docker itself shows.
fn short_id(id: &str) -> String {
    id.chars().take(12).collect()
}

/// docker repeats the host for v4 and v6 (`0.0.0.0:8080->8080/tcp, [::]:8080->8080/tcp`);
/// keep one line per published mapping, drop the duplicates.
fn tidy_ports(ports: &str) -> String {
    let mut seen: Vec<String> = Vec::new();
    for part in ports.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        // Collapse the 0.0.0.0 and [::] forms of the same published port:
        // both end in the same "->8080/tcp" (or are the same bare port).
        let key = part.rsplit("->").next().unwrap_or(part).to_string();
        if !seen.iter().any(|s| s.ends_with(&format!("->{key}")) || s == &key) {
            seen.push(part.to_string());
        }
    }
    seen.join(", ")
}

/// An id or name safe to hand to `docker <verb>` over ssh: hex ids and
/// compose names only, nothing that could break out of the command.
pub fn safe_ref(reference: &str) -> bool {
    !reference.is_empty() && reference.len() <= 128 && reference.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_come_from_docker_json_with_the_project_from_labels() {
        let text = r#"{"ID":"abc123def456789","Names":"shop-api-web-1","Image":"nginx:alpine","State":"running","Status":"Up 6 minutes","Ports":"0.0.0.0:8080->8080/tcp, [::]:8080->8080/tcp, 8443/tcp","CreatedAt":"2026-01-01 10:00:00 +0000 UTC","Labels":"com.docker.compose.project=shop-api,com.docker.compose.service=web"}
{"ID":"0011223344556677","Names":"scratch-postgres","Image":"postgres:16","State":"exited","Status":"Exited (0) 2 hours ago","Ports":"","CreatedAt":"2026-01-01 09:00:00 +0000 UTC","Labels":"maintainer=x"}
"#;
        let rows = parse(text);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, "abc123def456");
        assert_eq!(rows[0].name, "shop-api-web-1");
        assert_eq!(rows[0].project.as_deref(), Some("shop-api"));
        assert_eq!(rows[0].state, "running");
        assert_eq!(rows[0].ports, "0.0.0.0:8080->8080/tcp, 8443/tcp", "the v6 duplicate of 8080 is dropped, 8443 kept");
        assert_eq!(rows[1].project, None);
        assert_eq!(rows[1].ports, "");
    }

    #[test]
    fn a_blank_or_broken_line_is_skipped_not_fatal() {
        let rows = parse("\nnot json\n{\"ID\":\"x\",\"Names\":\"one\"}\n");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "one");
    }

    #[test]
    fn only_plain_refs_reach_the_shell() {
        assert!(safe_ref("shop-api-web-1"));
        assert!(safe_ref("abc123def456"));
        assert!(!safe_ref("x; rm -rf /"));
        assert!(!safe_ref("$(whoami)"));
        assert!(!safe_ref(""));
    }
}
