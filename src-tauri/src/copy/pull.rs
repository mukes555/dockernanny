//! The images the destination lacks, pulled there before anything stops.
//! Compose would pull them anyway when it creates the containers; pulling
//! first means an image that cannot be had (a registry that now refuses, a
//! name that is gone) ends the copy while the stack still runs at the
//! source, and the stack is stopped for less time.

use serde_json::Value;

use super::discover::{self, Download};
use super::steps::names;
use super::{why_it_failed, CopyRequest, Report, Sides};
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
pub fn note(label: &str, downloads: &[Download]) -> Option<String> {
    if downloads.is_empty() {
        return None;
    }
    let images: Vec<&str> = downloads.iter().map(|d| d.image.as_str()).collect();
    let count = if images.len() == 1 { "1 image".to_string() } else { format!("{} images", images.len()) };
    Some(format!("{label} downloads {count} first, before anything is stopped: {}.", images.join(", ")))
}

pub async fn run(ssh: &Ssh, sides: &Sides, downloads: &[Download], report: &Report<'_>) -> anyhow::Result<()> {
    let to = &sides.to;
    for download in downloads {
        report.step(Phase::Migrating, &names::download(&download.image, &to.label));
        let (sink, reason) = report.sink_keeping_error();
        let code = to.endpoint.docker_job(ssh, &download.pull_args(), sink)?.wait().await?;
        anyhow::ensure!(code == Some(0), "{} could not download {}: {}. Nothing was stopped.", to.label, download.image, why_it_failed(code, &reason));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sheet_names_what_will_be_downloaded() {
        assert_eq!(note("studio", &[]), None);
        let one = [Download { image: "redis:7-alpine".into(), platform: None }];
        assert_eq!(note("studio", &one).unwrap(), "studio downloads 1 image first, before anything is stopped: redis:7-alpine.");
        let two = [one[0].clone(), Download { image: "postgres:16".into(), platform: None }];
        assert!(note("studio", &two).unwrap().starts_with("studio downloads 2 images first"));
    }
}
