//! Holds whatever the platform needs to keep its Docker host awake (on
//! Windows: one `wsl -d Ubuntu -e sleep infinity`). Restarted with a growing
//! delay if it dies; killed when the app quits on purpose, left alone when
//! the app crashes.

use std::process::Child;
use std::time::{Duration, Instant};

use super::platform::Platform;

pub struct KeepAlive {
    child: Option<Child>,
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
        Self { child: None, next_try: Instant::now(), failures: 0, not_needed: false }
    }

    pub fn alive(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Called every second; cheap when nothing changed. Returns a line for
    /// the log when starting failed.
    pub fn tick(&mut self, platform: &dyn Platform) -> Option<String> {
        if self.not_needed || self.alive() || Instant::now() < self.next_try {
            return None;
        }
        match platform.spawn_keepalive() {
            Ok(Some(child)) => {
                self.child = Some(child);
                self.failures = 0;
                None
            }
            Ok(None) => {
                self.not_needed = true;
                None
            }
            Err(err) => {
                self.failures += 1;
                let delay = Duration::from_secs(2u64.saturating_pow(self.failures.min(5)));
                self.next_try = Instant::now() + delay;
                Some(format!("keep-alive could not start: {err}"))
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
