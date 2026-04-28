use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::Spawner;

use super::cmds::run;
use super::cmds::tlb_helper::TlbTable;
use super::{ShellBackend2, line_width_for_backend, print_shell_line};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppsPromptMode {
    Start,
    Pause,
    Unpause,
    Save,
    Load,
    Stop,
    Status,
}

impl AppsPromptMode {
    pub(crate) const fn next(self) -> Self {
        match self {
            Self::Start => Self::Pause,
            Self::Pause => Self::Unpause,
            Self::Unpause => Self::Save,
            Self::Save => Self::Load,
            Self::Load => Self::Stop,
            Self::Stop => Self::Status,
            Self::Status => Self::Start,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Pause => "pause",
            Self::Unpause => "unpause",
            Self::Save => "save",
            Self::Load => "load",
            Self::Stop => "stop",
            Self::Status => "status",
        }
    }
}

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn vm_state_label(state: crate::hv::HvVmState) -> &'static str {
    if !state.supported {
        "unsupported"
    } else if state.stop_requested {
        "stop-pending"
    } else if state.preserve_requested || state.preserve_exit {
        "save-pending"
    } else if state.running {
        "running"
    } else if state.starting {
        "starting"
    } else {
        "offline"
    }
}

fn active_vm_ids() -> Vec<u8> {
    (0..crate::hv::TRUEOS_VM_ID_LIMIT)
        .filter_map(|idx| {
            let vm_id = idx as u8;
            let state = crate::hv::vm_state(vm_id);
            (state.running || state.starting).then_some(vm_id)
        })
        .collect()
}

pub(crate) fn print_status(io: &'static dyn ShellBackend2) {
    const HEADERS: &[&str; 4] = &["vmid", "blueprint", "state", "store"];
    let table = TlbTable::with_width(HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[4, 0, 16, 8]);
    table.emit_header(|text| print_shell_line(io, text));
    for idx in 0..crate::hv::TRUEOS_VM_ID_LIMIT {
        let vm_id = idx as u8;
        let state = crate::hv::vm_state(vm_id);
        if !state.supported {
            continue;
        }
        let vm_id_text = alloc::format!("{}", vm_id);
        let blueprint = crate::hv::app_vm_archive(vm_id).unwrap_or_else(|| String::from("-"));
        let store = if crate::hv::store::has_committed_vm(vm_id) {
            "saved"
        } else {
            "-"
        };
        let row = [
            vm_id_text.as_str(),
            blueprint.as_str(),
            vm_state_label(state),
            store,
        ];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
}

fn print_available(io: &'static dyn ShellBackend2) {
    run::print_app_archive_table(io);
}

fn parse_id(token: Option<&str>) -> Option<u8> {
    token.and_then(|s| s.parse::<u8>().ok())
}

fn stop_vm(io: &'static dyn ShellBackend2, vm_id: u8, label: &str) {
    match crate::hv::stop(vm_id) {
        Ok(true) => line(io, alloc::format!("apps: vm{} {} requested", vm_id, label).as_str()),
        Ok(false) => line(io, alloc::format!("apps: vm{} not running", vm_id).as_str()),
        Err(err) => line(io, alloc::format!("apps: {} failed: {:?}", label, err).as_str()),
    }
}

fn stop_selected_or_all(io: &'static dyn ShellBackend2, id: Option<u8>, label: &str) {
    if let Some(vm_id) = id {
        stop_vm(io, vm_id, label);
        return;
    }
    let active = active_vm_ids();
    if active.is_empty() {
        line(io, "apps: no active app VMs");
        return;
    }
    for vm_id in active {
        stop_vm(io, vm_id, label);
    }
}

fn save_vm(io: &'static dyn ShellBackend2, vm_id: u8) {
    match crate::hv::request_preserve(vm_id) {
        Ok(true) => line(io, alloc::format!("apps: vm{} save requested", vm_id).as_str()),
        Ok(false) => match crate::hv::save_snapshot(vm_id) {
            Ok(bytes) => {
                line(io, alloc::format!("apps: vm{} saved {} bytes", vm_id, bytes).as_str())
            }
            Err(err) => line(io, alloc::format!("apps: save failed: {:?}", err).as_str()),
        },
        Err(err) => line(io, alloc::format!("apps: save failed: {:?}", err).as_str()),
    }
}

fn save_selected_or_all(io: &'static dyn ShellBackend2, id: Option<u8>) {
    if let Some(vm_id) = id {
        save_vm(io, vm_id);
        return;
    }
    let active = active_vm_ids();
    if active.is_empty() {
        line(io, "apps: no active app VMs");
        return;
    }
    for vm_id in active {
        save_vm(io, vm_id);
    }
}

fn load_vm(spawner: &Spawner, io: &'static dyn ShellBackend2, vm_id: u8) {
    match crate::hv::restore_snapshot(vm_id) {
        Ok(bytes) => {
            line(io, alloc::format!("apps: vm{} loaded {} bytes", vm_id, bytes).as_str());
            match crate::hv::start(vm_id, spawner, io, None) {
                Ok(()) => line(io, alloc::format!("apps: vm{} unpause requested", vm_id).as_str()),
                Err(crate::hv::StartError::AlreadyRunning) => {
                    line(io, alloc::format!("apps: vm{} already running", vm_id).as_str())
                }
                Err(err) => line(io, alloc::format!("apps: unpause failed: {:?}", err).as_str()),
            }
        }
        Err(err) => line(io, alloc::format!("apps: load failed: {:?}", err).as_str()),
    }
}

fn load_remote(io: &'static dyn ShellBackend2, endpoint: &str, vm_id: u8) {
    let request = crate::hv::hv_remote_restore_service::RemoteRestoreRequest {
        endpoint: String::from(endpoint),
        vm_id,
    };
    match crate::hv::hv_remote_restore_service::restore_from_remote(request) {
        Ok(bytes) => {
            line(io, alloc::format!("apps: vm{} remote-loaded {} bytes", vm_id, bytes).as_str())
        }
        Err(err) => line(io, alloc::format!("apps: remote load not ready: {:?}", err).as_str()),
    }
}

fn start_app(io: &'static dyn ShellBackend2, mut args: impl Iterator<Item = String>) {
    let Some(id_text) = args.next() else {
        print_available(io);
        return;
    };
    let Ok(id) = id_text.parse::<usize>() else {
        line(io, "apps: start expects an app id");
        print_available(io);
        return;
    };
    let app_args = args.collect::<Vec<_>>();
    run::submit_archive_id(io, id, app_args);
}

pub(crate) fn submit(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mode: AppsPromptMode,
    submitted: &str,
) {
    let mut parts = submitted.split_whitespace();
    let first = parts.next();
    let (action, rest): (AppsPromptMode, Vec<String>) = match first {
        None => (mode, Vec::new()),
        Some("start") => (AppsPromptMode::Start, parts.map(String::from).collect()),
        Some("pause") => (AppsPromptMode::Pause, parts.map(String::from).collect()),
        Some("unpause") => (AppsPromptMode::Unpause, parts.map(String::from).collect()),
        Some("save") => (AppsPromptMode::Save, parts.map(String::from).collect()),
        Some("load") => (AppsPromptMode::Load, parts.map(String::from).collect()),
        Some("stop") => (AppsPromptMode::Stop, parts.map(String::from).collect()),
        Some("status") => (AppsPromptMode::Status, parts.map(String::from).collect()),
        Some(other) => {
            let mut rest = Vec::new();
            rest.push(String::from(other));
            rest.extend(parts.map(String::from));
            (mode, rest)
        }
    };

    match action {
        AppsPromptMode::Start => start_app(io, rest.into_iter()),
        AppsPromptMode::Pause => {
            stop_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "pause")
        }
        AppsPromptMode::Unpause | AppsPromptMode::Load => {
            let mut args = rest.iter();
            let first = args.next().map(String::as_str);
            if let Some(endpoint) = first.filter(|s| s.contains("://")) {
                let vm_id = parse_id(args.next().map(String::as_str)).unwrap_or(0);
                load_remote(io, endpoint, vm_id);
            } else {
                let vm_id = parse_id(first).unwrap_or(0);
                load_vm(spawner, io, vm_id);
            }
        }
        AppsPromptMode::Save => {
            save_selected_or_all(io, parse_id(rest.first().map(String::as_str)))
        }
        AppsPromptMode::Stop => {
            stop_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "stop")
        }
        AppsPromptMode::Status => print_status(io),
    }
}
