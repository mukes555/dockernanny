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
        let inside_excluded = relative.components().any(|part| excludes.iter().any(|name| part.as_os_str() == name.as_str()));
        !inside_excluded
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
