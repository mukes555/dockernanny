//! Admin rights, only for the few steps that need them, and never for a
//! window. On Windows the app starts a second copy of itself elevated with
//! `--privileged <task>`; that copy runs one fixed task, writes what it did
//! to a log file (nothing else crosses the UAC boundary) and exits. On the
//! other platforms the single admin command goes through the system prompt
//! directly, so this file only has to parse and describe the task there.

use std::path::{Path, PathBuf};

/// The fixed list of things the elevated copy may do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    /// Windows: firewall rules for sshd and pairing, optionally the power
    /// settings that keep it awake, and optionally one named network marked Private.
    Firewall {
        pairing_port: u16,
        ssh_port: u16,
        keep_awake: bool,
        private_network: Option<String>,
    },
}

pub fn log_path() -> PathBuf {
    crate::store::home_dir().join("elevated.log")
}

/// The arguments the elevated copy is started with; `task_from_args` reads
/// them back. Every value is its own argument, so names with spaces survive.
pub fn task_args(task: &Task, log: &Path) -> Vec<String> {
    let mut args = vec!["--privileged".to_string()];
    match task {
        Task::Firewall { pairing_port, ssh_port, keep_awake, private_network } => {
            args.push("firewall".into());
            args.push("--pairing-port".into());
            args.push(pairing_port.to_string());
            args.push("--ssh-port".into());
            args.push(ssh_port.to_string());
            if *keep_awake {
                args.push("--keep-awake".into());
            }
            if let Some(name) = private_network {
                args.push("--private-network".into());
                args.push(name.clone());
            }
        }
    }
    args.push("--log".into());
    args.push(log.display().to_string());
    args
}

/// None when the process is the normal app.
pub fn task_from_args(args: &[String]) -> Option<(Task, PathBuf)> {
    let start = args.iter().position(|a| a == "--privileged")?;
    let kind = args.get(start + 1)?;
    let value_after = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let log = value_after("--log").map(PathBuf::from).unwrap_or_else(log_path);
    let task = match kind.as_str() {
        "firewall" => Task::Firewall {
            pairing_port: value_after("--pairing-port").and_then(|p| p.parse().ok()).unwrap_or(crate::pairing::PORT),
            ssh_port: value_after("--ssh-port").and_then(|p| p.parse().ok()).unwrap_or(crate::settings::DEFAULT_WSL_SSH_PORT),
            keep_awake: args.iter().any(|a| a == "--keep-awake"),
            private_network: value_after("--private-network"),
        },
        _ => return None,
    };
    Some((task, log))
}

/// Runs the task in this (already elevated) process and returns the exit code.
#[cfg(windows)]
pub fn run_task(task: &Task, log: &Path) -> i32 {
    match task {
        Task::Firewall { pairing_port, ssh_port, keep_awake, private_network } => super::windows_steps::elevated_batch(*pairing_port, *ssh_port, *keep_awake, private_network.as_deref(), log),
    }
}

#[cfg(not(windows))]
pub fn run_task(_task: &Task, _log: &Path) -> i32 {
    0
}

/// Starts this exe again with the UAC prompt, waits, returns its exit code.
#[cfg(windows)]
pub fn run_elevated(task: &Task) -> Result<i32, String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};

    let log = log_path();
    if is_elevated() {
        return Ok(run_task(task, &log));
    }
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let wide = |text: &str| -> Vec<u16> { std::ffi::OsStr::new(text).encode_wide().chain(std::iter::once(0)).collect() };
    let verb = wide("runas");
    let file = wide(&exe.display().to_string());
    // Windows splits the parameter string on spaces, so each value is quoted;
    // a value with a quote in it is refused rather than escaped.
    let quoted: Result<Vec<String>, String> = task_args(task, &log)
        .into_iter()
        .map(|arg| if arg.contains('"') { Err(format!("cannot pass {arg:?} to the administrator step")) } else { Ok(format!("\"{arg}\"")) })
        .collect();
    let parameters = wide(&quoted?.join(" "));

    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        // SW_HIDE: the elevated copy has no window of its own.
        info.nShow = 0;
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("the administrator prompt was refused or failed".into());
        }
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code: u32 = 1;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        Ok(code as i32)
    }
}

#[cfg(not(windows))]
pub fn run_elevated(_task: &Task) -> Result<i32, String> {
    Err("nothing on this platform needs the elevated helper".into())
}

#[cfg(windows)]
fn is_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut size: u32 = std::mem::size_of::<TOKEN_ELEVATION>() as u32;
        let ok = GetTokenInformation(token, TokenElevation, &mut elevation as *mut _ as *mut _, size, &mut size);
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_arguments_round_trip() {
        let task = Task::Firewall { pairing_port: 47433, ssh_port: 2200, keep_awake: true, private_network: Some("Home Wi-Fi".into()) };
        let args = task_args(&task, Path::new("/tmp/x.log"));
        let (parsed, log) = task_from_args(&args).unwrap();
        assert_eq!(parsed, task);
        assert_eq!(log, PathBuf::from("/tmp/x.log"));
        let no_power = Task::Firewall { pairing_port: 47433, ssh_port: 2222, keep_awake: false, private_network: None };
        assert_eq!(task_from_args(&task_args(&no_power, Path::new("/l"))).unwrap().0, no_power);
        assert!(task_from_args(&["--fake".to_string()]).is_none());
        assert!(task_from_args(&["--privileged".to_string(), "format-disk".to_string()]).is_none());
    }
}
