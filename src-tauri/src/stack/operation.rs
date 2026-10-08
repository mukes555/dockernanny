//! One operation at a time per stack. Start, Stop, Remove containers,
//! Restart and a copy each hold a ticket: a new one ends the one before
//! (its compose command is cancelled), only the latest may write the card's
//! status, and while any holds the stack the poll leaves its phase alone.
//! A slow answer from an older operation can then never overwrite a newer
//! state, and an operation that ends in any way frees the card.
//!
//! A copy is the exception: it cannot be stopped half way (the source may be
//! stopped for it and must be started again), so while one runs the other
//! operations are refused instead.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

use super::{set_status, StackStatus};
use crate::job::JobHandle;
use crate::AppState;

pub const COPY_RUNNING: &str = "a copy of this stack is running; wait for it to finish";

#[derive(Default)]
pub struct Operations {
    next_id: AtomicU64,
    running: Mutex<HashMap<String, Running>>,
    /// When each stack's last operation ended, in ms since the epoch.
    ended_ms: Mutex<HashMap<String, u64>>,
}

struct Running {
    id: u64,
    copy: bool,
    /// The compose command it waits on, cancelled when a newer operation starts.
    job: Option<JobHandle>,
}

impl Operations {
    /// An operation holds the stack; the poll leaves its phase to it.
    pub fn holds(&self, stack_id: &str) -> bool {
        self.running.lock().expect("operations lock").contains_key(stack_id)
    }

    pub fn copying(&self, stack_id: &str) -> bool {
        self.running.lock().expect("operations lock").get(stack_id).is_some_and(|r| r.copy)
    }

    /// A `ps` that started before the stack's last operation ended may show
    /// it mid-change; arriving after the fresh reading taken at the end, it
    /// must not replace it.
    pub fn ended_since(&self, stack_id: &str, started_ms: u64) -> bool {
        let ended = self.ended_ms.lock().expect("ended lock").get(stack_id).copied().unwrap_or(0);
        started_ms < ended
    }

    /// Takes the stack for a new operation and ends the one before, unless that is a copy.
    fn start(&self, stack_id: &str, copy: bool) -> anyhow::Result<u64> {
        let mut running = self.running.lock().expect("operations lock");
        if let Some(current) = running.get(stack_id) {
            anyhow::ensure!(!current.copy, COPY_RUNNING);
            if let Some(job) = &current.job {
                job.cancel();
            }
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        running.insert(stack_id.to_string(), Running { id, copy, job: None });
        Ok(id)
    }

    fn is_current(&self, stack_id: &str, id: u64) -> bool {
        self.running.lock().expect("operations lock").get(stack_id).is_some_and(|r| r.id == id)
    }

    fn attach(&self, stack_id: &str, id: u64, job: JobHandle) {
        let mut running = self.running.lock().expect("operations lock");
        match running.get_mut(stack_id) {
            Some(current) if current.id == id => current.job = Some(job),
            _ => job.cancel(),
        }
    }

    /// Gives the stack back, but only if no newer operation has taken it since.
    fn release(&self, stack_id: &str, id: u64) {
        let mut running = self.running.lock().expect("operations lock");
        let still_mine = running.get(stack_id).is_some_and(|r| r.id == id);
        if still_mine {
            running.remove(stack_id);
            self.ended_ms.lock().expect("ended lock").insert(stack_id.to_string(), super::now_ms());
        }
    }
}

/// The right to change one stack; dropping it gives the stack back.
pub struct Ticket {
    app: AppHandle,
    stack_id: String,
    id: u64,
}

impl Ticket {
    /// A compose operation. It ends the one before; refused while a copy runs.
    pub fn begin(app: &AppHandle, stack_id: &str) -> anyhow::Result<Ticket> {
        let id = app.state::<AppState>().operations.start(stack_id, false)?;
        Ok(Ticket { app: app.clone(), stack_id: stack_id.to_string(), id })
    }

    /// A copy: it ends a compose operation that runs, and holds the stack to the end.
    pub fn begin_copy(app: &AppHandle, stack_id: &str) -> anyhow::Result<Ticket> {
        let id = app.state::<AppState>().operations.start(stack_id, true)?;
        Ok(Ticket { app: app.clone(), stack_id: stack_id.to_string(), id })
    }

    /// False once a newer operation took the stack.
    pub fn is_current(&self) -> bool {
        self.app.state::<AppState>().operations.is_current(&self.stack_id, self.id)
    }

    /// Remembers the compose command this operation waits on, so a newer one
    /// can end it; one handed over after a newer operation began is ended at once.
    pub fn attach(&self, job: JobHandle) {
        self.app.state::<AppState>().operations.attach(&self.stack_id, self.id, job);
    }

    /// Changes the card's status, but only while this is the latest operation.
    pub fn set_status(&self, change: impl FnOnce(&mut StackStatus)) {
        if self.is_current() {
            set_status(&self.app, &self.stack_id, change);
        }
    }
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.app.state::<AppState>().operations.release(&self.stack_id, self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_newer_operation_takes_the_stack_and_the_older_one_cannot_give_it_back() {
        let operations = Operations::default();
        let first = operations.start("s1", false).unwrap();
        let second = operations.start("s1", false).unwrap();
        assert!(!operations.is_current("s1", first), "the first may no longer write the card");
        assert!(operations.is_current("s1", second));
        operations.release("s1", first);
        assert!(operations.holds("s1"), "the first ending must not free the stack the second holds");
        operations.release("s1", second);
        assert!(!operations.holds("s1"));
    }

    #[test]
    fn nothing_starts_while_a_copy_runs() {
        let operations = Operations::default();
        let copy = operations.start("s1", true).unwrap();
        let refused = operations.start("s1", false).unwrap_err();
        assert_eq!(refused.to_string(), COPY_RUNNING);
        assert!(operations.copying("s1") && operations.is_current("s1", copy));
        assert!(operations.start("s2", false).is_ok(), "other stacks are not affected");
    }

    #[test]
    fn a_reading_from_before_an_operation_ended_is_stale() {
        let operations = Operations::default();
        assert!(!operations.ended_since("s1", 0), "no operation yet: every reading counts");
        let reading_started = super::super::now_ms() - 1;
        let id = operations.start("s1", false).unwrap();
        operations.release("s1", id);
        assert!(operations.ended_since("s1", reading_started), "started before the end: stale");
        assert!(!operations.ended_since("s1", super::super::now_ms()), "started after the end: fresh");
        assert!(!operations.ended_since("s2", reading_started), "other stacks are not affected");
    }
}
