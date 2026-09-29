//! A child process whose output arrives line by line while it runs: remote
//! scripts over ssh, rsync, local compose. `Job` streams and can be
//! cancelled; `Output` is what a command left behind once it finished.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Context;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::tools;

/// A line longer than this is cut: a runaway build log must not eat memory.
const MAX_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

/// The last line of output that read like an error, so a failure can say
/// why in the tool's own words rather than only by its exit code.
#[derive(Debug, Default)]
pub struct LastError {
    line: Option<String>,
}

impl LastError {
    pub fn note(&mut self, text: &str) {
        if reads_like_error(text) {
            self.line = Some(text.trim().to_string());
        }
    }

    /// The line, or "exited with code N" when nothing read like an error.
    pub fn explain(&self, code: Option<i32>) -> String {
        self.line.clone().unwrap_or_else(|| format!("exited with code {}", code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())))
    }
}

pub fn reads_like_error(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("error") || lower.contains("failed")
}

/// Lines reach the window in batches: a build can print thousands a
/// second, and a message per line kept the window busy drawing.
const BATCH_EVERY: Duration = Duration::from_millis(100);

/// A sink that hands `send` what arrived every 100 ms while lines come, and
/// the rest once the sink is dropped, which is when its job has ended.
pub fn batched(mut send: impl FnMut(Vec<Line>) + Send + 'static) -> impl FnMut(Line) + Send + 'static {
    let buffer: Arc<Mutex<Vec<Line>>> = Arc::default();
    let flusher = buffer.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(BATCH_EVERY).await;
            // Looked at before taking, so a line pushed just before the sink
            // went is in this batch or the next, never lost.
            let sink_gone = Arc::strong_count(&flusher) == 1;
            let lines = std::mem::take(&mut *flusher.lock().expect("batch lock"));
            if !lines.is_empty() {
                send(lines);
            }
            if sink_gone {
                return;
            }
        }
    });
    move |line| buffer.lock().expect("batch lock").push(line)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Serialize)]
pub struct Line {
    pub stream: Stream,
    pub text: String,
}

/// A child process whose output is delivered line by line while it runs.
/// `cancel` kills it; `wait` gives the exit code.
pub struct Job {
    cancel: watch::Sender<bool>,
    done: JoinHandle<anyhow::Result<Option<i32>>>,
}

/// The part of a job that can be kept in a registry: enough to cancel it and
/// to tell afterwards whether it was cancelled rather than failed.
#[derive(Clone)]
pub struct JobHandle {
    cancel: watch::Sender<bool>,
}

impl JobHandle {
    pub fn cancel(&self) {
        let _ = self.cancel.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.cancel.borrow()
    }

    /// The same job, so a registry entry is removed only by its own job.
    pub fn same(&self, other: &JobHandle) -> bool {
        self.cancel.same_channel(&other.cancel)
    }
}

impl Job {
    pub fn spawn(mut cmd: Command, stdin: Option<String>, on_line: impl FnMut(Line) + Send + 'static) -> anyhow::Result<Self> {
        let stdin_mode = if stdin.is_some() { Stdio::piped() } else { Stdio::null() };
        cmd.stdin(stdin_mode).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
        let child = cmd.spawn().context("spawn process")?;
        let tracked = tools::track(&child);
        let (cancel, cancelled) = watch::channel(false);
        let done = tokio::spawn(async move {
            let mut child = child;
            let script_pipe = match stdin {
                Some(script) => feed_stdin(&mut child, &script).await?,
                None => None,
            };
            let result = drive(child, cancelled, on_line).await;
            drop(script_pipe);
            drop(tracked);
            result
        });
        Ok(Self { cancel, done })
    }

    pub fn handle(&self) -> JobHandle {
        JobHandle { cancel: self.cancel.clone() }
    }

    pub async fn wait(self) -> anyhow::Result<Option<i32>> {
        self.done.await.context("job task")?
    }
}

/// Runs a command to completion with a script on stdin, both streams captured.
pub async fn run_with_stdin(mut cmd: Command, script: &str) -> anyhow::Result<Output> {
    // A caller that gives up (a poll with a timeout) must take the child
    // with it, or a stuck remote command leaves sessions piling up.
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).kill_on_drop(true);
    let mut child = cmd.spawn().context("spawn process")?;
    let script_pipe = feed_stdin(&mut child, script).await?;
    let out = child.wait_with_output().await.context("wait for process")?;
    drop(script_pipe);
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
    })
}

/// Writes the script and hands back the open pipe. It stays open until the
/// process is done or given up on: a remote script ends when its stdin
/// does (`ssh::ends_with_the_connection`), so closing early would end it.
/// A process that ended before reading its script (ssh refused, a bad
/// config) gives a broken pipe here; that is not the error to report, so
/// the caller goes on to collect the process's own output and exit code.
async fn feed_stdin(child: &mut Child, script: &str) -> anyhow::Result<Option<ChildStdin>> {
    let mut stdin = child.stdin.take().context("child has no stdin")?;
    let written = match stdin.write_all(script.as_bytes()).await {
        Ok(()) => stdin.flush().await,
        Err(err) => Err(err),
    };
    match written {
        Ok(()) => Ok(Some(stdin)),
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(None),
        Err(err) => Err(err).context("write script"),
    }
}

/// A line as the pump found it: `redraw` when `\r` ended it, the way progress
/// bars overwrite their line, rather than `\n`.
struct Read {
    line: Line,
    redraw: bool,
}

/// Skips a redraw that repeats the redraw before it. Two equal lines that
/// ended in `\n` are both real: a log printing the same thing twice.
#[derive(Default)]
struct Redraws {
    last: Option<(String, bool)>,
}

impl Redraws {
    fn keeps(&mut self, read: &Read) -> bool {
        let repeat = matches!(&self.last, Some((text, true)) if *text == read.line.text);
        self.last = Some((read.line.text.clone(), read.redraw));
        !repeat
    }
}

async fn drive(mut child: Child, mut cancelled: watch::Receiver<bool>, mut on_line: impl FnMut(Line)) -> anyhow::Result<Option<i32>> {
    let (tx, mut rx) = mpsc::channel::<Read>(256);
    let stdout = tokio::spawn(pump(child.stdout.take(), Stream::Stdout, tx.clone()));
    let stderr = tokio::spawn(pump(child.stderr.take(), Stream::Stderr, tx));

    let mut code = None;
    let mut exited = false;
    let mut redraws = Redraws::default();
    loop {
        tokio::select! {
            read = rx.recv() => match read {
                Some(read) => {
                    if redraws.keeps(&read) {
                        on_line(read.line);
                    }
                }
                // Both pumps are gone, so every byte has been delivered.
                None => break,
            },
            status = child.wait(), if !exited => {
                exited = true;
                code = status.ok().and_then(|s| s.code());
            }
            _ = cancelled.changed(), if !exited => {
                let _ = child.start_kill();
            }
        }
    }
    let _ = stdout.await;
    let _ = stderr.await;
    if !exited {
        code = child.wait().await.ok().and_then(|s| s.code());
    }
    Ok(code)
}

/// Reads bytes, not lines: `\r` also ends a line so progress bars do not pile
/// up, and one enormous line is truncated instead of buffered forever. A
/// line ended by `\r` is held back one byte, because `\r\n` is an ordinary
/// line end, not a redraw.
async fn pump<R: AsyncRead + Unpin>(reader: Option<R>, stream: Stream, tx: mpsc::Sender<Read>) {
    let Some(mut reader) = reader else { return };
    let mut buf = vec![0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    let mut held: Option<String> = None;
    loop {
        let read = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &buf[..read] {
            if let Some(text) = held.take() {
                let plain_line_end = byte == b'\n';
                if send(&tx, stream, text, !plain_line_end).await.is_err() {
                    return;
                }
                if plain_line_end {
                    continue;
                }
            }
            let ends_line = byte == b'\n' || byte == b'\r';
            if ends_line && line.is_empty() {
                continue;
            }
            if ends_line {
                let text = String::from_utf8_lossy(&line).into_owned();
                line.clear();
                if byte == b'\r' {
                    held = Some(text);
                } else if send(&tx, stream, text, false).await.is_err() {
                    return;
                }
            } else if line.len() < MAX_LINE_BYTES {
                line.push(byte);
            } else if line.len() == MAX_LINE_BYTES {
                line.extend_from_slice(b" [line truncated]");
            }
        }
    }
    if let Some(text) = held {
        let _ = send(&tx, stream, text, true).await;
    }
    if !line.is_empty() {
        let text = String::from_utf8_lossy(&line).into_owned();
        let _ = send(&tx, stream, text, false).await;
    }
}

async fn send(tx: &mpsc::Sender<Read>, stream: Stream, text: String, redraw: bool) -> Result<(), mpsc::error::SendError<Read>> {
    tx.send(Read { line: Line { stream, text }, redraw }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[tokio::test]
    async fn a_process_that_ends_before_its_script_reports_its_own_error() {
        // More than a pipe buffer, so writing fails once the process is gone.
        let script = "x".repeat(1 << 20);
        let mut cmd = Command::new("sh");
        cmd.args(["-c", "echo refused >&2; exit 255"]);
        let out = run_with_stdin(cmd, &script).await.expect("the exit code and stderr, not a broken pipe");
        assert_eq!(out.code, Some(255));
        assert_eq!(out.stderr, "refused");
    }

    async fn pumped(bytes: &'static [u8]) -> Vec<(String, bool)> {
        let (tx, mut rx) = mpsc::channel::<Read>(64);
        pump(Some(bytes), Stream::Stdout, tx).await;
        let mut reads = Vec::new();
        while let Some(read) = rx.recv().await {
            reads.push((read.line.text, read.redraw));
        }
        reads
    }

    #[tokio::test]
    async fn only_a_carriage_return_alone_marks_a_redraw() {
        let reads = pumped(b"one\r\ntwo\nbar 10%\rbar 20%\rlast").await;
        assert_eq!(
            reads,
            vec![("one".into(), false), ("two".into(), false), ("bar 10%".into(), true), ("bar 20%".into(), true), ("last".into(), false)]
        );
    }

    #[tokio::test]
    async fn lines_arrive_in_order_and_the_last_ones_after_the_end() {
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<String>>();
        let mut sink = batched(move |lines| {
            let _ = tx.send(lines.into_iter().map(|line| line.text).collect());
        });
        for n in 0..5 {
            sink(Line { stream: Stream::Stdout, text: format!("line {n}") });
        }
        drop(sink);
        let mut received = Vec::new();
        while let Some(batch) = rx.recv().await {
            received.extend(batch);
        }
        assert_eq!(received, (0..5).map(|n| format!("line {n}")).collect::<Vec<_>>());
    }

    #[test]
    fn a_failure_is_explained_in_the_tools_own_words() {
        let mut last = LastError::default();
        assert_eq!(last.explain(Some(1)), "exited with code 1");
        assert_eq!(last.explain(None), "exited with code ?");
        // The lines compose printed when a registry refused an image.
        last.note(" Image quay.io/minio/minio:latest Pulling ");
        last.note("Error response from daemon: unauthorized: access to the requested resource is not authorized");
        last.note(" Container shop-db-1  Created");
        assert_eq!(last.explain(Some(1)), "Error response from daemon: unauthorized: access to the requested resource is not authorized");
        assert!(reads_like_error("target api: failed to solve: process did not complete successfully"));
        assert!(reads_like_error("rsync error: some files/attrs were not transferred (code 23)"));
        assert!(!reads_like_error(" Container shop-db-1  Created"));
    }

    #[test]
    fn a_log_that_repeats_itself_is_shown_every_time() {
        let read = |text: &str, redraw: bool| Read { line: Line { stream: Stream::Stdout, text: text.into() }, redraw };
        let mut redraws = Redraws::default();
        assert!(redraws.keeps(&read("tick", false)));
        assert!(redraws.keeps(&read("tick", false)), "the same line printed again is a real line");
        assert!(redraws.keeps(&read("spin", true)));
        assert!(!redraws.keeps(&read("spin", true)), "a redraw of the same text is noise");
        assert!(!redraws.keeps(&read("spin", false)), "the final print of a redrawn line is that same line");
        assert!(redraws.keeps(&read("spin", false)), "printed again after that, it is a new line");
    }
}
