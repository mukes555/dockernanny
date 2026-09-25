//! Copying a stack between this computer and the machines: the sources, the
//! plan the sheet shows, and the copy itself with progress on a stack card.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::copy::endpoint::Site;
use crate::copy::local::LocalProject;
use crate::copy::progress::{CopyProgress, Publish, Tracker};
use crate::copy::{self, CopyPlan, CopyRequest, EndpointRef, Report, Sides, Sink};
use crate::stack::{self, Phase, Stack};
use crate::{compose, sync, AppState};

type CmdResult<T> = Result<T, String>;

pub const PROGRESS_EVENT: &str = "copy:progress";

/// What `copy_stack` answers: the records, and which one shows the copy.
#[derive(Debug, Clone, Serialize)]
pub struct CopyStarted {
    pub stacks: Vec<Stack>,
    pub card_id: String,
}

/// Every copy this run of the app has seen, latest state each, so a panel
/// opened late still has the whole picture.
#[tauri::command]
pub fn copy_progress(state: State<'_, AppState>) -> Vec<CopyProgress> {
    let copies = state.copies.lock().expect("copies lock");
    let mut all: Vec<CopyProgress> = copies.values().cloned().collect();
    all.sort_by_key(|p| p.started_ms);
    all
}

#[tauri::command]
pub async fn local_projects(state: State<'_, AppState>) -> CmdResult<Vec<LocalProject>> {
    copy::local::local_projects(&state.ssh).await.map_err(|err| format!("{err:#}"))
}

#[tauri::command]
pub async fn copy_plan(state: State<'_, AppState>, request: CopyRequest) -> CmdResult<CopyPlan> {
    let resolved = resolve(&state, &request)?;
    copy::plan(&state.ssh, &resolved.sides, &request).await.map_err(|err| format!("{err:#}"))
}

/// Saves the destination record when there is a new one and starts the copy;
/// progress arrives on the card as `stack:status` and `stack:output`, and
/// on the panel as `copy:progress`.
#[tauri::command]
pub async fn copy_stack(app: AppHandle, state: State<'_, AppState>, request: CopyRequest) -> CmdResult<CopyStarted> {
    let resolved = resolve(&state, &request)?;
    if let Some(record) = &resolved.new_record {
        let mut stacks = state.store.stacks();
        stacks.push(record.clone());
        state.store.save_stacks(stacks).map_err(|err| format!("{err:#}"))?;
    }
    let Some(card) = resolved.card.clone() else {
        return Err("nothing to show the progress on".into());
    };
    if card.live_sync {
        stack::stop_watcher(&app, &card.id);
    }
    let sides = resolved.sides;
    let tracker = Tracker::new(&card.id, &sides.to.name, &sides.from.label, &sides.to.label, request.destination.clone(), publisher(&app));
    let progress = tracker.shared();
    let card_id = card.id.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let home = state.store.home().to_path_buf();
        let sink_app = app.clone();
        let sink_id = card.id.clone();
        let make_sink = move || -> Sink { Box::new(stack::output_sink(&sink_app, &sink_id)) };
        let status_app = app.clone();
        let status_id = card.id.clone();
        let status = move |phase: Phase, message: &str| {
            stack::set_status(&status_app, &status_id, |s| {
                s.phase = phase;
                s.message = Some(message.to_string());
            });
        };
        let report = Report { make_sink: &make_sink, status: &status, progress };
        match copy::run(&state.ssh, &home, &sides, &request, &report).await {
            Ok(outcome) => stack::set_status(&app, &card.id, |s| {
                if let Some(mirrored) = &outcome.mirrored {
                    s.synced_at_ms = Some(stack::now_ms());
                    s.synced_files = mirrored.files;
                }
                s.message = Some(outcome.summary.text(&sides.to));
            }),
            Err(err) => {
                tracing::warn!("copy failed: {err:#}");
                stack::set_status(&app, &card.id, |s| {
                    s.phase = Phase::Error;
                    s.message = Some(format!("copy failed: {err:#}"));
                });
            }
        }
        stack::refresh_now(&app, &card).await;
        if card.live_sync {
            stack::start_watcher(&app, &card);
        }
    });
    Ok(CopyStarted { stacks: state.store.stacks(), card_id })
}

/// Keeps the latest state per card and tells the window about every change.
fn publisher(app: &AppHandle) -> Publish {
    let app = app.clone();
    Box::new(move |progress: &CopyProgress| {
        let state = app.state::<AppState>();
        state.copies.lock().expect("copies lock").insert(progress.stack_id.clone(), progress.clone());
        let _ = app.emit(PROGRESS_EVENT, progress.clone());
    })
}

struct Resolved {
    sides: Sides,
    /// The stack record whose card shows the progress.
    card: Option<Stack>,
    /// A destination record that does not exist yet.
    new_record: Option<Stack>,
}

/// Turns the request's endpoint references into sites through the store and
/// decides which stack record the progress goes to. Refuses copies that
/// would empty their own source.
fn resolve(state: &AppState, request: &CopyRequest) -> CmdResult<Resolved> {
    let settings = state.store.settings().unwrap_or_default();
    let name = compose::sanitize_name(&request.name);
    if name.is_empty() {
        return Err("Give the copy a name.".into());
    }
    if request.source == request.destination {
        return Err("Pick two different places.".into());
    }
    if !request.config && !request.data {
        return Err("Pick config, data, or both.".into());
    }

    let (from, source_dir, source_record) = match &request.source {
        EndpointRef::ThisComputer => {
            let project = request.project.as_ref().ok_or("Pick a project on this computer.")?;
            let compose_file = copy::local::project_dir_of(project).join(&project.compose_rel);
            if !compose_file.is_file() {
                return Err(format!("{} is not there any more.", project.config_file));
            }
            (Site::local(&project.name, &project.project_dir, &project.compose_rel), project.project_dir.clone(), None)
        }
        EndpointRef::Machine { machine_id } => {
            let stack_id = request.stack_id.as_deref().ok_or("Pick a stack on the machine.")?;
            let stack = state.store.stack(stack_id).ok_or("That stack is no longer known.")?;
            let machine = state.store.machine(machine_id).ok_or("That machine is no longer known.")?;
            if stack.machine_id != machine.id {
                return Err("That stack does not live on that machine.".into());
            }
            let site = Site::machine(&stack.name, &stack.compose_rel, &machine.alias(), &machine.name);
            (site, stack.project_dir.clone(), Some(stack))
        }
    };
    let excludes = if request.excludes.is_empty() { sync::default_excludes() } else { request.excludes.clone() };

    match &request.destination {
        EndpointRef::ThisComputer => {
            let folder = if request.folder.trim().is_empty() { source_dir } else { request.folder.trim().to_string() };
            if folder.is_empty() {
                return Err("Pick a folder on this computer for the project.".into());
            }
            let to = Site::local(&name, &folder, &from.compose_rel);
            Ok(Resolved {
                sides: Sides { from, to, excludes, helper_image: settings.helper_image.clone() },
                card: source_record,
                new_record: None,
            })
        }
        EndpointRef::Machine { machine_id } => {
            let machine = state.store.machine(machine_id).ok_or("That machine is no longer known.")?;
            let to = Site::machine(&name, &from.compose_rel, &machine.alias(), &machine.name);
            let existing = state.store.stacks().into_iter().find(|s| s.machine_id == machine.id && s.name == name);
            let record = existing.clone().unwrap_or_else(|| Stack {
                id: Stack::new_id(),
                name: name.clone(),
                machine_id: machine.id.clone(),
                project_dir: source_dir,
                compose_rel: from.compose_rel.clone(),
                excludes: excludes.clone(),
                forward_ports: request.forward_ports,
                live_sync: false,
                port_overrides: request.port_overrides.clone(),
            });
            Ok(Resolved {
                sides: Sides { from, to, excludes, helper_image: settings.helper_image.clone() },
                card: Some(record.clone()),
                new_record: existing.is_none().then_some(record),
            })
        }
    }
}
