//! What a copy is doing, as data the window can draw: the steps planned up
//! front and their states, the transfer in flight with its bytes and speed,
//! the last lines of output. Every change goes to one `publish` callback; the
//! app sends it to the window as `copy:progress`, the headless example prints.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::EndpointRef;
use crate::stack::now_ms;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Pending,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
pub struct Step {
    pub name: String,
    pub state: StepState,
}

/// One stream of bytes on its way: a volume, an image or a container path.
#[derive(Debug, Clone, Serialize)]
pub struct Transfer {
    pub label: String,
    pub bytes: u64,
    /// The plan's size for a volume; `docker cp` and images have none.
    pub total_bytes: Option<u64>,
    pub per_second: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CopyProgress {
    /// The stack record whose card shows the copy.
    pub stack_id: String,
    pub name: String,
    pub from: String,
    pub to: String,
    pub destination: EndpointRef,
    pub steps: Vec<Step>,
    pub current: Option<Transfer>,
    pub lines: Vec<String>,
    pub started_ms: u64,
    pub finished_ms: Option<u64>,
    /// The summary when it went well, the error when it did not.
    pub outcome: Option<String>,
    pub failed: bool,
}

const KEPT_LINES: usize = 12;
/// Bytes and lines arrive faster than a window can draw; state changes
/// always go out, the rest at most this often.
const PUBLISH_EVERY: Duration = Duration::from_millis(200);
/// The speed is measured over this long, so it does not flicker.
const RATE_WINDOW: Duration = Duration::from_secs(1);

pub type Publish = Box<dyn FnMut(&CopyProgress) + Send>;

/// Shared between the copy and every sink that feeds it lines.
pub type Progress = Arc<Mutex<Tracker>>;

pub struct Tracker {
    progress: CopyProgress,
    publish: Publish,
    last_publish: Instant,
    rate_since: Instant,
    rate_bytes: u64,
}

impl Tracker {
    pub fn new(stack_id: &str, name: &str, from: &str, to: &str, destination: EndpointRef, publish: Publish) -> Tracker {
        let now = Instant::now();
        Tracker {
            progress: CopyProgress {
                stack_id: stack_id.to_string(),
                name: name.to_string(),
                from: from.to_string(),
                to: to.to_string(),
                destination,
                steps: Vec::new(),
                current: None,
                lines: Vec::new(),
                started_ms: now_ms(),
                finished_ms: None,
                outcome: None,
                failed: false,
            },
            publish,
            last_publish: now - PUBLISH_EVERY,
            rate_since: now,
            rate_bytes: 0,
        }
    }

    pub fn shared(self) -> Progress {
        Arc::new(Mutex::new(self))
    }

    pub fn snapshot(&self) -> CopyProgress {
        self.progress.clone()
    }

    /// The whole list, in order. Steps already known keep their state, so
    /// the list can grow once the copy has looked at what there is to carry.
    pub fn set_steps(&mut self, names: Vec<String>) {
        let known = std::mem::take(&mut self.progress.steps);
        self.progress.steps = names
            .into_iter()
            .map(|name| {
                let state = known.iter().find(|s| s.name == name).map(|s| s.state).unwrap_or(StepState::Pending);
                Step { name, state }
            })
            .collect();
        self.send(true);
    }

    /// The step that runs now; the one before it is done. A name the list
    /// did not plan is added in place, so nothing the copy does goes unseen.
    pub fn start(&mut self, name: &str) {
        self.close_running(StepState::Done);
        match self.progress.steps.iter_mut().find(|s| s.name == name && s.state == StepState::Pending) {
            Some(step) => step.state = StepState::Running,
            None => {
                let at = self.progress.steps.iter().position(|s| s.state == StepState::Pending).unwrap_or(self.progress.steps.len());
                self.progress.steps.insert(at, Step { name: name.to_string(), state: StepState::Running });
            }
        }
        self.send(true);
    }

    /// The running step failed, but the copy has something left to do (start
    /// the source again) before it reports the failure.
    pub fn fail_step(&mut self) {
        self.close_running(StepState::Failed);
        self.send(true);
    }

    pub fn done_step(&mut self) {
        self.close_running(StepState::Done);
        self.send(true);
    }

    pub fn skip(&mut self, name: &str) {
        if let Some(step) = self.progress.steps.iter_mut().find(|s| s.name == name && s.state == StepState::Pending) {
            step.state = StepState::Skipped;
            self.send(true);
        }
    }

    pub fn transfer(&mut self, label: &str, total_bytes: Option<u64>) {
        self.progress.current = Some(Transfer { label: label.to_string(), bytes: 0, total_bytes, per_second: 0 });
        self.rate_since = Instant::now();
        self.rate_bytes = 0;
        self.send(true);
    }

    pub fn bytes(&mut self, more: u64) {
        let Some(current) = self.progress.current.as_mut() else { return };
        current.bytes += more;
        let elapsed = self.rate_since.elapsed();
        let window_rate = ((current.bytes - self.rate_bytes) as f64 / elapsed.as_secs_f64().max(0.001)) as u64;
        if elapsed >= RATE_WINDOW {
            current.per_second = window_rate;
            self.rate_since = Instant::now();
            self.rate_bytes = current.bytes;
        } else if current.per_second == 0 && elapsed >= PUBLISH_EVERY {
            // A short transfer never fills a whole window; show what there is.
            current.per_second = window_rate;
        }
        self.send(false);
    }

    pub fn transferred(&mut self) {
        self.progress.current = None;
        self.send(true);
    }

    pub fn line(&mut self, text: &str) {
        let text = text.trim_end();
        if text.is_empty() {
            return;
        }
        self.progress.lines.push(text.to_string());
        if self.progress.lines.len() > KEPT_LINES {
            self.progress.lines.remove(0);
        }
        self.send(false);
    }

    pub fn finish(&mut self, outcome: &str) {
        self.close_running(StepState::Done);
        self.progress.current = None;
        self.progress.finished_ms = Some(now_ms());
        self.progress.outcome = Some(outcome.to_string());
        self.send(true);
    }

    pub fn fail(&mut self, error: &str) {
        self.close_running(StepState::Failed);
        for step in self.progress.steps.iter_mut().filter(|s| s.state == StepState::Pending) {
            step.state = StepState::Skipped;
        }
        self.progress.current = None;
        self.progress.finished_ms = Some(now_ms());
        self.progress.outcome = Some(error.to_string());
        self.progress.failed = true;
        self.send(true);
    }

    fn close_running(&mut self, as_state: StepState) {
        for step in self.progress.steps.iter_mut().filter(|s| s.state == StepState::Running) {
            step.state = as_state;
        }
    }

    fn send(&mut self, force: bool) {
        if !force && self.last_publish.elapsed() < PUBLISH_EVERY {
            return;
        }
        self.last_publish = Instant::now();
        (self.publish)(&self.progress);
    }
}

pub fn megabytes(bytes: u64) -> String {
    if bytes >= 1_000_000_000 {
        return format!("{:.2} GB", bytes as f64 / 1e9);
    }
    format!("{:.0} MB", bytes as f64 / 1e6)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tracker(seen: Arc<Mutex<Vec<CopyProgress>>>) -> Tracker {
        let publish: Publish = Box::new(move |p| seen.lock().unwrap().push(p.clone()));
        Tracker::new("s1", "shop", "this computer", "studio", EndpointRef::Machine { machine_id: "m1".into() }, publish)
    }

    #[test]
    fn steps_move_in_order_and_keep_their_states_when_the_list_grows() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut t = tracker(seen.clone());
        t.set_steps(vec!["look".into()]);
        t.start("look");
        t.set_steps(vec!["look".into(), "folder".into(), "start".into()]);
        assert_eq!(t.snapshot().steps[0].state, StepState::Running);
        t.start("folder");
        let steps = t.snapshot().steps;
        assert_eq!(steps[0].state, StepState::Done);
        assert_eq!(steps[1].state, StepState::Running);
        assert_eq!(steps[2].state, StepState::Pending);
        t.finish("2 of 2 services up");
        let done = t.snapshot();
        assert!(done.steps.iter().take(2).all(|s| s.state == StepState::Done));
        assert!(done.finished_ms.is_some() && !done.failed);
        assert!(seen.lock().unwrap().len() >= 5, "every state change is published");
    }

    #[test]
    fn an_unplanned_step_is_shown_in_place_and_a_failure_skips_the_rest() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut t = tracker(seen);
        t.set_steps(vec!["folder".into(), "start".into()]);
        t.start("folder");
        t.start("surprise");
        let names: Vec<_> = t.snapshot().steps.iter().map(|s| s.name.clone()).collect();
        assert_eq!(names, vec!["folder", "surprise", "start"]);
        t.fail("rsync exited with code 23");
        let steps = t.snapshot().steps;
        assert_eq!(steps[1].state, StepState::Failed);
        assert_eq!(steps[2].state, StepState::Skipped);
        assert!(t.snapshot().failed);
    }

    #[test]
    fn a_step_that_fails_before_the_cleanup_keeps_the_blame() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut t = tracker(seen);
        t.set_steps(vec!["copy".into(), "restart".into(), "check".into()]);
        t.start("copy");
        t.fail_step();
        t.start("restart");
        t.done_step();
        t.fail("writing on studio failed");
        let states: Vec<_> = t.snapshot().steps.iter().map(|s| s.state).collect();
        assert_eq!(states, vec![StepState::Failed, StepState::Done, StepState::Skipped]);
    }

    #[test]
    fn bytes_are_counted_and_lines_are_capped() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let mut t = tracker(seen);
        t.transfer("volume pgdata", Some(1000));
        t.bytes(300);
        t.bytes(300);
        assert_eq!(t.snapshot().current.as_ref().unwrap().bytes, 600);
        t.transferred();
        assert!(t.snapshot().current.is_none());
        for i in 0..20 {
            t.line(&format!("line {i}"));
        }
        let lines = t.snapshot().lines;
        assert_eq!(lines.len(), KEPT_LINES);
        assert_eq!(lines.last().unwrap(), "line 19");
        assert_eq!(megabytes(412_000_000), "412 MB");
        assert_eq!(megabytes(2_500_000_000), "2.50 GB");
    }
}
