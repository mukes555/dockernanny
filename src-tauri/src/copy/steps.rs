//! The list of steps a copy takes, decided up front so the progress panel
//! shows what is still to come. The names are written once, here, so the
//! planned list and the steps as they run are the same strings.

use super::discover::Download;
use super::{CopyRequest, Inventory, Sides};

pub mod names {
    pub fn look() -> String {
        "looking at both ends".into()
    }
    pub fn download(image: &str, label: &str) -> String {
        format!("downloading {image} on {label}")
    }
    pub fn stop(name: &str, label: &str) -> String {
        format!("stopping {name} on {label}")
    }
    pub fn folder(label: &str) -> String {
        format!("copying the project folder to {label}")
    }
    pub fn volume(key: &str, size: &str) -> String {
        format!("copying volume {key} ({size})")
    }
    pub fn image(image: &str) -> String {
        format!("sending image {image}")
    }
    pub fn create(label: &str) -> String {
        format!("creating the containers on {label}")
    }
    pub fn path(path: &str, service: &str) -> String {
        format!("copying {path} from {service}")
    }
    pub fn start(name: &str, label: &str) -> String {
        format!("starting {name} on {label}")
    }
    pub fn restart(name: &str, label: &str) -> String {
        format!("starting {name} again on {label}")
    }
    pub fn check(label: &str) -> String {
        format!("checking the result on {label}")
    }
}

/// Everything the copy will do, in order, once it has looked at both ends.
pub(crate) fn planned(sides: &Sides, request: &CopyRequest, inventory: Option<&Inventory>, downloads: &[Download], destination_exists: bool, source_running: bool) -> Vec<String> {
    let (from, to) = (&sides.from, &sides.to);
    let mut steps = vec![names::look()];
    // Before anything stops, so a missing image ends the copy early.
    for download in downloads {
        steps.push(names::download(&download.image, &to.label));
    }
    if request.data && destination_exists {
        steps.push(names::stop(&to.name, &to.label));
    }
    let stops_source = (request.stop_source || request.keep_source_stopped) && source_running;
    if stops_source {
        steps.push(names::stop(&from.name, &from.label));
    }
    if request.config {
        steps.push(names::folder(&to.label));
    }
    if let Some(inventory) = inventory {
        for volume in &inventory.volumes {
            steps.push(names::volume(&volume.key, &volume.size));
        }
        for image in &inventory.images {
            steps.push(names::image(image));
        }
        steps.push(names::create(&to.label));
        for selection in &request.data_selection {
            steps.push(names::path(&selection.path, &selection.service));
        }
    }
    steps.push(names::start(&to.name, &to.label));
    if stops_source && !request.keep_source_stopped {
        steps.push(names::restart(&from.name, &from.label));
    }
    steps.push(names::check(&to.label));
    steps
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::copy::discover::NamedVolume;
    use crate::copy::endpoint::Site;
    use crate::copy::{DataSelection, EndpointRef};

    fn request(config: bool, data: bool, stop: bool, leave: bool) -> CopyRequest {
        CopyRequest {
            source: EndpointRef::ThisComputer,
            project: None,
            stack_id: None,
            destination: EndpointRef::Machine { machine_id: "m1".into() },
            folder: String::new(),
            name: "shop".into(),
            config,
            data,
            data_selection: vec![DataSelection { service: "db".into(), path: "/var/lib/postgresql/data".into() }],
            stop_source: stop,
            keep_source_stopped: leave,
            port_overrides: HashMap::new(),
            excludes: Vec::new(),
            forward_ports: true,
        }
    }

    fn sides() -> Sides {
        Sides {
            from: Site::local("shop-api", "/home/alex/shop", "docker-compose.yml"),
            to: Site::machine("shop", "docker-compose.yml", "dn-m1", "studio"),
            excludes: Vec::new(),
            helper_image: "alpine:3".into(),
        }
    }

    #[test]
    fn the_step_list_follows_the_shape_of_the_copy() {
        let config_only = planned(&sides(), &request(true, false, false, false), None, &[], false, true);
        assert_eq!(config_only, vec!["looking at both ends", "copying the project folder to studio", "starting shop on studio", "checking the result on studio"]);

        let inventory = Inventory {
            volumes: vec![NamedVolume { key: "pgdata".into(), name: "shop-api_pgdata".into(), size: "412MB".into(), external: false }],
            images: vec!["shop-api-worker:local".into()],
        };
        let downloads = [Download { image: "postgres:16".into(), platform: None }];
        let both = planned(&sides(), &request(true, true, true, false), Some(&inventory), &downloads, true, true);
        assert_eq!(
            both,
            vec![
                "looking at both ends",
                "downloading postgres:16 on studio",
                "stopping shop on studio",
                "stopping shop-api on this computer",
                "copying the project folder to studio",
                "copying volume pgdata (412MB)",
                "sending image shop-api-worker:local",
                "creating the containers on studio",
                "copying /var/lib/postgresql/data from db",
                "starting shop on studio",
                "starting shop-api again on this computer",
                "checking the result on studio",
            ]
        );

        let moved = planned(&sides(), &request(false, true, false, true), Some(&inventory), &[], true, false);
        assert!(!moved.iter().any(|s| s.contains("project folder")), "config off: no folder step");
        assert!(!moved.iter().any(|s| s.starts_with("stopping shop-api")), "a source that is not running is not stopped");
        assert!(!moved.iter().any(|s| s.contains("again")), "a move never starts the source again");
    }
}
