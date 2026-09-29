//! Holds whatever the platform needs to keep its Docker host awake (on
//! Windows: one `wsl -d Ubuntu -e sleep infinity`). Restarted with a growing
//! delay if it cannot start or keeps dying at once; killed when the app
//! quits on purpose, left alone when the app crashes.

use std::process::Child;
use std::time::{Duration, Instant};

use super::platform::Platform;

/// A keep-alive that ends sooner than this after starting is failing (the
/// distribution missing, WSL broken), not being stopped: it is retried with
/// a growing delay instead of every second.
const SHORT_LIFE: Duration = Duration::from_secs(10);

pub struct KeepAlive {
    child: Option<Child>,
    started_at: Option<Instant>,
    next_try: Instant,
    failures: u32,
    /// The platform said nothing is needed; stop asking.
    not_needed: bool,
}

impl Default for KeepAlive {
    fn default() -> Self {
        Self::new()
    }
}

impl KeepAlive {
    pub fn new() -> Self {
        Self { child: None, started_at: None, next_try: Instant::now(), failures: 0, not_needed: false }
    }

    pub fn alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Called every second; cheap when nothing changed. Returns a line for
    /// the log when starting failed or the last one died at once.
    pub fn tick(&mut self, platform: &dyn Platform) -> Option<String> {
        if self.not_needed {
            return None;
        }
        if self.alive() {
            let lived_long = self.started_at.is_some_and(|at| at.elapsed() >= SHORT_LIFE);
            if lived_long {
                self.failures = 0;
            }
            return None;
        }
        let died_at_once = self.started_at.take().is_some_and(|at| at.elapsed() < SHORT_LIFE);
        let mut note = None;
        if died_at_once {
            self.back_off();
            note = Some("keep-alive ended right after starting; trying again later".to_string());
        }
        if Instant::now() < self.next_try {
            return note;
        }
        match platform.spawn_keepalive() {
            Ok(Some(child)) => {
                self.child = Some(child);
                self.started_at = Some(Instant::now());
                note
            }
            Ok(None) => {
                self.not_needed = true;
                None
            }
            Err(err) => {
                self.back_off();
                Some(format!("keep-alive could not start: {err}"))
            }
        }
    }

    fn back_off(&mut self) {
        self.failures += 1;
        let delay = Duration::from_secs(2u64.saturating_pow(self.failures.min(5)));
        self.next_try = Instant::now() + delay;
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.started_at = None;
    }
}
