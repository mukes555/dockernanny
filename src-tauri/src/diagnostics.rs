//! A report to paste into a bug report: what the app, this computer and its
//! tools are, plus the end of the logs, with everything personal replaced.
//! Known names (this computer, the user, the home folder, every machine) are
//! swapped for placeholders; any other IP address or email found in the text
//! is masked too, so a log line nobody anticipated cannot leak one.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// A name to hide and what to show instead, e.g. ("studio", "machine-1").
pub type Replacement = (String, String);

/// What the report says about the app; the rest comes from the logs.
pub struct Facts {
    pub use_machines: bool,
    pub share_this_computer: bool,
    pub machines: usize,
    pub machines_online: usize,
    pub stacks: usize,
    pub ssh: Option<String>,
    pub rsync: Option<String>,
    pub docker: Option<String>,
}

pub fn report(facts: &Facts, app_log: &Path, host_log: &Path, private: &[Replacement]) -> String {
    let on = |b: bool| if b { "on" } else { "off" };
    let tool = |v: &Option<String>| v.clone().unwrap_or_else(|| "not found".into());
    let mut text = format!(
        "dockerNanny {} on {} {}\nroles: use other machines {}, share this computer {}\nmachines: {} ({} online), stacks: {}\nssh: {}\nrsync: {}\ndocker: {}\n",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        on(facts.use_machines),
        on(facts.share_this_computer),
        facts.machines,
        facts.machines_online,
        facts.stacks,
        tool(&facts.ssh),
        tool(&facts.rsync),
        tool(&facts.docker),
    );
    text.push_str("\n--- end of app.log ---\n");
    text.push_str(&tail(app_log, 60));
    if facts.share_this_computer {
        text.push_str("\n--- end of host.log ---\n");
        text.push_str(&tail(host_log, 30));
    }
    redact(&text, private)
}

/// The last `lines` lines of a log, reading only its end: logs grow for as
/// long as the app runs.
fn tail(path: &Path, lines: usize) -> String {
    const WINDOW: u64 = 64 * 1024;
    let Ok(mut file) = std::fs::File::open(path) else {
        return "(no log yet)\n".into();
    };
    let length = file.metadata().map(|m| m.len()).unwrap_or(0);
    let _ = file.seek(SeekFrom::Start(length.saturating_sub(WINDOW)));
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    let mut out = all[start..].join("\n");
    out.push('\n');
    out
}

/// Replaces the known private names first (longest first, whole words only,
/// ignoring case), then masks any IP address or email that is left.
pub fn redact(text: &str, private: &[Replacement]) -> String {
    let mut known: Vec<&Replacement> = private.iter().filter(|(name, _)| name.chars().count() >= 2).collect();
    known.sort_by_key(|(name, _)| std::cmp::Reverse(name.len()));
    let mut out = text.to_string();
    for (name, placeholder) in known {
        out = replace_words(&out, name, placeholder);
    }
    mask_addresses(&out)
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Every occurrence of `needle` that is not part of a longer word, so a user
/// named `mk` does not turn `mkdir` into a placeholder.
fn replace_words(haystack: &str, needle: &str, replacement: &str) -> String {
    let lower_hay = haystack.to_lowercase();
    let lower_needle = needle.to_lowercase();
    // Lowercasing can change byte lengths for some scripts; fall back to exact matching then.
    let (search, target) = if lower_hay.len() == haystack.len() { (lower_hay.as_str(), lower_needle.as_str()) } else { (haystack, needle) };
    let mut out = String::with_capacity(haystack.len());
    let mut cursor = 0;
    while let Some(found) = search[cursor..].find(target) {
        let start = cursor + found;
        let end = start + target.len();
        let before_ok = haystack[..start].chars().next_back().is_none_or(|c| !is_word_char(c));
        let after_ok = haystack[end..].chars().next().is_none_or(|c| !is_word_char(c));
        out.push_str(&haystack[cursor..start]);
        if before_ok && after_ok {
            out.push_str(replacement);
        } else {
            out.push_str(&haystack[start..end]);
        }
        cursor = end;
    }
    out.push_str(&haystack[cursor..]);
    out
}

/// Walks the text token by token and masks IPv4, IPv6 and email addresses.
/// Loopback and "any" addresses stay: they are not personal and help debugging.
fn mask_addresses(text: &str) -> String {
    let is_token_char = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '@' | '_' | '-' | '%' | '+');
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for c in text.chars() {
        if is_token_char(c) {
            token.push(c);
        } else {
            out.push_str(&mask_token(&token));
            token.clear();
            out.push(c);
        }
    }
    out.push_str(&mask_token(&token));
    out
}

fn mask_token(token: &str) -> String {
    if token.is_empty() {
        return String::new();
    }
    if let Some((_, domain)) = token.split_once('@') {
        if domain.contains('.') && !domain.starts_with('.') {
            return "<email>".into();
        }
    }
    // IPv4, optionally followed by :port and trailing punctuation.
    let (address, rest) = match token.find([':', '-']) {
        Some(at) if is_ipv4(&token[..at]) => (&token[..at], &token[at..]),
        _ => (token.trim_end_matches('.'), &token[token.trim_end_matches('.').len()..]),
    };
    if is_ipv4(address) {
        return if address == "0.0.0.0" || address.starts_with("127.") { token.to_string() } else { format!("<ip>{rest}") };
    }
    if looks_like_ipv6(token) {
        return if matches!(token, "::" | "::1") { token.to_string() } else { "<ipv6>".into() };
    }
    token.to_string()
}

fn is_ipv4(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 4 && parts.iter().all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()) && p.parse::<u16>().is_ok_and(|n| n <= 255))
}

/// Hex groups joined by colons, with "::" or at least three colons, so a
/// clock time like 12:40:05 is never mistaken for one.
fn looks_like_ipv6(text: &str) -> bool {
    let colons = text.matches(':').count();
    let shaped = text.contains("::") || colons >= 3;
    shaped && text.chars().all(|c| c.is_ascii_hexdigit() || c == ':' || c == '.') && text.chars().any(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn private() -> Vec<Replacement> {
        vec![
            ("/home/alex".into(), "~".into()),
            ("alex".into(), "<you>".into()),
            ("desk".into(), "<this-computer>".into()),
            ("studio".into(), "machine-1".into()),
            ("mk".into(), "user-1".into()),
        ]
    }

    #[test]
    fn known_names_become_placeholders_as_whole_words_only() {
        let line = "copy desk -> studio as mk: ran mkdir in /home/alex/projects/shop for alex";
        assert_eq!(redact(line, &private()), "copy <this-computer> -> machine-1 as user-1: ran mkdir in ~/projects/shop for <you>");
        assert_eq!(redact("Studio answered", &private()), "machine-1 answered", "case does not matter");
    }

    #[test]
    fn stray_addresses_and_emails_are_masked_but_loopback_stays() {
        let line = "ssh 192.0.2.10:2222 failed; forward 127.0.0.1:8080 and 0.0.0.0:8080->80/tcp ok; peer fe80::1a2b:3c4d; mail someone@example.org; at 12:40:05";
        assert_eq!(
            redact(line, &[]),
            "ssh <ip>:2222 failed; forward 127.0.0.1:8080 and 0.0.0.0:8080->80/tcp ok; peer <ipv6>; mail <email>; at 12:40:05"
        );
        assert_eq!(redact("::1 and 10.0.0.300", &[]), "::1 and 10.0.0.300", "not an address: 300 is out of range");
    }

    #[test]
    fn the_log_tail_reads_only_the_last_lines() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("diag-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("app.log");
        let text: String = (1..=100).map(|n| format!("line {n}\n")).collect();
        std::fs::write(&log, text).unwrap();
        assert_eq!(tail(&log, 3), "line 98\nline 99\nline 100\n");
        assert_eq!(tail(&dir.join("missing.log"), 3), "(no log yet)\n");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
