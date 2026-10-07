//! The live re-sync: the default excludes, the one-direction mirror every
//! `up` uses, and the folder watcher that re-syncs after a burst of changes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher as _};

use crate::copy::endpoint::Site;
use crate::copy::{folder, Sink};
use crate::job::Line;
use crate::ssh::Ssh;
use crate::stack::Stack;
use crate::store;

const DEFAULT_EXCLUDES: [&str; 4] = [".git", "node_modules", ".DS_Store", ".tmp"];
/// One save or one git command produces a burst of events; wait for the burst to end.
const DEBOUNCE: Duration = Duration::from_millis(300);

pub fn default_excludes() -> Vec<String> {
    DEFAULT_EXCLUDES.iter().map(|s| s.to_string()).collect()
}

/// The result of one mirror: what `copy::folder` reports.
pub use crate::copy::folder::Mirrored as SyncResult;

/// Mirrors the stack's folder to its machine: the live re-sync and every
/// `up` go through here. The copy module owns rsync; this is the one
/// direction the rest of the app needs.
pub async fn run(ssh: &Ssh, alias: &str, stack: &Stack, on_line: impl FnMut(Line) + Send + 'static) -> anyhow::Result<SyncResult> {
    let from = Site::local(&stack.name, &stack.project_dir, &stack.compose_rel);
    let to = Site::machine(&stack.name, &stack.compose_rel, alias, "the machine");
    // The mirror deletes on the machine what is missing here. A folder that
    // lost its compose file (a removed git worktree, a moved project, a
    // folder Docker recreated empty for a bind mount) would wipe the
    // machine's working copy, so it is not sent at all.
    anyhow::ensure!(
        from.exists(ssh).await,
        "{} is no longer in {}, so nothing was sent and the copy on the machine is as it was. If the project moved, choose Change folder in the stack's menu.",
        stack.compose_rel,
        stack.project_dir
    );
    // One caller-supplied sink, shared by every leg the mirror may run.
    let shared = Arc::new(Mutex::new(on_line));
    let make_sink = move || -> Sink {
        let sink = shared.clone();
        Box::new(move |line| (sink.lock().expect("sink lock"))(line))
    };
    folder::mirror(ssh, &store::home_dir(), &from, &to, &stack.excludes, &make_sink).await
}

/// Stops watching when dropped.
pub struct Watcher {
    stopped: Arc<AtomicBool>,
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Calls `on_change` once per burst of file changes under `dir`, ignoring
/// anything inside an excluded folder. The callback runs on the watcher's own
/// thread and may block; changes that arrive meanwhile queue up for one more call.
pub fn watch(dir: PathBuf, excludes: Vec<String>, mut on_change: impl FnMut() + Send + 'static) -> anyhow::Result<Watcher> {
    let (tx, rx) = mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(tx)?;
    watcher.watch(&dir, RecursiveMode::Recursive)?;
    let stopped = Arc::new(AtomicBool::new(false));
    let flag = stopped.clone();
    std::thread::spawn(move || {
        let _keeps_watching = watcher;
        while !flag.load(Ordering::Relaxed) {
            let Ok(event) = rx.recv_timeout(Duration::from_millis(500)) else { continue };
            if !is_relevant(&event, &dir, &excludes) {
                continue;
            }
            let quiet_until = Instant::now() + DEBOUNCE;
            while rx.recv_timeout(quiet_until.saturating_duration_since(Instant::now())).is_ok() {}
            if flag.load(Ordering::Relaxed) {
                break;
            }
            on_change();
        }
    });
    Ok(Watcher { stopped })
}

/// Only the part of the path below the project folder counts: a project that
/// itself lives under a folder named like an exclude is still watched.
fn is_relevant(event: &notify::Result<notify::Event>, root: &Path, excludes: &[String]) -> bool {
    let Ok(event) = event else { return false };
    event.paths.iter().any(|path| {
        let relative = path.strip_prefix(root).unwrap_or(path);
        !is_excluded(relative, excludes)
    })
}

/// The excludes as rsync reads them: a name matches a folder of that name
/// anywhere, `/path` only that path from the project's root.
fn is_excluded(relative: &Path, excludes: &[String]) -> bool {
    excludes.iter().any(|exclude| match exclude.strip_prefix('/') {
        Some(from_root) => relative.starts_with(from_root),
        None => relative.components().any(|part| part.as_os_str() == exclude.as_str()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What happened when a git worktree was removed: Docker recreated one
    /// bind-mounted folder in it, empty, and Start mirrored that to the
    /// machine, deleting the machine's compose file.
    #[tokio::test]
    async fn a_folder_without_its_compose_file_is_never_sent() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("guard-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(folder.join("postgres/init")).unwrap();
        let stack = Stack {
            id: "s1".into(),
            name: "shop".into(),
            machine_id: "m1".into(),
            project_dir: folder.display().to_string(),
            compose_rel: "docker-compose.yml".into(),
            excludes: default_excludes(),
            forward_ports: true,
            live_sync: false,
            port_overrides: Default::default(),
        };
        // A short made-up home: the guard answers before ssh or rsync would run.
        let ssh = Ssh::new(Path::new("/nowhere")).unwrap();
        let Err(refused) = run(&ssh, "dn-nowhere", &stack, |_| {}).await else { panic!("the folder was sent") };
        let said = format!("{refused:#}");
        assert!(said.starts_with("docker-compose.yml is no longer in "), "{said}");
        assert!(said.contains("nothing was sent"), "{said}");
    }

    #[test]
    fn changes_inside_excluded_folders_are_ignored() {
        let excludes = vec!["node_modules".to_string(), ".git".to_string(), ".tmp".to_string()];
        let root = Path::new("/home/.tmp/p");
        let event = |path: &str| Ok(notify::Event::new(notify::EventKind::Any).add_path(PathBuf::from(path)));
        assert!(is_relevant(&event("/home/.tmp/p/src/index.ts"), root, &excludes));
        assert!(!is_relevant(&event("/home/.tmp/p/node_modules/x/index.js"), root, &excludes));
        assert!(!is_relevant(&event("/home/.tmp/p/.git/index"), root, &excludes));
        assert!(!is_relevant(&Err(notify::Error::generic("x")), root, &excludes));
    }

    #[test]
    fn an_anchored_exclude_matches_only_from_the_project_root() {
        let excludes = vec!["/data".to_string(), "cache".to_string()];
        assert!(is_excluded(Path::new("data/db.bin"), &excludes));
        assert!(is_excluded(Path::new("data"), &excludes));
        assert!(!is_excluded(Path::new("app/data/keep.txt"), &excludes), "a data folder deeper down is not the mounted one");
        assert!(!is_excluded(Path::new("database/x"), &excludes), "a longer name is a different folder");
        assert!(is_excluded(Path::new("app/cache/x"), &excludes), "a plain name still matches anywhere");
    }
}
