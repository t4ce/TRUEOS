use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use embassy_executor::Spawner;

use super::cmds::run;
use super::cmds::tlb_helper::TlbTable;
use super::{
    MatrixTarget, ShellBackend2, line_width_for_backend, matrix_target_for_backend,
    print_matrix_target_system_line as print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum AppsCommand {
    Start,
    Online,
    Dl,
    Peer,
    Pause,
    Preserve,
    Load,
    Stop,
    Kick,
    Status,
}

impl AppsCommand {
    const fn label(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Online => "online",
            Self::Dl => "dl",
            Self::Peer => "peer",
            Self::Pause => "pause",
            Self::Preserve => "preserve",
            Self::Load => "load",
            Self::Stop => "stop",
            Self::Kick => "kick",
            Self::Status => "status",
        }
    }
}

const APP_COMMANDS: [AppsCommand; 10] = [
    AppsCommand::Start,
    AppsCommand::Online,
    AppsCommand::Dl,
    AppsCommand::Peer,
    AppsCommand::Pause,
    AppsCommand::Preserve,
    AppsCommand::Load,
    AppsCommand::Stop,
    AppsCommand::Kick,
    AppsCommand::Status,
];

pub(crate) fn command_names_text() -> String {
    let mut out = String::new();
    for command in APP_COMMANDS {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(command.label());
    }
    out
}

fn line(io: &'static dyn ShellBackend2, text: &str) {
    print_shell_line(io, text);
}

fn vm_state_label(state: crate::hv::HvVmState) -> &'static str {
    if !state.supported {
        "unsupported"
    } else if state.restore_inflight {
        "load-pending"
    } else if state.stop_requested {
        "stop-pending"
    } else if state.prepare_pause_pending {
        "prepare-pause"
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
    print_hv_status(io);
}

fn replicatable_state_label(state: crate::hv::HvVmState, stored: bool) -> &'static str {
    if state.restore_inflight {
        "resuming"
    } else if state.prepare_pause_pending {
        "prepare-pause"
    } else if state.lifecycle_ready && (state.running || state.starting) {
        "ready"
    } else if state.pause_latched && (state.running || state.starting) {
        "pause-pending"
    } else if state.running {
        "running"
    } else if state.starting {
        "starting"
    } else if state.pause_latched && stored {
        "paused"
    } else if state.pause_latched {
        "latch-pending"
    } else {
        "offline"
    }
}

fn print_replicatable_vms(io: &'static dyn ShellBackend2) {
    const HEADERS: &[&str; 4] = &["vmid", "blueprint", "state", "store"];
    let table = TlbTable::with_width(HEADERS, line_width_for_backend(io).saturating_sub(2))
        .with_max_col_widths(&[4, 0, 16, 8]);
    let mut found = false;
    table.emit_header(|text| print_shell_line(io, text));
    for idx in 0..crate::hv::TRUEOS_VM_ID_LIMIT {
        let vm_id = idx as u8;
        let state = crate::hv::vm_state(vm_id);
        if !state.replicatable || !(state.running || state.starting || state.pause_latched) {
            continue;
        }
        found = true;
        let stored = state.pause_snapshot_ready;
        let vm_id_text = alloc::format!("{}", vm_id);
        let blueprint = crate::hv::app_vm_archive(vm_id).unwrap_or_else(|| String::from("-"));
        let row = [
            vm_id_text.as_str(),
            blueprint.as_str(),
            replicatable_state_label(state, stored),
            if stored { "saved" } else { "-" },
        ];
        table.emit_row(&row, |text| print_shell_line(io, text));
    }
    table.emit_footer(|text| print_shell_line(io, text));
    if !found {
        line(io, "apps: no running or paused replicatable Blueprints");
    } else {
        line(io, "apps: enter a vmid to toggle pause/resume");
    }
}

fn format_bytes(bytes: usize) -> String {
    const KIB: usize = 1024;
    const MIB: usize = 1024 * KIB;
    const GIB: usize = 1024 * MIB;

    if bytes >= GIB {
        alloc::format!("{} GiB", bytes / GIB)
    } else if bytes >= MIB {
        alloc::format!("{} MiB", bytes / MIB)
    } else if bytes >= KIB {
        alloc::format!("{} KiB", bytes / KIB)
    } else {
        alloc::format!("{} B", bytes)
    }
}

fn active_vm_ids_text(status: &crate::hv::HvStatus) -> String {
    let mut out = String::new();
    for maybe_id in status.active_vm_ids {
        if let Some(id) = maybe_id {
            if !out.is_empty() {
                out.push(',');
            }
            let _ = write!(out, "{}", id);
        }
    }
    if out.is_empty() {
        out.push('-');
    }
    out
}

fn print_hv_status(io: &'static dyn ShellBackend2) {
    let status = crate::hv::status();
    let heap_used = status
        .vm_shared_heap_total_bytes
        .saturating_sub(status.vm_shared_heap_free_bytes);

    line(
        io,
        alloc::format!(
            "apps: slots running={} starting={} limit={} active={}",
            status.running_count,
            status.starting_count,
            status.vm_id_limit,
            active_vm_ids_text(&status)
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "apps: shared heap used={} total={} free={}",
            format_bytes(heap_used),
            format_bytes(status.vm_shared_heap_total_bytes),
            format_bytes(status.vm_shared_heap_free_bytes)
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "apps: shared stack={} vmx_state={} stored_snapshots={}",
            format_bytes(status.vm_shared_stack_bytes),
            format_bytes(status.vm_shared_vmx_bytes),
            status.stored_vm_count
        )
        .as_str(),
    );
    line(
        io,
        alloc::format!(
            "apps: vmx vendor_intel={} has_vmx={} feature_control_locked={} outside_smx={} guest_module={}",
            status.vendor_intel,
            status.has_vmx,
            status.feature_control_locked,
            status.feature_control_vmx_outside_smx,
            status.guest_module_present
        )
        .as_str(),
    );
}

fn online_app(spawner: &Spawner, io: &'static dyn ShellBackend2, args: Vec<String>) {
    super::shell2_dl::submit_online(spawner, io, args.join(" ").as_str());
}

pub(crate) fn submit_online(spawner: &Spawner, io: &'static dyn ShellBackend2, submitted: &str) {
    super::shell2_dl::submit_online(spawner, io, submitted);
}

const PEER_HEADERS: &[&str; 5] = &["id", "peer", "node", "port", "vms"];

fn peer_vms_text(offers: &[crate::r::net::trueos_peer::PeerVmOffer]) -> String {
    if offers.is_empty() {
        return String::from("none");
    }
    let mut out = String::new();
    for offer in offers {
        if !out.is_empty() {
            out.push(',');
        }
        let _ = write!(out, "{}", offer.vm_id);
    }
    out
}

async fn query_peer_offers(
    peer: &crate::r::net::trueos_peer::PeerSnapshot,
) -> Result<Vec<crate::r::net::trueos_peer::PeerVmOffer>, String> {
    crate::r::net::trueos_peer::list_peer_vms(peer)
        .await
        .map_err(|err| alloc::format!("{:?}", err))
}

async fn print_peer_table(target: &MatrixTarget, width: usize) {
    let peers = crate::r::net::trueos_peer::peer_snapshots();
    if peers.is_empty() {
        print_matrix_target_line(target, "apps: no TRUEOS peers detected");
        return;
    }

    let table = TlbTable::with_width(PEER_HEADERS, width.saturating_sub(2))
        .with_max_col_widths(&[4, 15, 18, 5, 0]);
    table.emit_header(|text| print_matrix_target_line(target, text));
    for peer in peers {
        let id = alloc::format!("{}", peer.id);
        let addr = crate::r::net::trueos_peer::peer_addr_text(&peer);
        let node = alloc::format!("{:016X}", peer.node_id);
        let port = alloc::format!("{}", peer.port);
        let vms = match query_peer_offers(&peer).await {
            Ok(offers) => peer_vms_text(offers.as_slice()),
            Err(err) => alloc::format!("err:{}", err.as_str()),
        };
        let row = [
            id.as_str(),
            addr.as_str(),
            node.as_str(),
            port.as_str(),
            vms.as_str(),
        ];
        table.emit_row(&row, |text| print_matrix_target_line(target, text));
    }
    table.emit_footer(|text| print_matrix_target_line(target, text));
}

#[embassy_executor::task(pool_size = 2)]
async fn peer_app_task(
    target: MatrixTarget,
    width: usize,
    args: Vec<String>,
    spawner: Spawner,
    io: &'static dyn ShellBackend2,
) {
    if args.is_empty() {
        print_peer_table(&target, width).await;
        set_matrix_target_active(&target, false);
        return;
    }

    let peers = crate::r::net::trueos_peer::peer_snapshots();
    let Some(peer_id) = args.first().and_then(|arg| arg.parse::<usize>().ok()) else {
        print_matrix_target_line(&target, "apps: peer expects a peer id");
        print_peer_table(&target, width).await;
        set_matrix_target_active(&target, false);
        return;
    };
    let Some(peer) = peers.get(peer_id).cloned() else {
        let available = if peers.is_empty() {
            String::from("none")
        } else {
            let mut ids = String::new();
            for peer in peers.iter() {
                if !ids.is_empty() {
                    ids.push(',');
                }
                let _ = write!(ids, "{}", peer.id);
            }
            ids
        };
        print_matrix_target_line(
            &target,
            alloc::format!("apps: unknown peer id {} (available: {})", peer_id, available.as_str())
                .as_str(),
        );
        print_peer_table(&target, width).await;
        set_matrix_target_active(&target, false);
        return;
    };

    let Some(remote_vm_id) = args.get(1).and_then(|arg| arg.parse::<u8>().ok()) else {
        match query_peer_offers(&peer).await {
            Ok(offers) => {
                let addr = crate::r::net::trueos_peer::peer_addr_text(&peer);
                print_matrix_target_line(
                    &target,
                    alloc::format!(
                        "apps: peer {} {} vms={}",
                        peer.id,
                        addr.as_str(),
                        peer_vms_text(offers.as_slice()).as_str()
                    )
                    .as_str(),
                );
            }
            Err(err) => print_matrix_target_line(
                &target,
                alloc::format!("apps: peer query failed: {}", err).as_str(),
            ),
        }
        set_matrix_target_active(&target, false);
        return;
    };

    if !crate::hv::cross_principal_snapshot_restore_supported() {
        print_matrix_target_line(
            &target,
            "apps: peer snapshot resume is safety-gated until guest-writable backing relocation is implemented",
        );
        set_matrix_target_active(&target, false);
        return;
    }

    let local_vm_id = args
        .get(2)
        .and_then(|arg| arg.parse::<u8>().ok())
        .or_else(crate::hv::first_free_vm_id)
        .unwrap_or(remote_vm_id);
    let addr = crate::r::net::trueos_peer::peer_addr_text(&peer);
    print_matrix_target_line(
        &target,
        alloc::format!(
            "apps: fetching peer {} {} vm{} -> local vm{}",
            peer.id,
            addr.as_str(),
            remote_vm_id,
            local_vm_id
        )
        .as_str(),
    );

    match crate::r::net::trueos_peer::fetch_peer_vm(&peer, remote_vm_id).await {
        Ok(bytes) => {
            let fetched = bytes.len();
            match crate::hv::store::save_bytes_async(local_vm_id, bytes).await {
                Ok(saved) => {
                    print_matrix_target_line(
                        &target,
                        alloc::format!("apps: peer vm{} fetched {} bytes", local_vm_id, saved)
                            .as_str(),
                    );
                    match crate::hv::restore_snapshot_async(local_vm_id).await {
                        Ok(loaded) => {
                            print_matrix_target_line(
                                &target,
                                alloc::format!(
                                    "apps: vm{} loaded peer snapshot {} bytes",
                                    local_vm_id,
                                    loaded
                                )
                                .as_str(),
                            );
                            match crate::hv::start(local_vm_id, &spawner, io, None) {
                                Ok(()) => print_matrix_target_line(
                                    &target,
                                    alloc::format!("apps: vm{} peer start requested", local_vm_id)
                                        .as_str(),
                                ),
                                Err(crate::hv::StartError::AlreadyRunning) => {
                                    print_matrix_target_line(
                                        &target,
                                        alloc::format!("apps: vm{} already running", local_vm_id)
                                            .as_str(),
                                    )
                                }
                                Err(err) => print_matrix_target_line(
                                    &target,
                                    alloc::format!("apps: peer start failed: {:?}", err).as_str(),
                                ),
                            }
                        }
                        Err(err) => print_matrix_target_line(
                            &target,
                            alloc::format!("apps: peer restore failed: {:?}", err).as_str(),
                        ),
                    }
                }
                Err(err) => print_matrix_target_line(
                    &target,
                    alloc::format!(
                        "apps: peer save failed after {} fetched bytes: {:?}",
                        fetched,
                        err
                    )
                    .as_str(),
                ),
            }
        }
        Err(err) => print_matrix_target_line(
            &target,
            alloc::format!("apps: peer fetch failed: {:?}", err).as_str(),
        ),
    }

    set_matrix_target_active(&target, false);
}

fn peer_app(spawner: &Spawner, io: &'static dyn ShellBackend2, args: Vec<String>) {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io);
    set_matrix_target_active(&target, true);
    match peer_app_task(target.clone(), width, args, *spawner, io) {
        Ok(token) => {
            spawner.spawn(token);
        }
        Err(_) => {
            set_matrix_target_active(&target, false);
            line(io, "apps: peer task unavailable");
        }
    }
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

fn kick_vm(io: &'static dyn ShellBackend2, id: Option<u8>) {
    let Some(vm_id) = id else {
        line(io, "apps: kick expects a vmid");
        return;
    };
    match crate::hv::kick(vm_id) {
        Ok(true) => line(io, alloc::format!("apps: vm{} kick sent", vm_id).as_str()),
        Ok(false) => {
            line(io, alloc::format!("apps: vm{} is not running or has no owner", vm_id).as_str())
        }
        Err(err) => line(io, alloc::format!("apps: kick failed: {:?}", err).as_str()),
    }
}

fn preserve_vm(io: &'static dyn ShellBackend2, vm_id: u8) {
    match crate::hv::request_preserve(vm_id) {
        Ok(true) => line(io, alloc::format!("apps: vm{} preserve requested", vm_id).as_str()),
        Ok(false) => line(
            io,
            alloc::format!("apps: vm{} is not running; preserve must precede stop", vm_id).as_str(),
        ),
        Err(err) => line(io, alloc::format!("apps: preserve failed: {:?}", err).as_str()),
    }
}

fn preserve_selected_or_all(io: &'static dyn ShellBackend2, id: Option<u8>) {
    if let Some(vm_id) = id {
        preserve_vm(io, vm_id);
        return;
    }
    let active = active_vm_ids();
    if active.is_empty() {
        line(io, "apps: no active app VMs");
        return;
    }
    for vm_id in active {
        preserve_vm(io, vm_id);
    }
}

async fn load_vm(spawner: &Spawner, io: &'static dyn ShellBackend2, vm_id: u8) -> bool {
    match crate::hv::restore_snapshot_async(vm_id).await {
        Ok(bytes) => {
            line(io, alloc::format!("apps: vm{} loaded {} bytes", vm_id, bytes).as_str());
            match crate::hv::start(vm_id, spawner, io, None) {
                Ok(()) => {
                    line(io, alloc::format!("apps: vm{} resume requested", vm_id).as_str());
                    true
                }
                Err(crate::hv::StartError::AlreadyRunning) => {
                    line(io, alloc::format!("apps: vm{} already running", vm_id).as_str());
                    false
                }
                Err(err) => {
                    line(io, alloc::format!("apps: resume failed: {:?}", err).as_str());
                    false
                }
            }
        }
        Err(err) => {
            line(io, alloc::format!("apps: load failed: {:?}", err).as_str());
            false
        }
    }
}

#[embassy_executor::task(pool_size = 4)]
async fn load_vm_task(spawner: Spawner, io: &'static dyn ShellBackend2, vm_id: u8) {
    let _ = load_vm(&spawner, io, vm_id).await;
    crate::hv::finish_restore(vm_id);
}

fn schedule_load_vm(spawner: &Spawner, io: &'static dyn ShellBackend2, vm_id: u8) {
    match crate::hv::try_begin_restore(vm_id) {
        Ok(false) => {
            line(io, alloc::format!("apps: vm{} load already pending", vm_id).as_str());
        }
        Err(err) => {
            line(io, alloc::format!("apps: load failed: {:?}", err).as_str());
        }
        Ok(true) => match load_vm_task(*spawner, io, vm_id) {
            Ok(token) => {
                line(io, alloc::format!("apps: vm{} resume scheduled", vm_id).as_str());
                spawner.spawn(token);
            }
            Err(_) => {
                crate::hv::finish_restore(vm_id);
                line(io, "apps: load task unavailable");
            }
        },
    }
}

fn toggle_replicatable_vm(spawner: &Spawner, io: &'static dyn ShellBackend2, vm_id: u8) {
    let state = crate::hv::vm_state(vm_id);
    if !state.supported {
        line(io, alloc::format!("apps: unsupported vmid {}", vm_id).as_str());
        print_replicatable_vms(io);
        return;
    }
    if !state.replicatable {
        line(io, alloc::format!("apps: vm{} is not tagged replicatable", vm_id).as_str());
        print_replicatable_vms(io);
        return;
    }

    if state.running || state.starting {
        match crate::hv::request_replicatable_pause(vm_id) {
            Ok(true) => line(
                io,
                alloc::format!(
                    "apps: vm{} PreparePause requested; waiting for Blueprint Ready",
                    vm_id
                )
                .as_str(),
            ),
            Ok(false) => {
                line(io, alloc::format!("apps: vm{} is not available for pause", vm_id).as_str())
            }
            Err(err) => line(io, alloc::format!("apps: pause failed: {:?}", err).as_str()),
        }
        return;
    }

    if state.pause_latched {
        if !state.pause_snapshot_ready {
            line(io, alloc::format!("apps: vm{} pause is still latching", vm_id).as_str());
            return;
        }
        schedule_load_vm(spawner, io, vm_id);
        return;
    }

    line(io, alloc::format!("apps: vm{} has no replicatable lifecycle latch", vm_id).as_str());
    print_replicatable_vms(io);
}

fn pause_mode(spawner: &Spawner, io: &'static dyn ShellBackend2, args: &[String]) {
    let Some(id) = args.first() else {
        print_replicatable_vms(io);
        return;
    };
    let Ok(vm_id) = id.parse::<u8>() else {
        line(io, "apps: pause expects a vmid from the table");
        print_replicatable_vms(io);
        return;
    };
    toggle_replicatable_vm(spawner, io, vm_id);
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

#[embassy_executor::task(pool_size = 2)]
async fn start_app_task(
    target: MatrixTarget,
    width: usize,
    id: Option<usize>,
    app_args: Vec<String>,
) {
    // Apps commands execute on the BSP executor. Discovery, hash verification, and
    // module loading must stay async all the way to the Blueprint run queue;
    // never route this task through synchronous kfs or spawn_and_wait_local.
    match id {
        Some(id) => {
            let _ = run::submit_archive_id(target.clone(), width, id, app_args).await;
        }
        None => run::print_app_archive_table(&target, width).await,
    }
    set_matrix_target_active(&target, false);
}

fn start_app(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    mut args: impl Iterator<Item = String>,
) {
    let id = match args.next() {
        Some(id_text) => match id_text.parse::<usize>() {
            Ok(id) => Some(id),
            Err(_) => {
                line(io, "apps: start expects an app id");
                None
            }
        },
        None => None,
    };
    let app_args = args.collect::<Vec<_>>();
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io);
    set_matrix_target_active(&target, true);
    match start_app_task(target.clone(), width, id, app_args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            line(io, "apps: start task unavailable");
        }
    }
}

pub(crate) fn submit(spawner: &Spawner, io: &'static dyn ShellBackend2, submitted: &str) {
    let mut parts = submitted.split_whitespace();
    let action = match parts.next() {
        Some("start") => AppsCommand::Start,
        Some("online") => AppsCommand::Online,
        Some("dl") => AppsCommand::Dl,
        Some("peer") => AppsCommand::Peer,
        Some("pause" | "unpause") => AppsCommand::Pause,
        Some("preserve" | "save") => AppsCommand::Preserve,
        Some("load") => AppsCommand::Load,
        Some("stop") => AppsCommand::Stop,
        Some("kick") => AppsCommand::Kick,
        Some("status") => AppsCommand::Status,
        Some(_) | None => {
            line(
                io,
                "apps: expected start, online, dl, peer, pause, preserve, load, stop, kick, or status",
            );
            return;
        }
    };
    let rest = parts.map(String::from).collect::<Vec<_>>();

    match action {
        AppsCommand::Start => start_app(spawner, io, rest.into_iter()),
        AppsCommand::Online => online_app(spawner, io, rest),
        AppsCommand::Dl => super::shell2_dl::submit_download(spawner, io, rest.join(" ").as_str()),
        AppsCommand::Peer => peer_app(spawner, io, rest),
        AppsCommand::Pause => pause_mode(spawner, io, rest.as_slice()),
        AppsCommand::Load => {
            let mut args = rest.iter();
            let first = args.next().map(String::as_str);
            if let Some(endpoint) = first.filter(|s| s.contains("://")) {
                let vm_id = parse_id(args.next().map(String::as_str)).unwrap_or(0);
                load_remote(io, endpoint, vm_id);
            } else {
                let vm_id = parse_id(first).unwrap_or(0);
                schedule_load_vm(spawner, io, vm_id);
            }
        }
        AppsCommand::Preserve => {
            preserve_selected_or_all(io, parse_id(rest.first().map(String::as_str)))
        }
        AppsCommand::Stop => {
            stop_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "stop")
        }
        AppsCommand::Kick => kick_vm(io, parse_id(rest.first().map(String::as_str))),
        AppsCommand::Status => print_status(io),
    }
}
