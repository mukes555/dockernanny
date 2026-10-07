//! Copying a stack between two endpoints, in either direction: the project
//! folder (config) and the data (named volumes, anonymous volumes, folders a
//! container wrote). `endpoint` says where things are, `look` sees both ends,
//! `carry` moves things (with `folder` mirroring and `transfer` streaming),
//! `discover` finds the data, `check` looks at the destination before and
//! after, `report` tells the window. `run` is the whole copy without the
//! window, so the headless example and the app share it. Nothing on the
//! source is ever deleted; a source stopped for a consistent copy is started
//! again.

mod carry;
pub mod check;
pub mod discover;
pub mod endpoint;
pub mod folder;
pub mod local;
mod look;
pub mod progress;
pub mod pull;
mod report;
pub mod steps;
pub mod transfer;

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use check::Summary;
use discover::ContainerData;
use endpoint::Site;
use folder::Mirrored;
use local::LocalProject;
pub use report::{Report, Sink};
use steps::names;

use crate::compose;
use crate::machine::first_line;
use crate::ssh::Ssh;
use crate::stack::Phase;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EndpointRef {
    ThisComputer,
    Machine { machine_id: String },
}

/// One path inside one service's container that travels with the stack:
/// an anonymous volume's mount point or a folder the container changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSelection {
    pub service: String,
    pub path: String,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
pub struct CopyRequest {
    pub source: EndpointRef,
    /// The source when it is this computer.
    #[serde(default)]
    pub project: Option<LocalProject>,
    /// The source when it is a machine: one of this computer's stack records.
    #[serde(default)]
    pub stack_id: Option<String>,
    pub destination: EndpointRef,
    /// Where the folder lands when the destination is this computer; empty
    /// means the folder the stack record remembers.
    #[serde(default)]
    pub folder: String,
    /// The compose project name at the destination.
    pub name: String,
    pub config: bool,
    pub data: bool,
    #[serde(default)]
    pub data_selection: Vec<DataSelection>,
    /// Stop the source for the data copy, so a database copies consistently.
    #[serde(default)]
    pub stop_source: bool,
    /// Leave the source stopped afterwards: a move rather than a copy, which
    /// also frees its ports for the destination.
    #[serde(default)]
    pub keep_source_stopped: bool,
    #[serde(default)]
    pub port_overrides: HashMap<u16, u16>,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default = "default_true")]
    pub forward_ports: bool,
}

/// Both ends resolved against the store.
pub struct Sides {
    pub from: Site,
    pub to: Site,
    pub excludes: Vec<String>,
    /// The small image that reads and writes volumes on both ends (`tar` in a container).
    pub helper_image: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct VolumePlan {
    pub key: String,
    pub name: String,
    pub size: String,
    pub destination_name: String,
    pub external: bool,
}

/// What a copy would do, for the sheet to show before the user commits.
#[derive(Debug, Clone, Serialize)]
pub struct CopyPlan {
    pub from: String,
    pub to: String,
    pub destination_exists: bool,
    pub source_running: bool,
    /// The TCP ports the compose file publishes, for the port rows.
    pub ports: Vec<u16>,
    pub volumes: Vec<VolumePlan>,
    pub containers: Vec<ContainerData>,
    pub images: Vec<String>,
    pub notes: Vec<String>,
    pub warnings: Vec<String>,
}

pub struct Outcome {
    pub mirrored: Option<Mirrored>,
    pub summary: Summary,
}

/// Looks at both ends without changing anything.
pub async fn plan(ssh: &Ssh, sides: &Sides, request: &CopyRequest) -> anyhow::Result<CopyPlan> {
    let (from, to) = (&sides.from, &sides.to);
    let look = look::look(ssh, sides, request).await?;
    let mut warnings = Vec::new();
    if request.data && !request.config && !look.destination_exists {
        warnings.push(format!("{} has no copy of the project yet, so the config must travel too.", to.label));
    }

    let preview = compose::parse_model(&look.model, Path::new(&from.dir));
    let mut ports: Vec<u16> =
        preview.services.iter().flat_map(|s| s.ports.iter().filter(|p| p.protocol == "tcp").map(|p| p.published)).collect();
    ports.sort_unstable();
    ports.dedup();

    let (volumes, containers, images) = match &look.inventory {
        Some(inventory) => {
            let volumes = inventory
                .volumes
                .iter()
                .map(|v| VolumePlan {
                    key: v.key.clone(),
                    name: v.name.clone(),
                    size: v.size.clone(),
                    destination_name: transfer::volume_name_at(&to.name, v),
                    external: v.external,
                })
                .collect();
            if inventory.volumes.iter().any(|v| v.external) {
                warnings.push("External volumes keep their own name and are added to, not replaced.".into());
            }
            let containers = discover::container_data(ssh, from, &inventory.sizes).await?;
            (volumes, containers, inventory.images.clone())
        }
        None => (Vec::new(), Vec::new(), Vec::new()),
    };
    let mut notes = look.preflight;
    notes.extend(pull::note(sides, &look.downloads));

    Ok(CopyPlan {
        from: from.label.clone(),
        to: to.label.clone(),
        destination_exists: look.destination_exists,
        source_running: look.source_running,
        ports,
        volumes,
        containers,
        images,
        notes,
        warnings,
    })
}

/// The whole copy. Order matters: the destination's containers must exist
/// before `docker cp` can fill them, and a stopped source gives a consistent
/// copy of a database. The panel learns how it ended here, so the app and
/// the example need not.
pub async fn run(ssh: &Ssh, home: &Path, sides: &Sides, request: &CopyRequest, report: &Report<'_>) -> anyhow::Result<Outcome> {
    let result = run_steps(ssh, home, sides, request, report).await;
    let mut progress = report.progress.lock().expect("progress lock");
    match &result {
        Ok(outcome) => {
            let text = outcome.summary.text(&sides.to);
            tracing::info!("copy {} -> {}: done, {text}", sides.from.label, sides.to.label);
            progress.finish(&text);
        }
        Err(err) => {
            tracing::warn!("copy {} -> {}: failed: {err:#}", sides.from.label, sides.to.label);
            progress.fail(&format!("{err:#}"));
        }
    }
    result
}

async fn run_steps(ssh: &Ssh, home: &Path, sides: &Sides, request: &CopyRequest, report: &Report<'_>) -> anyhow::Result<Outcome> {
    let (from, to) = (&sides.from, &sides.to);
    report.progress.lock().expect("progress lock").set_steps(vec![names::look()]);
    report.step(Phase::Migrating, &names::look());
    let look = look::look(ssh, sides, request).await?;
    anyhow::ensure!(request.config || look.destination_exists, "{} has no copy of the project yet; copy the config too", to.label);
    for note in &look.preflight {
        report.say(&format!("    {note}"));
    }

    let inventory = look.inventory.as_ref();
    let steps = steps::planned(sides, request, inventory, &look.downloads, look.destination_exists, look.source_running);
    report.progress.lock().expect("progress lock").set_steps(steps);
    pull::run(ssh, sides, &look.downloads, report).await?;

    if request.data && look.destination_exists {
        // Its volumes are about to be replaced: never under a running database.
        report.step(Phase::Migrating, &names::stop(&to.name, &to.label));
        let stopped = to.compose_output(ssh, "stop").await?;
        anyhow::ensure!(
            stopped.ok(),
            "could not stop {} on {} before replacing its data: {}",
            to.name,
            to.label,
            first_line(&stopped.stderr)
        );
    }
    let stopped_source = (request.stop_source || request.keep_source_stopped) && look.source_running;
    if stopped_source {
        report.step(Phase::Migrating, &names::stop(&from.name, &from.label));
        let why = if request.keep_source_stopped { "it stays stopped" } else { "for a consistent copy" };
        report.say(&format!("    {why}"));
        let stopped = from.compose_output(ssh, "stop").await?;
        anyhow::ensure!(stopped.ok(), "could not stop the copy on {}: {}", from.label, stopped.stderr);
    }

    let carried = carry::carry(ssh, home, sides, request, inventory, &look.source_services, stopped_source, report).await;
    // The step that failed keeps the blame; starting the source again is
    // still done, and shown as its own step.
    if carried.is_err() {
        report.progress.lock().expect("progress lock").fail_step();
    }
    if stopped_source && !request.keep_source_stopped {
        report.step(Phase::Starting, &names::restart(&from.name, &from.label));
        let restarted = from.compose_output(ssh, "start").await;
        let restart_failure = match &restarted {
            Ok(out) if out.ok() => None,
            Ok(out) => Some(first_line(&out.stderr)),
            Err(err) => Some(format!("{err:#}")),
        };
        let mut progress = report.progress.lock().expect("progress lock");
        if restart_failure.is_some() {
            progress.fail_step();
        } else if carried.is_err() {
            progress.done_step();
        }
        drop(progress);
        // A source left stopped is worth an error even when the copy itself went well.
        if let (Ok(_), Some(why)) = (&carried, restart_failure) {
            anyhow::bail!("the copy is done, but {} could not be started again on {}: {why}", from.name, from.label);
        }
    }
    let mirrored = carried?;

    report.step(Phase::Starting, &names::check(&to.label));
    let summary = check::post_copy(ssh, to).await;
    report.say(&format!("    {}", summary.text(to)));
    Ok(Outcome { mirrored, summary })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_and_endpoints_read_from_the_wire() {
        let json = r#"{"source":{"kind":"machine","machine_id":"a1"},"stack_id":"s1","destination":{"kind":"this_computer"},"folder":"/home/alex/shop","name":"shop","config":true,"data":false}"#;
        let request: CopyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.source, EndpointRef::Machine { machine_id: "a1".into() });
        assert_eq!(request.destination, EndpointRef::ThisComputer);
        assert!(request.forward_ports);
        assert!(request.data_selection.is_empty());
    }
}
