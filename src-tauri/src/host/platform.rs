//! Everything about the sharing role that depends on the operating system
//! lives behind this trait: what this computer has, how to make it ready,
//! where another computer's key goes. The engine, the pairing server and
//! the page are the same on every platform.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::Serialize;

/// How long one of the probe's commands may take. Probes, pairing and Quit
/// all wait on the sharing role's one thread, so a stuck wsl.exe, PowerShell
/// or `docker version` (a wedged Docker Desktop) must not hold it forever.
pub const CHECK_LIMIT: Duration = Duration::from_secs(60);
/// Set up installs WSL, packages and Docker, and can wait for the user's
/// password prompt: minutes, but still not forever.
pub const SETUP_LIMIT: Duration = Duration::from_secs(30 * 60);

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
        Self { ok: false, stdout: String::new(), stderr: message.into() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Ok,
    Missing,
    Unknown,
    /// All installed; the user restarts the computer once to finish.
    Restart,
}

/// One line of the status list: what this computer has or lacks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    pub name: &'static str,
    pub state: State,
    pub detail: String,
}

pub fn row(name: &'static str, ok: bool, detail: impl Into<String>) -> Row {
    Row { name, state: if ok { State::Ok } else { State::Missing }, detail: detail.into() }
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
    /// Memory Docker may use, when the user chose it. 0 keeps the memory line
    /// already in .wslconfig, and writes half of the computer's only when there is none.
    #[serde(default)]
    pub memory_gb: u32,
    /// Mark this connected Public network as Private (Windows only, with consent).
    #[serde(default)]
    pub make_network_private: Option<String>,
    /// Change the power settings so the computer stays awake plugged in
    /// with the lid closed (Windows only; the user can say no).
    #[serde(default)]
    pub keep_awake: bool,
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
/// Runs a command to its end, or until `limit` has passed; then it is killed
/// and the answer says so. Both streams are read on their own threads, so a
/// chatty command cannot block on a full pipe while this waits.
pub fn run(program: &str, args: &[&str], stdin: Option<&str>, env: &[(&str, &str)], limit: Duration) -> Output {
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
    cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(err) => return Output::failed(format!("could not start {program}: {err}")),
    };
    if let (Some(text), Some(mut pipe)) = (stdin, child.stdin.take()) {
        let _ = pipe.write_all(text.as_bytes());
    }
    let stdout = read_on_a_thread(child.stdout.take());
    let stderr = read_on_a_thread(child.stderr.take());

    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                // The readers are left behind: a grandchild may still hold the pipes.
                return Output::failed(format!("{program} did not finish within {} s", limit.as_secs()));
            }
            Err(err) => return Output::failed(err.to_string()),
        }
    };
    let stdout = stdout.join().unwrap_or_default();
    let stderr = stderr.join().unwrap_or_default();
    Output { ok: status.success(), stdout: decode(&stdout), stderr: decode(&stderr) }
}

fn read_on_a_thread(pipe: Option<impl Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
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

/// The account a paired key goes to, checked with the rule the other computer
/// applies to the answer, before anything is written: a name it would refuse
/// must not leave a key behind.
pub fn checked_user(user: String) -> Result<String, String> {
    if crate::pairing::valid_user(&user) {
        return Ok(user);
    }
    Err(format!("the account name {user:?} cannot be used for ssh by dockerNanny"))
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
        // sk- keys live on a hardware security key (FIDO); they are ssh keys too.
        (Some(key_type), Some(blob)) if key_type.starts_with("ssh-") || key_type.starts_with("ecdsa-") || key_type.starts_with("sk-") => {
            format!("{key_type} {blob}")
        }
        _ => String::new(),
    }
}

/// The address after `inet` in ifconfig, `ip addr` or `ip -o addr` output
/// (the last puts it mid-line), without loopback and link-local.
pub fn parse_ifconfig(text: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    for line in text.lines() {
        let Some(address) = line.split_whitespace().skip_while(|t| *t != "inet").nth(1) else { continue };
        let address = address.split('/').next().unwrap_or(address);
        let skip = address.starts_with("127.")
            || address.starts_with("169.254.")
            || !address.contains('.')
            || addresses.iter().any(|a| a == address);
        if !skip {
            addresses.push(address.to_string());
        }
    }
    addresses
}

/// One of the app's Windows firewall rules, as far as it can be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    Missing,
    /// There, opening this port; None when the port could not be read.
    Open(Option<u16>),
}

impl Rule {
    /// Whether it opens `port`. A rule whose port cannot be read is trusted,
    /// so an unreadable answer never makes Set up ask for admin rights again.
    pub fn opens(&self, port: u16) -> bool {
        match self {
            Rule::Missing => false,
            Rule::Open(None) => true,
            Rule::Open(Some(open)) => *open == port,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FirewallRules {
    pub ssh: Rule,
    pub pairing: Rule,
}

/// A `dockerNanny SSH=2222` line from the rules query; an empty value means
/// there is no such rule, a value that is not one port means it is unreadable.
pub fn parse_rule(text: &str, name: &str) -> Rule {
    let prefix = format!("{name}=");
    let Some(value) = text.lines().find_map(|line| line.trim().strip_prefix(&prefix)) else { return Rule::Missing };
    if value.trim().is_empty() {
        return Rule::Missing;
    }
    Rule::Open(value.trim().parse().ok())
}

/// `   IPv4 Address. . . . . . . . . . . : 192.0.2.15` lines from Windows'
/// ipconfig, skipping the virtual adapters that only matter to WSL itself.
pub fn parse_ipconfig(text: &str) -> Vec<String> {
    let mut addresses = Vec::new();
    let mut in_virtual_adapter = false;
    for line in text.lines() {
        let is_adapter_header = !line.starts_with(' ') && line.contains("adapter");
        if is_adapter_header {
            in_virtual_adapter =
                line.contains("vEthernet") || line.contains("Hyper-V") || line.contains("VirtualBox") || line.contains("VMware");
            continue;
        }
        if in_virtual_adapter || !line.contains("IPv4") {
            continue;
        }
        let Some(address) = line.rsplit(':').next().map(str::trim) else { continue };
        let address = address.trim_end_matches("(Preferred)").trim();
        let usable = address.contains('.') && !address.starts_with("127.") && !address.starts_with("169.254.");
        if usable && !addresses.iter().any(|a| a == address) {
            addresses.push(address.to_string());
        }
    }
    addresses
}

/// This computer's IPv4 addresses on its networks, whatever the OS.
pub fn lan_addresses() -> Vec<String> {
    let text =
        |program: &str, args: &[&str]| decode(&crate::tools::native_std(program).args(args).output().map(|o| o.stdout).unwrap_or_default());
    if cfg!(windows) {
        return parse_ipconfig(&text("ipconfig", &[]));
    }
    // Recent Linux distributions ship `ip` and no longer ifconfig.
    let from_ifconfig = parse_ifconfig(&text("ifconfig", &[]));
    if !from_ifconfig.is_empty() {
        return from_ifconfig;
    }
    parse_ifconfig(&text("ip", &["-4", "-o", "addr"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn a_command_that_runs_too_long_is_ended_and_says_so() {
        let started = Instant::now();
        let out = run("sleep", &["30"], None, &[], Duration::from_millis(300));
        assert!(!out.ok);
        assert!(out.stderr.contains("did not finish"), "{}", out.stderr);
        assert!(started.elapsed() < Duration::from_secs(5), "it must not wait for the command");

        let quick = run("sh", &["-c", "echo hi; echo there >&2"], None, &[], CHECK_LIMIT);
        assert!(quick.ok);
        assert_eq!((quick.text(), quick.stderr.trim()), ("hi".to_string(), "there"));
    }

    #[test]
    fn firewall_rules_read_from_one_query() {
        let text = "dockerNanny SSH=2222\r\ndockerNanny Pair=\r\n";
        assert_eq!(parse_rule(text, "dockerNanny SSH"), Rule::Open(Some(2222)));
        assert_eq!(parse_rule(text, "dockerNanny Pair"), Rule::Missing);
        assert_eq!(parse_rule("dockerNanny SSH=2222,47433\n", "dockerNanny SSH"), Rule::Open(None));
        assert!(Rule::Open(None).opens(2200), "an unreadable port is trusted");
        assert!(!Rule::Open(Some(2222)).opens(2200));
        assert!(!Rule::Missing.opens(2222));
    }

    #[test]
    fn ipconfig_skips_virtual_adapters() {
        let text = "Ethernet adapter vEthernet (WSL):\n\n   IPv4 Address. . . . . . . . . . . : 172.20.0.1\n\nWireless LAN adapter Wi-Fi:\n\n   IPv4 Address. . . . . . . . . . . : 192.0.2.15(Preferred)\n   Autoconfiguration IPv4 Address. . : 169.254.1.1\n";
        assert_eq!(parse_ipconfig(text), vec!["192.0.2.15".to_string()]);
    }

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
        assert_eq!(host_key_from_pub("sk-ssh-ed25519@openssh.com AAAAGnNr alex@studio"), "sk-ssh-ed25519@openssh.com AAAAGnNr");
    }

    #[test]
    fn ifconfig_and_ip_addr_both_parse() {
        let mac = "lo0:\n\tinet 127.0.0.1 netmask 0xff000000\nen0:\n\tinet 192.0.2.10 netmask 0xffffff00\nen5:\n\tinet 169.254.3.4\n";
        assert_eq!(parse_ifconfig(mac), vec!["192.0.2.10".to_string()]);
        let linux = "1: lo\n    inet 127.0.0.1/8 scope host lo\n2: eth0\n    inet 192.0.2.20/24 brd 192.0.2.255 scope global eth0\n    inet6 fe80::1/64 scope link\n";
        assert_eq!(parse_ifconfig(linux), vec!["192.0.2.20".to_string()]);
        let one_line = "1: lo    inet 127.0.0.1/8 scope host lo\\       valid_lft forever\n2: eth0    inet 192.0.2.30/24 brd 192.0.2.255 scope global eth0\\       valid_lft forever\n";
        assert_eq!(parse_ifconfig(one_line), vec!["192.0.2.30".to_string()]);
    }
}
