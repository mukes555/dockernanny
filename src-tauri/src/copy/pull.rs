//! The images the destination lacks, pulled there before anything stops.
//! Compose would pull them anyway when it creates the containers; pulling
//! first means an image that cannot be had ends the copy while the stack
//! still runs at the source, and the stack is stopped for less time.
//!
//! An image the destination cannot download (a registry that refuses, a
//! private registry only the source is logged in to, a name that is gone)
//! is sent from the source instead, when the source's copy is built for
//! what the destination runs. Otherwise the copy stops, saying why.
//!
//! Only Docker's own pull decides whether an image can be had. Asking the
//! registry ahead (`docker manifest inspect`) took up to half a minute per
//! image and said "refused" for images that downloaded fine.

use anyhow::Context;
use serde_json::Value;

use super::discover::{self, Download};
use super::endpoint::Site;
use super::report::why_it_failed;
use super::steps::names;
use super::{transfer, CopyRequest, Report, Sides};
use crate::ssh::Ssh;
use crate::stack::Phase;

/// From the compose file that will run at the destination: the source's
/// when the config travels, else the one already there. When that file
/// cannot be read, nothing is pulled ahead and compose pulls at create, as
/// it always did.
pub async fn planned(ssh: &Ssh, sides: &Sides, request: &CopyRequest, source_model: &Value, carried: &[String]) -> Vec<Download> {
    let to = &sides.to;
    if request.config {
        return discover::images_to_download(ssh, to, source_model, carried).await;
    }
    match discover::model(ssh, to).await {
        Ok(there) => discover::images_to_download(ssh, to, &there, carried).await,
        Err(_) => Vec::new(),
    }
}

/// For the copy sheet, so the download is listed before the user commits.
pub fn note(sides: &Sides, downloads: &[Download]) -> Option<String> {
    if downloads.is_empty() {
        return None;
    }
    let (from, to) = (&sides.from.label, &sides.to.label);
    let images: Vec<&str> = downloads.iter().map(|d| d.image.as_str()).collect();
    let count = if images.len() == 1 { "1 image".to_string() } else { format!("{} images", images.len()) };
    let instead = sentence(&format!("{from}'s own copy goes instead of one it cannot download, if it is built for {to}."));
    Some(format!("{to} downloads {count} first, before anything is stopped: {}. {instead}", images.join(", ")))
}

pub async fn run(ssh: &Ssh, sides: &Sides, downloads: &[Download], report: &Report<'_>) -> anyhow::Result<()> {
    let to = &sides.to;
    for download in downloads {
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
    fn the_sheet_names_what_will_be_downloaded() {
        assert_eq!(note(&sides(), &[]), None);
        assert_eq!(
            note(&sides(), &[download("redis:7-alpine")]).unwrap(),
            "studio downloads 1 image first, before anything is stopped: redis:7-alpine. This computer's own copy goes instead of one it cannot download, if it is built for studio."
        );
        let two = note(&sides(), &[download("quay.io/minio/minio:latest"), download("redis:7-alpine")]).unwrap();
        assert!(two.starts_with("studio downloads 2 images first"));
        assert_eq!(sentence("this computer's copy"), "This computer's copy");
    }

    #[test]
    fn the_source_copy_goes_only_when_it_fits_the_destination() {
        assert_eq!(cannot_send("this computer", "studio", Some("linux/amd64"), Some("linux/amd64")), None);
        assert_eq!(cannot_send("this computer", "studio", Some("linux/arm64/v8"), Some("linux/arm64")), None);
        // What the MinIO copy met: an Apple silicon copy for an Intel machine.
        assert_eq!(
            cannot_send("this computer", "studio", Some("linux/arm64"), Some("linux/amd64")).unwrap(),
            "this computer's copy is built for linux/arm64, and studio needs linux/amd64."
        );
        assert_eq!(cannot_send("this computer", "studio", None, Some("linux/amd64")).unwrap(), "this computer does not have it either.");
        assert!(cannot_send("this computer", "studio", Some("linux/amd64"), None).unwrap().contains("did not say"));
    }
}
