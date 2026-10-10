//! Shell3 terminal leases. VM lifecycle stays in HV; bytes never reach Shell3's prompt.
use super::{RgbaColor, update::RenderedLine};
use crate::shell3::{MatrixSlotLease, MatrixTarget};
use crate::shell2::{MatrixSlotAttachment};
use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};
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
    launch: Option<crate::shell2::cmds::run::QueuedBlueprint>,
    input: VecDeque<u8>,
    notices: VecDeque<String>,
}
struct Route {
    native: Option<Native>,
    lease: MatrixSlotLease,
    frontend: u64,
    selected: bool,
    vm: Option<u8>,
    owner: Option<Owner>,
    screen: Terminal,
    surface_generation: u64,
    suppressed_text: Option<(u32, u32, u32)>,
}
impl Route {
    fn active(&self) -> bool { self.owner.is_some() || self.native.as_ref().is_some_and(|n| n.active) }
}
struct Registry {
    routes: Vec<Route>,
    ui4_windows: Vec<(u64, crate::ui4::WindowId)>,
    revisions: Vec<(u64, u64)>,
    next_revision: u64,
    remote_frontends: Vec<(u64, remote::Output)>,
}
impl Registry {
    const fn new() -> Self {
        Self {
            routes: Vec::new(),
            ui4_windows: Vec::new(),
            revisions: Vec::new(),
            next_revision: 0,
            remote_frontends: Vec::new(),
        }
    }
    fn changed(&mut self, id: u64) {
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
        .find(|entry| entry.0 == route.frontend)
        .map(|entry| entry.1)
}
pub(crate) fn vm_for_window(window: crate::ui4::WindowId) -> Option<u8> {
    let routes = ROUTES.lock();
    let frontend = routes.ui4_windows.iter().find(|entry| entry.1 == window)?.0;
    let owner = routes
        .routes
        .iter()
        .find(|route| route.frontend == frontend && route.selected)?
        .owner?;
    (crate::hv::vm_run_generation(owner.vm) == Some(owner.run)).then_some(owner.vm)
}
pub(super) fn new_frontend() -> u64 {
    NEXT_FRONTEND.fetch_add(1, Ordering::Relaxed)
}

/// SSH renders on the peer terminal. Local UI4 frontends keep the cell parser.
pub(super) fn bind_remote_frontend(frontend: u64) {
    let mut routes = ROUTES.lock();
    if !routes.remote_frontends.iter().any(|entry| entry.0 == frontend) {
        routes.remote_frontends.push((frontend, remote::Output::new()));
    }
}

pub(super) fn has_ui4_window(frontend: u64) -> bool {
    ROUTES.lock().ui4_windows.iter().any(|entry| entry.0 == frontend)
}

pub(super) fn remote_active(frontend: u64) -> bool {
    let routes = ROUTES.lock();
    routes.remote_frontends.iter().any(|entry| entry.0 == frontend)
        && routes.routes.iter().any(|route| route.frontend == frontend && route.owner.is_some())
}

pub(super) fn take_remote_output(frontend: u64, available: usize) -> Option<RemoteDrain> {
    let mut routes = ROUTES.lock();
    let active = routes.routes.iter().any(|route| route.frontend == frontend && route.owner.is_some());
    let output = &mut routes.remote_frontends.iter_mut().find(|entry| entry.0 == frontend)?.1;
    Some(output.take(available, active))
}

struct Attachment;
impl MatrixSlotAttachment for Attachment {
    fn on_matrix_slot_freed(&self, lease: &MatrixSlotLease) {
        let mut routes = ROUTES.lock();
        if let Some(index) = routes.routes.iter().position(|route| route.lease == *lease) {
            let route = routes.routes.remove(index);
            if route.owner.is_some() {
                if let Some((_, output)) = routes.remote_frontends.iter_mut().find(|entry| entry.0 == route.frontend) {
                    output.end();
                }
            }
            routes.changed(route.frontend);
        }
    }
}

pub(crate) fn attach(
    frontend: Frontend,
    target: &MatrixTarget,
) -> Result<(), alloc::string::String> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    {
        let mut routes = ROUTES.lock();
        if routes.routes.iter().any(|route| route.lease == lease) {return Err("tui: terminal target already attached".into());}
        routes.routes.push(Route {
            native: None,
            lease: lease.clone(),
            frontend: frontend.id,
            selected: true,
            vm: None,
            owner: None,
            screen: Terminal::new(frontend.cols, frontend.rows),
            surface_generation: 1,
            suppressed_text: None,
        });
        routes.changed(frontend.id);
    }
    if crate::shell2::attach_matrix_slot_resource(&lease, Arc::new(Attachment)).is_err() {
        Attachment.on_matrix_slot_freed(&lease);
        return Err("tui: terminal target expired or attachments full".into());
    }
    Ok(())
}

pub(crate) fn supports(target: &MatrixTarget) -> bool {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    crate::shell2::matrix_slot_is_live(&lease)
        && ROUTES
            .lock()
            .routes
            .iter()
            .any(|route| route.lease == lease)
}
pub(crate) fn bind_vm(target: &MatrixTarget, vm: u8) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    if let Some(route) = ROUTES
        .lock()
        .routes
        .iter_mut()
        .find(|route| route.lease == lease)
    {
        route.vm = Some(vm);
    }
}
pub(crate) fn claim(target: &MatrixTarget, vm: u8) -> Option<bool> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let run = crate::hv::vm_run_generation(vm)?;
    let live = crate::shell2::matrix_slot_is_live(&lease);
    let mut routes = ROUTES.lock();
    let index = routes
        .routes
        .iter()
        .position(|route| route.lease == lease)?;
    let frontend = routes.routes[index].frontend;
    let owner = Owner { vm, run };
    if !live
        || !routes.routes[index].selected
        || routes.routes[index].vm != Some(vm)
        || routes.routes.iter().any(|route| {
            route.frontend == frontend && route.active() && route.lease != lease
        })
        || routes.routes[index]
            .owner
            .is_some_and(|current| current != owner)
    {
        return Some(false);
    }
    if routes.routes[index].owner.is_none() {
        if let Some((_, output)) = routes.remote_frontends.iter_mut().find(|entry| entry.0 == frontend) {
            if !output.begin() { return Some(false); }
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
        if let Some((_, output)) = routes.remote_frontends.iter_mut().find(|entry| entry.0 == frontend) {
            output.end();
        }
    }
    routes.changed(frontend);
    let window = routes.ui4_windows.iter().find(|entry| entry.0 == frontend).map(|entry| entry.1);
    drop(routes);
    if let Some(window) = window { crate::ui4::drag_drop::release_terminal(crate::ui4::WindowOwner::Vm(vm), window); }
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
        if let Some((_, output)) = routes.remote_frontends.iter_mut().find(|entry| entry.0 == frontend) {
            let written = output.write(bytes);
            drop(routes);
            super::service::notify_work();
            return Some(written);
        }
        let route = routes.routes.iter_mut().find(|route| route.lease == lease)?;
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
    if supports(target) {return Err("tui: terminal target already attached".into());}
    if !park(frontend.id) {return Err("tui: could not release the previous terminal owner".into());}
    attach(frontend, target)?;
    let lease = crate::shell3::matrix_target_slot_lease(target);
    if let Some(route) = ROUTES.lock().routes.iter_mut().find(|r| r.lease == lease) {
        route.native = Some(Native {active: true, painted: false, return_to_default: false, launch: None, input: VecDeque::new(), notices: VecDeque::new()});
    }
    Ok(())
}
pub(super) fn cancel_native_attach(target: &MatrixTarget) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(index) = routes.routes.iter().position(|r| r.lease == lease && r.native.is_some()) {
        let route = routes.routes.remove(index);
        routes.changed(route.frontend);
    }
}
pub(super) fn native_slot(name: &str) -> bool {
    ROUTES.lock().routes.iter().any(|r| r.lease.name() == name && r.native.is_some())
}
pub(super) fn native_visible(target: &MatrixTarget) -> bool {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    ROUTES.lock().routes.iter().any(|r| r.lease == lease && r.native.as_ref().is_some_and(|n| n.active))
}
pub(super) fn native_pending_frame(frontend: u64, name: Option<&str>) -> bool {
    ROUTES.lock().routes.iter().any(|route| {
        route.frontend == frontend && Some(route.lease.name()) == name
            && route.native.as_ref().is_some_and(|native| native.active && !native.painted)
    })
}
pub(super) fn native_transport_scope(target: &MatrixTarget) -> Option<u8> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|route| route.lease == lease && route.native.is_some())?;
    Some(if routes.remote_frontends.iter().any(|entry| entry.0 == route.frontend) {
        crate::shell2::TRANSPORT_NET_TCP_SCOPE
    } else {
        crate::shell2::TRANSPORT_LOCAL_SCOPE
    })
}
pub(super) fn native_read(target: &MatrixTarget) -> Option<(Vec<u8>, Vec<String>)> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    let native = routes.routes.iter_mut().find(|r| r.lease == lease)?.native.as_mut()?;
    Some((native.input.drain(..).collect(), native.notices.drain(..).collect()))
}
pub(crate) fn native_notice(target: &MatrixTarget, message: &str) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(native) = routes.routes.iter_mut().find(|r| r.lease == lease).and_then(|r| r.native.as_mut()) {
        if native.notices.len() == 32 {native.notices.pop_front();}
        native.notices.push_back(message.into());
    }
    super::service::notify_work();
}
pub(super) fn native_write(target: &MatrixTarget, bytes: &[u8]) {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let mut routes = ROUTES.lock();
    if let Some(route) = routes.routes.iter_mut().find(|r| r.lease == lease && r.native.is_some()) {
        route.screen.feed(bytes);
        route.native.as_mut().unwrap().painted = true;
        let frontend = route.frontend;
        if route.active() {routes.changed(frontend);}
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
    routes.routes.iter_mut().filter(|r| r.frontend == frontend).fold(false, |exit, r| {
        exit | r.native.as_mut().is_some_and(|n| core::mem::take(&mut n.return_to_default))
    })
}

/// Resolve the current frontend on reentry, including a parked helper moved to SSH.
pub(super) fn native_frontend(target: &MatrixTarget) -> Option<Frontend> {
    let lease = crate::shell3::matrix_target_slot_lease(target);
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|r| r.lease == lease && r.native.as_ref().is_some_and(|n| n.active))?;
    let (cols, rows) = route.screen.dimensions();
    Some(Frontend { id: route.frontend, cols, rows })
}

pub(super) fn native_launch(target: &MatrixTarget, app: crate::shell2::cmds::run::QueuedBlueprint) {
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

pub(super) fn take_native_launch(frontend: u64) -> Option<crate::shell2::cmds::run::QueuedBlueprint> {
    ROUTES.lock().routes.iter_mut().filter(|r| r.frontend == frontend)
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
pub(super) fn park(frontend: u64) -> bool {
    {
        let mut routes = ROUTES.lock();
        let mut changed = false;
        for route in routes.routes.iter_mut().filter(|r| r.frontend == frontend) {
            if let Some(native) = route.native.as_mut() {
                changed |= native.active;
                native.active = false;
                native.input.clear();
                route.suppressed_text = None;
            }
        }
        if changed {routes.changed(frontend);}
    }
    let vm = ROUTES
        .lock()
        .routes
        .iter()
        .find(|route| route.frontend == frontend && route.owner.is_some())
        .and_then(|route| route.owner.map(|owner| owner.vm));
    vm.is_none_or(crate::hv::blueprint_console_return_to_cli)
}
pub(super) fn select(frontend: Frontend, name: Option<&str>) -> bool {
    // A helper has one input owner. A parked helper may move to another shell.
    {
        let mut routes = ROUTES.lock();
        if let Some(route) = routes.routes.iter_mut().find(|r| Some(r.lease.name()) == name && r.native.is_some()) {
            if route.frontend != frontend.id {
                if route.active() {return false;}
                let old = route.frontend;
                route.frontend = frontend.id;
                route.selected = false;
                routes.changed(old);
            }
        }
    }
    let needs_park = ROUTES.lock().routes.iter().any(|route| {
        route.frontend == frontend.id && route.active() && Some(route.lease.name()) != name
    });
    if needs_park && !park(frontend.id) {
        return false;
    }
    let mut routes = ROUTES.lock();
    let mut changed = false;
    for route in routes
        .routes
        .iter_mut()
        .filter(|route| route.frontend == frontend.id)
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
/// Explicit navigation reenters a parked host helper, including its current slot.
/// Resize/repaint selection above never silently undoes an operator park.
pub(super) fn select_for_navigation(frontend: Frontend, name: &str) -> bool {
    if !select(frontend, Some(name)) {return false;}
    let mut routes = ROUTES.lock();
    if let Some(route) = routes.routes.iter_mut().find(|r| r.frontend == frontend.id && r.lease.name() == name) {
        if let Some(native) = route.native.as_mut() {
            if !native.active {
                native.active = true;
                native.return_to_default = false;
                route.suppressed_text = None;
                routes.changed(frontend.id);
            }
        }
    }
    true
}
pub(super) fn request(frontend: Frontend, name: &str) -> Result<(), &'static str> {
    if native_slot(name) {
        if !select(frontend, Some(name)) {return Err("tui: terminal UI is owned by another Shell3");}
        let mut routes = ROUTES.lock();
        if let Some(route) = routes.routes.iter_mut().find(|r| r.frontend == frontend.id && r.lease.name() == name) {
            if let Some(native) = route.native.as_mut() {native.active = true; native.return_to_default = false;}
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
        if routes.routes[index].frontend != frontend.id {
            if routes.routes[index].owner.is_some() {
                return Err("tui: terminal UI is owned by another Shell3");
            }
            let old = routes.routes[index].frontend;
            routes.routes[index].frontend = frontend.id;
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
    ROUTES.lock().ui4_windows.retain(|entry| entry.0 != frontend);
    let names: Vec<_> = ROUTES
        .lock()
        .routes
        .iter()
        .filter(|route| route.frontend == frontend)
        .map(|route| alloc::string::String::from(route.lease.name()))
        .collect();
    for name in names {
        super::MatrixSlots::drop_slot(Some(&name));
    }
    ROUTES.lock().revisions.retain(|entry| entry.0 != frontend);
    ROUTES.lock().remote_frontends.retain(|entry| entry.0 != frontend);
}

pub(super) fn active(frontend: u64, name: Option<&str>) -> bool {
    ROUTES.lock().routes.iter().any(|route| {
        route.frontend == frontend && Some(route.lease.name()) == name && route.active()
    })
}

/// Only a currently owned lease may request mouse capture from the client.
pub(super) fn mouse_options(frontend: u64, name: Option<&str>) -> trueos_terminal::MouseOptions {
    ROUTES.lock().routes.iter().find(|route| {
        route.frontend == frontend && Some(route.lease.name()) == name && route.active()
    }).map(|route| route.screen.mouse_options()).unwrap_or_default()
}

pub(super) fn snapshot(frontend: u64, name: Option<&str>) -> Option<Vec<RenderedLine>> {
    let routes = ROUTES.lock();
    let route = routes.routes.iter().find(|route| {
        route.frontend == frontend && Some(route.lease.name()) == name && route.active()
    })?;
    if route.owner.is_some() && routes.remote_frontends.iter().any(|entry| entry.0 == frontend) {
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
                    if row_index == 0 { super::update::CONTROL_BACKGROUND }
                    else { super::update::MATRIX_BACKGROUND }
                } else { [0, 0, 0, 255] };
                row.iter()
                    .enumerate()
                    .map(|(col, cell)| {
                        let mut foreground = color(cell.style.foreground, [255, 255, 255, 255]);
                        let mut background = color(cell.style.background, default_background);
                        if cursor.visible && cursor.row == row_index && cursor.col == col {
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
                    })
                    .collect()
            })
            .collect(),
    )
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
        if let Some(native) = routes.routes.iter_mut().find(|r| r.frontend == frontend && Some(r.lease.name()) == name).and_then(|r| r.native.as_mut()).filter(|n| n.active) {
            // Bound untrusted transport bursts. Overflowed input is discarded.
            if native.input.len() + bytes.len() <= 4096 {native.input.extend(bytes.iter().copied());}
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
            .find(|route| route.frontend == frontend && Some(route.lease.name()) == name)
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
            route.frontend == frontend && Some(route.lease.name()) == name && route.active()
        }) else {
            return;
        };
        let (cols, rows) = route.screen.dimensions();
        let col =
            (event.local_x.max(0) as usize / (microfont::FWIDTH * scale as usize)).min(cols - 1);
        let row =
            (event.local_y.max(0) as usize / (microfont::FHEIGHT * scale as usize)).min(rows - 1);
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
            route.frontend == frontend && Some(route.lease.name()) == name && route.active()
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
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => Some(b"\x08"),
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
