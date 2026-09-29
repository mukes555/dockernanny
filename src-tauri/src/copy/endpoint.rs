//! Where a stack lives: this computer (its Docker, a local folder) or a
//! machine (its Docker over ssh, `~/.dockernanny/<name>` there). Every step
//! of a copy asks a `Site` for a Docker or compose process and never cares
//! which of the two it is talking to.

use std::process::Stdio;

use anyhow::Context;
use tokio::process::Command;

use crate::job::{Job, Line, Output};
use crate::ssh::Ssh;
use crate::stack::{compose_script, shell_quote};
use crate::tools;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Local,
    Machine { alias: String },
}

/// A stack as it exists at one endpoint: the compose project name there and
/// its folder.
#[derive(Debug, Clone)]
pub struct Site {
    pub endpoint: Endpoint,
    /// The compose project name (`-p`), also the volume prefix.
    pub name: String,
    /// Absolute on this computer; relative to the home folder on a machine.
    pub dir: String,
    pub compose_rel: String,
    /// "this computer" or the machine's name, for messages.
    pub label: String,
}

impl Site {
    pub fn local(name: &str, dir: &str, compose_rel: &str) -> Site {
        Site {
            endpoint: Endpoint::Local,
            name: name.to_string(),
            dir: dir.trim_end_matches('/').to_string(),
            compose_rel: compose_rel.to_string(),
            label: "this computer".into(),
        }
    }

    pub fn machine(name: &str, compose_rel: &str, alias: &str, label: &str) -> Site {
        Site {
            endpoint: Endpoint::Machine { alias: alias.to_string() },
            name: name.to_string(),
            dir: format!(".dockernanny/{name}"),
            compose_rel: compose_rel.to_string(),
            label: label.to_string(),
        }
    }

    pub fn is_local(&self) -> bool {
        self.endpoint == Endpoint::Local
    }

    pub fn compose_file(&self) -> String {
        format!("{}/{}", self.dir, self.compose_rel)
    }

    /// The compose command for a machine's login shell; local sites use argv.
    fn compose_text(&self, args: &str) -> String {
        compose_script(&self.dir, &self.compose_rel, &self.name, args)
    }

    fn compose_argv(&self, args: &str) -> Vec<String> {
        let mut argv = vec!["compose".to_string(), "-f".into(), self.compose_file(), "-p".into(), self.name.clone()];
        argv.extend(args.split_whitespace().map(str::to_string));
        argv
    }

    /// Every container of the project with its state, `compose ps --all`; an
    /// error when compose could not answer, which is not the same as none.
    pub async fn services(&self, ssh: &Ssh) -> anyhow::Result<Vec<crate::compose::ServiceState>> {
        let out = self.compose_output(ssh, "ps --all --format json").await?;
        anyhow::ensure!(out.ok(), "compose ps on {} failed: {}", self.label, crate::machine::first_line(&out.stderr));
        Ok(crate::compose::parse_ps(&out.stdout))
    }

    /// One compose command to completion, both streams captured.
    pub async fn compose_output(&self, ssh: &Ssh, args: &str) -> anyhow::Result<Output> {
        match &self.endpoint {
            Endpoint::Local => {
                let argv = self.compose_argv(args);
                let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                local_output(local_docker(&refs)).await
            }
            Endpoint::Machine { alias } => ssh.run(alias, &self.compose_text(args)).await,
        }
    }

    /// One compose command with its output streamed line by line.
    pub fn compose_job(&self, ssh: &Ssh, args: &str, on_line: impl FnMut(Line) + Send + 'static) -> anyhow::Result<Job> {
        match &self.endpoint {
            Endpoint::Local => {
                let argv = self.compose_argv(args);
                let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
                Job::spawn(local_docker(&refs), None, on_line)
            }
            Endpoint::Machine { alias } => ssh.job(alias, &self.compose_text(args), on_line),
        }
    }

    /// Whether the compose file is there.
    pub async fn exists(&self, ssh: &Ssh) -> bool {
        match &self.endpoint {
            Endpoint::Local => std::path::Path::new(&self.compose_file()).is_file(),
            Endpoint::Machine { alias } => {
                ssh.run(alias, &format!("test -f {}", shell_quote(&self.compose_file()))).await.map(|out| out.ok()).unwrap_or(false)
            }
        }
    }
}

impl Endpoint {
    /// A `docker` process whose stdin or stdout carries a stream. Machine
    /// commands go through `sh -lc`, so every argument is checked first.
    pub fn docker(&self, ssh: &Ssh, args: &[&str]) -> anyhow::Result<Command> {
        match self {
            Endpoint::Local => Ok(local_docker(args)),
            Endpoint::Machine { alias } => ssh.piped_command(alias, &format!("docker {}", shell_join(args)?)),
        }
    }

    /// A `docker` command to completion. The machine form is a script on
    /// stdin with every argument quoted, so templates like `{{json .Mounts}}`
    /// need no restriction.
    pub async fn docker_output(&self, ssh: &Ssh, args: &[&str]) -> anyhow::Result<Output> {
        match self {
            Endpoint::Local => local_output(local_docker(args)).await,
            Endpoint::Machine { alias } => {
                let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
                ssh.run(alias, &format!("docker {}", quoted.join(" "))).await
            }
        }
    }

    /// A `docker` command with its output streamed line by line.
    pub fn docker_job(&self, ssh: &Ssh, args: &[&str], on_line: impl FnMut(Line) + Send + 'static) -> anyhow::Result<Job> {
        match self {
            Endpoint::Local => Job::spawn(local_docker(args), None, on_line),
            Endpoint::Machine { alias } => {
                let quoted: Vec<String> = args.iter().map(|a| shell_quote(a)).collect();
                ssh.job(alias, &format!("docker {}", quoted.join(" ")), on_line)
            }
        }
    }

    /// A shell script to completion: `sh` here (inside WSL on Windows), the
    /// login shell there.
    pub async fn run_script(&self, ssh: &Ssh, script: &str) -> anyhow::Result<Output> {
        match self {
            Endpoint::Local => {
                let mut cmd = tools::unix("sh");
                cmd.arg("-c").arg(script);
                local_output(cmd).await
            }
            Endpoint::Machine { alias } => ssh.run(alias, script).await,
        }
    }
}

/// Docker on this computer, whatever context the user's shell has selected.
/// Native on every OS: Docker Desktop on Windows reads Windows paths.
pub fn local_docker(args: &[&str]) -> Command {
    let mut cmd = tools::native("docker");
    cmd.args(args).env("DOCKER_CONTEXT", "default");
    cmd
}

async fn local_output(mut cmd: Command) -> anyhow::Result<Output> {
    let out = cmd.stdin(Stdio::null()).output().await.context("run a local command")?;
    Ok(Output {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).trim_end().to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).trim_end().to_string(),
    })
}

/// One line for the machine's `sh -lc '...'`: arguments with a space or a
/// shell character are double-quoted; anything that could escape the quotes
/// is refused rather than escaped.
pub fn shell_join(args: &[&str]) -> anyhow::Result<String> {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        let breaks_out = arg.contains(['\'', '"', '\\', '$', '`', '\n']);
        anyhow::ensure!(!breaks_out, "argument {arg:?} cannot be sent to the machine");
        let needs_quotes = arg.is_empty()
            || arg.contains(|c: char| {
                c.is_whitespace() || matches!(c, '&' | '|' | ';' | '<' | '>' | '(' | ')' | '*' | '?' | '[' | ']' | '{' | '}' | '~' | '#')
            });
        parts.push(if needs_quotes { format!("\"{arg}\"") } else { arg.to_string() });
    }
    Ok(parts.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_join_quotes_what_needs_it_and_refuses_escapes() {
        assert_eq!(
            shell_join(&["run", "--rm", "-v", "x:/to", "alpine", "sh", "-c", "find /to -mindepth 1 -delete && tar xz -C /to"]).unwrap(),
            "run --rm -v x:/to alpine sh -c \"find /to -mindepth 1 -delete && tar xz -C /to\""
        );
        assert_eq!(shell_join(&["cp", "-a", "-", "shop-db-1:/var/lib"]).unwrap(), "cp -a - shop-db-1:/var/lib");
        assert!(shell_join(&["x'y"]).is_err());
        assert!(shell_join(&["$(id)"]).is_err());
        assert!(shell_join(&["a\nb"]).is_err());
    }

    #[test]
    fn sites_know_their_folders_and_commands() {
        let machine = Site::machine("shop", "docker-compose.yml", "dn-1234", "studio");
        assert_eq!(machine.dir, ".dockernanny/shop");
        assert_eq!(machine.compose_text("ps -a"), "cd '.dockernanny/shop' && docker compose -f 'docker-compose.yml' -p 'shop' ps -a");
        let local = Site::local("shop", "/home/alex/projects/shop/", "compose.yml");
        assert_eq!(local.compose_file(), "/home/alex/projects/shop/compose.yml");
        assert_eq!(local.compose_argv("up -d"), vec!["compose", "-f", "/home/alex/projects/shop/compose.yml", "-p", "shop", "up", "-d"]);
    }
}
