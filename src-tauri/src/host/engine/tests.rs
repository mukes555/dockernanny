//! The loop driven tick by tick, with a pretend computer and a page that
//! records what it is told.

use std::process::Child;

use super::super::platform::{row, Installed, Say};
use super::*;

#[derive(Default)]
struct RecordingPage {
    lines: Mutex<Vec<String>>,
    shown: Mutex<Vec<HostSnapshot>>,
}

impl Page for RecordingPage {
    fn in_view(&self) -> bool {
        true
    }
    fn log_line(&self, line: &str) {
        self.lines.lock().unwrap().push(line.to_string());
    }
    fn show(&self, snapshot: &HostSnapshot) {
        self.shown.lock().unwrap().push(snapshot.clone());
    }
}

#[derive(Default)]
struct Pretend {
    picture: Picture,
    refuse_removal: bool,
    removed: Mutex<Vec<String>>,
}

impl Platform for Pretend {
    fn os_name(&self) -> &'static str {
        "pretend"
    }
    fn probe(&self) -> Picture {
        self.picture.clone()
    }
    fn setup(&self, _options: &SetupOptions, _say: &mut Say) -> Vec<(&'static str, Outcome)> {
        Vec::new()
    }
    fn host_key(&self) -> String {
        String::new()
    }
    fn install_key(&self, _key: &str, _mark: &str) -> Result<Installed, String> {
        Err("not in these tests".into())
    }
    fn remove_key(&self, mark: &str) -> Result<(), String> {
        if self.refuse_removal {
            return Err("authorized_keys is read-only".into());
        }
        self.removed.lock().unwrap().push(mark.to_string());
        Ok(())
    }
    fn spawn_keepalive(&self) -> Result<Option<Child>, String> {
        Ok(None)
    }
    fn established_peers(&self, _port: u16) -> Vec<String> {
        Vec::new()
    }
    fn lan_ipv4(&self) -> Vec<String> {
        vec!["192.0.2.7".into()]
    }
    fn hostname(&self) -> String {
        "pretend".into()
    }
}

fn test_home(test: &str) -> PathBuf {
    let home = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(".tmp").join(format!("engine-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();
    home
}

fn computer(address: &str, mark: &str) -> PairedComputer {
    PairedComputer {
        name: String::new(),
        address: address.into(),
        key_type: "ssh-ed25519".into(),
        paired_at_ms: 1,
        mark: mark.into(),
        fingerprint: String::new(),
    }
}

#[test]
fn the_page_hears_of_a_change_once() {
    let home = test_home("once");
    let page = Arc::new(RecordingPage::default());
    let mut picture = Picture { ssh_port: 22, ..Picture::default() };
    picture.rows.push(row("Docker", true, "29.8.1"));
    let platform = Arc::new(Pretend { picture, ..Pretend::default() });
    let mut engine = Loop::new(page.clone(), platform, 0, home);

    engine.every_second();
    engine.every_second();
    let shown = page.shown.lock().unwrap();
    assert_eq!(shown.len(), 1, "an unchanged snapshot is not sent again");
    assert!(shown[0].probed);
    assert_eq!(shown[0].rows, vec![row("Docker", true, "29.8.1")]);
    assert_eq!(shown[0].addresses, vec!["192.0.2.7".to_string()]);
}

#[test]
fn a_pairing_port_that_cannot_open_is_said_once() {
    let home = test_home("bind");
    let holder = std::net::TcpListener::bind(("0.0.0.0", 0)).unwrap();
    let taken = holder.local_addr().unwrap().port();
    let page = Arc::new(RecordingPage::default());
    let platform = Arc::new(Pretend { picture: Picture { ready_for_pairing: true, ..Picture::default() }, ..Pretend::default() });
    let mut engine = Loop::new(page.clone(), platform, taken, home);

    for _ in 0..3 {
        engine.every_second();
    }
    assert!(!engine.listening);
    let lines = page.lines.lock().unwrap();
    let said = lines.iter().filter(|line| line.contains("could not open")).count();
    assert_eq!(said, 1, "{lines:?}");
}

#[test]
fn a_pairing_is_remembered_and_said() {
    let home = test_home("paired");
    let page = Arc::new(RecordingPage::default());
    let mut engine = Loop::new(page.clone(), Arc::new(Pretend::default()), 0, home.clone());
    let (events, received) = channel();
    engine.pairing_events = Some(received);
    let paired = Event::Paired {
        name: "studio".into(),
        from: "192.0.2.20".into(),
        key_type: "ssh-ed25519".into(),
        mark: "dockernanny:aaaa".into(),
        fingerprint: "SHA256:abc".into(),
    };
    events.send(paired).unwrap();

    engine.every_second();
    assert_eq!(paired::load(&home).len(), 1, "written to paired.json");
    let shown = page.shown.lock().unwrap();
    assert_eq!(shown.last().unwrap().paired[0].mark, "dockernanny:aaaa");
    let note = shown.last().unwrap().pairing.note.clone().unwrap_or_default();
    assert!(note.starts_with("Paired with studio (192.0.2.20)."), "{note}");
    assert!(note.contains("SHA256:abc"));
}

#[test]
fn forgetting_takes_away_only_the_marked_key_line() {
    let home = test_home("forget");
    paired::remember(&home, computer("192.0.2.20", "dockernanny:aaaa")).unwrap();
    paired::remember(&home, computer("192.0.2.21", "")).unwrap();
    let platform = Arc::new(Pretend::default());
    let mut engine = Loop::new(Arc::new(RecordingPage::default()), platform.clone(), 0, home.clone());

    engine.forget("192.0.2.20");
    assert_eq!(*platform.removed.lock().unwrap(), vec!["dockernanny:aaaa".to_string()]);
    assert!(engine.notice.as_ref().unwrap().text.contains("can no longer log in"));

    // Keys paired before they were marked cannot be told apart from the user's own lines.
    engine.forget("192.0.2.21");
    assert_eq!(platform.removed.lock().unwrap().len(), 1, "no line is removed without a mark");
    assert!(engine.notice.as_ref().unwrap().text.contains("by hand"));
    assert!(engine.paired.is_empty());
    assert!(paired::load(&home).is_empty());
}

#[test]
fn a_key_that_could_not_be_removed_keeps_its_entry() {
    let home = test_home("refused");
    paired::remember(&home, computer("192.0.2.20", "dockernanny:aaaa")).unwrap();
    let platform = Arc::new(Pretend { refuse_removal: true, ..Pretend::default() });
    let mut engine = Loop::new(Arc::new(RecordingPage::default()), platform, 0, home.clone());

    engine.forget("192.0.2.20");
    let notice = engine.notice.clone().unwrap();
    assert!(notice.failed);
    assert!(notice.text.contains("authorized_keys is read-only"), "{}", notice.text);
    assert_eq!(engine.paired.len(), 1);
    assert_eq!(paired::load(&home).len(), 1);
}
