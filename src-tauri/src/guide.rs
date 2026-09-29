//! The Windows setup script and the tiny web server that hands it over: the
//! machine fetches it from this computer with one PowerShell line, so nothing
//! has to be copied by hand before SSH exists. The script carries this
//! computer's public key, which is not a secret.

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context;
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::watch;

const TEMPLATE: &str = include_str!("setup.ps1");
pub const SERVE_PORT: u16 = 47431;
pub const FETCHED_EVENT: &str = "guide:fetched";

pub struct ScriptOptions {
    pub public_key: String,
    pub port: u16,
    pub memory_gb: u32,
    pub distro: String,
    /// The machine's lid and sleep settings change only when this is on.
    pub keep_awake: bool,
    /// A Public network is marked Private only when this is on.
    pub make_private: bool,
}

pub fn build_script(options: &ScriptOptions) -> String {
    let powershell_bool = |on: bool| if on { "true" } else { "false" };
    // 0: not chosen, so the machine's own memory line stays and half is written only when there is none.
    let memory_chosen = if options.memory_gb > 0 { format!("{}GB", options.memory_gb) } else { String::new() };
    TEMPLATE
        .replace("__LINUX_SCRIPT__", &crate::host::wsl_script::shared_body())
        .replace("__PUBKEY__", options.public_key.trim())
        .replace("__PORT__", &options.port.to_string())
        .replace("__MEMORY_CHOSEN__", &memory_chosen)
        .replace("__DISTRO__", options.distro.trim())
        .replace("__KEEP_AWAKE__", powershell_bool(options.keep_awake))
        .replace("__MAKE_PRIVATE__", powershell_bool(options.make_private))
}

/// The public half of a key: the `.pub` next to it, or derived with ssh-keygen.
pub async fn public_key(key_path: &str) -> anyhow::Result<String> {
    let pub_path = format!("{key_path}.pub");
    if let Ok(text) = tokio::fs::read_to_string(&pub_path).await {
        return Ok(text.trim().to_string());
    }
    // No stdin: a passphrase prompt must fail at once instead of waiting forever.
    // Windows ships its own ssh-keygen, which reads the key where it is.
    let out = crate::tools::native("ssh-keygen")
        .args(["-y", "-f", key_path])
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .context("run ssh-keygen")?;
    anyhow::ensure!(
        out.status.success(),
        "no {pub_path}, and ssh-keygen could not read the key: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// IPv4 addresses of this computer on its networks, for the fetch command.
pub async fn lan_addresses() -> Vec<String> {
    tokio::task::spawn_blocking(crate::host::platform::lan_addresses).await.unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
pub struct Fetched {
    pub from: String,
    pub at_ms: u64,
}

/// A running script server; dropping it stops the listener.
pub struct ScriptServer {
    stop: watch::Sender<bool>,
    /// The port actually bound: the one asked for, or a free one when 0 was asked.
    pub port: u16,
}

impl Drop for ScriptServer {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

pub async fn serve(port: u16, script: String, on_fetch: impl Fn(Fetched) + Send + Sync + 'static) -> anyhow::Result<ScriptServer> {
    let listener = TcpListener::bind(SocketAddr::from(([0, 0, 0, 0], port))).await.with_context(|| format!("listen on port {port}"))?;
    let bound = listener.local_addr()?.port();
    let (stop, mut stopped) = watch::channel(false);
    let on_fetch = std::sync::Arc::new(on_fetch);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                accepted = listener.accept() => {
                    let Ok((socket, peer)) = accepted else { continue };
                    let body = script.clone();
                    let on_fetch = on_fetch.clone();
                    tokio::spawn(async move {
                        if let Ok(socket) = answer(socket, &body).await {
                            on_fetch(Fetched { from: peer.ip().to_string(), at_ms: now_ms() });
                            finish(socket).await;
                        }
                    });
                }
                _ = stopped.changed() => break,
            }
        }
    });
    Ok(ScriptServer { stop, port: bound })
}

/// Answers `GET /setup.ps1` with the script and everything else with a 404,
/// so a port scanner does not count as "the machine fetched it". Hands the
/// socket back on a fetch, so the caller can report it before the close.
async fn answer(mut socket: tokio::net::TcpStream, body: &str) -> anyhow::Result<tokio::net::TcpStream> {
    let mut request = [0u8; 2048];
    let read = tokio::time::timeout(std::time::Duration::from_secs(5), socket.read(&mut request)).await.context("request timed out")??;
    let first_line = String::from_utf8_lossy(&request[..read]).lines().next().unwrap_or("").to_string();
    if !is_script_request(&first_line) {
        let _ = socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
        finish(socket).await;
        anyhow::bail!("not a script request");
    }
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
    socket.write_all(response.as_bytes()).await?;
    Ok(socket)
}

/// Closes the connection the polite way: our side first, then wait for the
/// client to hang up. Closing while request bytes are still unread makes the
/// kernel send a reset instead of the answer, which macOS does readily.
async fn finish(mut socket: tokio::net::TcpStream) {
    let _ = socket.shutdown().await;
    let mut sink = [0u8; 1024];
    let drained = async {
        while let Ok(n) = socket.read(&mut sink).await {
            if n == 0 {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(std::time::Duration::from_secs(2), drained).await;
}

fn is_script_request(first_line: &str) -> bool {
    let mut parts = first_line.split_whitespace();
    parts.next() == Some("GET") && parts.next() == Some("/setup.ps1")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn key_exists(key_path: &str) -> bool {
    Path::new(key_path).is_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    impl ScriptOptions {
        fn sample() -> Self {
            ScriptOptions {
                public_key: "k".into(),
                port: 2222,
                memory_gb: 8,
                distro: "Ubuntu".into(),
                keep_awake: false,
                make_private: false,
            }
        }
    }

    #[test]
    fn script_carries_the_options_and_no_placeholders() {
        let script = build_script(&ScriptOptions {
            public_key: "ssh-ed25519 AAAAC3 alex@studio\n".into(),
            port: 2222,
            memory_gb: 8,
            distro: "Ubuntu".into(),
            keep_awake: true,
            make_private: false,
        });
        assert!(script.contains("$pubkey = 'ssh-ed25519 AAAAC3 alex@studio'"));
        assert!(script.contains("$port = 2222"));
        assert!(script.contains("$memoryChosen = '8GB'"));
        assert!(script.contains("$distro = 'Ubuntu'"));
        assert!(script.contains("$keepAwake = $true"));
        assert!(script.contains("$makePrivate = $false"), "the network is left alone unless asked");
        assert!(script.contains("set_ini()") && script.contains("echo dockernanny-linux-ok"), "the Linux part is the shared script");
        assert!(!script.contains("__"));
        let not_chosen = build_script(&ScriptOptions { memory_gb: 0, ..ScriptOptions::sample() });
        assert!(not_chosen.contains("$memoryChosen = ''"), "the machine's own memory line is kept");
    }

    #[test]
    fn only_the_script_path_counts_as_a_fetch() {
        assert!(is_script_request("GET /setup.ps1 HTTP/1.1"));
        assert!(!is_script_request("GET / HTTP/1.1"));
        assert!(!is_script_request("POST /setup.ps1 HTTP/1.1"));
        assert!(!is_script_request(""));
    }

    #[tokio::test]
    async fn a_bare_connection_is_not_a_fetch() {
        let fetched = std::sync::Arc::new(std::sync::Mutex::new(0u32));
        let seen = fetched.clone();
        let server = serve(0, "x".into(), move |_| *seen.lock().unwrap() += 1).await.unwrap();
        let port = server.port;
        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"HELLO\r\n\r\n").await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 404"));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(*fetched.lock().unwrap(), 0);
    }

    #[tokio::test]
    async fn server_answers_with_the_script() {
        let fetched = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = fetched.clone();
        let server = serve(0, "Write-Host hi".into(), move |f| seen.lock().unwrap().push(f.from)).await.unwrap();
        let port = server.port;

        let mut client = tokio::net::TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        client.write_all(b"GET /setup.ps1 HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).await.unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("Write-Host hi"));
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(fetched.lock().unwrap().as_slice(), ["127.0.0.1"]);
    }

    #[tokio::test]
    async fn public_key_comes_from_the_pub_file() {
        let dir = std::env::temp_dir().join(format!("dockernanny-key-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let key = dir.join("id_test");
        std::fs::write(&key, "private").unwrap();
        std::fs::write(dir.join("id_test.pub"), "ssh-ed25519 AAAA test@mac\n").unwrap();
        let text = public_key(key.to_str().unwrap()).await.unwrap();
        assert_eq!(text, "ssh-ed25519 AAAA test@mac");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
