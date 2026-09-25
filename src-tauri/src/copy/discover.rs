//! What a stack keeps at an endpoint: its named volumes with sizes, and what
//! each container keeps outside them (anonymous volumes the image declared,
//! folders the container wrote into its own layer). Both are lost by a plain
//! volume copy: an app that keeps its accounts in its own layer arrives empty.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::endpoint::{Endpoint, Site};
use crate::compose;
use crate::ssh::Ssh;

/// Below this, the container's own layer holds nothing worth a `docker diff`.
const DIFF_THRESHOLD_BYTES: u64 = 1_000_000;
/// Paths every container scribbles in that never hold data.
const NOISE: [&str; 9] = ["/tmp", "/var/cache", "/var/log", "/var/lib/apt", "/run", "/root/.cache", "/proc", "/sys", "/dev"];
/// How deep a changed path is reported: `/opt/keycloak/data`, not every file.
const GROUP_DEPTH: usize = 3;

/// A volume the compose file names.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedVolume {
    pub key: String,
    pub name: String,
    pub size: String,
    pub external: bool,
}

/// A volume the image created for itself (`VOLUME` in the Dockerfile).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnonymousVolume {
    pub name: String,
    pub destination: String,
    pub size: String,
}

/// A folder the container changed, with how many entries the diff listed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChangedPath {
    pub path: String,
    pub entries: u32,
    /// Looks like data (a database, a `data` folder), ticked by default.
    pub suggested: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContainerData {
    pub service: String,
    pub container: String,
    pub anonymous_volumes: Vec<AnonymousVolume>,
    pub changed_paths: Vec<ChangedPath>,
    /// Why nothing was looked at, when that is the case.
    pub note: Option<String>,
}

/// The compose model at a site, as `docker compose config` sees it there.
pub async fn model(ssh: &Ssh, site: &Site) -> anyhow::Result<Value> {
    let out = site.compose_output(ssh, "config --format json").await?;
    anyhow::ensure!(out.ok(), "docker compose config failed on {}: {}", site.label, out.stderr.lines().last().unwrap_or(""));
    serde_json::from_str(&out.stdout).map_err(|err| anyhow::anyhow!("unreadable compose model from {}: {err}", site.label))
}

pub async fn named_volumes(ssh: &Ssh, site: &Site, model: &Value) -> Vec<NamedVolume> {
    let sizes = volume_sizes(ssh, &site.endpoint).await;
    compose::parse_volumes(model)
        .into_iter()
        .map(|def| NamedVolume {
            size: sizes.get(&def.name).cloned().unwrap_or_else(|| "?".into()),
            key: def.key,
            name: def.name,
            external: def.external,
        })
        .collect()
}

/// `docker system df -v` is the only place the engine reports volume sizes.
pub async fn volume_sizes(ssh: &Ssh, endpoint: &Endpoint) -> HashMap<String, String> {
    let Ok(out) = endpoint.docker_output(ssh, &["system", "df", "-v", "--format", "json"]).await else { return HashMap::new() };
    let value: Value = serde_json::from_str(&out.stdout).unwrap_or_default();
    value
        .get("Volumes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|v| Some((v.get("Name")?.as_str()?.to_string(), v.get("Size")?.as_str()?.to_string())))
        .collect()
}

/// One entry per service that has a container, in service order.
pub async fn container_data(ssh: &Ssh, site: &Site) -> anyhow::Result<Vec<ContainerData>> {
    let out = site.compose_output(ssh, "ps -a --format json").await?;
    anyhow::ensure!(out.ok(), "docker compose ps failed on {}: {}", site.label, out.stderr);
    let services = compose::parse_ps(&out.stdout);
    let sizes = volume_sizes(ssh, &site.endpoint).await;

    let mut result = Vec::new();
    for service in services.iter().filter(|s| !s.container.is_empty()) {
        let mounts = site.endpoint.docker_output(ssh, &["inspect", "-f", "{{json .Mounts}}", &service.container]).await?;
        let (mut anonymous, destinations) = parse_mounts(&mounts.stdout);
        for volume in &mut anonymous {
            volume.size = sizes.get(&volume.name).cloned().unwrap_or_else(|| "?".into());
        }

        let filter = format!("name=^{}$", service.container);
        let size = site.endpoint.docker_output(ssh, &["ps", "-a", "-s", "--format", "{{.Size}}", "--filter", &filter]).await?;
        let layer_bytes = parse_rw_bytes(size.stdout.trim());
        let (changed_paths, note) = if layer_bytes < DIFF_THRESHOLD_BYTES {
            (Vec::new(), Some("nothing written inside the container".to_string()))
        } else {
            let diff = site.endpoint.docker_output(ssh, &["diff", &service.container]).await?;
            (group_diff(&diff.stdout, &destinations), None)
        };
        result.push(ContainerData {
            service: service.service.clone(),
            container: service.container.clone(),
            anonymous_volumes: anonymous,
            changed_paths,
            note,
        });
    }
    Ok(result)
}

/// Images the destination lacks and cannot pull: built at the source, or
/// loaded from a file, so they have no registry digest. Services with
/// `build:` are rebuilt from the folder and do not count.
pub async fn images_to_carry(ssh: &Ssh, from: &Site, to: &Site, model: &Value) -> Vec<String> {
    let preview = compose::parse_model(model, std::path::Path::new(&from.dir));
    let mut images = Vec::new();
    for service in preview.services.iter().filter(|s| !s.builds) {
        let Some(image) = &service.image else { continue };
        if images.contains(image) {
            continue;
        }
        let there = to.endpoint.docker_output(ssh, &["image", "inspect", "-f", "{{.Id}}", image]).await.map(|o| o.ok()).unwrap_or(false);
        if there {
            continue;
        }
        let digests = from.endpoint.docker_output(ssh, &["image", "inspect", "-f", "{{len .RepoDigests}}", image]).await.map(|o| o.stdout.trim().to_string()).unwrap_or_default();
        if digests == "0" {
            images.push(image.clone());
        }
    }
    images
}

/// Anonymous volumes (a 64 hex name) and the destination of every mount, so
/// the diff can ignore what lives in a mount.
pub fn parse_mounts(json: &str) -> (Vec<AnonymousVolume>, Vec<String>) {
    let mounts: Vec<Value> = serde_json::from_str(json.trim()).unwrap_or_default();
    let mut anonymous = Vec::new();
    let mut destinations = Vec::new();
    for mount in &mounts {
        let text = |key: &str| mount.get(key).and_then(Value::as_str).unwrap_or("").to_string();
        let destination = text("Destination");
        if !destination.is_empty() {
            destinations.push(destination.clone());
        }
        let name = text("Name");
        let is_anonymous = text("Type") == "volume" && name.len() == 64 && name.chars().all(|c| c.is_ascii_hexdigit());
        if is_anonymous {
            anonymous.push(AnonymousVolume {
                name,
                destination,
                size: String::new(),
            });
        }
    }
    (anonymous, destinations)
}

/// The mounts a copy into this container must go around: bind mounts (their
/// files travel with the project folder) and anything read-only (Keycloak's
/// `data/import`). A writable volume, such as the anonymous one a database
/// image declares, is where the data belongs, so it is not in the list.
pub fn mounts_to_go_around(json: &str) -> Vec<String> {
    let mounts: Vec<Value> = serde_json::from_str(json.trim()).unwrap_or_default();
    let mut around = Vec::new();
    for mount in &mounts {
        let destination = mount.get("Destination").and_then(Value::as_str).unwrap_or("");
        let is_bind = mount.get("Type").and_then(Value::as_str) == Some("bind");
        let read_only = mount.get("RW").and_then(Value::as_bool) == Some(false);
        if !destination.is_empty() && (is_bind || read_only) {
            around.push(destination.to_string());
        }
    }
    around
}

/// `180MB (virtual 657MB)` from `docker ps -s`: the container's own layer.
pub fn parse_rw_bytes(size: &str) -> u64 {
    let own = size.split('(').next().unwrap_or("").trim();
    super::check::parse_human_size(own)
}

/// `docker diff` lines grouped to a few components deep, minus the noise and
/// minus anything inside a mount. Entries are counted per group.
pub fn group_diff(text: &str, mounts: &[String]) -> Vec<ChangedPath> {
    let mut groups: Vec<(String, u32)> = Vec::new();
    for line in text.lines() {
        let Some((kind, path)) = line.split_once(' ') else { continue };
        if kind == "D" || !path.starts_with('/') {
            continue;
        }
        let noise = NOISE.iter().any(|prefix| path == *prefix || path.starts_with(&format!("{prefix}/")));
        let in_mount = mounts.iter().any(|m| path == m || path.starts_with(&format!("{m}/")));
        if noise || in_mount {
            continue;
        }
        let key = truncate(path, GROUP_DEPTH);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, count)) => *count += 1,
            None => groups.push((key, 1)),
        }
    }
    // `C /opt` and `C /opt/keycloak` only say that something below changed.
    let ancestors: Vec<String> = groups
        .iter()
        .filter(|(key, _)| groups.iter().any(|(other, _)| other != key && other.starts_with(&format!("{key}/"))))
        .map(|(key, _)| key.clone())
        .collect();
    let mut paths: Vec<ChangedPath> = groups
        .into_iter()
        .filter(|(key, _)| !ancestors.contains(key))
        .map(|(path, entries)| ChangedPath { suggested: looks_like_data(&path), path, entries })
        .collect();
    paths.sort_by(|a, b| b.suggested.cmp(&a.suggested).then(a.path.cmp(&b.path)));
    paths
}

fn truncate(path: &str, depth: usize) -> String {
    let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).take(depth).collect();
    format!("/{}", parts.join("/"))
}

fn looks_like_data(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let database_dir = ["/var/lib/postgresql", "/var/lib/mysql", "/var/lib/mongo", "/var/lib/redis", "/var/lib/clickhouse"].iter().any(|d| lower.starts_with(d));
    database_dir || lower.contains("data") || lower.contains("/db") || lower.contains("storage")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYCLOAK_DIFF: &str = "C /tmp\nA /tmp/hsperfdata_keycloak\nA /tmp/vertx-cache/-1082421131219886147\nC /opt\nC /opt/keycloak\nC /opt/keycloak/data\nA /opt/keycloak/data/h2\nA /opt/keycloak/data/h2/keycloakdb.mv.db\nA /opt/keycloak/data/import\nA /opt/keycloak/data/transaction-logs\nA /opt/keycloak/data/transaction-logs/ShadowNoFileLockStore\nC /opt/keycloak/lib\nC /opt/keycloak/lib/quarkus\nC /opt/keycloak/lib/quarkus/build-system.properties\nA /opt/keycloak/.keycloak\nA /opt/keycloak/.keycloak/kcadm.config\n";

    #[test]
    fn keycloak_diff_groups_to_data_lib_and_config() {
        let mounts = vec!["/opt/keycloak/data/import".to_string()];
        let paths = group_diff(KEYCLOAK_DIFF, &mounts);
        let names: Vec<&str> = paths.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(names, vec!["/opt/keycloak/data", "/opt/keycloak/.keycloak", "/opt/keycloak/lib"]);
        let data = &paths[0];
        assert!(data.suggested);
        // /opt/keycloak/data itself plus four entries below it; the mounted import folder is not counted.
        assert_eq!(data.entries, 5);
        assert!(!paths[2].suggested);
    }

    #[test]
    fn deleted_and_noisy_entries_are_ignored() {
        let paths = group_diff("D /etc/hosts\nA /var/log/app.log\nA /run/x\nA /data/db.sqlite\n", &[]);
        assert_eq!(paths, vec![ChangedPath { path: "/data/db.sqlite".into(), entries: 1, suggested: true }]);
    }

    #[test]
    fn mounts_split_into_anonymous_volumes_and_destinations() {
        let json = r#"[{"Type":"bind","Source":"/home/alex/x","Destination":"/opt/import"},{"Type":"volume","Name":"shop_pgdata","Destination":"/var/lib/postgresql/data"},{"Type":"volume","Name":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Destination":"/var/lib/mysql"}]"#;
        let (anonymous, destinations) = parse_mounts(json);
        assert_eq!(anonymous.len(), 1);
        assert_eq!(anonymous[0].destination, "/var/lib/mysql");
        assert_eq!(destinations, vec!["/opt/import", "/var/lib/postgresql/data", "/var/lib/mysql"]);
    }

    #[test]
    fn a_copy_goes_around_binds_and_read_only_mounts_but_into_writable_volumes() {
        let json = r#"[{"Type":"bind","Destination":"/opt/keycloak/data/import","RW":false},{"Type":"bind","Destination":"/app/config","RW":true},{"Type":"volume","Name":"shop_cache","Destination":"/srv/cache","RW":false},{"Type":"volume","Name":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef","Destination":"/var/lib/postgresql/data","RW":true}]"#;
        assert_eq!(mounts_to_go_around(json), vec!["/opt/keycloak/data/import", "/app/config", "/srv/cache"]);
    }

    #[test]
    fn container_layer_size_is_the_part_before_virtual() {
        assert_eq!(parse_rw_bytes("180MB (virtual 657MB)"), 180_000_000);
        assert_eq!(parse_rw_bytes("63B (virtual 459MB)"), 63);
        assert_eq!(parse_rw_bytes(""), 0);
    }
}
