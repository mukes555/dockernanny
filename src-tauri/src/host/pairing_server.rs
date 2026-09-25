//! The machine's side of pairing: another computer connects to the pairing
//! port, sends one JSON line with the code shown on this screen and its ssh
//! public key, and gets one JSON line back. Pairing is off until the user
//! turns it on, stays on for ten minutes, changes the code after a success
//! or three wrong guesses, and locks after ten wrong guesses in total until
//! it is turned on again.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use super::platform::Installed;
use crate::pairing::valid_public_key;

pub const ARMED_FOR: Duration = Duration::from_secs(10 * 60);
const ROTATE_AFTER_WRONG: u32 = 3;
const LOCK_AFTER_WRONG: u32 = 10;
const WRONG_GUESS_DELAY: Duration = Duration::from_secs(1);
const MAX_REQUEST_BYTES: u64 = 4096;

#[derive(Debug, Deserialize)]
struct Request {
    code: String,
    pubkey: String,
    /// The name the other computer wants shown; older senders call it `mac_name`.
    #[serde(default, alias = "mac_name")]
    from: String,
}

#[derive(Debug, Serialize, Default)]
struct Answer {
    ok: bool,
    user: String,
    port: u16,
    hostname: String,
    host_key: String,
    error: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Paired { name: String, from: String, key_type: String },
    WrongCode { from: String, remaining: u32 },
    Locked { from: String },
    Failed(String),
}

/// The current code and the state of the pairing window.
pub struct Code {
    pub digits: String,
    wrong: u32,
    total_wrong: u32,
    armed_until: Option<Instant>,
    locked: bool,
}

impl Default for Code {
    fn default() -> Self {
        Self::new()
    }
}

impl Code {
    pub fn new() -> Self {
        Self {
            digits: new_digits(),
            wrong: 0,
            total_wrong: 0,
            armed_until: None,
            locked: false,
        }
    }

    /// Opens the pairing window with a fresh code and a clean slate.
    pub fn arm(&mut self) {
        self.rotate();
        self.total_wrong = 0;
        self.locked = false;
        self.armed_until = Some(Instant::now() + ARMED_FOR);
    }

    pub fn disarm(&mut self) {
        self.armed_until = None;
    }

    pub fn is_armed(&self) -> bool {
        !self.locked && self.armed_until.map(|t| Instant::now() < t).unwrap_or(false)
    }

    pub fn is_locked(&self) -> bool {
        self.locked
    }

    pub fn remaining(&self) -> Duration {
        self.armed_until.map(|t| t.saturating_duration_since(Instant::now())).unwrap_or(Duration::ZERO)
    }

    fn rotate(&mut self) {
        self.digits = new_digits();
        self.wrong = 0;
    }

    /// Shown as `481 923`: easier to read across the room.
    pub fn display(&self) -> String {
        format!("{} {}", &self.digits[..3], &self.digits[3..])
    }
}

/// Six digits from the operating system's random source.
fn new_digits() -> String {
    let mut bytes = [0u8; 4];
    getrandom::fill(&mut bytes).expect("the OS random source is available");
    let number = u32::from_le_bytes(bytes) % 1_000_000;
    format!("{number:06}")
}

/// Compares the guess with the code without leaking where they differ.
fn same_code(given: &str, expected: &str) -> bool {
    let given = given.as_bytes();
    let expected = expected.as_bytes();
    let mut diff = (given.len() != expected.len()) as u8;
    for (i, byte) in expected.iter().enumerate() {
        diff |= given.get(i).copied().unwrap_or(0) ^ byte;
    }
    diff == 0
}

/// Listens on a thread until `stop` is set; the code decides whether a
/// request is honoured. `install` puts a validated key in place and says
/// which user and port to use; errors go back as text.
pub fn serve(port: u16, code: Arc<Mutex<Code>>, install: impl Fn(&str) -> Result<Installed, String> + Send + Sync + 'static, events: Sender<Event>, stop: Arc<AtomicBool>) -> std::io::Result<u16> {
    let listener = TcpListener::bind(("0.0.0.0", port))?;
    // Port 0 asks the OS for a free one; the caller learns which it got.
    let bound = listener.local_addr()?.port();
    // Non-blocking so the thread can notice `stop` and free the port when
    // sharing is turned off; a blocked accept would hold it forever.
    listener.set_nonblocking(true)?;
    let install = Arc::new(install);
    std::thread::spawn(move || loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let (stream, peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(200));
                continue;
            }
            Err(_) => continue,
        };
        let _ = stream.set_nonblocking(false);
        let answer = handle(&stream, &code, install.as_ref(), &events, &peer.ip().to_string());
        let _ = write_answer(stream, &answer);
    });
    Ok(bound)
}

fn handle(stream: &TcpStream, code: &Mutex<Code>, install: &(impl Fn(&str) -> Result<Installed, String> + ?Sized), events: &Sender<Event>, from: &str) -> Answer {
    let refuse = |error: &str| Answer {
        ok: false,
        error: error.into(),
        ..Default::default()
    };
    let request = match read_request(stream) {
        Ok(request) => request,
        Err(err) => return refuse(&err),
    };

    let mut current = code.lock().expect("code lock");
    if !current.is_armed() {
        return refuse("pairing is off on the machine; turn it on there first");
    }
    if !same_code(request.code.trim(), &current.digits) {
        current.wrong += 1;
        current.total_wrong += 1;
        if current.total_wrong >= LOCK_AFTER_WRONG {
            current.locked = true;
            current.disarm();
            let _ = events.send(Event::Locked { from: from.into() });
        } else if current.wrong >= ROTATE_AFTER_WRONG {
            current.rotate();
        }
        let remaining = LOCK_AFTER_WRONG.saturating_sub(current.total_wrong);
        let _ = events.send(Event::WrongCode { from: from.into(), remaining });
        drop(current);
        // A guess costs a second, so the ten allowed cannot be tried in a hurry.
        std::thread::sleep(WRONG_GUESS_DELAY);
        return refuse("wrong code");
    }

    let key = match valid_public_key(&request.pubkey) {
        Ok(key) => key,
        Err(err) => return refuse(&err),
    };
    match install(&key) {
        Ok(installed) => {
            // One success closes the window; the user opens it again for the next computer.
            current.disarm();
            current.rotate();
            let _ = events.send(Event::Paired {
                name: request.from.clone(),
                from: from.into(),
                key_type: key.split_whitespace().next().unwrap_or_default().to_string(),
            });
            Answer {
                ok: true,
                user: installed.user,
                port: installed.port,
                hostname: installed.hostname,
                host_key: installed.host_key,
                error: String::new(),
            }
        }
        Err(err) => {
            let _ = events.send(Event::Failed(err.clone()));
            refuse(&err)
        }
    }
}

fn read_request(stream: &TcpStream) -> Result<Request, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
    let mut line = String::new();
    let mut reader = BufReader::new(stream.take(MAX_REQUEST_BYTES));
    reader.read_line(&mut line).map_err(|e| format!("read failed: {e}"))?;
    if !line.ends_with('\n') {
        return Err("request too large or cut short".into());
    }
    serde_json::from_str(line.trim()).map_err(|e| format!("bad request: {e}"))
}

fn write_answer(mut stream: TcpStream, answer: &Answer) -> std::io::Result<()> {
    let mut text = serde_json::to_string(answer).unwrap_or_default();
    text.push('\n');
    stream.write_all(text.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_six_digits_and_display_with_a_gap() {
        let code = Code::new();
        assert_eq!(code.digits.len(), 6);
        assert!(code.digits.chars().all(|c| c.is_ascii_digit()));
        assert_eq!(code.display().len(), 7);
        assert!(!code.is_armed());
    }

    #[test]
    fn constant_time_compare_handles_lengths() {
        assert!(same_code("123456", "123456"));
        assert!(!same_code("123457", "123456"));
        assert!(!same_code("12345", "123456"));
        assert!(!same_code("", "123456"));
    }

    fn talk(port: u16, code: &str, key: &str) -> serde_json::Value {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream.write_all(format!("{{\"code\":\"{code}\",\"pubkey\":\"{key}\",\"from\":\"desk\"}}\n").as_bytes()).unwrap();
        let mut reply = String::new();
        BufReader::new(&stream).read_line(&mut reply).unwrap();
        serde_json::from_str(&reply).unwrap()
    }

    fn installer(_key: &str) -> Result<Installed, String> {
        Ok(Installed { user: "alex".into(), port: 2222, hostname: "studio".into(), host_key: "ssh-ed25519 AAAAhost".into() })
    }

    const KEY: &str = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIL test";

    #[test]
    fn pairing_off_refuses_even_the_right_code() {
        let code = Arc::new(Mutex::new(Code::new()));
        let digits = code.lock().unwrap().digits.clone();
        let (tx, _rx) = std::sync::mpsc::channel();
        let port = serve(0, code, installer, tx, Arc::new(AtomicBool::new(false))).unwrap();
        let answer = talk(port, &digits, KEY);
        assert_eq!(answer["ok"], false);
        assert!(answer["error"].as_str().unwrap().contains("off"));
    }

    #[test]
    fn armed_code_pairs_once_then_closes_and_a_bad_key_is_refused() {
        let code = Arc::new(Mutex::new(Code::new()));
        code.lock().unwrap().arm();
        let digits = code.lock().unwrap().digits.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        let port = serve(0, code.clone(), installer, tx, Arc::new(AtomicBool::new(false))).unwrap();
        assert_eq!(talk(port, "000000", KEY)["ok"], false);
        let bad = talk(port, &digits, "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIL'; echo pwned");
        assert_eq!(bad["error"], "key data is not base64");
        let answer = talk(port, &digits, KEY);
        assert_eq!(answer["ok"], true);
        assert_eq!(answer["host_key"], "ssh-ed25519 AAAAhost");
        assert!(!code.lock().unwrap().is_armed());
        assert_eq!(talk(port, &digits, KEY)["ok"], false);
        assert!(rx.try_iter().any(|e| matches!(e, Event::Paired { .. })));
    }

    #[test]
    fn a_stopped_listener_frees_its_port() {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = std::sync::mpsc::channel();
        let port = serve(0, Arc::new(Mutex::new(Code::new())), installer, tx, stop.clone()).unwrap();
        stop.store(true, Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(500));
        assert!(TcpListener::bind(("0.0.0.0", port)).is_ok());
    }
}
