//! How a copy tells what it is doing, three ways at once: raw lines (the
//! card's output), the card's phase, and the step list with its bytes (the
//! progress panel). The app wires them to the window, the example prints.

use std::sync::{Arc, Mutex};

use super::progress::Progress;
use crate::job::{LastError, Line, Stream};
use crate::stack::Phase;

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
    pub(super) fn sink_keeping_error(&self) -> (Sink, Arc<Mutex<LastError>>) {
        let mut inner = self.sink();
        let reason = Arc::new(Mutex::new(LastError::default()));
        let kept = reason.clone();
        let sink: Sink = Box::new(move |l: Line| {
            kept.lock().expect("reason lock").note(&l.text);
            inner(l);
        });
        (sink, reason)
    }

    pub(super) fn say(&self, text: &str) {
        self.sink()(line(text));
    }

    /// One planned step begins: the card, the panel and the log all say so.
    pub(super) fn step(&self, phase: Phase, name: &str) {
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

    pub(super) fn transfer(&self, label: &str, total_bytes: Option<u64>) {
        self.progress.lock().expect("progress lock").transfer(label, total_bytes);
    }

    pub fn bytes(&self, more: u64) {
        self.progress.lock().expect("progress lock").bytes(more);
    }

    pub(super) fn transferred(&self) {
        self.progress.lock().expect("progress lock").transferred();
    }
}

fn line(text: &str) -> Line {
    Line { stream: Stream::Stdout, text: text.to_string() }
}

/// Why a Docker command failed: the last error line it printed, or its
/// exit code when it printed none.
pub(super) fn why_it_failed(code: Option<i32>, reason: &Mutex<LastError>) -> String {
    reason.lock().expect("reason lock").explain(code)
}
