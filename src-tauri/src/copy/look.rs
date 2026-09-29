//! What a copy finds at both ends before it changes anything. `plan` shows
//! it to the user; `run` acts on it. Both ask here, so a run cannot drift
//! from the plan the user approved.

use std::collections::HashMap;

use serde_json::Value;

use super::discover::{self, Download, NamedVolume};
use super::endpoint::Site;
use super::{check, pull, CopyRequest, Sides};
use crate::compose::ServiceState;
use crate::ssh::Ssh;

/// What the source has that a data copy carries.
pub(crate) struct Inventory {
    pub(crate) volumes: Vec<NamedVolume>,
    pub(crate) images: Vec<String>,
    /// Every volume's size on the source engine, read once (`docker system df -v`).
    pub(crate) sizes: HashMap<String, String>,
}

impl Inventory {
    async fn at(ssh: &Ssh, from: &Site, to: &Site, model: &Value) -> Self {
        let sizes = discover::volume_sizes(ssh, &from.endpoint).await;
        let volumes = discover::named_volumes(model, &sizes);
        let images = if from.is_local() { discover::images_to_carry(ssh, from, to, model).await } else { Vec::new() };
        Self { volumes, images, sizes }
    }

    /// Bytes the destination needs for the volumes, as far as sizes are known.
    pub(crate) fn needed_bytes(&self) -> u64 {
        self.volumes.iter().map(|v| check::parse_human_size(&v.size)).sum()
    }
}

pub(crate) struct Look {
    pub(crate) destination_exists: bool,
    /// The compose model at the source, as `docker compose config` sees it there.
    pub(crate) model: Value,
    pub(crate) source_services: Vec<ServiceState>,
    pub(crate) source_running: bool,
    /// Only when data travels.
    pub(crate) inventory: Option<Inventory>,
    /// What the destination has room and tools for, one line each.
    pub(crate) preflight: Vec<String>,
    /// Images the destination downloads before anything stops.
    pub(crate) downloads: Vec<Download>,
}

/// Looks at both ends. Fails when the source has no compose file, when the
/// destination lacks the compose plugin or room, and when `compose ps` does
/// not answer: a failed ps must not read as "not running" and skip the stop
/// a consistent copy needs.
pub(crate) async fn look(ssh: &Ssh, sides: &Sides, request: &CopyRequest) -> anyhow::Result<Look> {
    let (from, to) = (&sides.from, &sides.to);
    anyhow::ensure!(from.exists(ssh).await, "{} has no compose file for {} at {}", from.label, from.name, from.compose_file());
    let destination_exists = to.exists(ssh).await;
    let source_services = from.services(ssh).await?;
    let source_running = source_services.iter().any(|s| s.state == "running");
    let model = discover::model(ssh, from).await?;
    let inventory = if request.data { Some(Inventory::at(ssh, from, to, &model).await) } else { None };
    let needed = inventory.as_ref().map(Inventory::needed_bytes).unwrap_or(0);
    let preflight = check::preflight(ssh, to, needed).await?;
    let carried = inventory.as_ref().map(|i| i.images.as_slice()).unwrap_or(&[]);
    let downloads = pull::planned(ssh, sides, request, &model, carried).await;
    Ok(Look { destination_exists, model, source_services, source_running, inventory, preflight, downloads })
}
