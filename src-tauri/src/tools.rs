//! How this computer starts the programs it needs. The Unix tools that reach
//! machines (ssh, rsync, sh) run directly on macOS and Linux; on Windows they
//! run inside the WSL distribution chosen in Settings, because Windows' own
//! OpenSSH has no ControlMaster and there is no native rsync. Programs of the
//! operating system itself (docker, ssh-keygen, netstat) run natively and
//! never flash a console window. Long-running children end with the app.

use std::path::Path;

use anyhow::Context;

/// Picks the WSL distribution the tools run in, and says whether it is a
/// different one than before. Only Windows has one; elsewhere nothing
/// changes. Made at start and whenever Settings are saved.
pub fn configure(distro: &str) -> bool {
    #[cfg(windows)]
    return wsl::configure(distro);
    #[cfg(not(windows))]
    {
        let _ = distro;
        false
    }
}

/// The WSL distribution the tools run in; None off Windows.
pub fn wsl_distro() -> Option<String> {
    #[cfg(windows)]
    return Some(wsl::distro());
    #[cfg(not(windows))]
    None
}

/// A Unix tool: itself, or `wsl.exe -d <distro> --exec <program>` on Windows.
pub fn unix(program: &str) -> tokio::process::Command {
    tokio::process::Command::from(unix_std(program))
}

pub fn unix_std(program: &str) -> std::process::Command {
    #[cfg(windows)]
    {
        let mut cmd = native_std("wsl.exe");
        cmd.args(["-d", &wsl::distro(), "--exec", program]).env("WSL_UTF8", "1");
        cmd
    }
    #[cfg(not(windows))]
    std::process::Command::new(program)
}

/// Windows only: a Unix tool run as root inside the distribution, which WSL
/// allows the Windows user without a password or an administrator prompt.
#[cfg(windows)]
pub fn unix_as_root(program: &str) -> tokio::process::Command {
    let mut cmd = native("wsl.exe");
    cmd.args(["-d", &wsl::distro(), "-u", "root", "--exec", program]).env("WSL_UTF8", "1");
    cmd
}

/// A program of this operating system, started without a console window.
pub fn native(program: &str) -> tokio::process::Command {
    tokio::process::Command::from(native_std(program))
}

pub fn native_std(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// A path on this computer as the tools see it: unchanged off Windows,
/// `C:\Users\alex\shop` as `/mnt/c/Users/alex/shop` inside WSL.
pub fn path(local: &Path) -> String {
    let text = local.display().to_string();
    if cfg!(windows) {
        wsl_path(&text)
    } else {
        text
    }
}

/// `C:\a\b` becomes `/mnt/c/a/b`; a path without a drive letter only gets
/// forward slashes. Pure, so it is tested on every OS.
pub fn wsl_path(windows: &str) -> String {
    let bytes = windows.as_bytes();
    let has_drive = bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':';
    let slashed = windows.replace('\\', "/");
    if !has_drive {
        return slashed;
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let rest = slashed[2..].trim_start_matches('/');
    if rest.is_empty() {
        format!("/mnt/{drive}")
    } else {
        format!("/mnt/{drive}/{rest}")
    }
}

/// Where the tools keep the generated ssh config, the pinned host keys, key
/// copies and control sockets. Off Windows that is the app folder itself. On
/// Windows it is `~/.dockernanny` inside the distribution: a socket cannot
/// live on `/mnt/c`, and ssh refuses a config or key that looks
/// world-readable there.
pub fn home(app_home: &Path) -> String {
    #[cfg(windows)]
    {
        let custom = std::env::var_os("DOCKERNANNY_HOME").is_some();
        format!("{}/{}", wsl::home(), folder_in_wsl(custom, app_home))
    }
    #[cfg(not(windows))]
    app_home.display().to_string()
}

/// The tools' folder name inside WSL. A second instance or a test run with
/// its own `DOCKERNANNY_HOME` gets its own folder there too, named after its
/// app folder, so it never overwrites the app's ssh config or sockets.
pub fn folder_in_wsl(custom_home: bool, app_home: &Path) -> String {
    if !custom_home {
        return ".dockernanny".into();
    }
    let name = app_home.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let plain: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')).collect();
    let plain = plain.trim_start_matches('.');
    if plain.is_empty() {
        ".dockernanny-custom".into()
    } else {
        format!(".dockernanny-{plain}")
    }
}

/// Creates the folders the tools write into, open to their owner only.
pub fn make_private_dirs(app_home: &Path, names: &[&str]) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        let home = home(app_home);
        let dirs: Vec<String> = names.iter().map(|name| format!("{home}/{name}")).collect();
        wsl::sh("mkdir -p \"$@\" && chmod 700 \"$@\"", &dirs, None)
    }
    #[cfg(not(windows))]
    {
        for name in names {
            let dir = app_home.join(name);
            std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
            restrict_to_owner(&dir, 0o700);
        }
        Ok(())
    }
}

/// Writes one of the files the tools read (the ssh config, known hosts)
/// into the app folder, owner-only, and on Windows mirrors it into the
/// tools' home. The app folder copy is the one the app reads back.
pub fn write_file(app_home: &Path, name: &str, contents: &str) -> anyhow::Result<()> {
    let local = app_home.join(name);
    std::fs::write(&local, contents).with_context(|| format!("write {}", local.display()))?;
    restrict_to_owner(&local, 0o600);
    #[cfg(windows)]
    wsl::put_file(&format!("{}/{name}", home(app_home)), contents.as_bytes())?;
    Ok(())
}

/// The private key path to put in the ssh config. Off Windows it is the key
/// itself. On Windows the key is copied into the tools' home as
/// `keys/<machine id>` with mode 600, because a key on `/mnt/c` looks
/// world-readable to ssh and is refused.
pub fn key_for_tools(app_home: &Path, machine_id: &str, key_path: &str) -> anyhow::Result<String> {
    #[cfg(windows)]
    {
        let key = std::fs::read(key_path).with_context(|| format!("read the key {key_path}"))?;
        let copy = format!("{}/keys/{machine_id}", home(app_home));
        wsl::put_file(&copy, &key)?;
        Ok(copy)
    }
    #[cfg(not(windows))]
    {
        let _ = (app_home, machine_id);
        Ok(key_path.to_string())
    }
}

/// Removes a file the tools made, such as a stale control socket.
pub fn remove_file(path: &str) {
    #[cfg(windows)]
    let _ = wsl::sh("rm -f -- \"$1\"", &[path.to_string()], None);
    #[cfg(not(windows))]
    let _ = std::fs::remove_file(path);
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode));
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path, _mode: u32) {}

// Every long-running child (ssh, rsync, the docker streams) is remembered
// so it ends with the app: `kill_on_drop` never fires when the app simply exits.

#[cfg(not(windows))]
static CHILDREN: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

pub fn track(child: &tokio::process::Child) {
    #[cfg(windows)]
    job::adopt(child);
    #[cfg(not(windows))]
    if let Some(pid) = child.id() {
        CHILDREN.lock().expect("children lock").push(pid);
    }
}

pub fn untrack(pid: Option<u32>) {
    #[cfg(not(windows))]
    if let Some(pid) = pid {
        CHILDREN.lock().expect("children lock").retain(|p| *p != pid);
    }
    #[cfg(windows)]
    let _ = pid;
}

/// Ends every tracked child. Called once, at exit. On Windows the job
/// object does the same by itself if the app crashes.
pub fn end_all_children() {
    #[cfg(windows)]
    job::end_all();
    #[cfg(not(windows))]
    {
        let pids: Vec<String> = CHILDREN.lock().expect("children lock").iter().map(|p| p.to_string()).collect();
        if pids.is_empty() {
            return;
        }
        let _ = native_std("kill").args(&pids).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
    }
}

/// The WSL distribution and its home folder, found once per distribution.
#[cfg(windows)]
mod wsl {
    use std::io::Write;
    use std::process::Stdio;
    use std::sync::RwLock;

    use anyhow::Context;

    struct Chosen {
        distro: String,
        home: Option<String>,
        /// When the distribution last failed to say where its home is.
        failed_at: Option<std::time::Instant>,
    }

    /// Where the tools' files are said to be while the distribution does not
    /// answer: an ssh error then names it, instead of pointing somewhere real.
    const NO_HOME: &str = "/dockernanny-needs-wsl";
    const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(20);

    static CHOSEN: RwLock<Option<Chosen>> = RwLock::new(None);

    pub fn configure(distro: &str) -> bool {
        let distro = if distro.trim().is_empty() { crate::settings::DEFAULT_WSL_DISTRO } else { distro.trim() };
        let mut chosen = CHOSEN.write().expect("wsl lock");
        let previous = chosen.as_ref().map(|c| c.distro.clone());
        if previous.as_deref() == Some(distro) {
            return false;
        }
        *chosen = Some(Chosen { distro: distro.to_string(), home: None, failed_at: None });
        // The first choice is made at start, before the tools are prepared; it is not a change.
        previous.is_some()
    }

    pub fn distro() -> String {
        CHOSEN.read().expect("wsl lock").as_ref().map(|c| c.distro.clone()).unwrap_or_else(|| crate::settings::DEFAULT_WSL_DISTRO.into())
    }

    /// `$HOME` inside the distribution, asked once and kept. While the
    /// distribution does not answer (WSL missing or starting) the answer is
    /// NO_HOME, and it is asked again at most every RETRY_AFTER, so every ssh
    /// command does not start wsl.exe once more.
    pub fn home() -> String {
        let (cached, failed_recently) = {
            let chosen = CHOSEN.read().expect("wsl lock");
            let cached = chosen.as_ref().and_then(|c| c.home.clone());
            let failed_recently = chosen.as_ref().and_then(|c| c.failed_at).is_some_and(|at| at.elapsed() < RETRY_AFTER);
            (cached, failed_recently)
        };
        if let Some(home) = cached {
            return home;
        }
        if failed_recently {
            return NO_HOME.into();
        }
        let out = super::unix_std("sh").args(["-c", "printf %s \"$HOME\""]).stdin(Stdio::null()).output();
        let found = out
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|h| h.starts_with('/'));
        let mut chosen = CHOSEN.write().expect("wsl lock");
        let Some(entry) = chosen.as_mut() else { return found.unwrap_or_else(|| NO_HOME.into()) };
        match found {
            Some(home) => {
                entry.home = Some(home.clone());
                entry.failed_at = None;
                home
            }
            None => {
                entry.failed_at = Some(std::time::Instant::now());
                NO_HOME.into()
            }
        }
    }

    /// A shell script inside the distribution with its arguments as `$1...`,
    /// so paths never need quoting, and optional bytes on stdin.
    pub fn sh(script: &str, args: &[String], stdin: Option<&[u8]>) -> anyhow::Result<()> {
        let mut cmd = super::unix_std("sh");
        cmd.arg("-c").arg(script).arg("sh").args(args);
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = cmd.spawn().context("start wsl.exe (is WSL installed?)")?;
        if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(bytes).context("write to wsl.exe")?;
        }
        let out = child.wait_with_output().context("wait for wsl.exe")?;
        let stderr = crate::host::platform::decode(&out.stderr);
        anyhow::ensure!(out.status.success(), "inside WSL ({}): {}", distro(), stderr.trim());
        Ok(())
    }

    /// Writes bytes to a path inside the distribution, owner-only, creating the folder.
    pub fn put_file(path: &str, contents: &[u8]) -> anyhow::Result<()> {
        sh("umask 077 && mkdir -p \"$(dirname \"$1\")\" && cat > \"$1\"", &[path.to_string()], Some(contents))
    }
}

/// A job object that holds every tracked child: closing it, which Windows
/// also does when the app crashes, ends them all.
#[cfg(windows)]
mod job {
    use std::sync::OnceLock;

    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // A HANDLE is a raw pointer; kept as a number so it can sit in a static.
    static JOB: OnceLock<usize> = OnceLock::new();

    fn handle() -> HANDLE {
        let job = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let size = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
            SetInformationJobObject(job, JobObjectExtendedLimitInformation, &limits as *const _ as *const _, size);
            job as usize
        });
        job as HANDLE
    }

    pub fn adopt(child: &tokio::process::Child) {
        if let Some(raw) = child.raw_handle() {
            unsafe {
                AssignProcessToJobObject(handle(), raw as HANDLE);
            }
        }
    }

    pub fn end_all() {
        unsafe {
            TerminateJobObject(handle(), 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_become_wsl_paths() {
        assert_eq!(wsl_path(r"C:\Users\alex\projects\shop"), "/mnt/c/Users/alex/projects/shop");
        assert_eq!(wsl_path(r"D:\"), "/mnt/d");
        assert_eq!(wsl_path("e:/data/x"), "/mnt/e/data/x");
        assert_eq!(wsl_path("/home/alex/shop"), "/home/alex/shop");
        assert_eq!(wsl_path(r"relative\dir"), "relative/dir");
    }

    #[test]
    fn a_custom_app_folder_gets_its_own_folder_in_wsl() {
        assert_eq!(folder_in_wsl(false, Path::new(r"C:\Users\alex\.dockernanny")), ".dockernanny");
        assert_eq!(folder_in_wsl(true, Path::new("/x/test home")), ".dockernanny-testhome");
        assert_eq!(folder_in_wsl(true, Path::new("/x/.dockernanny-smoke")), ".dockernanny-dockernanny-smoke");
        assert_eq!(folder_in_wsl(true, Path::new("/")), ".dockernanny-custom");
    }

    #[test]
    fn off_windows_the_tools_use_the_app_folder_as_is() {
        if cfg!(windows) {
            return;
        }
        assert_eq!(home(Path::new("/home/alex/.dockernanny")), "/home/alex/.dockernanny");
        assert_eq!(path(Path::new("/home/alex/shop")), "/home/alex/shop");
        assert_eq!(key_for_tools(Path::new("/h"), "m1", "/home/alex/.ssh/id_ed25519").unwrap(), "/home/alex/.ssh/id_ed25519");
    }
}
