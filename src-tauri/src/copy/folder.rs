//! Mirroring the project folder with rsync between two endpoints. rsync
//! refuses two remote ends, so a machine to machine copy goes through a
//! staging folder on this computer, removed afterwards.

use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use anyhow::Context;

use super::endpoint::{Endpoint, Site};
use super::Sink;
use crate::job::Job;
use crate::ssh::Ssh;
use crate::stack::shell_quote;
use crate::tools;

pub struct Mirrored {
    pub files: u32,
    pub warning: Option<String>,
}

/// One direction of the copy: `from`'s contents become `to`'s contents.
/// `--delete` keeps the copy exact but never removes excluded paths, so a
/// node_modules a container created inside a bind mount survives.
pub async fn mirror(ssh: &Ssh, home: &Path, from: &Site, to: &Site, excludes: &[String], make_sink: &(dyn Fn() -> Sink + Send + Sync)) -> anyhow::Result<Mirrored> {
    match (&from.endpoint, &to.endpoint) {
        (Endpoint::Local, Endpoint::Local) => anyhow::bail!("both ends are this computer"),
        (_, Endpoint::Machine { .. }) if from.is_local() => {
            prepare(ssh, to).await?;
            run_rsync(&rsync_args(&ssh.rsync_transport(), &rsync_path(from), &rsync_path(to), excludes), make_sink()).await
        }
        (Endpoint::Machine { .. }, Endpoint::Local) => {
            prepare(ssh, to).await?;
            run_rsync(&rsync_args(&ssh.rsync_transport(), &rsync_path(from), &rsync_path(to), excludes), make_sink()).await
        }
        (Endpoint::Machine { .. }, Endpoint::Machine { .. }) => {
            let staging = Site::local(&to.name, &home.join("staging").join(&to.name).display().to_string(), &to.compose_rel);
            let result = relay(ssh, from, &staging, to, excludes, make_sink).await;
            let _ = std::fs::remove_dir_all(&staging.dir);
            result
        }
        (Endpoint::Local, Endpoint::Machine { .. }) => unreachable!("handled above"),
    }
}

async fn relay(ssh: &Ssh, from: &Site, staging: &Site, to: &Site, excludes: &[String], make_sink: &(dyn Fn() -> Sink + Send + Sync)) -> anyhow::Result<Mirrored> {
    prepare(ssh, staging).await?;
    // --delete on the pull too, so a staging folder left by a crashed run cannot leak old files.
    run_rsync(&rsync_args(&ssh.rsync_transport(), &rsync_path(from), &rsync_path(staging), excludes), make_sink()).await?;
    prepare(ssh, to).await?;
    run_rsync(&rsync_args(&ssh.rsync_transport(), &rsync_path(staging), &rsync_path(to), excludes), make_sink()).await
}

/// rsync only creates the last path component, so the parent must exist.
async fn prepare(ssh: &Ssh, site: &Site) -> anyhow::Result<()> {
    match &site.endpoint {
        Endpoint::Local => std::fs::create_dir_all(&site.dir).with_context(|| format!("create {}", site.dir)),
        Endpoint::Machine { alias } => {
            let out = ssh.run(alias, &format!("mkdir -p {}", shell_quote(&site.dir))).await?;
            anyhow::ensure!(out.ok(), "could not create {} on {}: {}", site.dir, site.label, out.stderr);
            Ok(())
        }
    }
}

/// `dir/` here, `alias:dir/` there. The trailing slash sends the folder's
/// contents, not the folder itself. A local folder is named as rsync sees
/// it, which on Windows is under `/mnt/<drive>` inside WSL.
pub fn rsync_path(site: &Site) -> String {
    match &site.endpoint {
        Endpoint::Local => format!("{}/", tools::path(Path::new(&site.dir))),
        Endpoint::Machine { alias } => format!("{alias}:{}/", site.dir),
    }
}

/// Not -a: owner and group would only produce warnings when the users differ.
pub fn rsync_args(transport: &str, source: &str, destination: &str, excludes: &[String]) -> Vec<String> {
    let mut args: Vec<String> = ["-rltpz", "--delete", "--itemize-changes", "-e", transport].iter().map(|s| s.to_string()).collect();
    for exclude in excludes {
        args.push("--exclude".into());
        args.push(exclude.clone());
    }
    args.push(source.to_string());
    args.push(destination.to_string());
    args
}

async fn run_rsync(args: &[String], mut on_line: Sink) -> anyhow::Result<Mirrored> {
    let mut cmd = tools::unix("rsync");
    cmd.args(args);
    let files = Arc::new(AtomicU32::new(0));
    let counter = files.clone();
    let job = Job::spawn(cmd, None, move |line| {
        if is_file_change(&line.text) {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        on_line(line);
    })?;
    let code = job.wait().await?;
    let files = files.load(Ordering::Relaxed);
    match code {
        Some(0) => Ok(Mirrored { files, warning: None }),
        // Partial transfer: usually files owned by root inside a bind mount.
        Some(23) | Some(24) => Ok(Mirrored {
            files,
            warning: Some("some files could not be copied (rsync reported a partial transfer)".into()),
        }),
        other => anyhow::bail!("rsync exited with {other:?}"),
    }
}

/// `--itemize-changes` lines: a sent file starts with `>f`, `<f` or `cf`;
/// deletions with `*deleting`. Directories and attribute-only lines do not count.
pub fn is_file_change(line: &str) -> bool {
    let sent_file = line.len() > 2 && (line.starts_with('>') || line.starts_with('<') || line.starts_with('c')) && line.as_bytes()[1] == b'f';
    sent_file || line.starts_with("*deleting")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsync_arguments_for_each_direction() {
        let local = Site::local("shop", "/home/alex/projects/shop", "docker-compose.yml");
        let machine = Site::machine("shop", "docker-compose.yml", "dn-1234", "studio");
        assert_eq!(rsync_path(&local), "/home/alex/projects/shop/");
        assert_eq!(rsync_path(&machine), "dn-1234:.dockernanny/shop/");
        let args = rsync_args("ssh -F /x/cfg", &rsync_path(&machine), &rsync_path(&local), &[".git".into()]);
        assert_eq!(args, vec!["-rltpz", "--delete", "--itemize-changes", "-e", "ssh -F /x/cfg", "--exclude", ".git", "dn-1234:.dockernanny/shop/", "/home/alex/projects/shop/"]);
    }

    #[test]
    fn itemized_lines_count_files_not_directories() {
        assert!(is_file_change(">f+++++++++ docker-compose.yml"));
        assert!(is_file_change("<f.st...... app/index.js"));
        assert!(is_file_change("*deleting   old.txt"));
        assert!(!is_file_change("cd+++++++++ html/"));
        assert!(!is_file_change(".d..t...... ./"));
        assert!(!is_file_change("sending incremental file list"));
    }
}
