//! Shell3 terminal leases. VM lifecycle stays in HV; bytes never reach Shell3's prompt.
use super::matrix_target::{
    MatrixSlotAttachment, TRANSPORT_LOCAL_SCOPE, TRANSPORT_NET_TCP_SCOPE,
    attach_matrix_slot_resource, matrix_slot_is_live,
};
use super::service::QueuedBlueprint;
use super::{RgbaColor, update::RenderedLine};
use crate::shell3::{MatrixSlotLease, MatrixTarget};
use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;
use trueos_terminal::{Terminal, TerminalColor};
mod remote;
pub(crate) use remote::Drain as RemoteDrain;

#[derive(Clone, Copy)]
pub(crate) struct Frontend {
    pub id: u64,
    pub cols: usize,
    pub rows: usize,
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Owner {
    vm: u8,
    run: u64,
}
struct Native {
    active: bool,
    painted: bool,
    return_to_default: bool,
    launch: Option<QueuedBlueprint>,
    input: VecDeque<u8>,
    notices: VecDeque<String>,
}
struct Route {
    native: Option<Native>,
    lease: MatrixSlotLease,
    frontend: Option<u64>,
    selected: bool,
    vm: Option<u8>,
    owner: Option<Owner>,
    screen: Terminal,
    surface_generation: u64,
    suppressed_text: Option<(u32, u32, u32)>,
    stdout_pending: Vec<u8>,
}
impl Route {
    fn active(&self) -> bool {
        self.owner.is_some() || self.native.as_ref().is_some_and(|n| n.active)
    }
}
struct Registry {
    routes: Vec<Route>,
    ui4_windows: Vec<(u64, crate::ui4::WindowId)>,
    revisions: Vec<(u64, u64)>,
    next_revision: u64,
    remote_frontends: Vec<(u64, remote::Output)>,
    pending_launches: Vec<(u64, QueuedBlueprint)>,
    navigation_holds: Vec<u64>,
}
impl Registry {
    const fn new() -> Self {
        Self {
            routes: Vec::new(),
            ui4_windows: Vec::new(),
            revisions: Vec::new(),
            next_revision: 0,
            remote_frontends: Vec::new(),
            pending_launches: Vec::new(),
            navigation_holds: Vec::new(),
        }
    }
    fn changed(&mut self, id: impl Into<Option<u64>>) {
        let Some(id) = id.into() else {
            return;
        };
        self.next_revision = self.next_revision.wrapping_add(1).max(1);
        let revision = self.next_revision;
        if let Some(entry) = self.revisions.iter_mut().find(|entry| entry.0 == id) {
            entry.1 = revision;
        } else {
            self.revisions.push((id, revision));
        }
    }
}
static ROUTES: Mutex<Registry> = Mutex::new(Registry::new());
static NEXT_FRONTEND: AtomicU64 = AtomicU64::new(1);
pub(super) fn bind_ui4_window(frontend: u64, window: crate::ui4::WindowId) {
    let mut routes = ROUTES.lock();
    if let Some(entry) = routes
        .ui4_windows
        .iter_mut()
        .find(|entry| entry.0 == frontend)
    {
        entry.1 = window;
    } else {
        routes.ui4_windows.push((frontend, window));
    }
}
pub(crate) fn window_for_vm(vm: u8) -> Option<crate::ui4::WindowId> {
    let run = crate::hv::vm_run_generation(vm)?;
    let routes = ROUTES.lock();
    let route = routes
        .routes
        .iter()
        .find(|route| route.selected && route.owner == Some(Owner { vm, run }))?;
    routes
        .ui4_windows
        .iter()
        .find(|entry| Some(entry.0) == route.frontend)
        .map(|entry| entry.1)
}
pub(crate) fn vm_for_window(window: crate::ui4::WindowId) -> Option<u8> {
    let routes = ROUTES.lock();
    let frontend = routes.ui4_windows.iter().find(|entry| entry.1 == window)?.0;
    let owner = routes
        .routes
        .iter()
        .find(|route| route.frontend == Some(frontend) && route.selected)?
        .owner?;
    (crate::hv::vm_run_generation(owner.vm) == Some(owner.run)).then_some(owner.vm)
}
pub(super) fn new_frontend() -> u64 {
    NEXT_FRONTEND.fetch_add(1, Ordering::Relaxed)
}

/// SSH renders on the peer terminal. Local UI4 frontends keep the cell parser.
pub(super) fn bind_remote_frontend(frontend: u64) {
    let mut routes = ROUTES.lock();
    if !routes
        .remote_frontends
        .iter()
        .any(|entry| entry.0 == frontend)
    {
        routes
            .remote_frontends
            .push((frontend, remote::Output::new()));
    }
}

pub(super) fn remote_active(frontend: u64) -> bool {
    let routes = ROUTES.lock();
    routes
        .remote_frontends
        .iter()
        .any(|entry| entry.0 == frontend)
        && routes.routes.iter().any(|route| {
            route.frontend == Some(frontend) && route.selected && route.owner.is_some()
        })
}

pub(super) fn take_remote_output(frontend: u64, available: usize) -> Option<RemoteDrain> {
    static AGE: crate::r::input_timing::Site =
        crate::r::input_timing::Site::new("ssh-app-output-queue-age");
    let mut routes = ROUTES.lock();
    // Startup may claim and buffer its first frame while command feedback
    // still owns presentation. Do not send app ANSI bytes until navigation.
    if routes.navigation_holds.contains(&frontend) {
        return None;
    }
    let active = routes
        .routes
        .iter()
        .any(|route| route.frontend == Some(frontend) && route.owner.is_some());
    let output = &mut routes
        .remote_frontends
        .iter_mut()
        .find(|entry| entry.0 == frontend)?
        .1;
    let since = output.queued_since_ms();
    let drained = output.take(available, active);
    drop(routes);
    if !drained.bytes.is_empty() {
        if let Some(since) = since {
            AGE.record(crate::r::input_timing::now_ms().saturating_sub(since));
        }
    }
    Some(drained)
}

struct Attachment;
impl MatrixSlotAttachment for Attachment {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease) {
        let mut routes = ROUTES.lock();
        if let Some(index) = routes.routes.iter().position(|route| route.lease == *lease) {
            let route = routes.routes.remove(index);
            if route.owner.is_some() {
                if let Some((_, output)) = routes
                    .remote_frontends
                    .iter_mut()
                    .find(|entry| Some(entry.0) == route.frontend)
                {
                    output.end();
                }
            }
            routes.changed(route.frontend);
        }
    }
}

/// An output buffer belongs to the Matrix identity, even without a frontend.
pub(crate) fn reserve(target: &MatrixTarget) -> Result<(), String> {
    create_route(None, target)
}

pub(crate) fn attach(frontend: Frontend, target: &MatrixTarget) -> Result<(), String> {
    create_route(Some(frontend), target)
}

/// Commit a prepared Blueprint launch after attachment and task-token admission.
/// This cannot fail and runs before the admitted task is spawned. Native-to-
/// native attachment continues to require explicit navigation.
pub(crate) fn park_native_for_launch(frontend: Frontend) {
    let mut routes = ROUTES.lock();
    park_native_for_attachment(&mut routes, Some(frontend.id));
    routes.changed(Some(frontend.id));
}

/// Attaching a new foreground-capable launch reserves its input route now,
/// while accepted-word feedback may defer selecting/presenting the surface.
/// Retire native helper input ownership before the guest's one-shot lease
/// claim; keep its screen/paint state available for a later explicit resume.
/// Blueprint owners retain their separate epoch/release handshake.
fn park_native_for_attachment(routes: &mut Registry, frontend: Option<u64>) {
    let Some(frontend) = frontend else {
        return;
    };
    for route in routes
        .routes
        .iter_mut()
        .filter(|route| route.frontend == Some(frontend))
    {
        if let Some(native) = route.native.as_mut() {
            native.active = false;
            native.input.clear();
            route.suppressed_text = None;
        }
    }
}

fn create_route(frontend: Option<Frontend>, target: &MatrixTarget) -> Result<(), String> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let (cols, rows) = frontend
        .map_or((super::DEFAULT_COLUMNS, super::DEFAULT_ROWS), |view| (view.cols, view.rows));
    let frontend_id = frontend.map(|view| view.id);
    {
        let mut routes = ROUTES.lock();
        let selected = frontend_id.is_none_or(|id| !routes.navigation_holds.contains(&id));
        if let Some(route) = routes.routes.iter_mut().find(|route| route.lease == lease) {
            if route.frontend.is_some() {
                return Err("tui: terminal target already attached".into());
            }
            route.frontend = frontend_id;
            route.selected = selected;
            route.screen.resize(cols, rows);
            route.surface_generation = route.surface_generation.wrapping_add(1).max(1);
            routes.changed(frontend_id);
            return Ok(());
        }
        routes.routes.push(Route {
            native: None,
            lease: lease.clone(),
            frontend: frontend_id,
            selected,
            vm: None,
            owner: None,
            screen: Terminal::new(cols, rows),
            surface_generation: 1,
            suppressed_text: None,
            stdout_pending: Vec::new(),
        });
        routes.changed(frontend_id);
    }
    if attach_matrix_slot_resource(&lease, Arc::new(Attachment)).is_err() {
        Attachment.on_matrix_slot_freed(&lease);
        return Err("tui: terminal target expired or attachments full".into());
    }
    Ok(())
}

pub(crate) fn supports(target: &MatrixTarget) -> bool {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    matrix_slot_is_live(&lease)
        && ROUTES
            .lock()
            .routes
            .iter()
            .any(|route| route.lease == lease)
}
pub(crate) fn bind_vm(target: &MatrixTarget, vm: u8) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    // Reusing a VM ID must not leave the previous output buffer wired to its
    // new occupant. The old slot remains available as a final transcript.
    let mut changed = Vec::new();
    for route in routes
        .routes
        .iter_mut()
        .filter(|route| route.vm == Some(vm) && route.lease != lease)
    {
        route.vm = None;
        route.owner = None;
        changed.push(route.frontend);
    }
    for frontend in changed {
        routes.changed(frontend);
    }
    if let Some(route) = routes.routes.iter_mut().find(|route| route.lease == lease) {
        route.vm = Some(vm);
    }
}

// Report at most once per VM run; a failed app claim must not flood lifecycle
// output if it retries. Unknown-run failures have a separate one-shot slot.
static CLAIM_REJECT_RUN: [AtomicU64; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicU64::new(0) }; crate::allcaps::hv::VM_ID_LIMIT];
static CLAIM_REJECT_NO_RUN: [AtomicBool; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { AtomicBool::new(false) }; crate::allcaps::hv::VM_ID_LIMIT];
fn log_claim_rejection(vm: u8, run: u64, frontend: Option<u64>, reason: &'static str) {
    let Some(last) = CLAIM_REJECT_RUN.get(vm as usize) else {
        return;
    };
    if run == 0 {
        if CLAIM_REJECT_NO_RUN[vm as usize].swap(true, Ordering::Relaxed) {
            return;
        }
    } else if last.swap(run, Ordering::Relaxed) == run {
        return;
    }
    crate::log_os::blueprint_important_line(format_args!(
        "terminal-claim-rejected vm={} generation={} frontend={} reason={}\n",
        vm,
        run,
        frontend.unwrap_or(0),
        reason
    ));
}

pub(crate) fn claim(target: &MatrixTarget, vm: u8) -> Option<bool> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let Some(run) = crate::hv::vm_run_generation(vm) else {
        log_claim_rejection(vm, 0, None, "missing-run");
        return None;
    };
    let live = matrix_slot_is_live(&lease);
    let mut routes = ROUTES.lock();
    let Some(index) = routes.routes.iter().position(|route| route.lease == lease) else {
        drop(routes);
        log_claim_rejection(vm, run, None, "missing-route");
        return None;
    };
    let frontend = routes.routes[index].frontend;
    let owner = Owner { vm, run };
    let rejection = if !live {
        Some("dead-slot")
    } else if !routes.routes[index].selected
        && !frontend.is_some_and(|frontend| routes.navigation_holds.contains(&frontend))
    {
        Some("not-selected")
    } else if routes.routes[index].vm != Some(vm) {
        Some("wrong-vm")
    } else if routes.routes.iter().any(|route| {
        frontend.is_some() && route.frontend == frontend && route.active() && route.lease != lease
    }) {
        Some("other-active-route")
    } else if routes.routes[index]
        .owner
        .is_some_and(|current| current != owner)
    {
        Some("stale-owner")
    } else {
        None
    };
    if let Some(reason) = rejection {
        drop(routes);
        log_claim_rejection(vm, run, frontend, reason);
        return Some(false);
    }
    if routes.routes[index].owner.is_none() {
        if let Some((_, output)) = routes
            .remote_frontends
            .iter_mut()
            .find(|entry| Some(entry.0) == frontend)
        {
            if !output.begin() {
                drop(routes);
                log_claim_rejection(vm, run, frontend, "remote-begin");
                return Some(false);
            }
        }
        let route = &mut routes.routes[index];
        route.screen.reset();
        route.suppressed_text = None;
        route.owner = Some(owner);
        route.surface_generation = route.surface_generation.wrapping_add(1).max(1);
        routes.changed(frontend);
    }
    Some(true)
}
pub(crate) fn release(target: &MatrixTarget, vm: u8) -> Option<bool> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let run = crate::hv::vm_run_generation(vm)?;
    let mut routes = ROUTES.lock();
    let route = routes
        .routes
        .iter_mut()
        .find(|route| route.lease == lease)?;
    if route.vm != Some(vm)
        || route
            .owner
            .is_some_and(|owner| owner != (Owner { vm, run }))
    {
        return Some(false);
    }
    let frontend = route.frontend;
    let was_owned = route.owner.is_some();
    route.owner = None;
    route.suppressed_text = None;
    if was_owned {
        if let Some((_, output)) = routes
            .remote_frontends
            .iter_mut()
            .find(|entry| Some(entry.0) == frontend)
        {
            output.end();
        }
    }
    routes.changed(frontend);
    let window = routes
        .ui4_windows
        .iter()
        .find(|entry| Some(entry.0) == frontend)
        .map(|entry| entry.1);
    drop(routes);
    if let Some(window) = window {
        crate::ui4::drag_drop::release_terminal(crate::ui4::WindowOwner::Vm(vm), window);
    }
    Some(true)
}
pub(crate) fn surface(
    target: &MatrixTarget,
) -> Option<crate::hv::BlueprintTerminalSurfaceSnapshot> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|route| route.lease == lease)?;
    let (cols, rows) = route.screen.dimensions();
    Some(crate::hv::BlueprintTerminalSurfaceSnapshot {
        generation: route.surface_generation,
        cols: cols as u32,
        rows: rows as u32,
    })
}
/// Ordinary stdout uses the slot transcript until an explicit terminal claim.
/// Terminal owners retain byte-exact ANSI output through the existing path.
pub(crate) fn write_stdout(target: &MatrixTarget, vm: u8, bytes: &[u8]) -> usize {
    crate::allocators::with_host_alloc_domain(|| {
        let lease = crate::shell3::matrix_target_slot_lease(target);
        let run = crate::hv::vm_run_generation(vm);
        let mut routes = ROUTES.lock();
        let Some(route) = routes
            .routes
            .iter_mut()
            .find(|route| route.lease == lease && route.vm == Some(vm))
        else {
            return 0;
        };
        if route
            .owner
            .is_some_and(|owner| Some(owner.run) == run && owner.vm == vm)
        {
            drop(routes);
            return write_inner(target, vm, bytes).unwrap_or(0);
        }
        // Preserve split UTF-8 writes and cap unterminated lines at 16 KiB.
        let mut lines = Vec::new();
        for &byte in bytes {
            if byte == b'\n' {
                if route.stdout_pending.last() == Some(&b'\r') {
                    route.stdout_pending.pop();
                }
                lines.push(String::from_utf8_lossy(&route.stdout_pending).into_owned());
                route.stdout_pending.clear();
            } else {
                route.stdout_pending.push(byte);
                if route.stdout_pending.len() >= 16 * 1024 {
                    lines.push(String::from_utf8_lossy(&route.stdout_pending).into_owned());
                    route.stdout_pending.clear();
                }
            }
        }
        drop(routes);
        for line in lines {
            super::MatrixSlots::echo_output(&lease, line);
        }
        bytes.len()
    })
}

pub(crate) fn write(target: &MatrixTarget, vm: u8, bytes: &[u8]) -> Option<usize> {
    crate::allocators::with_host_alloc_domain(|| write_inner(target, vm, bytes))
}
fn write_inner(target: &MatrixTarget, vm: u8, bytes: &[u8]) -> Option<usize> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let run = crate::hv::vm_run_generation(vm)?;
    let responses = {
        let mut routes = ROUTES.lock();
        let route = routes
            .routes
            .iter_mut()
            .find(|route| route.lease == lease)?;
        if route.owner != Some(Owner { vm, run }) {
            return Some(0);
        }
        let frontend = route.frontend;
        if let Some((_, output)) = routes
            .remote_frontends
            .iter_mut()
            .find(|entry| Some(entry.0) == frontend)
        {
            let written = output.write_at(bytes, crate::r::input_timing::now_ms());
            drop(routes);
            super::service::notify_work();
            return Some(written);
        }
        let route = routes
            .routes
            .iter_mut()
            .find(|route| route.lease == lease)?;
        route.screen.feed(bytes);
        let responses = route.screen.take_responses();
        if route.screen.take_dirty() {
            routes.changed(frontend);
        }
        responses
    };
    if !responses.is_empty() {
        crate::hv::blueprint_console_submit_stdin_for_target(vm, target, run, &responses);
    }
    super::service::notify_work();
    Some(bytes.len())
}

/// Host helpers share the same terminal surface and input routing as Blueprints.
pub(super) fn attach_native(frontend: Frontend, target: &MatrixTarget) -> Result<(), String> {
    if supports(target) {
        return Err("tui: terminal target already attached".into());
    }
    if !park(frontend.id) {
        return Err("tui: could not release the previous terminal owner".into());
    }
    attach(frontend, target)?;
    let lease = crate::shell3::matrix_target_slot_lease(target);
    if let Some(route) = ROUTES.lock().routes.iter_mut().find(|r| r.lease == lease) {
        route.native = Some(Native {
            active: route.selected,
            painted: false,
            return_to_default: false,
            launch: None,
            input: VecDeque::new(),
            notices: VecDeque::new(),
        });
    }
    Ok(())
}
pub(super) fn cancel_native_attach(target: &MatrixTarget) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(index) = routes
        .routes
        .iter()
        .position(|r| r.lease == lease && r.native.is_some())
    {
        let route = routes.routes.remove(index);
        routes.changed(route.frontend);
    }
}
pub(super) fn native_slot(name: &str) -> bool {
    ROUTES
        .lock()
        .routes
        .iter()
        .any(|r| r.lease.name() == name && r.native.is_some())
}
pub(super) fn native_visible(target: &MatrixTarget) -> bool {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    ROUTES
        .lock()
        .routes
        .iter()
        .any(|r| r.lease == lease && r.native.as_ref().is_some_and(|n| n.active))
}
pub(super) fn native_pending_frame(frontend: u64, name: Option<&str>) -> bool {
    ROUTES.lock().routes.iter().any(|route| {
        route.frontend == Some(frontend)
            && Some(route.lease.name()) == name
            && route
                .native
                .as_ref()
                .is_some_and(|native| native.active && !native.painted)
    })
}
pub(super) fn native_transport_scope(target: &MatrixTarget) -> Option<u8> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes
        .routes
        .iter()
        .find(|route| route.lease == lease && route.native.is_some())?;
    Some(
        if routes
            .remote_frontends
            .iter()
            .any(|entry| Some(entry.0) == route.frontend)
        {
            TRANSPORT_NET_TCP_SCOPE
        } else {
            TRANSPORT_LOCAL_SCOPE
        },
    )
}
pub(super) fn native_read(target: &MatrixTarget) -> Option<(Vec<u8>, Vec<String>)> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    let native = routes
        .routes
        .iter_mut()
        .find(|r| r.lease == lease)?
        .native
        .as_mut()?;
    Some((native.input.drain(..).collect(), native.notices.drain(..).collect()))
}
pub(crate) fn native_notice(target: &MatrixTarget, message: &str) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(native) = routes
        .routes
        .iter_mut()
        .find(|r| r.lease == lease)
        .and_then(|r| r.native.as_mut())
    {
        if native.notices.len() == 32 {
            native.notices.pop_front();
        }
        native.notices.push_back(message.into());
    }
    super::service::notify_work();
}
pub(super) fn native_write(target: &MatrixTarget, bytes: &[u8]) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(route) = routes
        .routes
        .iter_mut()
        .find(|r| r.lease == lease && r.native.is_some())
    {
        route.screen.feed(bytes);
        route.native.as_mut().unwrap().painted = true;
        let frontend = route.frontend;
        if route.active() {
            routes.changed(frontend);
        }
    }
    drop(routes);
    super::service::notify_work();
}
pub(super) fn native_return(target: &MatrixTarget) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(route) = routes.routes.iter_mut().find(|r| r.lease == lease) {
        if let Some(native) = route.native.as_mut() {
            native.active = false;
            native.return_to_default = true;
            native.input.clear();
            route.suppressed_text = None;
            let frontend = route.frontend;
            routes.changed(frontend);
        }
    }
    drop(routes);
    super::service::notify_work();
}
pub(super) fn take_native_return(frontend: u64) -> bool {
    let mut routes = ROUTES.lock();
    routes
        .routes
        .iter_mut()
        .filter(|r| r.frontend == Some(frontend))
        .fold(false, |exit, r| {
            exit | r
                .native
                .as_mut()
                .is_some_and(|n| core::mem::take(&mut n.return_to_default))
        })
}

/// Resolve the current frontend on reentry, including a parked helper moved to SSH.
pub(super) fn native_frontend(target: &MatrixTarget) -> Option<Frontend> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes
        .routes
        .iter()
        .find(|r| r.lease == lease && r.native.as_ref().is_some_and(|n| n.active))?;
    let (cols, rows) = route.screen.dimensions();
    Some(Frontend {
        id: route.frontend?,
        cols,
        rows,
    })
}

pub(super) fn native_launch(target: &MatrixTarget, app: QueuedBlueprint) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(route) = routes.routes.iter_mut().find(|r| r.lease == lease) {
        if let Some(native) = route.native.as_mut() {
            native.active = false;
            native.input.clear();
            native.launch = Some(app);
            let frontend = route.frontend;
            routes.changed(frontend);
        }
    }
    drop(routes);
    super::service::notify_work();
}

pub(super) fn take_native_launch(frontend: u64) -> Option<QueuedBlueprint> {
    let mut routes = ROUTES.lock();
    if let Some(index) = routes
        .pending_launches
        .iter()
        .position(|entry| entry.0 == frontend)
    {
        return Some(routes.pending_launches.remove(index).1);
    }
    routes
        .routes
        .iter_mut()
        .filter(|r| r.frontend == Some(frontend))
        .find_map(|r| r.native.as_mut()?.launch.take())
}

pub(super) fn revision(frontend: u64) -> u64 {
    ROUTES
        .lock()
        .revisions
        .iter()
        .find(|entry| entry.0 == frontend)
        .map_or(0, |entry| entry.1)
}
/// Launches may prepare their routes while accepted-word feedback owns the UI.
pub(super) fn hold_navigation(frontend: u64, hold: bool) {
    let mut routes = ROUTES.lock();
    if hold {
        if !routes.navigation_holds.contains(&frontend) {
            routes.navigation_holds.push(frontend);
        }
    } else {
        routes.navigation_holds.retain(|id| *id != frontend);
    }
}

pub(super) fn park(frontend: u64) -> bool {
    {
        let mut routes = ROUTES.lock();
        let mut changed = false;
        for route in routes
            .routes
            .iter_mut()
            .filter(|r| r.frontend == Some(frontend))
        {
            if let Some(native) = route.native.as_mut() {
                changed |= native.active;
                native.active = false;
                native.input.clear();
                route.suppressed_text = None;
            }
        }
        if changed {
            routes.changed(frontend);
        }
    }
    let candidates: Vec<_> = ROUTES
        .lock()
        .routes
        .iter()
        .filter(|route| route.frontend == Some(frontend))
        .filter_map(|route| route.vm.map(|vm| (vm, route.owner.is_some())))
        .collect();
    let mut parked = true;
    for (vm, active) in candidates {
        if active || crate::hv::blueprint_terminal_reentry_requested(vm) {
            parked &= crate::hv::blueprint_console_return_to_cli(vm);
        }
    }
    parked
}
pub(super) fn select(frontend: Frontend, name: Option<&str>) -> bool {
    // A helper has one input owner. A parked helper may move to another shell.
    {
        let mut routes = ROUTES.lock();
        if let Some(route) = routes
            .routes
            .iter_mut()
            .find(|r| Some(r.lease.name()) == name)
        {
            if route.frontend != Some(frontend.id) {
                if route.active() && route.frontend.is_some() {
                    return false;
                }
                let old = route.frontend;
                route.frontend = Some(frontend.id);
                route.selected = false;
                routes.changed(old);
            }
        }
    }
    let needs_park = ROUTES.lock().routes.iter().any(|route| {
        route.frontend == Some(frontend.id) && route.active() && Some(route.lease.name()) != name
    });
    if needs_park && !park(frontend.id) {
        return false;
    }
    let mut routes = ROUTES.lock();
    let mut changed = false;
    for route in routes
        .routes
        .iter_mut()
        .filter(|route| route.frontend == Some(frontend.id))
    {
        let selected = Some(route.lease.name()) == name;
        changed |= route.selected != selected;
        if let Some(native) = route.native.as_mut() {
            if selected && !route.selected {
                native.active = true;
                native.return_to_default = false;
                route.suppressed_text = None;
            } else if !selected {
                native.active = false;
                native.input.clear();
                native.return_to_default = false;
            }
        }
        route.selected = selected;
        if selected && route.screen.dimensions() != (frontend.cols, frontend.rows) {
            route.screen.resize(frontend.cols, frontend.rows);
            route.surface_generation = route.surface_generation.wrapping_add(1).max(1);
            changed = true;
        }
    }
    if changed {
        routes.changed(frontend.id);
    }
    true
}
/// Selecting a Matrix slot leaves its command pane in Shell3's control.
/// Only an explicit `tui` attaches its terminal, including native helpers.
pub(super) fn select_for_navigation(frontend: Frontend, _name: &str) -> bool {
    if !park(frontend.id) {
        return false;
    }
    // Shell3 tracks the selected Matrix slot separately. Do not mark its TUI
    // selected here: a delayed initial claim must not make navigation enter it.
    select(frontend, None)
}
/// Explicit `tui` releases the previous attachment before requesting reentry.
/// Ordinary slot navigation continues to respect a live terminal owner.
pub(super) fn request_takeover(frontend: Frontend, name: &str) -> Result<(), &'static str> {
    let previous = {
        let routes = ROUTES.lock();
        routes
            .routes
            .iter()
            .find(|route| route.lease.name() == name)
            .filter(|route| route.frontend != Some(frontend.id))
            .map(|route| (route.lease.clone(), route.frontend, route.vm, route.owner.is_some()))
    };
    if let Some((lease, old, vm, active)) = previous {
        // Keep HV's terminal epoch handshake and SSH's end-of-TUI output intact.
        if active && vm.is_some_and(|vm| !crate::hv::blueprint_console_return_to_cli(vm)) {
            return Err("tui: could not release the previous terminal owner");
        }
        let mut routes = ROUTES.lock();
        if let Some(route) = routes.routes.iter_mut().find(|route| route.lease == lease) {
            if route.frontend != old || route.owner.is_some() {
                return Err("tui: terminal attachment changed during takeover");
            }
            if let Some(native) = route.native.as_mut() {
                native.active = false;
                native.input.clear();
                native.return_to_default = false;
            }
            route.frontend = None;
            route.selected = false;
            route.suppressed_text = None;
            routes.changed(old);
        }
    }
    request(frontend, name)
}

pub(super) fn request(frontend: Frontend, name: &str) -> Result<(), &'static str> {
    if native_slot(name) {
        {
            let routes = ROUTES.lock();
            if routes.navigation_holds.contains(&frontend.id) {
                if routes.routes.iter().any(|route| {
                    route.lease.name() == name
                        && route.frontend != Some(frontend.id)
                        && route.active()
                }) {
                    return Err("tui: terminal UI is owned by another Shell3");
                }
                return Ok(());
            }
        }
        if !select(frontend, Some(name)) {
            return Err("tui: terminal UI is owned by another Shell3");
        }
        let mut routes = ROUTES.lock();
        if let Some(route) = routes
            .routes
            .iter_mut()
            .find(|r| r.frontend == Some(frontend.id) && r.lease.name() == name)
        {
            if let Some(native) = route.native.as_mut() {
                native.active = true;
                native.return_to_default = false;
            }
            route.suppressed_text = None;
        }
        routes.changed(frontend.id);
        return Ok(());
    }
    let vm = {
        let mut routes = ROUTES.lock();
        let index = routes
            .routes
            .iter()
            .position(|route| route.lease.name() == name)
            .ok_or("tui: Blueprint has no Shell3 terminal route")?;
        if routes.routes[index].frontend != Some(frontend.id) {
            if routes.routes[index].owner.is_some() {
                return Err("tui: terminal UI is owned by another Shell3");
            }
            let old = routes.routes[index].frontend;
            routes.routes[index].frontend = Some(frontend.id);
            routes.changed(old);
        }
        routes.routes[index]
            .vm
            .ok_or("tui: Blueprint is still starting")?
    };
    if !select(frontend, Some(name)) {
        return Err("tui: could not release the previous terminal owner");
    }
    crate::hv::blueprint_terminal_request_reentry(vm)
}
pub(super) fn detach(frontend: u64) {
    hold_navigation(frontend, false);
    ROUTES
        .lock()
        .ui4_windows
        .retain(|entry| entry.0 != frontend);
    let (apps, native): (Vec<_>, Vec<_>) = {
        let routes = ROUTES.lock();
        routes
            .routes
            .iter()
            .filter(|route| route.frontend == Some(frontend))
            .map(|route| (route.lease.clone(), route.vm, route.native.is_some()))
            .partition(|(_, _, native)| !native)
    };
    for (lease, vm, _) in apps {
        if let Some(vm) = vm {
            let _ = crate::hv::blueprint_console_return_to_cli(vm);
        }
        let mut routes = ROUTES.lock();
        if let Some(route) = routes.routes.iter_mut().find(|route| route.lease == lease) {
            route.frontend = None;
            route.selected = true;
            route.owner = None;
        }
    }
    for (lease, _, _) in native {
        super::MatrixSlots::drop_lease(&lease);
    }
    ROUTES.lock().revisions.retain(|entry| entry.0 != frontend);
    ROUTES
        .lock()
        .remote_frontends
        .retain(|entry| entry.0 != frontend);
}

pub(super) fn active(frontend: u64, name: Option<&str>) -> bool {
    ROUTES.lock().routes.iter().any(|route| {
        route.frontend == Some(frontend)
            && route.selected
            && Some(route.lease.name()) == name
            && route.active()
    })
}

/// Only a currently owned lease may request mouse capture from the client.
pub(super) fn mouse_options(frontend: u64, name: Option<&str>) -> trueos_terminal::MouseOptions {
    ROUTES
        .lock()
        .routes
        .iter()
        .find(|route| {
            route.frontend == Some(frontend)
                && route.selected
                && Some(route.lease.name()) == name
                && route.active()
        })
        .map(|route| route.screen.mouse_options())
        .unwrap_or_default()
}

pub(super) fn snapshot(frontend: u64, name: Option<&str>) -> Option<Vec<RenderedLine>> {
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|route| {
        route.frontend == Some(frontend)
            && route.selected
            && Some(route.lease.name()) == name
            && route.active()
    })?;
    if route.owner.is_some()
        && routes
            .remote_frontends
            .iter()
            .any(|entry| entry.0 == frontend)
    {
        return None;
    }
    // Keep the existing shell pixels until the helper supplies its first frame.
    if route.native.as_ref().is_some_and(|native| !native.painted) {
        return None;
    }
    let (cols, _) = route.screen.dimensions();
    let cursor = route.screen.cursor();
    Some(
        route
            .screen
            .cells()
            .chunks(cols)
            .enumerate()
            .map(|(row_index, row)| {
                let default_background = if route.native.is_some() {
                    if row_index < 3 {
                        super::update::CONTROL_BACKGROUND
                    } else if routes
                        .remote_frontends
                        .iter()
                        .any(|entry| entry.0 == frontend)
                    {
                        // ANSI helpers inherit the client terminal’s background.
                        RgbaColor::BlackTransparent.rgba()
                    } else {
                        super::update::MATRIX_BACKGROUND
                    }
                } else {
                    super::update::MATRIX_BACKGROUND
                };
                row.iter()
                    .enumerate()
                    .map(|(col, cell)| {
                        rendered_cell(
                            cell,
                            default_background,
                            cursor.visible && cursor.row == row_index && cursor.col == col,
                        )
                    })
                    .collect()
            })
            .collect(),
    )
}

fn rendered_cell(
    cell: &trueos_terminal::Cell,
    default_background: [u8; 4],
    cursor: bool,
) -> (char, Option<RgbaColor>) {
    let mut foreground = color(cell.style.foreground, [255, 255, 255, 255]);
    let mut background = color(cell.style.background, default_background);
    // Resolve terminal defaults before swapping: a reversed default style is
    // the selection highlight used throughout Codex. The block cursor inverts
    // the resulting colors again so it stays visible on selected text.
    if cell.style.reversed != cursor {
        core::mem::swap(&mut foreground, &mut background);
    }
    (
        cell.glyph,
        Some(RgbaColor::Terminal {
            foreground,
            background,
            underline: cell.style.underline,
        }),
    )
}

#[cfg(test)]
mod rendered_cell_tests {
    use super::*;

    #[test]
    fn codex_reverse_video_selection_resolves_default_colors() {
        let mut terminal = Terminal::new(2, 1);
        terminal.feed(b"\x1b[7mA\x1b[27mB");
        let background = [16, 16, 16, 255];
        let selected = rendered_cell(&terminal.cells()[0], background, false)
            .1
            .unwrap();
        let normal = rendered_cell(&terminal.cells()[1], background, false)
            .1
            .unwrap();
        assert_eq!(selected.rgba(), background);
        assert_eq!(selected.background(), Some([255, 255, 255, 255]));
        assert_eq!(normal.rgba(), [255, 255, 255, 255]);
        assert_eq!(normal.background(), Some(background));
    }

    #[test]
    fn reverse_video_keeps_explicit_colors_and_underlines() {
        let mut terminal = Terminal::new(2, 1);
        terminal.feed(b"\x1b[38;2;1;2;3m\x1b[48;5;196m\x1b[4;7mA");
        let cell = rendered_cell(&terminal.cells()[0], [0; 4], false);
        let style = cell.1.unwrap();
        assert_eq!(cell.0, 'A');
        assert_eq!(style.rgba(), [255, 0, 0, 255]);
        assert_eq!(style.background(), Some([1, 2, 3, 255]));
        assert!(style.underline());
    }

    #[test]
    fn block_cursor_inverts_selected_text_again() {
        let mut terminal = Terminal::new(2, 1);
        terminal.feed(b"\x1b[31;44;7mA");
        let selected = rendered_cell(&terminal.cells()[0], [0; 4], false)
            .1
            .unwrap();
        let cursor = rendered_cell(&terminal.cells()[0], [0; 4], true).1.unwrap();
        assert_eq!(cursor.rgba(), selected.background().unwrap());
        assert_eq!(cursor.background(), Some(selected.rgba()));
    }
}

fn color(color: TerminalColor, default: [u8; 4]) -> [u8; 4] {
    match color {
        TerminalColor::Default => default,
        TerminalColor::Rgb { red, green, blue } => [red, green, blue, 255],
        TerminalColor::Indexed(index) => {
            const ANSI: [[u8; 3]; 16] = [
                [0, 0, 0],
                [205, 49, 49],
                [13, 188, 121],
                [229, 229, 16],
                [36, 114, 200],
                [188, 63, 188],
                [17, 168, 205],
                [229, 229, 229],
                [102, 102, 102],
                [241, 76, 76],
                [35, 209, 139],
                [245, 245, 67],
                [59, 142, 234],
                [214, 112, 214],
                [41, 184, 219],
                [255, 255, 255],
            ];
            let rgb = match index {
                0..=15 => ANSI[index as usize],
                16..=231 => {
                    let v = index - 16;
                    let levels = [0, 95, 135, 175, 215, 255];
                    [
                        levels[(v / 36) as usize],
                        levels[(v / 6 % 6) as usize],
                        levels[(v % 6) as usize],
                    ]
                }
                _ => {
                    let gray = 8 + (index - 232) * 10;
                    [gray, gray, gray]
                }
            };
            [rgb[0], rgb[1], rgb[2], 255]
        }
    }
}

pub(super) fn input(frontend: u64, name: Option<&str>, bytes: &[u8]) -> bool {
    {
        let mut routes = ROUTES.lock();
        if let Some(native) = routes
            .routes
            .iter_mut()
            .find(|r| r.frontend == Some(frontend) && Some(r.lease.name()) == name)
            .and_then(|r| r.native.as_mut())
            .filter(|n| n.active)
        {
            // Bound untrusted transport bursts. Overflowed input is discarded.
            if native.input.len() + bytes.len() <= 4096 {
                native.input.extend(bytes.iter().copied());
            }
            drop(routes);
            super::service::notify_work();
            return true;
        }
    }
    let owner_target = {
        let routes = ROUTES.lock();
        routes
            .routes
            .iter()
            .find(|route| {
                route.frontend == Some(frontend)
                    && route.selected
                    && Some(route.lease.name()) == name
            })
            .and_then(|route| route.owner.map(|owner| (owner, route.lease.clone())))
    };
    let Some((owner, lease)) = owner_target else {
        return false;
    };
    crate::hv::blueprint_console_submit_stdin_for_lease(owner.vm, &lease, owner.run, bytes);
    true
}
pub(super) fn pointer(
    frontend: u64,
    name: Option<&str>,
    event: &crate::ui4::Ui4PointerEvent,
    scale: u32,
) {
    let bytes = {
        let routes = ROUTES.lock();
        let Some(route) = routes.routes.iter().find(|route| {
            route.frontend == Some(frontend)
                && route.selected
                && Some(route.lease.name()) == name
                && route.active()
        }) else {
            return;
        };
        let (cols, rows) = route.screen.dimensions();
        let Some((cell_width, cell_height)) = crate::r::services::shell3_font_service::cell_extent(scale) else { return; };
        let col =
            (event.local_x.max(0) as usize / (cell_width as usize)).min(cols - 1);
        let row =
            (event.local_y.max(0) as usize / (cell_height as usize)).min(rows - 1);
        let mut bytes = Vec::new();
        for (mask, button) in [
            (1, trueos_terminal::MouseButton::Left),
            (4, trueos_terminal::MouseButton::Middle),
            (2, trueos_terminal::MouseButton::Right),
        ] {
            if event.buttons_pressed & mask != 0 {
                if let Some(sequence) = route.screen.mouse_button(button, true, col, row) {
                    bytes.extend(sequence);
                }
            }
            if event.buttons_released & mask != 0 {
                if let Some(sequence) = route.screen.mouse_button(button, false, col, row) {
                    bytes.extend(sequence);
                }
            }
        }
        for _ in 0..event.wheel.unsigned_abs().min(8) {
            if let Some(sequence) = route.screen.mouse_wheel(event.wheel > 0, col, row) {
                bytes.extend(sequence);
            }
        }
        if event.dx != 0 || event.dy != 0 {
            let held = if event.buttons_down & 1 != 0 {
                Some(trueos_terminal::MouseButton::Left)
            } else if event.buttons_down & 2 != 0 {
                Some(trueos_terminal::MouseButton::Right)
            } else if event.buttons_down & 4 != 0 {
                Some(trueos_terminal::MouseButton::Middle)
            } else {
                None
            };
            if let Some(sequence) = route.screen.mouse_motion(held, col, row) {
                bytes.extend(sequence);
            }
        }
        bytes
    };
    if !bytes.is_empty() {
        input(frontend, name, &bytes);
    }
}

/// Consume named-key/text pairs once; modifiers become ordinary terminal bytes.
pub(super) fn keyboard(
    frontend: u64,
    name: Option<&str>,
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> bool {
    use crate::r::keyboard::*;
    let suppress = {
        let mut routes = ROUTES.lock();
        let Some(route) = routes.routes.iter_mut().find(|route| {
            route.frontend == Some(frontend)
                && route.selected
                && Some(route.lease.name()) == name
                && route.active()
        }) else {
            return false;
        };
        let key = (event.controller_id, event.device_seq, event.codepoint);
        if event.kind == KEYBOARD_OUTPUT_KIND_TEXT {
            route.suppressed_text.take() == Some(key)
        } else {
            route.suppressed_text = (event.kind == KEYBOARD_OUTPUT_KIND_KEY
                && event.codepoint != 0
                && named_key_sequence(event.key_code).is_some())
            .then_some(key);
            false
        }
    };
    if !suppress {
        input(frontend, name, &keyboard_bytes(event));
    }
    true
}
fn keyboard_bytes(event: &crate::r::keyboard::TrueosKeyboardOutputEvent) -> Vec<u8> {
    use crate::r::keyboard::*;
    let mut bytes = if event.kind == KEYBOARD_OUTPUT_KIND_KEY {
        named_key_sequence(event.key_code).unwrap_or(&[]).to_vec()
    } else if event.kind == KEYBOARD_OUTPUT_KIND_TEXT {
        let Some(ch) = char::from_u32(event.codepoint) else {
            return Vec::new();
        };
        let mut buffer = [0; 4];
        let text = ch.encode_utf8(&mut buffer).as_bytes();
        if event.modifiers & 0x11 != 0 && text.len() == 1 {
            let control = match text[0] {
                b'a'..=b'z' => Some(text[0] - b'a' + 1),
                b'A'..=b'Z' => Some(text[0] - b'A' + 1),
                b'@' | b' ' => Some(0),
                b'[' => Some(27),
                b'\\' => Some(28),
                b']' => Some(29),
                b'^' => Some(30),
                b'_' => Some(31),
                b'?' => Some(127),
                _ => None,
            };
            control.map_or_else(|| text.to_vec(), |byte| alloc::vec![byte])
        } else {
            text.to_vec()
        }
    } else {
        Vec::new()
    };
    if event.modifiers & 0x44 != 0 && !bytes.is_empty() {
        bytes.insert(0, 27);
    }
    bytes
}
fn named_key_sequence(key_code: u16) -> Option<&'static [u8]> {
    match key_code {
        // Crossterm treats BS (0x08) as Ctrl-H; DEL is terminal Backspace.
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => Some(b"\x7f"),
        crate::r::keyboard::KEYBOARD_KEY_TAB => Some(b"\t"),
        crate::r::keyboard::KEYBOARD_KEY_ENTER => Some(b"\r"),
        crate::r::keyboard::KEYBOARD_KEY_ESCAPE => Some(b"\x1b"),
        crate::r::keyboard::KEYBOARD_KEY_SPACE => Some(b" "),
        crate::r::keyboard::KEYBOARD_KEY_DELETE => Some(b"\x1b[3~"),
        crate::r::keyboard::KEYBOARD_KEY_INSERT => Some(b"\x1b[2~"),
        crate::r::keyboard::KEYBOARD_KEY_HOME => Some(b"\x1b[H"),
        crate::r::keyboard::KEYBOARD_KEY_END => Some(b"\x1b[F"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_UP => Some(b"\x1b[5~"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_DOWN => Some(b"\x1b[6~"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => Some(b"\x1b[A"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => Some(b"\x1b[B"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => Some(b"\x1b[D"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => Some(b"\x1b[C"),
        crate::r::keyboard::KEYBOARD_KEY_F1 => Some(b"\x1bOP"),
        crate::r::keyboard::KEYBOARD_KEY_F2 => Some(b"\x1bOQ"),
        crate::r::keyboard::KEYBOARD_KEY_F3 => Some(b"\x1bOR"),
        crate::r::keyboard::KEYBOARD_KEY_F4 => Some(b"\x1bOS"),
        crate::r::keyboard::KEYBOARD_KEY_F5 => Some(b"\x1b[15~"),
        crate::r::keyboard::KEYBOARD_KEY_F6 => Some(b"\x1b[17~"),
        crate::r::keyboard::KEYBOARD_KEY_F7 => Some(b"\x1b[18~"),
        crate::r::keyboard::KEYBOARD_KEY_F8 => Some(b"\x1b[19~"),
        crate::r::keyboard::KEYBOARD_KEY_F9 => Some(b"\x1b[20~"),
        crate::r::keyboard::KEYBOARD_KEY_F10 => Some(b"\x1b[21~"),
        crate::r::keyboard::KEYBOARD_KEY_F11 => Some(b"\x1b[23~"),
        crate::r::keyboard::KEYBOARD_KEY_F12 => Some(b"\x1b[24~"),
        _ => None,
    }
}

pub(crate) fn frontend_for_target(target: &MatrixTarget) -> Option<Frontend> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|route| route.lease == lease)?;
    let (cols, rows) = route.screen.dimensions();
    Some(Frontend {
        id: route.frontend?,
        cols,
        rows,
    })
}
pub(crate) fn queue_launch(frontend: u64, app: QueuedBlueprint) {
    ROUTES.lock().pending_launches.push((frontend, app));
    super::service::notify_work();
}
