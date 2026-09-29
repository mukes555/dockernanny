//! `docker compose ps --all --format json`, and what each container's state
//! means, judged the way Compose itself judges it when `up --wait` waits for
//! a stack (its `isServiceHealthy` and `getDependencyCondition`):
//!
//! - running with no health check, or a passing one: ready;
//! - running while its health check has not passed yet ("starting"), or
//!   restarting: not ready yet, which is no failure;
//! - unhealthy: a failure;
//! - a service another one waits for with `condition:
//!   service_completed_successfully` is a one-shot job, and its exit 0 is
//!   success; any other stopped container is not ready.
//!
//! Compose records those conditions on the waiting container, as the label
//! `com.docker.compose.depends_on`, so `ps` alone tells which services are
//! jobs; no second call is needed.

use serde::Serialize;
use serde_json::Value;

use super::Port;

const DEPENDS_ON_LABEL: &str = "com.docker.compose.depends_on";
const COMPLETED: &str = "service_completed_successfully";

/// One row of `compose ps`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
pub struct ServiceState {
    pub service: String,
    pub container: String,
    pub state: String,
    pub health: String,
    pub exit_code: i32,
    pub ports: Vec<Port>,
    /// Another service waits for this one to finish: a one-shot job.
    pub job: bool,
    pub readiness: Readiness,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Readiness {
    /// Running, with no health check or a passing one.
    Ready,
    /// Running while its first health check has not passed yet, or restarting.
    Starting,
    /// A one-shot job that finished with exit code 0.
    Done,
    /// Running, but its health check fails.
    Unhealthy,
    /// Not running: stopped, exited, created, paused.
    #[default]
    Stopped,
}

/// The judgement above for one container.
pub fn readiness_of(state: &str, health: &str, exit_code: i32, job: bool) -> Readiness {
    match state {
        "running" => match health {
            "starting" => Readiness::Starting,
            "unhealthy" => Readiness::Unhealthy,
            _ => Readiness::Ready,
        },
        "restarting" => Readiness::Starting,
        "exited" if job && exit_code == 0 => Readiness::Done,
        _ => Readiness::Stopped,
    }
}

/// `compose ps --format json`: one object per line today, an array in older
/// versions. `Publishers` is null when nothing is published, and every port
/// appears once per address family; this computer only cares about the number.
pub fn parse_ps(text: &str) -> Vec<ServiceState> {
    let trimmed = text.trim();
    let objects: Vec<Value> = if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).unwrap_or_default()
    } else {
        trimmed.lines().filter_map(|line| serde_json::from_str(line).ok()).collect()
    };
    let jobs: Vec<String> = objects.iter().flat_map(|entry| awaited_to_complete(entry.get("Labels"))).collect();
    let mut services: Vec<ServiceState> = objects.iter().map(|entry| parse_ps_entry(entry, &jobs)).collect();
    services.sort_by(|a, b| a.service.cmp(&b.service));
    services
}

fn parse_ps_entry(entry: &Value, jobs: &[String]) -> ServiceState {
    let text = |key: &str| entry.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let mut ports: Vec<Port> = Vec::new();
    for publisher in entry.get("Publishers").and_then(Value::as_array).into_iter().flatten() {
        let number = |key: &str| publisher.get(key).and_then(Value::as_u64).unwrap_or(0) as u16;
        let port = Port {
            target: number("TargetPort"),
            published: number("PublishedPort"),
            protocol: publisher.get("Protocol").and_then(Value::as_str).unwrap_or("tcp").to_string(),
        };
        let unpublished = port.published == 0;
        if unpublished || ports.contains(&port) {
            continue;
        }
        ports.push(port);
    }
    let service = text("Service");
    let state = text("State");
    let health = text("Health");
    let exit_code = entry.get("ExitCode").and_then(Value::as_i64).unwrap_or(0) as i32;
    let job = jobs.contains(&service);
    let readiness = readiness_of(&state, &health, exit_code, job);
    ServiceState { service, container: text("Name"), state, health, exit_code, ports, job, readiness }
}

/// The services a container waits for with `service_completed_successfully`,
/// from its labels. `ps` gives the labels as one `key=value,key=value` string,
/// and the depends_on value holds commas of its own (`a:cond:false,b:cond:false`),
/// as may other labels' values: a piece without `=` continues the value before it.
fn awaited_to_complete(labels: Option<&Value>) -> Vec<String> {
    let Some(labels) = labels.and_then(Value::as_str) else { return Vec::new() };
    let mut current_key = "";
    let mut depends_on: Vec<&str> = Vec::new();
    for piece in labels.split(',') {
        let piece_value = match piece.split_once('=') {
            Some((key, value)) if !key.contains(':') => {
                current_key = key;
                value
            }
            _ => piece,
        };
        if current_key == DEPENDS_ON_LABEL {
            depends_on.push(piece_value);
        }
    }
    depends_on
        .iter()
        .filter_map(|entry| {
            let mut parts = entry.split(':');
            let service = parts.next()?;
            let condition = parts.next()?;
            (condition == COMPLETED && !service.is_empty()).then(|| service.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ps_json_lines_dedupe_address_families() {
        let text = r#"{"ExitCode":0,"Health":"","Name":"s-web-1","Publishers":[{"URL":"0.0.0.0","TargetPort":80,"PublishedPort":8087,"Protocol":"tcp"},{"URL":"::","TargetPort":80,"PublishedPort":8087,"Protocol":"tcp"}],"Service":"web","State":"running"}
{"ExitCode":1,"Health":"","Name":"s-db-1","Publishers":null,"Service":"db","State":"exited"}"#;
        let services = parse_ps(text);
        assert_eq!(services.len(), 2);
        assert_eq!(services[0].service, "db");
        assert_eq!(services[0].exit_code, 1);
        assert!(services[0].ports.is_empty());
        assert_eq!(services[1].ports, vec![Port { target: 80, published: 8087, protocol: "tcp".into() }]);
    }

    #[test]
    fn ps_array_and_empty_are_fine() {
        let array = r#"[{"Name":"a","Service":"a","State":"running","Publishers":[{"TargetPort":1,"PublishedPort":0,"Protocol":"tcp"}]}]"#;
        let services = parse_ps(array);
        assert_eq!(services.len(), 1);
        assert!(services[0].ports.is_empty());
        assert!(parse_ps("").is_empty());
    }

    /// The labels of a real stack (Compose 5.5): keycloak waits for the setup
    /// job keycloak-db, which waits for postgres to be healthy; another label's
    /// value has commas of its own.
    #[test]
    fn a_job_is_known_from_the_label_on_the_service_that_waits_for_it() {
        let text = r#"{"Service":"keycloak","Name":"s-keycloak-1","State":"running","Health":"","ExitCode":0,"Labels":"com.docker.compose.depends_on=keycloak-db:service_completed_successfully:false,org.opencontainers.image.description=Identity, and access, management,com.docker.compose.project=s"}
{"Service":"keycloak-db","Name":"s-keycloak-db-1","State":"exited","Health":"","ExitCode":0,"Labels":"com.docker.compose.depends_on=postgres:service_healthy:false,com.docker.compose.project=s"}
{"Service":"postgres","Name":"s-postgres-1","State":"running","Health":"healthy","ExitCode":0,"Labels":"com.docker.compose.depends_on=,com.docker.compose.project=s"}"#;
        let services = parse_ps(text);
        let by_name = |name: &str| services.iter().find(|s| s.service == name).unwrap();
        assert!(by_name("keycloak-db").job);
        assert_eq!(by_name("keycloak-db").readiness, Readiness::Done);
        assert!(!by_name("postgres").job, "service_healthy is not a job");
        assert_eq!(by_name("postgres").readiness, Readiness::Ready);
        assert_eq!(by_name("keycloak").readiness, Readiness::Ready);
    }

    #[test]
    fn readiness_follows_composes_rules() {
        assert_eq!(readiness_of("running", "", 0, false), Readiness::Ready, "no health check: running is ready");
        assert_eq!(readiness_of("running", "healthy", 0, false), Readiness::Ready);
        assert_eq!(readiness_of("running", "starting", 0, false), Readiness::Starting, "not ready yet, not failed");
        assert_eq!(readiness_of("running", "unhealthy", 0, false), Readiness::Unhealthy);
        assert_eq!(readiness_of("restarting", "", 1, false), Readiness::Starting);
        assert_eq!(readiness_of("exited", "", 0, true), Readiness::Done, "a job that finished");
        assert_eq!(readiness_of("exited", "", 1, true), Readiness::Stopped, "a job that failed");
        assert_eq!(readiness_of("exited", "", 0, false), Readiness::Stopped, "an undeclared exit is not done");
        assert_eq!(readiness_of("created", "", 0, false), Readiness::Stopped);
    }
}
