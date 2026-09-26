//! The images the destination lacks, pulled there before anything stops.
//! Compose would pull them anyway when it creates the containers; pulling
//! first means an image that cannot be had ends the copy while the stack
//! still runs at the source, and the stack is stopped for less time.
//!
//! An image the destination cannot download (a registry that refuses, a
//! private registry only the source is logged in to, a name that is gone)
//! is sent from the source instead, when the source's copy is built for
//! what the destination runs. Otherwise the copy stops, saying why.

use std::time::Duration;

use anyhow::Context;
use serde_json::Value;
use tokio::time::timeout;

use super::discover::{self, Download};
use super::endpoint::Site;
use super::steps::names;
use super::{transfer, why_it_failed, CopyRequest, Report, Sides};
use crate::ssh::Ssh;
use crate::stack::Phase;

/// A registry that does not answer within this is treated as refusing; the
/// pull still decides.
const ASK_TIMEOUT: Duration = Duration::from_secs(20);

pub struct Pulls {
    /// In the order they run: the ones the registry refused first.
    pub downloads: Vec<Download>,
    /// Asked before anything downloads, so a copy that cannot go ahead is
    /// known in seconds.
    pub refused: Vec<String>,
}

/// From the compose file that will run at the destination: the source's
/// when the config travels, else the one already there. When that file
/// cannot be read, nothing is pulled ahead and compose pulls at create, as
/// it always did.
pub async fn planned(ssh: &Ssh, sides: &Sides, request: &CopyRequest, source_model: &Value, carried: &[String]) -> Pulls {
    let to = &sides.to;
    let downloads = if request.config {
        discover::images_to_download(ssh, to, source_model, carried).await
    } else {
        match discover::model(ssh, to).await {
            Ok(there) => discover::images_to_download(ssh, to, &there, carried).await,
            Err(_) => Vec::new(),
        }
    };
    let refused = refused_by_registry(ssh, to, &downloads).await;
    Pulls { downloads: refused_first(downloads, &refused), refused }
}

/// Asks each image's registry, the way the destination would, without
/// downloading anything. Only the order follows from it: the daemon's pull
/// decides, because it can reach mirrors the command line cannot.
async fn refused_by_registry(ssh: &Ssh, to: &Site, downloads: &[Download]) -> Vec<String> {
    let mut refused = Vec::new();
    for download in downloads {
        let args = ["manifest", "inspect", download.image.as_str()];
        let answered = matches!(timeout(ASK_TIMEOUT, to.endpoint.docker_output(ssh, &args)).await, Ok(Ok(out)) if out.ok());
        if !answered {
            refused.push(download.image.clone());
        }
    }
    refused
}

fn refused_first(downloads: Vec<Download>, refused: &[String]) -> Vec<Download> {
    let (mut first, rest): (Vec<Download>, Vec<Download>) = downloads.into_iter().partition(|d| refused.contains(&d.image));
    first.extend(rest);
    first
}

/// For the copy sheet, so the download is listed before the user commits.
pub fn notes(sides: &Sides, pulls: &Pulls) -> Vec<String> {
    let (from, to) = (&sides.from.label, &sides.to.label);
    if pulls.downloads.is_empty() {
        return Vec::new();
    }
    let images: Vec<&str> = pulls.downloads.iter().map(|d| d.image.as_str()).collect();
    let count = if images.len() == 1 { "1 image".to_string() } else { format!("{} images", images.len()) };
    let mut notes = vec![format!("{to} downloads {count} first, before anything is stopped: {}.", images.join(", "))];
    if !pulls.refused.is_empty() {
        let instead = sentence(&format!("{from}'s own copy is sent instead if it is built for {to}; otherwise the copy stops before anything changes."));
        notes.push(format!("The registry already refuses {}. {instead}", pulls.refused.join(", ")));
    }
    notes
}

pub async fn run(ssh: &Ssh, sides: &Sides, pulls: &Pulls, report: &Report<'_>) -> anyhow::Result<()> {
    let to = &sides.to;
    for download in &pulls.downloads {
        report.step(Phase::Migrating, &names::download(&download.image, &to.label));
        let (sink, reason) = report.sink_keeping_error();
        let code = to.endpoint.docker_job(ssh, &download.pull_args(), sink)?.wait().await?;
        if code == Some(0) {
            continue;
        }
        let why = why_it_failed(code, &reason);
        report.progress.lock().expect("progress lock").fail_step();
        send_instead(ssh, sides, download, &why, report).await?;
    }
    Ok(())
}

/// The destination could not download the image: the source's own copy
/// goes instead, through `docker save` and `docker load`, when it is built
/// for the platform the destination needs.
async fn send_instead(ssh: &Ssh, sides: &Sides, download: &Download, why: &str, report: &Report<'_>) -> anyhow::Result<()> {
    let (from, to) = (&sides.from, &sides.to);
    let needed = match &download.platform {
        Some(asked) => Some(asked.clone()),
        None => docker_answer(ssh, to, &["version", "--format", "{{.Server.Os}}/{{.Server.Arch}}"]).await,
    };
    let has = docker_answer(ssh, from, &["image", "inspect", "--format", "{{.Os}}/{{.Architecture}}", &download.image]).await;
    if let Some(problem) = cannot_send(&from.label, &to.label, has.as_deref(), needed.as_deref()) {
        anyhow::bail!("{} could not download {}: {why}. {} Nothing was stopped.", to.label, download.image, sentence(&problem));
    }
    report.step(Phase::Migrating, &names::send_instead(&download.image, &from.label));
    report.say(&format!("    {} could not download it ({why}); sending {}'s copy", to.label, from.label));
    report.transfer(&format!("image {}", download.image), None);
    let sent = transfer::copy_image(ssh, from, to, &download.image, report).await;
    report.transferred();
    sent.with_context(|| format!("image {}", download.image))
}

async fn docker_answer(ssh: &Ssh, site: &Site, args: &[&str]) -> Option<String> {
    let out = site.endpoint.docker_output(ssh, args).await.ok()?;
    let text = out.stdout.trim().to_string();
    (out.ok() && !text.is_empty()).then_some(text)
}

/// Why the source's copy cannot stand in, or None when it can.
fn cannot_send(from: &str, to: &str, has: Option<&str>, needed: Option<&str>) -> Option<String> {
    let Some(has) = has else {
        return Some(format!("{from} does not have it either."));
    };
    let Some(needed) = needed else {
        return Some(format!("{to} did not say which platform it runs, so {from}'s copy is not sent."));
    };
    if os_and_arch(has) == os_and_arch(needed) {
        return None;
    }
    Some(format!("{from}'s copy is built for {has}, and {to} needs {needed}."))
}

/// A computer's label ("this computer") can open a sentence.
fn sentence(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "linux/arm64/v8" and "linux/arm64" are the same machine for this purpose.
fn os_and_arch(platform: &str) -> String {
    platform.split('/').take(2).collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::copy::endpoint::Site;

    fn sides() -> Sides {
        Sides {
            from: Site::local("shop", "/home/alex/shop", "compose.yaml"),
            to: Site::machine("shop", "compose.yaml", "dn-m1", "studio"),
            excludes: Vec::new(),
            helper_image: "alpine:3".into(),
        }
    }

    fn download(image: &str) -> Download {
        Download { image: image.into(), platform: None }
    }

    #[test]
    fn the_sheet_names_what_will_be_downloaded_and_what_is_refused() {
        let none = Pulls { downloads: Vec::new(), refused: Vec::new() };
        assert!(notes(&sides(), &none).is_empty());
        let one = Pulls { downloads: vec![download("redis:7-alpine")], refused: Vec::new() };
        assert_eq!(notes(&sides(), &one), vec!["studio downloads 1 image first, before anything is stopped: redis:7-alpine."]);
        let refused = Pulls { downloads: vec![download("quay.io/minio/minio:latest"), download("redis:7-alpine")], refused: vec!["quay.io/minio/minio:latest".into()] };
        let said = notes(&sides(), &refused);
        assert!(said[0].starts_with("studio downloads 2 images first"));
        assert!(said[1].starts_with("The registry already refuses quay.io/minio/minio:latest. This computer's own copy is sent instead"));
        assert_eq!(sentence("this computer's copy"), "This computer's copy");
    }

    #[test]
    fn refused_images_go_first_and_the_rest_keep_their_order() {
        let downloads = vec![download("clamav/clamav:stable"), download("quay.io/minio/minio:latest"), download("redis:7-alpine")];
        let ordered = refused_first(downloads, &["quay.io/minio/minio:latest".into()]);
        let names: Vec<&str> = ordered.iter().map(|d| d.image.as_str()).collect();
        assert_eq!(names, vec!["quay.io/minio/minio:latest", "clamav/clamav:stable", "redis:7-alpine"]);
    }

    #[test]
    fn the_source_copy_goes_only_when_it_fits_the_destination() {
        assert_eq!(cannot_send("this computer", "Mk", Some("linux/amd64"), Some("linux/amd64")), None);
        assert_eq!(cannot_send("this computer", "Mk", Some("linux/arm64/v8"), Some("linux/arm64")), None);
        // What the MinIO copy met: an Apple silicon copy for an Intel machine.
        assert_eq!(cannot_send("this computer", "Mk", Some("linux/arm64"), Some("linux/amd64")).unwrap(), "this computer's copy is built for linux/arm64, and Mk needs linux/amd64.");
        assert_eq!(cannot_send("this computer", "Mk", None, Some("linux/amd64")).unwrap(), "this computer does not have it either.");
        assert!(cannot_send("this computer", "Mk", Some("linux/amd64"), None).unwrap().contains("did not say"));
    }
}
