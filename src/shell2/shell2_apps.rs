use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use embassy_executor::Spawner;

use super::cmds::run;
use super::cmds::tlb_helper::TlbTable;
use super::{
    MatrixTarget, ShellBackend2, line_width_for_backend, matrix_target_for_backend,
    print_matrix_target_line, print_shell_line, set_matrix_target_active,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum AppsPromptMode {
    Start,
    Online,
    Peer,
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
            Self::Start => Self::Online,
            Self::Online => Self::Peer,
            Self::Peer => Self::Pause,
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
            Self::Online => "online",
            Self::Peer => "peer",
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
    print_hv_status(io);
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

fn print_available(io: &'static dyn ShellBackend2) {
    run::print_app_archive_table(io);
}

#[derive(Clone)]
struct OnlineApp {
    name: String,
    url: String,
}

const ONLINE_APPS_URL: &str = "https://trueos.eu/apps";
const ONLINE_LIST_MAX_BYTES: usize = 1024 * 1024;
const ONLINE_APP_MAX_BYTES: usize = 64 * 1024 * 1024;
const ONLINE_FETCH_TIMEOUT_MS: u32 = 45_000;
const ONLINE_HEADERS: &[&str; 3] = &["id", "module", "url"];

async fn fetch_url_bytes(url: String, max_bytes: usize) -> Result<Vec<u8>, String> {
    crate::r::net::https::get_bytes_shared(url.as_str(), ONLINE_FETCH_TIMEOUT_MS, max_bytes).await
}

async fn fetch_online_apps_html() -> Result<Vec<u8>, String> {
    fetch_url_bytes(String::from(ONLINE_APPS_URL), ONLINE_LIST_MAX_BYTES).await
}

fn absolutize_online_url(href: &str) -> String {
    if href.contains("://") {
        String::from(href)
    } else if href.starts_with('/') {
        alloc::format!("https://trueos.eu{}", href)
    } else {
        alloc::format!("https://trueos.eu/apps/{}", href)
    }
}

fn parse_attr_value<'a>(text: &'a str, attr: &str) -> Option<&'a str> {
    let pos = text.find(attr)?;
    let rest = &text[pos + attr.len()..];
    let quote = rest.as_bytes().first().copied()?;
    if quote != b'"' && quote != b'\'' {
        return None;
    }
    let rest = &rest[1..];
    let end = rest.as_bytes().iter().position(|&b| b == quote)?;
    Some(&rest[..end])
}

fn parse_online_apps(html: &str) -> Vec<OnlineApp> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(li_start) = rest.find("<li") {
        rest = &rest[li_start + 3..];
        let li_end = rest.find("</li>").unwrap_or(rest.len());
        let item = &rest[..li_end];
        let Some(a_start) = item.find("<a") else {
            rest = &rest[li_end..];
            continue;
        };
        let link = &item[a_start..];
        let Some(tag_end) = link.find('>') else {
            rest = &rest[li_end..];
            continue;
        };
        let tag = &link[..tag_end];
        let Some(href) = parse_attr_value(tag, "href=") else {
            rest = &rest[li_end..];
            continue;
        };
        let Some(text_end) = crate::r::pat::find_str(&link[tag_end + 1..], "</a>") else {
            rest = &rest[li_end..];
            continue;
        };
        let name = link[tag_end + 1..tag_end + 1 + text_end].trim();
        if href.ends_with(".bp") && !name.is_empty() {
            out.push(OnlineApp {
                name: name.to_string(),
                url: absolutize_online_url(href),
            });
        }
        rest = &rest[li_end..];
    }
    out
}

async fn online_apps() -> Result<Vec<OnlineApp>, String> {
    let html = fetch_online_apps_html().await?;
    let text = core::str::from_utf8(html.as_slice())
        .map_err(|_| String::from("online apps list is not UTF-8"))?;
    Ok(parse_online_apps(text))
}

fn print_online_apps_target(target: &MatrixTarget, width: usize, apps: &[OnlineApp]) {
    if apps.is_empty() {
        print_matrix_target_line(target, "apps: online list is empty");
        return;
    }
    let table = TlbTable::with_width(ONLINE_HEADERS, width.saturating_sub(2));
    table.emit_header(|text| print_matrix_target_line(target, text));
    for (idx, app) in apps.iter().enumerate() {
        let id = alloc::format!("{}", idx);
        let row = [id.as_str(), app.name.as_str(), app.url.as_str()];
        table.emit_row(&row, |text| print_matrix_target_line(target, text));
    }
    table.emit_footer(|text| print_matrix_target_line(target, text));
}

#[embassy_executor::task(pool_size = 2)]
async fn online_app_task(target: MatrixTarget, width: usize, mut args: Vec<String>) {
    crate::r::readiness::wait_for(
        crate::r::readiness::NET_V4_CONFIGURED | crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
    )
    .await;

    let log = |text: &str| print_matrix_target_line(&target, text);
    if args.is_empty() {
        log("apps: fetching online app list");
        match online_apps().await {
            Ok(apps) => print_online_apps_target(&target, width, apps.as_slice()),
            Err(err) => log(alloc::format!("apps: online list failed: {}", err).as_str()),
        }
        set_matrix_target_active(&target, false);
        return;
    }

    let id_text = args.remove(0);
    let Ok(id) = id_text.parse::<usize>() else {
        log("apps: online expects an app id");
        set_matrix_target_active(&target, false);
        return;
    };
    let app_args = args;
    let apps = match online_apps().await {
        Ok(apps) => apps,
        Err(err) => {
            log(alloc::format!("apps: online list failed: {}", err).as_str());
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let Some(app) = apps.get(id) else {
        log("apps: unknown online app id");
        print_online_apps_target(&target, width, apps.as_slice());
        set_matrix_target_active(&target, false);
        return;
    };
    log(alloc::format!("apps: fetching {} from {}", app.name.as_str(), app.url.as_str()).as_str());
    match fetch_url_bytes(app.url.clone(), ONLINE_APP_MAX_BYTES).await {
        Ok(module_bytes) => {
            let _ = run::enqueue_blueprint_bytes(
                target.clone(),
                app.name.clone(),
                module_bytes,
                app_args,
            );
        }
        Err(err) => log(alloc::format!("apps: online fetch failed: {}", err).as_str()),
    }
    set_matrix_target_active(&target, false);
}

fn online_app(spawner: &Spawner, io: &'static dyn ShellBackend2, args: Vec<String>) {
    let target = matrix_target_for_backend(io);
    let width = line_width_for_backend(io);
    set_matrix_target_active(&target, true);
    match online_app_task(target.clone(), width, args) {
        Ok(token) => {
            spawner.spawn(token);
        }
        Err(_) => {
            set_matrix_target_active(&target, false);
            line(io, "apps: online task unavailable");
        }
    }
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
            match crate::hv::store::save_bytes(local_vm_id, bytes) {
                Ok(saved) => {
                    print_matrix_target_line(
                        &target,
                        alloc::format!("apps: peer vm{} fetched {} bytes", local_vm_id, saved)
                            .as_str(),
                    );
                    match crate::hv::restore_snapshot(local_vm_id) {
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

fn preserve_vm(io: &'static dyn ShellBackend2, vm_id: u8, label: &str) {
    match crate::hv::request_preserve(vm_id) {
        Ok(true) => line(io, alloc::format!("apps: vm{} {} requested", vm_id, label).as_str()),
        Ok(false) => match crate::hv::save_snapshot(vm_id) {
            Ok(bytes) => {
                line(io, alloc::format!("apps: vm{} saved {} bytes", vm_id, bytes).as_str())
            }
            Err(err) => line(io, alloc::format!("apps: save failed: {:?}", err).as_str()),
        },
        Err(err) => line(io, alloc::format!("apps: save failed: {:?}", err).as_str()),
    }
}

fn preserve_selected_or_all(io: &'static dyn ShellBackend2, id: Option<u8>, label: &str) {
    if let Some(vm_id) = id {
        preserve_vm(io, vm_id, label);
        return;
    }
    let active = active_vm_ids();
    if active.is_empty() {
        line(io, "apps: no active app VMs");
        return;
    }
    for vm_id in active {
        preserve_vm(io, vm_id, label);
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
        Some("online") => (AppsPromptMode::Online, parts.map(String::from).collect()),
        Some("peer") => (AppsPromptMode::Peer, parts.map(String::from).collect()),
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
        AppsPromptMode::Online => online_app(spawner, io, rest),
        AppsPromptMode::Peer => peer_app(spawner, io, rest),
        AppsPromptMode::Pause => {
            preserve_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "pause")
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
            preserve_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "save")
        }
        AppsPromptMode::Stop => {
            stop_selected_or_all(io, parse_id(rest.first().map(String::as_str)), "stop")
        }
        AppsPromptMode::Status => print_status(io),
    }
}
