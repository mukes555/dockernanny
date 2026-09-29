//! Everything about the sharing role that depends on the operating system
//! lives behind this trait: what this computer has, how to make it ready,
//! where another computer's key goes. The engine, the pairing server and
//! the page are the same on every platform.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
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
    fn lan_ipv4(&self) -> Vec<String> {
        lan_addresses()
    }
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

/// This computer's IPv4 addresses on its networks, as the OS lists them
/// (no tool output to parse): no loopback, no link-local, and none on an
/// adapter that only reaches containers and virtual machines here.
pub fn lan_addresses() -> Vec<String> {
    let mut addresses: Vec<String> = Vec::new();
    for interface in if_addrs::get_if_addrs().unwrap_or_default() {
        let IpAddr::V4(ip) = interface.ip() else { continue };
        let local_only = ip.is_loopback() || ip.is_link_local();
        let address = ip.to_string();
        let usable = !local_only && !host_only_adapter(&interface.name) && !addresses.contains(&address);
        if usable {
            addresses.push(address);
        }
    }
    addresses
}

/// Adapters nobody on the network can pair through: Windows' WSL and
/// hypervisor adapters (by their names in Network Connections) and the
/// Linux bridges of Docker and libvirt.
fn host_only_adapter(name: &str) -> bool {
    let windows = ["vEthernet", "Hyper-V", "VirtualBox", "VMware"].iter().any(|word| name.contains(word));
    let linux = ["docker", "br-", "veth", "virbr"].iter().any(|prefix| name.starts_with(prefix));
    windows || linux
}

/// Whether something on this computer accepts connections on `port`.
pub fn listening_here(port: u16) -> bool {
    let loopbacks = [IpAddr::from(Ipv4Addr::LOCALHOST), IpAddr::from(Ipv6Addr::LOCALHOST)];
    loopbacks.into_iter().any(|ip| TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(1)).is_ok())
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
    fn host_only_adapters_are_left_out() {
        for name in
            ["vEthernet (WSL)", "VirtualBox Host-Only Network", "VMware Network Adapter VMnet8", "docker0", "br-3f2a", "veth91c", "virbr0"]
        {
            assert!(host_only_adapter(name), "{name}");
        }
        for name in ["Wi-Fi", "Ethernet 2", "en0", "eth0", "wlp2s0", "tailscale0", "utun4"] {
            assert!(!host_only_adapter(name), "{name}");
        }
    }

    #[test]
    fn this_computer_s_addresses_and_ports_are_read_from_the_system() {
        for address in lan_addresses() {
            assert!(!address.starts_with("127.") && !address.starts_with("169.254."), "{address}");
        }
        let holder = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        assert!(listening_here(holder.local_addr().unwrap().port()));
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
}
