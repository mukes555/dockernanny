//! A child process whose output arrives line by line while it runs: remote
//! scripts over ssh, rsync, local compose. `Job` streams and can be
//! cancelled; `Output` is what a command left behind once it finished.

use std::process::Stdio;

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
}

impl Job {
    pub fn spawn(mut cmd: Command, stdin: Option<String>, on_line: impl FnMut(Line) + Send + 'static) -> anyhow::Result<Self> {
        let stdin_mode = if stdin.is_some() { Stdio::piped() } else { Stdio::null() };
        cmd.stdin(stdin_mode)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let child = cmd.spawn().context("spawn process")?;
        tools::track(&child);
        let (cancel, cancelled) = watch::channel(false);
        let done = tokio::spawn(async move {
            let mut child = child;
            let pid = child.id();
            let script_pipe = match stdin {
                Some(script) => Some(feed_stdin(&mut child, &script).await?),
                None => None,
            };
            let result = drive(child, cancelled, on_line).await;
            drop(script_pipe);
            tools::untrack(pid);
            result
        });
        Ok(Self { cancel, done })
    }

    pub fn handle(&self) -> JobHandle {
        JobHandle {
            cancel: self.cancel.clone(),
        }
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
async fn feed_stdin(child: &mut Child, script: &str) -> anyhow::Result<ChildStdin> {
    let mut stdin = child.stdin.take().context("child has no stdin")?;
    stdin.write_all(script.as_bytes()).await.context("write script")?;
    stdin.flush().await.context("write script")?;
    Ok(stdin)
}

async fn drive(mut child: Child, mut cancelled: watch::Receiver<bool>, mut on_line: impl FnMut(Line)) -> anyhow::Result<Option<i32>> {
    let (tx, mut rx) = mpsc::channel::<Line>(256);
    let stdout = tokio::spawn(pump(child.stdout.take(), Stream::Stdout, tx.clone()));
    let stderr = tokio::spawn(pump(child.stderr.take(), Stream::Stderr, tx));

    let mut code = None;
    let mut exited = false;
    // Progress output redraws the same line with `\r`; showing it once is enough.
    let mut last_text = String::new();
    loop {
        tokio::select! {
            line = rx.recv() => match line {
                Some(line) => {
                    if line.text != last_text {
                        last_text = line.text.clone();
                        on_line(line);
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
/// up, and one enormous line is truncated instead of buffered forever.
async fn pump<R: AsyncRead + Unpin>(reader: Option<R>, stream: Stream, tx: mpsc::Sender<Line>) {
    let Some(mut reader) = reader else { return };
    let mut buf = vec![0u8; 8192];
    let mut line: Vec<u8> = Vec::new();
    loop {
        let read = match reader.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        for &byte in &buf[..read] {
            let ends_line = byte == b'\n' || byte == b'\r';
            if ends_line {
                if line.is_empty() {
                    continue;
                }
                let text = String::from_utf8_lossy(&line).into_owned();
                line.clear();
                if tx.send(Line { stream, text }).await.is_err() {
                    return;
                }
            } else if line.len() < MAX_LINE_BYTES {
                line.push(byte);
            } else if line.len() == MAX_LINE_BYTES {
                line.extend_from_slice(b" [line truncated]");
            }
        }
    }
    if !line.is_empty() {
        let text = String::from_utf8_lossy(&line).into_owned();
        let _ = tx.send(Line { stream, text }).await;
    }
}
