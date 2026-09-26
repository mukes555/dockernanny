//! Copying a stack between two endpoints, in either direction: the project
//! folder (config) and the data (named volumes, anonymous volumes, folders a
//! container wrote). `endpoint` says where things are, `folder` mirrors,
//! `transfer` streams, `discover` finds the data, `check` looks at the
//! destination before and after. `run` is the whole copy without the window,
//! so the headless example and the app share it. Nothing on the source is
//! ever deleted; a source stopped for a consistent copy is started again.

pub mod check;
pub mod discover;
pub mod endpoint;
pub mod folder;
pub mod local;
pub mod progress;
pub mod pull;
pub mod steps;
pub mod transfer;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use check::Summary;
use discover::{ContainerData, NamedVolume};
use endpoint::Site;
use folder::Mirrored;
use local::LocalProject;
use progress::Progress;
use steps::names;

use crate::compose;
use crate::job::{Line, Stream};
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

/// Where a line of progress goes; the app makes one per call, the example
/// prints them.
pub type Sink = Box<dyn FnMut(Line) + Send + 'static>;

/// The three ways a copy reports: raw lines (the card's output), the card's
/// phase and message, and the step list with bytes (the progress panel).
pub struct Report<'a> {
    pub make_sink: &'a (dyn Fn() -> Sink + Send + Sync),
    pub status: &'a (dyn Fn(Phase, &str) + Send + Sync),
    pub progress: Progress,
}

impl Report<'_> {
    /// A sink that also feeds the panel's last lines.
    pub fn sink(&self) -> Sink {
        let mut inner = (self.make_sink)();
        let progress = self.progress.clone();
        Box::new(move |l: Line| {
            progress.lock().expect("progress lock").line(&l.text);
            inner(l);
        })
    }

    /// A sink that also keeps the last line reading like an error, so a
    /// failure can say why in Docker's own words, not only its exit code.
    fn sink_keeping_error(&self) -> (Sink, Arc<Mutex<Option<String>>>) {
        let mut inner = self.sink();
        let reason = Arc::new(Mutex::new(None));
        let kept = reason.clone();
        let sink: Sink = Box::new(move |l: Line| {
            if reads_like_error(&l.text) {
                *kept.lock().expect("reason lock") = Some(l.text.trim().to_string());
            }
            inner(l);
        });
        (sink, reason)
    }

    fn say(&self, text: &str) {
        self.sink()(line(text));
    }

    /// One planned step begins: the card, the panel and the log all say so.
    fn step(&self, phase: Phase, name: &str) {
        let (from, to) = {
            let mut progress = self.progress.lock().expect("progress lock");
            progress.start(name);
            let snapshot = progress.snapshot();
            (snapshot.from, snapshot.to)
        };
        tracing::info!("copy {from} -> {to}: {name}");
        (self.status)(phase, name);
        self.say(&format!("==> {name}"));
    }

    fn transfer(&self, label: &str, total_bytes: Option<u64>) {
        self.progress.lock().expect("progress lock").transfer(label, total_bytes);
    }

    pub fn bytes(&self, more: u64) {
        self.progress.lock().expect("progress lock").bytes(more);
    }

    fn transferred(&self) {
        self.progress.lock().expect("progress lock").transferred();
    }
}

pub struct Outcome {
    pub mirrored: Option<Mirrored>,
    pub summary: Summary,
}

pub(crate) fn line(text: &str) -> Line {
    Line {
        stream: Stream::Stdout,
        text: text.to_string(),
    }
}

fn reads_like_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("error") || lower.contains("failed")
}

/// Why a Docker command failed: the last error line it printed, or its
/// exit code when it printed none.
fn why_it_failed(code: Option<i32>, reason: &Mutex<Option<String>>) -> String {
    let said = reason.lock().expect("reason lock").take();
    said.unwrap_or_else(|| format!("exited with code {}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())))
}

/// Looks at both ends without changing anything.
pub async fn plan(ssh: &Ssh, sides: &Sides, request: &CopyRequest) -> anyhow::Result<CopyPlan> {
    let (from, to) = (&sides.from, &sides.to);
    anyhow::ensure!(from.exists(ssh).await, "{} has no compose file for {} at {}", from.label, from.name, from.compose_file());
    let destination_exists = to.exists(ssh).await;
    let mut warnings = Vec::new();
    if request.data && !request.config && !destination_exists {
        warnings.push(format!("{} has no copy of the project yet, so the config must travel too.", to.label));
    }

    let source_ps = from.compose_output(ssh, "ps -a --format json").await?;
    let source_running = compose::parse_ps(&source_ps.stdout).iter().any(|s| s.state == "running");
    let model = discover::model(ssh, from).await?;
    let preview = compose::parse_model(&model, std::path::Path::new(&from.dir));
    let mut ports: Vec<u16> = preview.services.iter().flat_map(|s| s.ports.iter().filter(|p| p.protocol == "tcp").map(|p| p.published)).collect();
    ports.sort_unstable();
    ports.dedup();

    let (volumes, containers, images, needed) = if request.data {
        let named = discover::named_volumes(ssh, from, &model).await;
        let needed: u64 = named.iter().map(|v| check::parse_human_size(&v.size)).sum();
        let volumes = named
            .iter()
            .map(|v| VolumePlan {
                key: v.key.clone(),
                name: v.name.clone(),
                size: v.size.clone(),
                destination_name: transfer::volume_name_at(&to.name, v),
                external: v.external,
            })
            .collect();
        if named.iter().any(|v| v.external) {
            warnings.push("External volumes keep their own name and are added to, not replaced.".into());
        }
        let containers = discover::container_data(ssh, from).await?;
        let images = if from.is_local() { discover::images_to_carry(ssh, from, to, &model).await } else { Vec::new() };
        (volumes, containers, images, needed)
    } else {
        (Vec::new(), Vec::new(), Vec::new(), 0)
    };
    let mut notes = check::preflight(ssh, to, needed).await?;
    let pulls = pull::planned(ssh, sides, request, &model, &images).await;
    notes.extend(pull::notes(sides, &pulls));

    Ok(CopyPlan {
        from: from.label.clone(),
        to: to.label.clone(),
        destination_exists,
        source_running,
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
    anyhow::ensure!(from.exists(ssh).await, "{} has no compose file for {}", from.label, from.name);
    let destination_exists = to.exists(ssh).await;
    anyhow::ensure!(request.config || destination_exists, "{} has no copy of the project yet; copy the config too", to.label);

    let model = discover::model(ssh, from).await?;
    let inventory = if request.data { Some(Inventory::at(ssh, from, to, &model).await) } else { None };
    let needed = inventory.as_ref().map(|i| i.volumes.iter().map(|v| check::parse_human_size(&v.size)).sum()).unwrap_or(0);
    for note in check::preflight(ssh, to, needed).await? {
        report.say(&format!("    {note}"));
    }
    let carried = inventory.as_ref().map(|i| i.images.as_slice()).unwrap_or(&[]);
    let pulls = pull::planned(ssh, sides, request, &model, carried).await;

    let source_ps = from.compose_output(ssh, "ps -a --format json").await?;
    let source_services = compose::parse_ps(&source_ps.stdout);
    let source_running = source_services.iter().any(|s| s.state == "running");
    let steps = steps::planned(sides, request, inventory.as_ref(), &pulls.downloads, destination_exists, source_running);
    report.progress.lock().expect("progress lock").set_steps(steps);
    pull::run(ssh, sides, &pulls, report).await?;

    if request.data && destination_exists {
        report.step(Phase::Migrating, &names::stop(&to.name, &to.label));
        let _ = to.compose_output(ssh, "stop").await;
    }
    let stopped_source = (request.stop_source || request.keep_source_stopped) && source_running;
    if stopped_source {
        report.step(Phase::Migrating, &names::stop(&from.name, &from.label));
        let why = if request.keep_source_stopped { "it stays stopped" } else { "for a consistent copy" };
        report.say(&format!("    {why}"));
        let stopped = from.compose_output(ssh, "stop").await?;
        anyhow::ensure!(stopped.ok(), "could not stop the copy on {}: {}", from.label, stopped.stderr);
    }

    let carried = carry(ssh, home, sides, request, inventory.as_ref(), &source_services, stopped_source, report).await;
    // The step that failed keeps the blame; starting the source again is
    // still done, and shown as its own step.
    if carried.is_err() {
        report.progress.lock().expect("progress lock").fail_step();
    }
    if stopped_source && !request.keep_source_stopped {
        report.step(Phase::Starting, &names::restart(&from.name, &from.label));
        let _ = from.compose_output(ssh, "start").await;
        if carried.is_err() {
            report.progress.lock().expect("progress lock").done_step();
        }
    }
    let mirrored = carried?;

    report.step(Phase::Starting, &names::check(&to.label));
    let summary = check::post_copy(ssh, to).await;
    report.say(&format!("    {}", summary.text(to)));
    Ok(Outcome { mirrored, summary })
}

/// What the source has that a data copy carries.
pub(crate) struct Inventory {
    pub(crate) volumes: Vec<NamedVolume>,
    pub(crate) images: Vec<String>,
}

impl Inventory {
    async fn at(ssh: &Ssh, from: &Site, to: &Site, model: &Value) -> Self {
        let volumes = discover::named_volumes(ssh, from, model).await;
        let images = if from.is_local() { discover::images_to_carry(ssh, from, to, model).await } else { Vec::new() };
        Self { volumes, images }
    }
}

/// Config, then data, then `up` at the destination. Split out so the caller
/// can start the source again whether this succeeded or not.
#[allow(clippy::too_many_arguments)]
async fn carry(ssh: &Ssh, home: &Path, sides: &Sides, request: &CopyRequest, inventory: Option<&Inventory>, source_services: &[compose::ServiceState], source_stopped: bool, report: &Report<'_>) -> anyhow::Result<Option<Mirrored>> {
    let (from, to) = (&sides.from, &sides.to);

    let mirrored = if request.config {
        report.step(Phase::Migrating, &names::folder(&to.label));
        let make_sink = || report.sink();
        Some(folder::mirror(ssh, home, from, to, &sides.excludes, &make_sink).await?)
    } else {
        None
    };

    if let Some(inventory) = inventory {
        for volume in &inventory.volumes {
            report.step(Phase::Migrating, &names::volume(&volume.key, &volume.size));
            let size = check::parse_human_size(&volume.size);
            report.transfer(&format!("volume {}", volume.key), (size > 0).then_some(size));
            let copied = transfer::copy_volume(ssh, from, to, volume, &sides.helper_image, report).await;
            report.transferred();
            copied.with_context(|| format!("volume {}", volume.name))?;
        }
        for image in &inventory.images {
            report.step(Phase::Migrating, &names::image(image));
            report.say(&format!("    it only exists on {}", from.label));
            report.transfer(&format!("image {image}"), None);
            let sent = transfer::copy_image(ssh, from, to, image, report).await;
            report.transferred();
            sent.with_context(|| format!("image {image}"))?;
        }
        report.step(Phase::Migrating, &names::create(&to.label));
        let (sink, reason) = report.sink_keeping_error();
        let code = to.compose_job(ssh, "create --build --remove-orphans", sink)?.wait().await?;
        anyhow::ensure!(code == Some(0), "compose create on {} failed: {}", to.label, why_it_failed(code, &reason));

        if !request.data_selection.is_empty() {
            copy_container_data(ssh, sides, request, source_services, source_stopped, report).await?;
        }
    }

    report.step(Phase::Starting, &names::start(&to.name, &to.label));
    let (sink, reason) = report.sink_keeping_error();
    let code = to.compose_job(ssh, "up -d --build --remove-orphans", sink)?.wait().await?;
    anyhow::ensure!(code == Some(0), "compose up on {} failed: {}", to.label, why_it_failed(code, &reason));
    Ok(mirrored)
}

/// `docker cp` streams for every ticked path. A source container that still
/// runs is stopped for its own copy, so the files are consistent.
async fn copy_container_data(ssh: &Ssh, sides: &Sides, request: &CopyRequest, source_services: &[compose::ServiceState], source_stopped: bool, report: &Report<'_>) -> anyhow::Result<()> {
    let (from, to) = (&sides.from, &sides.to);
    let destination_ps = to.compose_output(ssh, "ps -a --format json").await?;
    let destination_services = compose::parse_ps(&destination_ps.stdout);

    for selection in &request.data_selection {
        let step = names::path(&selection.path, &selection.service);
        let Some(source) = source_services.iter().find(|s| s.service == selection.service) else {
            report.progress.lock().expect("progress lock").skip(&step);
            report.say(&format!("    {}: no container on {}, skipped", selection.service, from.label));
            continue;
        };
        let destination = destination_services
            .iter()
            .find(|s| s.service == selection.service)
            .with_context(|| format!("service {} has no container on {}", selection.service, to.label))?;
        report.step(Phase::Migrating, &step);
        let pause = !source_stopped && source.state == "running";
        if pause {
            report.say(&format!("    stopping {} for a consistent copy", source.container));
            let _ = from.endpoint.docker_output(ssh, &["stop", &source.container]).await;
        }
        report.transfer(&format!("{} from {}", selection.path, selection.service), None);
        let copied = transfer::copy_path(ssh, from, to, &source.container, &selection.path, &destination.container, report).await;
        report.transferred();
        if pause {
            let _ = from.endpoint.docker_output(ssh, &["start", &source.container]).await;
        }
        copied.with_context(|| format!("{}:{}", selection.service, selection.path))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failure_is_explained_in_dockers_words() {
        let reason = Mutex::new(None);
        assert_eq!(why_it_failed(Some(1), &reason), "exited with code 1");
        // The lines compose printed when a registry refused an image.
        for text in [" Image quay.io/minio/minio:latest Pulling ", "Error response from daemon: unauthorized: access to the requested resource is not authorized"] {
            if reads_like_error(text) {
                *reason.lock().unwrap() = Some(text.trim().to_string());
            }
        }
        assert_eq!(why_it_failed(Some(1), &reason), "Error response from daemon: unauthorized: access to the requested resource is not authorized");
        assert!(reads_like_error("target api: failed to solve: process did not complete successfully"));
        assert!(!reads_like_error(" Container shop-db-1  Created"));
    }

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
