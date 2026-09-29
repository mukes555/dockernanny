//! Pairing, the one step that happens before ssh exists. This computer sends
//! one JSON line to the machine's pairing port with the code shown on its
//! screen and this computer's public key; one line comes back with the user,
//! port and ssh host key to use. The key travels in the clear; it is public.
//!
//! Both sides of the exchange are checked here, so the host role can reuse
//! the same rules: the answer comes from a peer we have not authenticated,
//! and the request comes from anyone on the network.

use std::time::Duration;

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::time::timeout;

pub const PORT: u16 = 47433;
const MAX_ANSWER_BYTES: u64 = 8192;
const MAX_KEY_BYTES: usize = 1024;

/// What this computer sends. `from` is only shown on the machine's screen.
#[derive(Serialize)]
struct Request<'a> {
    code: &'a str,
    pubkey: &'a str,
    from: &'a str,
}

#[derive(Debug, Deserialize)]
pub struct Answer {
    pub ok: bool,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub hostname: String,
    /// The machine's sshd host key (`ssh-ed25519 AAAA...`), pinned on this
    /// computer so later connections trust only that key. Older machines
    /// leave it empty.
    #[serde(default)]
    pub host_key: String,
    #[serde(default)]
    pub error: String,
}

pub async fn pair(address: &str, port: u16, code: &str, pubkey: &str, from: &str) -> anyhow::Result<Answer> {
    let stream = timeout(Duration::from_secs(5), TcpStream::connect((address, port)))
        .await
        .context("the machine did not answer; is dockerNanny open there with pairing turned on?")?
        .with_context(|| format!("could not connect to {address}:{port}"))?;
    let (reader, mut writer) = stream.into_split();
    let mut line = serde_json::to_string(&Request { code, pubkey, from })?;
    line.push('\n');
    writer.write_all(line.as_bytes()).await?;

    let mut reply = String::new();
    timeout(Duration::from_secs(30), BufReader::new(reader.take(MAX_ANSWER_BYTES)).read_line(&mut reply))
        .await
        .context("the machine took too long to answer")??;
    serde_json::from_str(reply.trim()).context("unreadable answer from the machine")
}

/// An account name from the POSIX portable set (letters, digits, `.`, `_`,
/// `-`), not starting with `-` or `.`: every name `useradd` makes, and the
/// `john.doe` and `John` of directory logins, but nothing that could change
/// the meaning of the ssh config line it is written into.
pub fn valid_user(user: &str) -> bool {
    let first_ok = user.chars().next().map(|c| c.is_ascii_alphanumeric() || c == '_').unwrap_or(false);
    let rest_ok = user.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    first_ok && rest_ok && user.len() <= 64
}

/// A host name or address: letters, digits, dots, dashes, colons for IPv6.
pub fn valid_host(host: &str) -> bool {
    !host.is_empty() && host.len() <= 253 && host.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
}

fn known_key_type(key_type: &str) -> bool {
    matches!(
        key_type,
        "ssh-ed25519"
            | "ssh-rsa"
            | "ecdsa-sha2-nistp256"
            | "ecdsa-sha2-nistp384"
            | "ecdsa-sha2-nistp521"
            | "sk-ssh-ed25519@openssh.com"
            | "sk-ecdsa-sha2-nistp256@openssh.com"
    )
}

fn base64_blob(blob: &str) -> bool {
    blob.len() >= 16 && blob.len() <= 4096 && blob.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '='))
}

/// `keytype base64`, with a known key type and only base64 characters, so it
/// can be written into known_hosts as is.
pub fn valid_host_key(key: &str) -> Option<(String, String)> {
    let mut fields = key.split_whitespace();
    let key_type = fields.next()?;
    let blob = fields.next()?;
    (known_key_type(key_type) && base64_blob(blob)).then(|| (key_type.to_string(), blob.to_string()))
}

/// A public key line another computer sends, reduced to `type base64`. The
/// comment is dropped and the blob must be base64, so the result can be
/// written into authorized_keys by a shell script as is.
pub fn valid_public_key(text: &str) -> Result<String, String> {
    let key = text.trim();
    if key.is_empty() {
        return Err("empty key".into());
    }
    if key.len() > MAX_KEY_BYTES {
        return Err("key is too long".into());
    }
    let mut fields = key.split_whitespace();
    let key_type = fields.next().unwrap_or("");
    let blob = fields.next().unwrap_or("");
    if !known_key_type(key_type) {
        return Err("not an ssh public key".into());
    }
    if !base64_blob(blob) {
        return Err("key data is not base64".into());
    }
    Ok(format!("{key_type} {blob}"))
}

/// A key's fingerprint the way `ssh-keygen -l` prints it: `SHA256:` and the
/// unpadded base64 of the hash. Both screens show it after pairing, so
/// people can compare them; a key swapped on the way would differ. None for
/// a line that is not `type base64`.
pub fn fingerprint(key: &str) -> Option<String> {
    use base64::Engine;
    use sha2::Digest;
    let blob = key.split_whitespace().nth(1)?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(blob).ok()?;
    let digest = sha2::Sha256::digest(&bytes);
    Some(format!("SHA256:{}", base64::engine::general_purpose::STANDARD_NO_PAD.encode(digest)))
}

/// This computer's name, as the machine shows it after pairing.
pub async fn computer_name() -> String {
    let name = if cfg!(target_os = "macos") {
        let out = tokio::process::Command::new("scutil").args(["--get", "ComputerName"]).output().await;
        out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
    } else {
        std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default()
    };
    if name.is_empty() {
        "this computer".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_are_plain_unix_names() {
        assert!(valid_user("alex"));
        assert!(valid_user("_svc-1"));
        assert!(valid_user("john.doe"), "directory logins have dots");
        assert!(valid_user("Alex"), "and capitals");
        assert!(!valid_user("-oProxyCommand=x"), "never an option");
        assert!(!valid_user(".hidden"));
        assert!(!valid_user("alex\nPermitLocalCommand yes"));
        assert!(!valid_user(""));
        assert!(!valid_user("root user"));
        assert!(!valid_user("alex@corp"));
    }

    #[test]
    fn hosts_have_no_room_for_directives() {
        assert!(valid_host("192.0.2.15"));
        assert!(valid_host("studio.local"));
        assert!(valid_host("fe80::1"));
        assert!(!valid_host("studio local"));
        assert!(!valid_host("a\nProxyCommand x"));
    }

    #[test]
    fn host_keys_are_type_and_base64_only() {
        assert!(valid_host_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGuHc5oJ").is_some());
        assert!(valid_host_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGuHc5oJ root@machine").is_some());
        assert!(valid_host_key("ssh-dss AAAAC3NzaC1lZDI1NTE5AAAAIGuHc5oJ").is_none());
        assert!(valid_host_key("ssh-ed25519 AAAA'; echo pwned").is_none());
        assert!(valid_host_key("").is_none());
    }

    #[test]
    fn public_keys_are_reduced_to_type_and_base64() {
        assert_eq!(
            valid_public_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIL alex@studio\n").unwrap(),
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIL"
        );
        assert!(valid_public_key("ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQ x").is_ok());
        assert!(valid_public_key("").is_err());
        assert!(valid_public_key("ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIL'; echo pwned").is_err());
        assert!(valid_public_key("ssh-ed25519 AAAA\nssh-ed25519 BBBB").is_err());
        assert!(valid_public_key("command=\"rm -rf /\" ssh-ed25519 AAAA").is_err());
        assert!(valid_public_key(&format!("ssh-ed25519 {}", "A".repeat(2000))).is_err());
    }

    #[test]
    fn fingerprints_match_ssh_keygen() {
        // A throwaway key; `ssh-keygen -lf` printed this fingerprint for it.
        let key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG+dazxQtDeYmpmJw5f8QrXiTcPkEF9k95hWRAYN7Q20 test";
        assert_eq!(fingerprint(key).as_deref(), Some("SHA256:TxEHBXXWzSnbkxqbV6FxMRokiF0jHb/hTzN/O0wkqU8"));
        assert_eq!(fingerprint("ssh-ed25519 not*base64"), None);
        assert_eq!(fingerprint(""), None);
    }
}
