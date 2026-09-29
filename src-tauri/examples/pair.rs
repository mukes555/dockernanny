//! Headless pairing against a machine (or against this computer running the app
//! with sharing on and `DOCKERNANNY_FAKE_HOST=1`):
//!
//!   cargo run --example pair -- 127.0.0.1 481923 ~/.ssh/id_ed25519

use dockernanny_lib::{guide, pairing};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [address, code, key] = args.as_slice() else {
        eprintln!("usage: pair <address> <code> <key_path>");
        std::process::exit(2);
    };
    let pubkey = guide::public_key(key).await?;
    let answer = pairing::pair(address, pairing::PORT, code, &pubkey, &pairing::computer_name().await).await?;
    println!(
        "ok={} user={} port={} hostname={} host_key={} error={}",
        answer.ok, answer.user, answer.port, answer.hostname, answer.host_key, answer.error
    );
    Ok(())
}
