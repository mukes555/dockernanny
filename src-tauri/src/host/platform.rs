//! Everything about the sharing role that depends on the operating system
//! lives behind this trait: what this computer has, how to make it ready,
//! where another computer's key goes. The engine, the pairing server and
//! the page are the same on every platform.

use std::process::{Child, Command, Stdio};

use serde::Serialize;

#[derive(Debug, Clone, Default)]
pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn text(&self) -> String {
        self.stdout.trim().to_string()
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            stdout: String::new(),
            stderr: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Missing,
    Unknown,
}

/// One line of the status list: what this computer has or lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub name: &'static str,
    pub state: State,
    pub detail: String,
}

pub fn row(name: &'static str, ok: bool, detail: impl Into<String>) -> Row {
    Row {
        name,
        state: if ok { State::Ok } else { State::Missing },
        detail: detail.into(),
    }
}

/// The network Windows is connected to, and whether it is marked Public
/// (which blocks every inbound rule).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NetworkProfile {
    pub name: String,
    pub public: bool,
}

/// What this computer looks like right now.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Picture {
    pub rows: Vec<Row>,
    /// The account other computers will log in as.
    pub user: Option<String>,
    pub ssh_port: u16,
    pub sshd_listening: bool,
    /// Pairing may only listen once this is true: on Windows a Cancel on
    /// the firewall prompt blocks the port for good.
    pub ready_for_pairing: bool,
    pub total_memory_gb: u32,
    pub network: Option<NetworkProfile>,
}

impl Picture {
    pub fn all_ok(&self) -> bool {
        self.rows.iter().all(|r| r.state != State::Missing)
    }
}

#[derive(Debug, Clone)]
pub enum Outcome {
    Done(String),
    Changed(String),
    Failed(String),
    /// Something the user must do before the rest can continue.
    NeedsUser(String),
}

pub type Say = dyn FnMut(&str) + Send;

/// What the key installer reports back once the key is in place.
#[derive(Debug, Clone)]
pub struct Installed {
    pub user: String,
    pub port: u16,
    pub hostname: String,
    /// `type base64` of this computer's sshd host key, empty when unknown.
    pub host_key: String,
}

/// What the user chose before Set up ran.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct SetupOptions {
    /// Memory Docker may use; 0 means half of what the computer has.
    #[serde(default)]
    pub memory_gb: u32,
    /// Mark this connected Public network as Private (Windows only, with consent).
    #[serde(default)]
    pub make_network_private: Option<String>,
    /// The port the firewall opens for pairing; filled from the settings.
    #[serde(default)]
    pub pairing_port: u16,
}

pub trait Platform: Send + Sync {
    fn os_name(&self) -> &'static str;
    fn probe(&self) -> Picture;
    /// Runs every step in order; stops at the first failure or user action.
    fn setup(&self, options: &SetupOptions, say: &mut Say) -> Vec<(&'static str, Outcome)>;
    /// `type base64` of the sshd host key the other computer should pin.
    fn host_key(&self) -> String;
    fn install_key(&self, key: &str) -> Result<Installed, String>;
    /// A process to hold so the Docker host never idles out; None when the OS needs nothing.
    fn spawn_keepalive(&self) -> Result<Option<Child>, String>;
    /// The peer addresses with an ssh session open on `port` right now.
    fn established_peers(&self, port: u16) -> Vec<String>;
    fn lan_ipv4(&self) -> Vec<String>;
    fn hostname(&self) -> String;
}

/// Runs a program to completion with optional stdin. Never shows a console
/// window on Windows.
pub fn run(program: &str, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)]) -> Output {
    use std::io::Write;
    let mut cmd = Command::new(program);
    cmd.args(args);
    for (key, value) in env {
        cmd.env(key, value);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return Output::failed(format!("could not start {program}: {err}")),
    };
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(out) => Output {
            ok: out.status.success(),
            stdout: decode(&out.stdout),
            stderr: decode(&out.stderr),
        },
        Err(err) => Output::failed(err.to_string()),
    }
}

/// wsl.exe answers in UTF-8 with WSL_UTF8=1, but older builds ignore it and
/// send UTF-16; both are handled, everywhere.
pub fn decode(bytes: &[u8]) -> String {
    let looks_utf16 = bytes.len() >= 2 && bytes[1] == 0 && bytes[0] != 0;
    if looks_utf16 {
        let units: Vec<u16> = bytes.chunks(2).map(|pair| u16::from_le_bytes([pair[0], *pair.get(1).unwrap_or(&0)])).collect();
        return String::from_utf16_lossy(&units).replace('\0', "");
    }
    String::from_utf8_lossy(bytes).replace('\0', "")
}

/// Appends a key to an authorized_keys file, creating the folder with the
/// right permissions; the caller decides which user and how to run it. The
/// key was reduced to `type base64` by the pairing server, so it cannot
/// break out of the quotes.
pub fn authorized_keys_script(user: &str, key: &str) -> String {
    format!(
        "set -e\nhome=$(getent passwd '{user}' 2>/dev/null | cut -d: -f6); [ -n \"$home\" ] || home=$(eval echo ~'{user}')\n\
         install -d -m 700 \"$home/.ssh\"\ntouch \"$home/.ssh/authorized_keys\"\n\
         grep -qF '{key}' \"$home/.ssh/authorized_keys\" || echo '{key}' >> \"$home/.ssh/authorized_keys\"\n\
         chmod 600 \"$home/.ssh/authorized_keys\"\nchown -R '{user}' \"$home/.ssh\" 2>/dev/null || true\necho dockernanny-key-ok\n"
    )
}

/// `type base64` from a `ssh_host_*_key.pub` line; empty when unreadable.
pub fn host_key_from_pub(text: &str) -> String {
    let mut fields = text.split_whitespace();
    match (fields.next(), fields.next()) {
        (Some(key_type), Some(blob)) if key_type.starts_with("ssh-") || key_type.starts_with("ecdsa-") => format!("{key_type} {blob}"),
        _ => String::new(),
    }
}

/// `inet 192.0.2.10 ...` lines from ifconfig, without loopback and link-local.
pub fn parse_ifconfig(text: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.trim_start().strip_prefix("inet ") else { continue };
        let Some(address) = rest.split_whitespace().next() else { continue };
        let address = address.split('/').next().unwrap_or(address);
        let skip = address.starts_with("127.") || address.starts_with("169.254.") || !address.contains('.') || addresses.iter().any(|a| a == address);
        if !skip {
            addresses.push(address.to_string());
        }
    }
    addresses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_and_utf8_both_decode() {
        assert_eq!(decode("Ubuntu\n".as_bytes()), "Ubuntu\n");
        let utf16: Vec<u8> = "Ubuntu".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(decode(&utf16), "Ubuntu");
    }

    #[test]
    fn host_key_keeps_type_and_blob_only() {
        assert_eq!(host_key_from_pub("ssh-ed25519 AAAAC3Nz root@machine\n"), "ssh-ed25519 AAAAC3Nz");
        assert_eq!(host_key_from_pub("garbage"), "");
    }

    #[test]
    fn ifconfig_and_ip_addr_both_parse() {
        let mac = "lo0:\n\tinet 127.0.0.1 netmask 0xff000000\nen0:\n\tinet 192.0.2.10 netmask 0xffffff00\nen5:\n\tinet 169.254.3.4\n";
        assert_eq!(parse_ifconfig(mac), vec!["192.0.2.10".to_string()]);
        let linux = "1: lo\n    inet 127.0.0.1/8 scope host lo\n2: eth0\n    inet 192.0.2.20/24 brd 192.0.2.255 scope global eth0\n    inet6 fe80::1/64 scope link\n";
        assert_eq!(parse_ifconfig(linux), vec!["192.0.2.20".to_string()]);
    }
}
