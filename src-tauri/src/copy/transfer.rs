//! The streams that carry data between two endpoints: a volume, an image, or
//! a path inside a container. Each is one `docker` process piped into
//! another; when both ends are machines the two `ssh` processes are joined
//! on this computer without a temp file.

use std::path::Path;
use std::process::Stdio;

use anyhow::Context;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::discover::{self, NamedVolume};
use super::endpoint::Site;
use super::Report;
use crate::ssh::{self, Ssh};

/// Relayed a piece at a time, so the bytes can be counted on the way.
const CHUNK: usize = 64 * 1024;

/// Compose expects `<project>_<key>` with matching labels; external volumes
/// keep their own name.
pub fn volume_name_at(project: &str, volume: &NamedVolume) -> String {
    if volume.external {
        return volume.name.clone();
    }
    format!("{project}_{}", volume.key)
}

/// `tar cz` out of the source volume into the destination volume, which is
/// created if missing and emptied first, so the copy replaces rather than
/// overlays. External volumes are never emptied: they may be shared.
pub async fn copy_volume(ssh: &Ssh, from: &Site, to: &Site, volume: &NamedVolume, helper_image: &str, report: &Report<'_>) -> anyhow::Result<()> {
    let destination = volume_name_at(&to.name, volume);
    anyhow::ensure!(safe_name(&volume.name) && safe_name(&destination), "unsafe volume name");
    let exists = to.endpoint.docker_output(ssh, &["volume", "inspect", &destination]).await?.ok();
    if !exists {
        let project = format!("com.docker.compose.project={}", to.name);
        let key = format!("com.docker.compose.volume={}", volume.key);
        let created = if volume.external {
            to.endpoint.docker_output(ssh, &["volume", "create", &destination]).await?
        } else {
            to.endpoint.docker_output(ssh, &["volume", "create", "--label", &project, "--label", &key, &destination]).await?
        };
        anyhow::ensure!(created.ok(), "could not create the volume on {}: {}", to.label, created.stderr);
    }

    let source_mount = format!("{}:/from:ro", volume.name);
    let reader = from.endpoint.docker(ssh, &["run", "--rm", "-v", &source_mount, helper_image, "tar", "cz", "-C", "/from", "."])?;
    let destination_mount = format!("{destination}:/to");
    let unpack = if volume.external { "tar xz -C /to" } else { "find /to -mindepth 1 -delete && tar xz -C /to" };
    let writer = to.endpoint.docker(ssh, &["run", "--rm", "-i", "-v", &destination_mount, helper_image, "sh", "-c", unpack])?;
    let bytes = pipe(from, to, reader, writer, report).await?;
    report.say(&format!("    copied, {} through the pipe", super::progress::megabytes(bytes)));
    Ok(())
}

/// `docker save` at the source into `docker load` at the destination, for
/// an image the destination cannot pull because it only exists there.
pub async fn copy_image(ssh: &Ssh, from: &Site, to: &Site, image: &str, report: &Report<'_>) -> anyhow::Result<()> {
    anyhow::ensure!(safe_image(image), "unsafe image name");
    let reader = from.endpoint.docker(ssh, &["save", image])?;
    let writer = to.endpoint.docker(ssh, &["load"])?;
    let bytes = pipe(from, to, reader, writer, report).await?;
    report.say(&format!("    loaded, {}", super::progress::megabytes(bytes)));
    Ok(())
}

/// `docker cp -a` out of one container into the same place in the other.
/// A bind or read-only mount inside the path (Keycloak's read-only
/// `data/import`) cannot be written through `docker cp`, so the path is
/// copied around it: entry by entry, down to where nothing such is mounted.
/// A writable volume there (a database's anonymous volume) is written into.
pub async fn copy_path(ssh: &Ssh, from: &Site, to: &Site, from_container: &str, path: &str, to_container: &str, report: &Report<'_>) -> anyhow::Result<()> {
    anyhow::ensure!(safe_name(from_container) && safe_name(to_container), "unsafe container name");
    anyhow::ensure!(safe_path(path), "unsafe path");
    let inspect = to.endpoint.docker_output(ssh, &["inspect", "-f", "{{json .Mounts}}", to_container]).await?;
    let mounts = discover::mounts_to_go_around(&inspect.stdout);

    let mut bytes = 0;
    let mut queue = vec![path.to_string()];
    while let Some(current) = queue.pop() {
        match around_mounts(&current, &mounts) {
            Around::Skip => report.say(&format!("    {current} is a mount on {}, left as it is", to.label)),
            Around::Whole => bytes += stream_path(ssh, from, to, from_container, &current, to_container, report).await?,
            Around::Descend => {
                for child in children(ssh, from, from_container, &current).await? {
                    queue.push(format!("{current}/{child}"));
                }
            }
        }
    }
    report.say(&format!("    copied, {}", super::progress::megabytes(bytes)));
    Ok(())
}

#[derive(Debug, PartialEq, Eq)]
pub enum Around {
    /// The path is a mount point itself: the destination has its own.
    Skip,
    /// Nothing mounted inside: one stream for the whole path.
    Whole,
    /// A mount somewhere inside: look at the entries one level down.
    Descend,
}

pub fn around_mounts(path: &str, mounts: &[String]) -> Around {
    if mounts.iter().any(|m| m == path) {
        return Around::Skip;
    }
    let inside = format!("{}/", path.trim_end_matches('/'));
    if mounts.iter().any(|m| m.starts_with(&inside)) {
        Around::Descend
    } else {
        Around::Whole
    }
}

/// The tar is rooted at the path's last component, so it lands in the
/// parent directory over there; `-a` keeps owners, which databases insist on.
async fn stream_path(ssh: &Ssh, from: &Site, to: &Site, from_container: &str, path: &str, to_container: &str, report: &Report<'_>) -> anyhow::Result<u64> {
    anyhow::ensure!(safe_path(path), "unsafe path {path}");
    let parent = parent_of(path);
    let source = format!("{from_container}:{path}");
    let destination = format!("{to_container}:{parent}");
    let reader = from.endpoint.docker(ssh, &["cp", "-a", &source, "-"])?;
    let writer = to.endpoint.docker(ssh, &["cp", "-a", "-", &destination])?;
    pipe(from, to, reader, writer, report).await
}

/// The direct entries of a directory inside the source container, read from
/// the tar `docker cp` produces: its first component is the directory
/// itself, the second a child.
async fn children(ssh: &Ssh, from: &Site, container: &str, path: &str) -> anyhow::Result<Vec<String>> {
    let script = format!("docker cp -a {container}:{path} - | tar t | awk -F/ 'NF > 1 && $2 != \"\" {{ print $2 }}' | sort -u");
    let out = from.endpoint.run_script(ssh, &script).await?;
    anyhow::ensure!(out.ok(), "could not list {path} on {}: {}", from.label, out.stderr);
    Ok(out.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

/// Runs `reader` with its stdout relayed into `writer`'s stdin, a chunk at a
/// time, counting the bytes for the progress panel. Nothing is buffered
/// beyond one chunk. Both children are tracked so quitting the app ends
/// them. Returns the bytes that went through.
pub async fn pipe(from: &Site, to: &Site, mut reader: tokio::process::Command, mut writer: tokio::process::Command, report: &Report<'_>) -> anyhow::Result<u64> {
    let mut reader = reader.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null()).spawn().with_context(|| format!("start the stream on {}", from.label))?;
    ssh::track_child(&reader);
    let mut stdout = reader.stdout.take().context("no stdout")?;
    let mut writer = writer.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().with_context(|| format!("start the stream on {}", to.label))?;
    ssh::track_child(&writer);
    let mut stdin = writer.stdin.take().context("no stdin")?;

    let relay = async {
        let mut buffer = vec![0u8; CHUNK];
        let mut total = 0u64;
        loop {
            let n = stdout.read(&mut buffer).await?;
            if n == 0 {
                break;
            }
            stdin.write_all(&buffer[..n]).await?;
            total += n as u64;
            report.bytes(n as u64);
        }
        // Closing the writer's stdin is what tells it the stream is over.
        stdin.shutdown().await?;
        drop(stdin);
        Ok::<u64, std::io::Error>(total)
    };
    let (relayed, read, written) = tokio::join!(relay, reader.wait_with_output(), writer.wait_with_output());
    let read = read?;
    let written = written?;
    for text in [String::from_utf8_lossy(&read.stderr), String::from_utf8_lossy(&written.stderr)] {
        for l in text.lines().filter(|l| !l.trim().is_empty()).take(20) {
            report.say(&format!("    | {l}"));
        }
    }
    anyhow::ensure!(read.status.success(), "reading on {} failed", from.label);
    anyhow::ensure!(written.status.success(), "writing on {} failed", to.label);
    // A relay error after both children succeeded can only be a closed pipe
    // the writer did not mind; the children's exit codes are what count.
    Ok(relayed.unwrap_or(0))
}

pub fn parent_of(path: &str) -> String {
    Path::new(path).parent().map(|p| p.display().to_string()).filter(|p| !p.is_empty()).unwrap_or_else(|| "/".into())
}

pub fn safe_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

pub fn safe_image(image: &str) -> bool {
    !image.is_empty() && image.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | '@'))
}

/// An absolute path with nothing that could escape the quotes on a machine.
pub fn safe_path(path: &str) -> bool {
    let absolute = path.starts_with('/') && path.len() > 1;
    let plain = path.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '_' | '-' | '.' | '+' | '@'));
    absolute && plain && !path.split('/').any(|part| part == "..")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_names_follow_compose_convention() {
        let named = NamedVolume { key: "pgdata".into(), name: "shop-api_pgdata".into(), size: "1GB".into(), external: false };
        assert_eq!(volume_name_at("shop", &named), "shop_pgdata");
        let external = NamedVolume { key: "shared".into(), name: "shared".into(), size: "1GB".into(), external: true };
        assert_eq!(volume_name_at("shop", &external), "shared");
    }

    #[test]
    fn a_path_is_copied_around_the_mounts_inside_it() {
        let mounts = vec!["/opt/keycloak/data/import".to_string(), "/opt/keycloak/themes".to_string(), "/var/lib/postgresql/data".to_string()];
        assert_eq!(around_mounts("/opt/keycloak/data", &mounts), Around::Descend);
        assert_eq!(around_mounts("/opt/keycloak/data/import", &mounts), Around::Skip);
        assert_eq!(around_mounts("/opt/keycloak/data/h2", &mounts), Around::Whole);
        assert_eq!(around_mounts("/opt/keycloak/data-old", &mounts), Around::Whole, "a sibling with the same prefix is not inside");
        assert_eq!(around_mounts("/app/storage", &mounts), Around::Whole);
    }

    #[test]
    fn only_plain_names_and_paths_reach_the_shell() {
        assert!(safe_name("shop-api_pgdata"));
        assert!(!safe_name("bad name"));
        assert!(!safe_name("x;rm -rf /"));
        assert!(!safe_name(""));
        assert!(safe_image("registry.example.com:5000/team/api:1.2"));
        assert!(safe_image("api@sha256:0123abcd"));
        assert!(!safe_image("api; rm -rf /"));
        assert!(safe_path("/var/lib/postgresql/data"));
        assert!(!safe_path("relative/path"));
        assert!(!safe_path("/"));
        assert!(!safe_path("/opt/../etc"));
        assert!(!safe_path("/opt/it's"));
        assert_eq!(parent_of("/var/lib/postgresql/data"), "/var/lib/postgresql");
        assert_eq!(parent_of("/data"), "/");
    }

    use super::super::progress::{Publish, Tracker};
    use super::super::{EndpointRef, Sink};
    use crate::stack::Phase;
    use std::sync::{Arc, Mutex};

    fn report_for_test<'a>(make_sink: &'a (dyn Fn() -> Sink + Send + Sync), status: &'a (dyn Fn(Phase, &str) + Send + Sync), seen: Arc<Mutex<Vec<u64>>>) -> Report<'a> {
        let publish: Publish = Box::new(move |p| {
            if let Some(current) = &p.current {
                seen.lock().unwrap().push(current.bytes);
            }
        });
        let tracker = Tracker::new("s1", "b", "a", "b", EndpointRef::ThisComputer, publish);
        Report { make_sink, status, progress: tracker.shared() }
    }

    #[tokio::test]
    async fn the_pipe_joins_two_local_processes_and_counts_the_bytes() {
        let from = Site::local("a", "/tmp/a", "x.yml");
        let to = Site::local("b", "/tmp/b", "x.yml");
        let make_sink = || -> Sink { Box::new(|_| {}) };
        let status = |_: Phase, _: &str| {};
        let seen = Arc::new(Mutex::new(Vec::new()));
        let report = report_for_test(&make_sink, &status, seen.clone());
        report.transfer("test", None);

        let mut reader = tokio::process::Command::new("head");
        reader.args(["-c", "1048576", "/dev/zero"]);
        let mut writer = tokio::process::Command::new("sh");
        writer.args(["-c", "[ \"$(wc -c)\" -eq 1048576 ]"]);
        let bytes = pipe(&from, &to, reader, writer, &report).await.unwrap();
        assert_eq!(bytes, 1_048_576);
        report.transferred();
        assert!(report.progress.lock().unwrap().snapshot().current.is_none());
    }

    #[tokio::test]
    async fn a_writer_that_fails_is_reported() {
        let from = Site::local("a", "/tmp/a", "x.yml");
        let to = Site::local("b", "/tmp/b", "x.yml");
        let make_sink = || -> Sink { Box::new(|_| {}) };
        let status = |_: Phase, _: &str| {};
        let report = report_for_test(&make_sink, &status, Arc::new(Mutex::new(Vec::new())));
        let mut reader = tokio::process::Command::new("printf");
        reader.arg("hello");
        let mut writer = tokio::process::Command::new("sh");
        writer.args(["-c", "cat >/dev/null; exit 3"]);
        let err = pipe(&from, &to, reader, writer, &report).await.unwrap_err();
        assert!(err.to_string().starts_with("writing on"), "{err}");
    }
}
