//! The compose projects this computer's own Docker knows about: the sources
//! a copy can start from when the source is this computer.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::discover::{volume_sizes, NamedVolume};
use super::endpoint::{local_docker, Endpoint};
use crate::compose;
use crate::ssh::Ssh;

/// A compose project the local engine knows about.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProject {
    pub name: String,
    pub status: String,
    pub config_file: String,
    pub project_dir: String,
    pub compose_rel: String,
    pub volumes: Vec<NamedVolume>,
    pub ports: Vec<u16>,
    pub warnings: Vec<String>,
}

pub fn project_dir_of(project: &LocalProject) -> &Path {
    Path::new(&project.project_dir)
}

/// Everything `docker compose ls` lists on this computer, with volumes and sizes.
pub async fn local_projects(ssh: &Ssh) -> anyhow::Result<Vec<LocalProject>> {
    let out = local_docker(&["compose", "ls", "--all", "--format", "json"]).output().await.context("run docker compose ls")?;
    anyhow::ensure!(out.status.success(), "docker compose ls failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    let listed: Vec<Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    let sizes = volume_sizes(ssh, &Endpoint::Local).await;

    let mut projects = Vec::new();
    for entry in listed {
        let name = entry.get("Name").and_then(Value::as_str).unwrap_or("").to_string();
        let status = entry.get("Status").and_then(Value::as_str).unwrap_or("").to_string();
        let files = entry.get("ConfigFiles").and_then(Value::as_str).unwrap_or("");
        let Some(config_file) = files.split(',').next().map(str::trim).filter(|f| !f.is_empty()) else { continue };
        let config_path = PathBuf::from(config_file);
        let Some(project_dir) = config_path.parent() else { continue };
        let compose_rel = config_path.file_name().and_then(|n| n.to_str()).unwrap_or("docker-compose.yml").to_string();
        let mut project = LocalProject {
            name,
            status,
            config_file: config_file.to_string(),
            project_dir: project_dir.display().to_string(),
            compose_rel,
            volumes: Vec::new(),
            ports: Vec::new(),
            warnings: Vec::new(),
        };

        match compose::config_model(project_dir, &project.compose_rel).await {
            Ok((model, _stderr)) => {
                let preview = compose::parse_model(&model, project_dir);
                project.warnings = preview.warnings;
                project.ports = preview.services.iter().flat_map(|s| s.ports.iter().map(|p| p.published)).collect();
                project.volumes = compose::parse_volumes(&model)
                    .into_iter()
                    .map(|def| NamedVolume {
                        size: sizes.get(&def.name).cloned().unwrap_or_else(|| "?".into()),
                        key: def.key,
                        name: def.name,
                        external: def.external,
                    })
                    .collect();
                if project.volumes.iter().any(|v| v.external) {
                    project.warnings.push("external volumes are copied under their own name".into());
                }
            }
            Err(err) => project.warnings.push(format!("could not read the compose file: {err:#}")),
        }
        projects.push(project);
    }
    projects.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(projects)
}
