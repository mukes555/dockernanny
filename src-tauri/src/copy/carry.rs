//! The part of a copy that moves things: the project folder, the volumes,
//! images only the source has, the paths inside containers, and at the end
//! the stack started at the destination. The caller stops and starts the
//! source around it.

use std::path::Path;

use anyhow::Context;

use super::folder::{self, Mirrored};
use super::look::Inventory;
use super::report::why_it_failed;
use super::steps::names;
use super::{check, transfer, CopyRequest, Report, Sides};
use crate::compose::ServiceState;
use crate::machine::first_line;
use crate::ssh::Ssh;
use crate::stack::Phase;

/// Config, then data, then `up` at the destination. Split out so the caller
/// can start the source again whether this succeeded or not.
#[allow(clippy::too_many_arguments)]
pub(super) async fn carry(
    ssh: &Ssh,
    home: &Path,
    sides: &Sides,
    request: &CopyRequest,
    inventory: Option<&Inventory>,
    source_services: &[ServiceState],
    source_stopped: bool,
    report: &Report<'_>,
) -> anyhow::Result<Option<Mirrored>> {
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
    // After a data copy, `create --build` above has built the images, so up
    // starts what is there; a config-only copy builds here, once.
    let built_already = inventory.is_some();
    let code = to.compose_job(ssh, crate::stack::up_args(!built_already), sink)?.wait().await?;
    anyhow::ensure!(code == Some(0), "compose up on {} failed: {}", to.label, why_it_failed(code, &reason));
    Ok(mirrored)
}

/// `docker cp` streams for every ticked path. A source container that still
/// runs is stopped for its own copy, so the files are consistent.
async fn copy_container_data(
    ssh: &Ssh,
    sides: &Sides,
    request: &CopyRequest,
    source_services: &[ServiceState],
    source_stopped: bool,
    report: &Report<'_>,
) -> anyhow::Result<()> {
    let (from, to) = (&sides.from, &sides.to);
    let destination_services = to.services(ssh).await?;

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
            let stopped = from.endpoint.docker_output(ssh, &["stop", &source.container]).await?;
            anyhow::ensure!(
                stopped.ok(),
                "could not stop {} on {} for a consistent copy: {}",
                source.container,
                from.label,
                first_line(&stopped.stderr)
            );
        }
        report.transfer(&format!("{} from {}", selection.path, selection.service), None);
        let copied = transfer::copy_path(ssh, from, to, &source.container, &selection.path, &destination.container, report).await;
        report.transferred();
        if pause {
            // Checked before the copy's own result: a service left stopped is the more urgent news.
            let started = from.endpoint.docker_output(ssh, &["start", &source.container]).await;
            let started_again = matches!(&started, Ok(out) if out.ok());
            anyhow::ensure!(started_again, "{} on {} could not be started again after its copy", source.container, from.label);
        }
        copied.with_context(|| format!("{}:{}", selection.service, selection.path))?;
    }
    Ok(())
}
