//! Lists the compose projects this computer's Docker knows about, as the copy
//! sheet sees them. Read-only.
//!
//!   cargo run --example projects

use dockernanny_lib::copy::local::local_projects;
use dockernanny_lib::ssh::Ssh;
use dockernanny_lib::store;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let home = store::home_dir();
    std::fs::create_dir_all(&home)?;
    let ssh = Ssh::new(&home)?;
    for project in local_projects(&ssh).await? {
        println!("{} [{}] {}", project.name, project.status, project.project_dir);
        println!("  compose: {}  ports: {:?}", project.compose_rel, project.ports);
        for volume in &project.volumes {
            println!("  volume {} = {} ({}{})", volume.key, volume.name, volume.size, if volume.external { ", external" } else { "" });
        }
        for warning in &project.warnings {
            println!("  note: {warning}");
        }
    }
    Ok(())
}
