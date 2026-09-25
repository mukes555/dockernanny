//! Who uses this shared computer: the computers that paired with it, kept
//! in `paired.json` next to the other app files, and the ones with an ssh
//! session open right now, read from the kernel's connection table.

use std::path::Path;

use serde::{Deserialize, Serialize};

const FILE: &str = "paired.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PairedComputer {
    /// The name the other computer gave itself; may be empty.
    pub name: String,
    /// Where the pairing request came from.
    pub address: String,
    /// `ssh-ed25519` and the like, from the key that was installed.
    pub key_type: String,
    pub paired_at_ms: u64,
}

/// A computer with an ssh session open on the sharing port right now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Connected {
    pub address: String,
    /// From the paired list, when the address matches.
    pub name: Option<String>,
    pub since_ms: u64,
}

pub fn load(home: &Path) -> Vec<PairedComputer> {
    let text = std::fs::read_to_string(home.join(FILE)).unwrap_or_default();
    serde_json::from_str(&text).unwrap_or_default()
}

/// Adds or replaces the entry for that address, so pairing again from the
/// same computer (a new key, a new name) keeps one line.
pub fn remember(home: &Path, computer: PairedComputer) -> std::io::Result<Vec<PairedComputer>> {
    let mut all = load(home);
    all.retain(|c| c.address != computer.address);
    all.push(computer);
    let text = serde_json::to_string_pretty(&all).unwrap_or_else(|_| "[]".into());
    std::fs::create_dir_all(home)?;
    std::fs::write(home.join(FILE), text)?;
    Ok(all)
}

/// The peer addresses of established TCP connections to `port`, from either
/// `ss -tn` (Linux, WSL) or `netstat -an` (macOS): both print one line per
/// socket with the state, the local address and the peer address.
pub fn parse_established(output: &str, port: u16) -> Vec<String> {
    let mut peers: Vec<String> = output
        .lines()
        .filter(|line| line.contains("ESTAB"))
        .filter_map(|line| {
            // ss: State Recv-Q Send-Q Local Peer; netstat: Proto Recv-Q Send-Q Local Foreign (state).
            // Both put the local address fourth and the peer fifth.
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (local, peer) = (fields.get(3)?, fields.get(4)?);
            let (_, local_port) = split_address(local)?;
            if local_port != port {
                return None;
            }
            let (peer_host, _) = split_address(peer)?;
            Some(peer_host)
        })
        .filter(|host| !host.is_empty() && host != "127.0.0.1" && host != "::1")
        .collect();
    peers.sort();
    peers.dedup();
    peers
}

/// `192.0.2.20:51234`, `192.0.2.20.51234` (netstat) or `[::ffff:192.0.2.20]:51234`.
fn split_address(text: &str) -> Option<(String, u16)> {
    let (host, port) = if let Some(rest) = text.strip_prefix('[') {
        let (host, port) = rest.split_once("]:")?;
        (host.to_string(), port)
    } else if text.matches(':').count() == 1 {
        let (host, port) = text.rsplit_once(':')?;
        (host.to_string(), port)
    } else {
        let (host, port) = text.rsplit_once('.')?;
        (host.to_string(), port)
    };
    let host = host.strip_prefix("::ffff:").unwrap_or(&host).to_string();
    Some((host, port.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_and_netstat_lines_give_the_peers_on_the_port() {
        let ss = "State  Recv-Q Send-Q Local Address:Port    Peer Address:Port\n\
                  ESTAB  0      0      192.0.2.10:2222       192.0.2.20:51234\n\
                  ESTAB  0      0      192.0.2.10:2222       192.0.2.20:51240\n\
                  ESTAB  0      0      [::ffff:192.0.2.10]:2222 [::ffff:192.0.2.21]:40000\n\
                  ESTAB  0      0      192.0.2.10:47433      192.0.2.30:9999\n\
                  LISTEN 0      128    0.0.0.0:2222          0.0.0.0:*\n";
        assert_eq!(parse_established(ss, 2222), vec!["192.0.2.20", "192.0.2.21"]);

        let netstat = "Active Internet connections (including servers)\n\
                       Proto Recv-Q Send-Q  Local Address          Foreign Address        (state)\n\
                       tcp4       0      0  192.0.2.10.22          192.0.2.20.52001       ESTABLISHED\n\
                       tcp4       0      0  127.0.0.1.22           127.0.0.1.52002        ESTABLISHED\n\
                       tcp4       0      0  192.0.2.10.443         192.0.2.20.52003       ESTABLISHED\n\
                       tcp46      0      0  *.22                   *.*                    LISTEN\n";
        assert_eq!(parse_established(netstat, 22), vec!["192.0.2.20"]);
    }

    #[test]
    fn the_paired_file_keeps_one_line_per_address() {
        let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("paired-test-{}", std::process::id()));
        let first = PairedComputer { name: "desk".into(), address: "192.0.2.20".into(), key_type: "ssh-ed25519".into(), paired_at_ms: 1 };
        remember(&home, first.clone()).unwrap();
        let again = PairedComputer { name: "desk again".into(), ..first.clone() };
        let other = PairedComputer { name: String::new(), address: "192.0.2.21".into(), key_type: "ssh-rsa".into(), paired_at_ms: 3 };
        remember(&home, again.clone()).unwrap();
        let all = remember(&home, other.clone()).unwrap();
        assert_eq!(all, vec![again, other]);
        assert_eq!(load(&home), all);
        let _ = std::fs::remove_dir_all(&home);
    }
}
