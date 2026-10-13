//! Select a retained VM slot for execution control, or an AppDB app to start.
use super::{
    MatrixSlotLease, MatrixTarget, helper,
    picker::{Event, Picker},
    service,
    tui::{self, Frontend},
};
use alloc::{format, string::String, vec::Vec};
use trueos_time::{Duration, Timer};

pub(super) fn recognizes(name: &str) -> bool {
    matches!(name, "start" | "stop" | "pause" | "resume" | "kill")
}

/// The hypervisor rechecks this lease under lifecycle admission, so a stale
/// selection cannot control a later guest that happens to reuse its VM id.
pub(super) fn control(action: &str, lease: &MatrixSlotLease) -> String {
    let Some(vm) = crate::hv::matrix::vm_for_lease(lease) else {
        return format!("{action}: selected VM slot is no longer active");
    };
    let result = match action {
        "stop" => crate::hv::stop_for_matrix_slot(vm, lease).map_err(|error| format!("{error:?}")),
        "pause" => {
            crate::hv::pause_for_matrix_slot(vm, lease).map_err(|error| format!("{error:?}"))
        }
        "resume" => {
            crate::hv::resume_for_matrix_slot(vm, lease).map_err(|error| format!("{error:?}"))
        }
        "kill" => crate::hv::kill_for_matrix_slot(vm, lease).map_err(|error| format!("{error:?}")),
        _ => return format!("{action}: unsupported VM control"),
    };
    match result {
        Ok(true) => format!("{action}: vm{vm} request accepted"),
        Ok(false) => format!("{action}: vm{vm} state changed or request already satisfied"),
        Err(error) => format!("{action}: vm{vm} {error}"),
    }
}

enum Choice {
    App(String),
    Vm(MatrixSlotLease),
}

fn choices(action: &str) -> Vec<(String, Choice)> {
    if action == "start" {
        return service::appdb_names_snapshot()
            .1
            .into_iter()
            .map(|name| (name.clone(), Choice::App(name)))
            .collect();
    }
    let mut choices = Vec::new();
    for id in 0..crate::hv::status().vm_id_limit {
        let vm = id as u8;
        let state = crate::hv::vm_state(vm);
        if !state.running && !state.starting {
            continue;
        }
        let Some(target) = crate::hv::blueprint_console_target(vm) else {
            continue;
        };
        let lease = super::matrix_target_slot_lease(&target);
        let label = crate::hv::app_vm_display_label(vm).unwrap_or_else(|| "-".into());
        choices.push((
            format!("§{} vm{} {} {}", lease.name(), vm, label, service::hv_vm_phase(&state)),
            Choice::Vm(lease),
        ));
    }
    choices
}

pub(super) fn start(action: &str, frontend: Frontend) -> Result<(), String> {
    if !recognizes(action) {
        return Err("Unknown VM control.".into());
    }
    let Some((target, spawner)) = helper::admit(action, frontend)? else {
        return Ok(());
    };
    let token = match lifecycle_task(action.into(), target.clone()) {
        Ok(token) => token,
        Err(_) => {
            tui::cancel_native_attach(&target);
            return Err("VM control task pool is full.".into());
        }
    };
    spawner.spawn(token);
    Ok(())
}

#[trueos_executor::task(pool_size = 5)]
async fn lifecycle_task(action: String, target: MatrixTarget) {
    let mut picker = Picker::default();
    let mut screen = helper::Screen::default();
    let mut entries = choices(&action);
    let mut result = String::new();
    let mut visible = true;
    while let Some((bytes, _)) = tui::native_read(&target) {
        if !tui::native_visible(&target) {
            visible = false;
            Timer::after(Duration::from_millis(20)).await;
            continue;
        }
        if !visible {
            entries = choices(&action);
            picker.selected = picker.selected.min(entries.len().saturating_sub(1));
            visible = true;
        }
        let Some(surface) = tui::surface(&target) else {
            break;
        };
        let now = crate::chronos::monotonic_nanos();
        match picker.input(&bytes, entries.len(), surface.rows as usize, now) {
            Some(Event::Quit) => {
                tui::native_return(&target);
            }
            Some(Event::Choose(index)) => {
                if let Some((_, choice)) = entries.get(index) {
                    match choice {
                        Choice::Vm(lease) => result = control(&action, lease),
                        Choice::App(name) => {
                            if let Some(frontend) = tui::native_frontend(&target) {
                                match service::launch_appdb(name, frontend) {
                                    Ok(app) => tui::native_launch(&target, app),
                                    Err(error) => result = error,
                                }
                            }
                        }
                    }
                }
                entries = choices(&action);
                picker.selected = picker.selected.min(entries.len().saturating_sub(1));
            }
            _ => {}
        }
        let summary = if result.is_empty() {
            if entries.is_empty() {
                "No eligible apps or VM slots."
            } else {
                "Select an app or VM slot."
            }
        } else {
            &result
        };
        let labels: Vec<&str> = entries.iter().map(|(label, _)| label.as_str()).collect();
        let lines = picker.frame(&format!("VM {action}"), summary, &labels, surface.rows as usize);
        screen.paint(&target, &lines, surface.cols as usize, surface.rows as usize);
        Timer::after(Duration::from_millis(20)).await;
    }
}
