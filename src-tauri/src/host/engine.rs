//! The part of the sharing role that keeps working while the window is
//! hidden: it probes this computer, opens the pairing port when the firewall
//! allows it, holds the keep-alive, and runs setup on request. It is one
//! std thread; the page only shows the snapshot it publishes.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::keepalive::KeepAlive;
use super::paired::{self, Connected, PairedComputer};
use super::pairing_server::{self, Code, Event};
use super::platform::{Outcome, Picture, Platform, SetupOptions};
use super::{HostSnapshot, LogLine, Notice, PairingState, LOG_EVENT, SNAPSHOT_EVENT};
use crate::stack::now_ms;
use crate::store;

const PROBE_EVERY: Duration = Duration::from_secs(10);
/// While nobody can see the window: on Windows a probe starts wsl.exe and
/// PowerShell, the dearest things this app runs.
const PROBE_EVERY_HIDDEN: Duration = Duration::from_secs(60);
/// The connection table is cheap to read; inside WSL it is one wsl.exe call.
const PEERS_EVERY: Duration = Duration::from_secs(5);
const PEERS_EVERY_HIDDEN: Duration = Duration::from_secs(60);
const MAX_LOG_LINES: usize = 600;
/// A probe slower than this is logged even when nothing else changed.
const SLOW_PROBE: Duration = Duration::from_secs(15);

fn env_flag(name: &str) -> bool {
    std::env::var(name).map(|v| v == "1").unwrap_or(false)
}

pub enum ToEngine {
    Probe,
    Setup(SetupOptions),
    /// Accept pairing requests for the next ten minutes.
    ArmPairing,
    DisarmPairing,
    /// Remove the key of the computer that paired from this address, and its entry.
    Forget(String),
    Quit,
}

/// The window's handle on the thread. Dropping it does not stop the thread;
/// `Quit` does.
pub struct Engine {
    pub to_engine: Sender<ToEngine>,
    pub snapshot: Arc<Mutex<HostSnapshot>>,
    pub log: Arc<Mutex<Vec<String>>>,
    stop_listener: Arc<AtomicBool>,
    keepalive: Arc<Mutex<KeepAlive>>,
}

impl Engine {
    pub fn quit(&self) {
        let _ = self.to_engine.send(ToEngine::Quit);
        self.stop_listener.store(true, Ordering::SeqCst);
        // Stopped here as well: the thread may be deep in a slow probe while
        // the app exits, and the keep-alive must not outlive a deliberate quit.
        self.keepalive.lock().expect("keepalive lock").stop();
    }
}

pub fn start(app: AppHandle, platform: Arc<dyn Platform>, pairing_port: u16) -> Engine {
    let (to_engine, requests) = channel::<ToEngine>();
    let snapshot = Arc::new(Mutex::new(HostSnapshot { os: platform.os_name().to_string(), ..Default::default() }));
    let log = Arc::new(Mutex::new(Vec::new()));
    let stop_listener = Arc::new(AtomicBool::new(false));
    let keepalive = Arc::new(Mutex::new(KeepAlive::new()));
    let engine = Engine {
        to_engine,
        snapshot: snapshot.clone(),
        log: log.clone(),
        stop_listener: stop_listener.clone(),
        keepalive: keepalive.clone(),
    };
    std::thread::Builder::new()
        .name("dockernanny-host".into())
        .spawn(move || {
            let mut state = Loop {
                app,
                platform,
                pairing_port,
                snapshot,
                log,
                code: Arc::new(Mutex::new(Code::new())),
                setup_running: Arc::new(AtomicBool::new(false)),
                keepalive,
                picture: Picture::default(),
                probed: false,
                addresses: Vec::new(),
                listening: false,
                bind_failure: None,
                last_missing: None,
                stop_listener,
                pairing_events: None,
                pairing_note: None,
                notice: None,
                was_armed: false,
                last_probe: None,
                published: None,
                paired: paired::load(&store::home_dir()),
                host_fingerprint: None,
                connected: Vec::new(),
                first_seen: HashMap::new(),
                last_peers: None,
            };
            state.run(requests);
        })
        .expect("spawn the host thread");
    engine
}

struct Loop {
    app: AppHandle,
    platform: Arc<dyn Platform>,
    pairing_port: u16,
    snapshot: Arc<Mutex<HostSnapshot>>,
    log: Arc<Mutex<Vec<String>>>,
    code: Arc<Mutex<Code>>,
    setup_running: Arc<AtomicBool>,
    /// Shared with `Engine`, which stops it on quit even while this thread is busy.
    keepalive: Arc<Mutex<KeepAlive>>,
    picture: Picture,
    probed: bool,
    addresses: Vec<String>,
    listening: bool,
    /// The last reason the pairing port could not open, so it is logged once.
    bind_failure: Option<String>,
    /// What the last probe found missing, so an unchanged reading is not logged.
    last_missing: Option<String>,
    stop_listener: Arc<AtomicBool>,
    pairing_events: Option<Receiver<Event>>,
    pairing_note: Option<String>,
    notice: Option<Notice>,
    was_armed: bool,
    last_probe: Option<Instant>,
    published: Option<HostSnapshot>,
    paired: Vec<PairedComputer>,
    /// `SHA256:...` of this computer's ssh host key, read when pairing is first turned on.
    host_fingerprint: Option<String>,
    connected: Vec<Connected>,
    /// When each peer address was first seen with a session open, so the
    /// page can say "since 12:40".
    first_seen: HashMap<String, u64>,
    last_peers: Option<Instant>,
}

impl Loop {
    fn run(&mut self, requests: Receiver<ToEngine>) {
        self.say(&format!("sharing started on {}", self.platform.os_name()));
        self.every_second();
        loop {
            match requests.recv_timeout(Duration::from_secs(1)) {
                Ok(ToEngine::Probe) => self.last_probe = None,
                Ok(ToEngine::Setup(options)) => self.setup(options),
                Ok(ToEngine::ArmPairing) => self.arm_pairing(),
                Ok(ToEngine::DisarmPairing) => self.disarm_pairing(),
                Ok(ToEngine::Forget(address)) => self.forget(&address),
                Ok(ToEngine::Quit) | Err(RecvTimeoutError::Disconnected) => {
                    self.keepalive.lock().expect("keepalive lock").stop();
                    self.say("sharing stopped");
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.every_second();
        }
    }

    fn say(&self, line: &str) {
        super::log_to_file(line);
        {
            let mut log = self.log.lock().expect("host log lock");
            log.push(line.to_string());
            if log.len() > MAX_LOG_LINES {
                let excess = log.len() - MAX_LOG_LINES;
                log.drain(..excess);
            }
        }
        let _ = self.app.emit(LOG_EVENT, LogLine { line: line.to_string() });
    }

    fn probe(&mut self) {
        let started = Instant::now();
        self.picture = self.platform.probe();
        self.addresses = self.platform.lan_ipv4();
        self.probed = true;
        self.last_probe = Some(Instant::now());
        let missing: Vec<&str> = self.picture.rows.iter().filter(|r| r.state == super::platform::State::Missing).map(|r| r.name).collect();
        let missing = if missing.is_empty() { "nothing".to_string() } else { missing.join(", ") };
        // A reading every ten seconds would fill the log with the same line;
        // what changed, and a probe slow enough to be a problem, are worth one.
        let changed = self.last_missing.as_deref() != Some(missing.as_str());
        let slow = started.elapsed() > SLOW_PROBE;
        if changed || slow {
            super::log_to_file(&format!("probe took {:?}; missing: {missing}", started.elapsed()));
        }
        self.last_missing = Some(missing);
    }

    /// The port opens only once the platform says the firewall allows it.
    /// Requests are still refused until the user turns pairing on.
    fn listen_if_ready(&mut self) {
        if self.listening || !self.picture.ready_for_pairing {
            return;
        }
        let (events_tx, events_rx) = channel::<Event>();
        let platform = self.platform.clone();
        let install = move |key: &str, mark: &str| platform.install_key(key, mark);
        match pairing_server::serve(self.pairing_port, self.code.clone(), install, events_tx, self.stop_listener.clone()) {
            Ok(_) => {
                self.listening = true;
                self.pairing_events = Some(events_rx);
                self.say(&format!("pairing port {} open; pairing itself is off until you turn it on", self.pairing_port));
                // Headless tests have no window to click in.
                if env_flag("DOCKERNANNY_ARM_PAIRING") {
                    self.arm_pairing();
                }
            }
            Err(err) => {
                // Tried again every second; said once, and again only if the reason changes.
                let why = format!("pairing port {} could not open: {err}", self.pairing_port);
                if self.bind_failure.as_deref() != Some(why.as_str()) {
                    self.say(&why);
                    self.bind_failure = Some(why);
                }
            }
        }
    }

    fn drain_pairing_events(&mut self) {
        let Some(events) = &self.pairing_events else { return };
        let notes: Vec<String> = events
            .try_iter()
            .map(|event| match event {
                Event::Paired { name, from, key_type, mark, fingerprint } => {
                    let key_note = format!("Its key: {fingerprint}, which the other computer shows too.");
                    let computer =
                        PairedComputer { name: name.clone(), address: from.clone(), key_type, paired_at_ms: now_ms(), mark, fingerprint };
                    match paired::remember(&store::home_dir(), computer) {
                        Ok(all) => self.paired = all,
                        Err(err) => super::log_to_file(&format!("paired.json could not be written: {err}")),
                    }
                    format!(
                        "Paired with {} ({from}). It can use this computer now. {key_note}",
                        if name.is_empty() { "another computer".into() } else { name }
                    )
                }
                Event::WrongCode { from, remaining } => format!("Wrong code from {from} ({remaining} tries left before pairing locks)."),
                Event::Locked { from } => {
                    format!("Pairing locked after too many wrong codes from {from}. Turn it on again when you are ready.")
                }
                Event::Failed(err) => format!("Pairing failed: {err}"),
            })
            .collect();
        for note in notes {
            self.say(&note);
            self.pairing_note = Some(note);
        }
    }

    fn arm_pairing(&mut self) {
        let digits = {
            let mut code = self.code.lock().expect("code lock");
            code.arm();
            code.digits.clone()
        };
        self.was_armed = true;
        self.pairing_note = None;
        // Shown next to the code, to compare with what the other computer pins.
        if self.host_fingerprint.is_none() {
            self.host_fingerprint = crate::pairing::fingerprint(&self.platform.host_key());
        }
        self.say(&format!("pairing on for {} minutes", pairing_server::ARMED_FOR.as_secs() / 60));
        // The code is written to the log only when a test asks for it.
        if env_flag("DOCKERNANNY_LOG_CODE") {
            self.say(&format!("pairing code {digits}"));
        }
    }

    fn disarm_pairing(&mut self) {
        self.code.lock().expect("code lock").disarm();
        self.was_armed = false;
        self.say("pairing off");
    }

    /// Takes a paired computer's access away: the line its key got in
    /// authorized_keys goes, found by its mark, and then its entry. Only
    /// that line is touched. A computer paired before keys were marked
    /// cannot be told apart from the user's own lines, so the notice says
    /// what is left to do by hand.
    fn forget(&mut self, address: &str) {
        let Some(computer) = self.paired.iter().find(|c| c.address == address).cloned() else { return };
        let name = if computer.name.is_empty() { address.to_string() } else { computer.name.clone() };
        let has_mark = !computer.mark.is_empty();
        if has_mark {
            if let Err(err) = self.platform.remove_key(&computer.mark) {
                self.notice = Some(Notice { text: format!("{name} could not be forgotten: {err}"), failed: true });
                return;
            }
        }
        match paired::forget(&store::home_dir(), address) {
            Ok(all) => self.paired = all,
            Err(err) => super::log_to_file(&format!("paired.json could not be written: {err}")),
        }
        let text = if has_mark {
            format!("{name} can no longer log in to this computer; its key is gone.")
        } else {
            format!(
                "{name} is off the list. Its key was added by an older dockerNanny without a mark, so remove its line from ~/.ssh/authorized_keys by hand{}.",
                if cfg!(windows) { " (inside WSL)" } else { "" }
            )
        };
        self.say(&text);
        self.notice = Some(Notice { text, failed: false });
    }

    fn setup(&mut self, options: SetupOptions) {
        if self.setup_running.swap(true, Ordering::SeqCst) {
            return;
        }
        self.notice = None;
        let platform = self.platform.clone();
        let flag = self.setup_running.clone();
        let app = self.app.clone();
        let log = self.log.clone();
        let snapshot = self.snapshot.clone();
        std::thread::spawn(move || {
            let mut say = move |line: &str| {
                super::log_to_file(line);
                log.lock().expect("host log lock").push(line.to_string());
                let _ = app.emit(LOG_EVENT, LogLine { line: line.to_string() });
            };
            let results = platform.setup(&options, &mut say);
            let notice = results.iter().find_map(|(name, outcome)| match outcome {
                Outcome::Failed(text) => Some(Notice { text: format!("{name}: {text}"), failed: true }),
                Outcome::NeedsUser(text) => Some(Notice { text: text.clone(), failed: false }),
                _ => None,
            });
            say(if notice.is_none() { "setup complete" } else { "setup stopped; see the notice" });
            // The loop picks the notice up on its next tick and probes again.
            snapshot.lock().expect("host snapshot lock").notice = notice;
            flag.store(false, Ordering::SeqCst);
        });
    }

    fn every_second(&mut self) {
        let setup_running = self.setup_running.load(Ordering::SeqCst);
        let in_view = crate::tray::window_in_view(&self.app);
        let probe_every = if in_view { PROBE_EVERY } else { PROBE_EVERY_HIDDEN };
        let probe_due = self.last_probe.map(|t| t.elapsed() >= probe_every).unwrap_or(true);
        if probe_due && !setup_running {
            self.probe();
        }
        if !setup_running {
            let finished = self.snapshot.lock().expect("host snapshot lock").notice.take();
            if let Some(notice) = finished {
                self.notice = Some(notice);
                self.last_probe = None;
            }
        }
        if self.picture.sshd_listening {
            let note = self.keepalive.lock().expect("keepalive lock").tick(self.platform.as_ref());
            if let Some(line) = note {
                self.say(&line);
            }
        }
        self.listen_if_ready();
        self.drain_pairing_events();
        self.look_at_peers(if in_view { PEERS_EVERY } else { PEERS_EVERY_HIDDEN });
        let armed = self.code.lock().expect("code lock").is_armed();
        if self.was_armed && !armed {
            self.was_armed = false;
            self.say("pairing off (paired, timed out, or locked)");
        }
        self.publish();
    }

    /// Who has an ssh session open, matched against the paired list for a name.
    fn look_at_peers(&mut self, every: Duration) {
        let due = self.last_peers.map(|t| t.elapsed() >= every).unwrap_or(true);
        if !due {
            return;
        }
        self.last_peers = Some(Instant::now());
        let peers = if self.picture.sshd_listening { self.platform.established_peers(self.picture.ssh_port) } else { Vec::new() };
        self.first_seen.retain(|address, _| peers.contains(address));
        self.connected = peers
            .into_iter()
            .map(|address| {
                let since_ms = *self.first_seen.entry(address.clone()).or_insert_with(now_ms);
                let name = self.paired.iter().find(|p| p.address == address && !p.name.is_empty()).map(|p| p.name.clone());
                Connected { address, name, since_ms }
            })
            .collect();
    }

    /// Emits the snapshot only when something in it changed.
    fn publish(&mut self) {
        let pairing = {
            let code = self.code.lock().expect("code lock");
            PairingState {
                listening: self.listening,
                armed: code.is_armed(),
                locked: code.is_locked(),
                code: code.is_armed().then(|| code.display()),
                remaining_s: code.remaining().as_secs(),
                note: self.pairing_note.clone(),
            }
        };
        let fresh = HostSnapshot {
            os: self.platform.os_name().to_string(),
            probed: self.probed,
            rows: self.picture.rows.clone(),
            user: self.picture.user.clone(),
            ssh_port: self.picture.ssh_port,
            ready_for_pairing: self.picture.ready_for_pairing,
            total_memory_gb: self.picture.total_memory_gb,
            network: self.picture.network.clone(),
            addresses: self.addresses.clone(),
            setup_running: self.setup_running.load(Ordering::SeqCst),
            notice: self.notice.clone(),
            pairing,
            paired: self.paired.clone(),
            host_fingerprint: self.host_fingerprint.clone(),
            connected: self.connected.clone(),
        };
        if self.published.as_ref() == Some(&fresh) {
            return;
        }
        *self.snapshot.lock().expect("host snapshot lock") = fresh.clone();
        let _ = self.app.emit(SNAPSHOT_EVENT, fresh.clone());
        self.published = Some(fresh);
    }
}
