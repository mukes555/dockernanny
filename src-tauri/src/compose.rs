//! What a compose file says, as Docker itself reads it. `compose config`
//! resolves variables and expands short syntax, so the preview shows exactly
//! what will run. Its output can contain secrets (env_file is inlined), so it
//! is parsed in memory and dropped. The same file also reads `compose ps`.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::Serialize;
use serde_json::Value;
use tokio::process::Command;

pub const COMPOSE_FILES: [&str; 4] = ["compose.yaml", "compose.yml", "docker-compose.yml", "docker-compose.yaml"];

#[derive(Debug, Clone, Serialize, Default)]
pub struct Preview {
    pub project_dir: String,
    pub compose_rel: String,
    pub name: String,
    pub services: Vec<ServicePreview>,
    pub warnings: Vec<String>,
    pub has_env_file: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServicePreview {
    pub name: String,
    pub image: Option<String>,
    pub builds: bool,
    pub ports: Vec<Port>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Port {
    pub target: u16,
    pub published: u16,
    pub protocol: String,
}

/// One row of `compose ps`.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServiceState {
    pub service: String,
    pub container: String,
    pub state: String,
    pub health: String,
    pub exit_code: i32,
    pub ports: Vec<Port>,
}

/// The compose file for a dropped path: the file itself, or the first
/// conventional name inside a dropped folder.
pub fn locate(dropped: &Path) -> anyhow::Result<(PathBuf, String)> {
    if dropped.is_file() {
        let dir = dropped.parent().context("the compose file has no parent folder")?;
        let name = dropped.file_name().and_then(|n| n.to_str()).context("unreadable file name")?;
        return Ok((dir.to_path_buf(), name.to_string()));
    }
    for name in COMPOSE_FILES {
        if dropped.join(name).is_file() {
            return Ok((dropped.to_path_buf(), name.to_string()));
        }
    }
    anyhow::bail!("No compose file in {} (looked for {})", dropped.display(), COMPOSE_FILES.join(", "))
}

/// The resolved compose model as Docker itself reads it, plus the warnings
/// compose printed while reading (unset variables). Never persisted: env_file
/// contents are inlined into it.
pub async fn config_model(project_dir: &Path, compose_rel: &str) -> anyhow::Result<(Value, String)> {
    let out = Command::new("docker")
        .args(["compose", "-f", compose_rel, "config", "--format", "json"])
        .current_dir(project_dir)
        // Must not depend on which context the user's terminal selected.
        .env("DOCKER_CONTEXT", "default")
        .output()
        .await
        .context("run docker compose config (is Docker installed on this computer?)")?;
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    if !out.status.success() {
        anyhow::bail!("docker compose config failed:\n{}", stderr.trim());
    }
    let model: Value = serde_json::from_slice(&out.stdout).context("parse compose config output")?;
    Ok((model, stderr))
}

pub async fn preview(dropped: &Path) -> anyhow::Result<Preview> {
    let (project_dir, compose_rel) = locate(dropped)?;
    let (model, stderr) = config_model(&project_dir, &compose_rel).await?;

    let mut preview = parse_model(&model, &project_dir);
    preview.warnings.extend(variable_warnings(&stderr));
    preview.name = sanitize_name(project_dir.file_name().and_then(|n| n.to_str()).unwrap_or("stack"));
    preview.has_env_file = project_dir.join(".env").is_file();
    preview.project_dir = project_dir.display().to_string();
    preview.compose_rel = compose_rel;
    Ok(preview)
}

/// Top-level named volumes: key in the file, name on the engine, external or not.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VolumeDef {
    pub key: String,
    pub name: String,
    pub external: bool,
}

pub fn parse_volumes(model: &Value) -> Vec<VolumeDef> {
    let Some(map) = model.get("volumes").and_then(Value::as_object) else { return Vec::new() };
    let mut volumes: Vec<VolumeDef> = map
        .iter()
        .map(|(key, def)| VolumeDef {
            key: key.clone(),
            name: def.get("name").and_then(Value::as_str).unwrap_or(key).to_string(),
            external: def.get("external").and_then(Value::as_bool).unwrap_or(false),
        })
        .collect();
    volumes.sort_by(|a, b| a.key.cmp(&b.key));
    volumes
}

pub fn parse_model(model: &Value, project_dir: &Path) -> Preview {
    let mut preview = Preview::default();
    let Some(map) = model.get("services").and_then(Value::as_object) else {
        return preview;
    };
    let mut names: Vec<&String> = map.keys().collect();
    names.sort();
    for name in names {
        let service = &map[name];
        let mut ports = Vec::new();
        for port in service.get("ports").and_then(Value::as_array).into_iter().flatten() {
            match parse_port(port) {
                Ok(port) => ports.push(port),
                Err(reason) => preview.warnings.push(format!("{name}: {reason}")),
            }
        }
        for volume in service.get("volumes").and_then(Value::as_array).into_iter().flatten() {
            if let Some(warning) = bind_mount_warning(volume, project_dir) {
                preview.warnings.push(format!("{name}: {warning}"));
            }
        }
        preview.services.push(ServicePreview {
            name: name.clone(),
            image: service.get("image").and_then(Value::as_str).map(String::from),
            builds: service.get("build").map(|b| !b.is_null()).unwrap_or(false),
            ports,
        });
    }
    preview
}

/// Long-form port entries. `published` is a string in the canonical model and
/// may be empty or a range; both are reported rather than guessed.
fn parse_port(port: &Value) -> Result<Port, String> {
    let target = port.get("target").and_then(Value::as_u64).unwrap_or(0) as u16;
    let protocol = port.get("protocol").and_then(Value::as_str).unwrap_or("tcp").to_string();
    let published = match port.get("published") {
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    if published.is_empty() {
        return Err(format!("container port {target} has no host port, so this computer cannot reach it"));
    }
    if published.contains('-') {
        return Err(format!("port range {published} is not supported yet; publish single ports"));
    }
    let published: u16 = published.parse().map_err(|_| format!("could not read published port {published}"))?;
    if protocol == "udp" {
        return Err(format!("port {published}/udp is not forwarded (SSH forwards TCP only)"));
    }
    Ok(Port {
        target,
        published,
        protocol,
    })
}

fn bind_mount_warning(volume: &Value, project_dir: &Path) -> Option<String> {
    let is_bind = volume.get("type").and_then(Value::as_str) == Some("bind");
    let source = volume.get("source").and_then(Value::as_str)?;
    let inside_project = Path::new(source).starts_with(project_dir);
    if !is_bind || inside_project {
        return None;
    }
    Some(format!("mounts {source}, which is outside the project folder and will not exist on the machine"))
}

/// `The "X" variable is not set. Defaulting to a blank string.` on stderr.
fn variable_warnings(stderr: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for line in stderr.lines().filter(|line| line.contains("variable is not set")) {
        let Some(start) = line.find("\\\"").map(|i| i + 2).or_else(|| line.find('"').map(|i| i + 1)) else { continue };
        let rest = &line[start..];
        let Some(end) = rest.find(['"', '\\']) else { continue };
        let name = rest[..end].to_string();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    names.into_iter().map(|name| format!("{name} is not set here, so it will be blank on the machine too unless .env defines it")).collect()
}

/// Compose project names: lowercase, `[a-z0-9_-]`, starting with a letter or digit.
pub fn sanitize_name(raw: &str) -> String {
    let mut name = String::new();
    let mut last_dash = false;
    for ch in raw.to_lowercase().chars() {
        let keep = ch.is_ascii_alphanumeric() || ch == '_';
        if keep {
            name.push(ch);
            last_dash = false;
        } else if !last_dash && !name.is_empty() {
            name.push('-');
            last_dash = true;
        }
    }
    let name = name.trim_matches(|c| c == '-' || c == '_').to_string();
    if name.is_empty() {
        return "stack".into();
    }
    name
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
    let mut services: Vec<ServiceState> = objects.iter().map(parse_ps_entry).collect();
    services.sort_by(|a, b| a.service.cmp(&b.service));
    services
}

fn parse_ps_entry(entry: &Value) -> ServiceState {
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
    ServiceState {
        service: text("Service"),
        container: text("Name"),
        state: text("State"),
        health: text("Health"),
        exit_code: entry.get("ExitCode").and_then(Value::as_i64).unwrap_or(0) as i32,
        ports,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{"name":"sample","services":{
      "echo":{"image":"hashicorp/http-echo","ports":[
        {"mode":"ingress","target":5678,"published":"8088","protocol":"tcp"},
        {"mode":"ingress","target":9099,"published":"9099","protocol":"udp"},
        {"mode":"ingress","target":7000,"published":"7000-7002","protocol":"tcp"},
        {"mode":"ingress","target":6000,"protocol":"tcp"}],
        "volumes":[{"type":"bind","source":"/etc/hosts","target":"/tmp/hosts","read_only":true}]},
      "web":{"build":{"context":"/proj","dockerfile":"Dockerfile"},"ports":[{"mode":"ingress","target":80,"published":"8087","protocol":"tcp"}],
        "volumes":[{"type":"bind","source":"/proj/html","target":"/usr/share/nginx/html"},{"type":"volume","source":"data","target":"/data"}]}}}"#;

    #[test]
    fn model_gives_services_ports_and_warnings() {
        let model: Value = serde_json::from_str(CONFIG).unwrap();
        let preview = parse_model(&model, Path::new("/proj"));
        assert_eq!(preview.services.len(), 2);
        let echo = &preview.services[0];
        assert_eq!(echo.name, "echo");
        assert_eq!(echo.image.as_deref(), Some("hashicorp/http-echo"));
        assert!(!echo.builds);
        assert_eq!(echo.ports, vec![Port { target: 5678, published: 8088, protocol: "tcp".into() }]);
        let web = &preview.services[1];
        assert!(web.builds);
        assert_eq!(web.ports[0].published, 8087);
        assert!(preview.warnings.iter().any(|w| w.contains("9099/udp")));
        assert!(preview.warnings.iter().any(|w| w.contains("7000-7002")));
        assert!(preview.warnings.iter().any(|w| w.contains("6000")));
        assert!(preview.warnings.iter().any(|w| w.contains("/etc/hosts")));
        assert!(!preview.warnings.iter().any(|w| w.contains("/proj/html")));
    }

    #[test]
    fn named_volumes_come_with_engine_names() {
        let model: Value = serde_json::from_str(r#"{"volumes":{"pgdata":{"name":"app_pgdata"},"shared":{"name":"shared","external":true}}}"#).unwrap();
        let volumes = parse_volumes(&model);
        assert_eq!(volumes.len(), 2);
        assert_eq!(volumes[0], VolumeDef { key: "pgdata".into(), name: "app_pgdata".into(), external: false });
        assert!(volumes[1].external);
    }

    #[test]
    fn unset_variables_become_warnings() {
        let stderr = "time=\"x\" level=warning msg=\"The \\\"MISSING_VAR\\\" variable is not set. Defaulting to a blank string.\"\ntime=\"x\" level=warning msg=\"The \\\"MISSING_VAR\\\" variable is not set. Defaulting to a blank string.\"";
        let warnings = variable_warnings(stderr);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].starts_with("MISSING_VAR is not set"));
    }

    #[test]
    fn names_follow_compose_rules() {
        assert_eq!(sanitize_name("My Project (v2)"), "my-project-v2");
        assert_eq!(sanitize_name("--weird__"), "weird");
        assert_eq!(sanitize_name("???"), "stack");
        assert_eq!(sanitize_name("api_server"), "api_server");
    }

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
}
