//! UI4 input focus and delivery over the kernel HID rings.
//!
//! The HID/HUT layer owns device discovery and identity. UI4 starts at its
//! sequence rings: it hit-tests windows, keeps one selected-frame association
//! per cursor plus a most-recent application input focus, maintains pointer
//! capture per cursor source, associates keyboards through HUT combos, and
//! queues callbacks for the trusted `WindowOwner`. Consumers never drain a
//! global HID queue.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use embassy_sync::signal::Signal;
use heapless::Vec;
use spin::Mutex;
use trueos_time::{Duration, with_timeout};

use super::{
    OutputId, Ui4CursorSource, WindowId, WindowOwner, WindowPlacement, WindowSnapshot, WindowState,
};

const MAX_CURSOR_ROUTES: usize = 32;
const MAX_OWNER_QUEUES: usize = 64;
const MAX_OWNER_EVENTS: usize = 256;
const _: () = assert!(MAX_OWNER_EVENTS >= super::window_broker::MAX_WINDOWS);
const CURSOR_BATCH: usize = 64;
const KEYBOARD_BATCH: usize = 64;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const SECONDARY_BUTTON_MASK: u32 = 1 << 1;
const MIDDLE_BUTTON_MASK: u32 = 1 << 2;
// Exact-window F1 capture remains outside the current UI input contract. Full
// display capture is deliberately available only through Shell2 `shot`.
const INTERACTIVE_SCREENSHOT_ENABLED: bool = false;
const FRAME_DRAG_GESTURE_MIN_TRAVEL_PX: u32 = 8;
const DOCK_REFERENCE_WIDTH_MM: u32 = 64;
const DOCK_REFERENCE_HEIGHT_MM: u32 = 40;
const DOCK_CORNER_MM: u32 = 24;
const DOCK_EDGE_DEPTH_MM: u32 = 12;
const CONTEXT_MENU_BORDER_PX: u32 = 2;
pub(super) const CONTEXT_MENU_OFFSET_PX: u32 = 14;
pub(super) const CONTEXT_MENU_WIDTH_PX: u32 = super::color_picker::PICKER_WIDTH;
pub(super) const CONTEXT_MENU_ROW_GAP_PX: u32 = 2;
pub(super) const CONTEXT_MENU_ROW_HEIGHT_PX: u32 = (microfont::FHEIGHT as u32)
    .saturating_mul(2)
    .saturating_add(CONTEXT_MENU_ROW_GAP_PX);
pub(super) const DESKTOP_CONTEXT_MENU_ENTRY_COUNT: u32 = 3;
pub(super) const DESKTOP_CONTEXT_MENU_HORIZONTAL_INSET_PX: u32 = 12;
pub(super) const DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX: u32 = 12;

static OWNER_QUEUE_DROPS: AtomicU32 = AtomicU32::new(0);
static KEYBOARD_TEXT_FORWARDS: AtomicU32 = AtomicU32::new(0);
static FAT_SOFTWARE_CURSORS: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DesktopShellLaunchRequest {
    source: Ui4CursorSource,
    x: u32,
    y: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct DesktopShellLaunch {
    pub(super) source: Ui4CursorSource,
    pub(super) x: u32,
    pub(super) y: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DesktopShellLaunchIntent {
    token: u32,
    launch: DesktopShellLaunch,
}

static DESKTOP_SHELL_LAUNCH_REQUESTS: Mutex<Vec<DesktopShellLaunchRequest, MAX_CURSOR_ROUTES>> =
    Mutex::new(Vec::new());
static DESKTOP_SHELL_LAUNCH_INTENTS: Mutex<Vec<DesktopShellLaunchIntent, MAX_CURSOR_ROUTES>> =
    Mutex::new(Vec::new());
static DESKTOP_SHELL_LAUNCH_SIGNAL: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum DesktopContextMenuAction {
    ColorPicker,
    Shell,
    Link,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4PointerEvent {
    pub(crate) source: Ui4CursorSource,
    pub(crate) window: WindowId,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) local_x: i32,
    pub(crate) local_y: i32,
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) wheel: i16,
    pub(crate) buttons_down: u32,
    pub(crate) buttons_pressed: u32,
    pub(crate) buttons_released: u32,
    pub(crate) combo_id: u32,
    pub(crate) vcursor: bool,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Ui4KeyboardEvent {
    
    pub(crate) window: WindowId,
    pub(crate) event: crate::r::keyboard::TrueosKeyboardOutputEvent,
    
    
}



static INPUT_BROKER: Mutex<InputBroker> = Mutex::new(InputBroker::new());
static OWNER_QUEUES: Mutex<Vec<OwnerQueue, MAX_OWNER_QUEUES>> = Mutex::new(Vec::new());
/// Resize is final broker state, not a lossy input sample. The global capacity
/// covers every broker window while avoiding one registry-sized inline array
/// per owner queue.
static OWNER_RESIZE_STATES: Mutex<Vec<OwnerResizeState, MAX_OWNER_EVENTS>> = Mutex::new(Vec::new());
static SLOT4_VISUAL_CHANGE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();

fn capture_gt_power_mode_hotkey(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> super::GlobalKeyboardDisposition {
    if event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        || event.key_code != crate::r::keyboard::KEYBOARD_KEY_F12
    {
        return super::GlobalKeyboardDisposition::PassThrough;
    }

    if event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_PRESS != 0 {
        match crate::intel::toggle_global_gt_power_mode() {
            Ok(marker) => crate::log_info!(target: "ui4";
                "ui4/input: F12 global GT power mode accepted marker={} active={} requested_mhz={} actual_mhz={} rp0_mhz={} spirit_delivery=async-engine-marker key_delivery=consumed\n",
                marker.generation,
                marker.active as u8,
                marker.requested_mhz,
                marker.actual_mhz,
                marker.rp0_mhz,
            ),
            Err(reason) => crate::log_warn!(target: "ui4";
                "ui4/input: F12 global GT power mode rejected reason={} spirit_delivery=none key_delivery=consumed\n",
                reason,
            ),
        }
    }
    // Consume both edges so applications never observe half of the global
    // function-key gesture.
    super::GlobalKeyboardDisposition::Consume
}

fn capture_spirit_scanrunmon_hotkey(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> super::GlobalKeyboardDisposition {
    if event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        || event.key_code != crate::r::keyboard::KEYBOARD_KEY_F10
    {
        return super::GlobalKeyboardDisposition::PassThrough;
    }
    if event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_PRESS != 0 {
        if let Err(error) = crate::spirit::scanrunmon() {
            crate::log_warn!(target: "ui4"; "ui4/input: F10 scanrunmon rejected error={:?}\n", error);
        }
    }
    super::GlobalKeyboardDisposition::Consume
}

fn capture_software_cursor_size_hotkey(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> super::GlobalKeyboardDisposition {
    if event.kind != crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        || event.key_code != crate::r::keyboard::KEYBOARD_KEY_F11
    {
        return super::GlobalKeyboardDisposition::PassThrough;
    }

    if event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_PRESS != 0 {
        let fat = !FAT_SOFTWARE_CURSORS.fetch_xor(true, Ordering::AcqRel);
        SLOT4_VISUAL_CHANGE.signal(());
        crate::log_info!(target: "ui4";
            "ui4/input: F11 global software cursor size toggled mode={} scale={}x key_delivery=consumed\n",
            if fat { "fat" } else { "small" },
            if fat { 3 } else { 1 },
        );
    }
    // Consume both edges so applications never observe half of the global
    // function-key gesture.
    super::GlobalKeyboardDisposition::Consume
}

#[trueos_executor::task]
pub(crate) async fn ui4_input_service_task(ap1_spawner: crate::workers::WorkerSpawner) {
    match super::register_global_keyboard_hook(u8::MAX, capture_spirit_scanrunmon_hotkey) {
        Ok(_) => {
            crate::log_info!(target: "ui4"; "ui4/input: global hotkey online key=F10 action=spirit-scanrunmon duration_ms=250\n")
        }
        Err(error) => {
            crate::log_warn!(target: "ui4"; "ui4/input: global hotkey unavailable key=F10 error={:?}\n", error)
        }
    }
    let cursor_size_hook =
        super::register_global_keyboard_hook(u8::MAX, capture_software_cursor_size_hotkey);
    match cursor_size_hook {
        Ok(_) => crate::log_info!(target: "ui4";
            "ui4/input: global hotkey online key=F11 action=toggle-software-cursor-size small=1x fat=3x key_delivery=consumed\n"
        ),
        Err(error) => crate::log_warn!(target: "ui4";
            "ui4/input: global hotkey unavailable key=F11 error={:?}\n",
            error,
        ),
    }
    let power_mode_hook =
        super::register_global_keyboard_hook(u8::MAX, capture_gt_power_mode_hotkey);
    match power_mode_hook {
        Ok(_) => crate::log_info!(target: "ui4";
            "ui4/input: global hotkey online key=F12 action=toggle-gt-power-mode delivery=engine-confirmed-marker->spirit-async key_delivery=consumed\n"
        ),
        Err(error) => crate::log_warn!(target: "ui4";
            "ui4/input: global hotkey unavailable key=F12 error={:?}\n",
            error,
        ),
    }
    let launcher_spawner = crate::workers::pick_background_spawner().unwrap_or(ap1_spawner);
    match ui4_desktop_shell_launcher_task() {
        Ok(token) => {
            launcher_spawner.spawn(token);
            crate::log_info!(target: "ui4";
                "ui4/input: desktop shell launcher online carrier_slot={} fallback_ap1={}\n",
                launcher_spawner.cpu_slot(),
                launcher_spawner.cpu_slot() == ap1_spawner.cpu_slot(),
            );
        }
        Err(error) => crate::log_warn!(target: "ui4";
            "ui4/input: desktop shell launcher unavailable error={:?}\n",
            error,
        ),
    }
    crate::log_info!(target: "ui4";
        "ui4/input: service online source=hid-sequence-rings cursor_wake=producer-signal keyboard_watchdog_hz={} selection=per-cursor-zero-or-one-frame+most-recent-input-focus first-click=absorb-select keyboard=global-hooks-before-ui4/hut-combo/exact-slot/recent-selector-fallback start_key=reveal-menu-button cursor=slot4-software/all-active-sources/per-frame-per-cursor hardware-cursor=preferred-physical-source/concurrent virtual=vcursor frame_drag=secondary-button/per-cursor-selected-frame-only dock=top-center-maximize+center-sides-halves+corners-quadrants+bottom-center-resize-latch-10s/dpi-mm-first outline=primary-button/selected-frame-only desktop_menu=per-cursor/color-picker+shell owner_events=selected-frame-only screenshot=parked\n",
        super::INTERACTION_CADENCE_HZ,
    );
    loop {
        let cursor_activity = INPUT_BROKER.lock().pump();
        super::context_menu::dispatch_pending_callbacks();
        if cursor_activity {
            SLOT4_VISUAL_CHANGE.signal(());
        }
        // Cursor reports wake the broker as soon as their absolute snapshot
        // and HUT identity are published. The timeout preserves the existing
        // bounded-latency path for keyboard/general HID producers and acts as
        // a lost-wakeup watchdog without turning cursor ingestion into a poll.
        let _ = with_timeout(
            Duration::from_hz(super::INTERACTION_CADENCE_HZ),
            crate::usb2::hid::wait_cursor_event_ready(),
        )
        .await;
    }
}

pub(super) fn request_desktop_shell_launch(source: Ui4CursorSource, x: u32, y: u32) {
    let queued = DESKTOP_SHELL_LAUNCH_REQUESTS
        .lock()
        .push(DesktopShellLaunchRequest { source, x, y })
        .is_ok();
    if queued {
        DESKTOP_SHELL_LAUNCH_SIGNAL.signal(());
        crate::log_info!(target: "ui4";
            "ui4/input: desktop shell launch requested cursor={}:{}:{} position={},{} source=app.db archive=shell.bp policy=local-builtin-best-effort\n",
            source.controller_id,
            source.slot_id,
            source.ep_target,
            x,
            y,
        );
    } else {
        crate::log_warn!(target: "ui4";
            "ui4/input: desktop shell launch dropped cursor={}:{}:{} reason=request-queue-full capacity={}\n",
            source.controller_id,
            source.slot_id,
            source.ep_target,
            MAX_CURSOR_ROUTES,
        );
    }
}

#[trueos_executor::task(pool_size = 1)]
async fn ui4_desktop_shell_launcher_task() {
    loop {
        let request = {
            let mut requests = DESKTOP_SHELL_LAUNCH_REQUESTS.lock();
            (!requests.is_empty()).then(|| requests.remove(0))
        };
        if request.is_some() {
            let _ = crate::shell3::service::request_shell3();
        } else {
            DESKTOP_SHELL_LAUNCH_SIGNAL.wait().await;
        }
    }
}

pub(super) fn claim_desktop_shell_launch(owner: WindowOwner) -> Option<DesktopShellLaunch> {
    let WindowOwner::Vm(vm_id) = owner else {
        return None;
    };
    let archive = crate::hv::blueprint_process_arg(vm_id, 0)?;
    if !archive
        .rsplit('/')
        .next()
        .unwrap_or(archive.as_str())
        .trim_end_matches(".bp")
        .eq_ignore_ascii_case("shell")
    {
        return None;
    }
    let identity = crate::hv::blueprint_instance_identity(vm_id)?;
    let token: u32 = identity.name?.strip_prefix("ui4_shell_")?.parse().ok()?;
    let mut intents = DESKTOP_SHELL_LAUNCH_INTENTS.lock();
    let index = intents.iter().position(|intent| intent.token == token)?;
    Some(intents.remove(index).launch)
}

pub(super) async fn wait_slot4_visual_change() {
    SLOT4_VISUAL_CHANGE.wait().await;
}

pub(super) fn notify_slot4_visual_change() {
    SLOT4_VISUAL_CHANGE.signal(());
}

pub(crate) fn take_owner_input_events(owner: WindowOwner) -> Vec<Ui4InputEvent, MAX_OWNER_EVENTS> {
    let mut out = Vec::new();
    // Deliver final resize state first. Ordinary transitions remain queued if
    // every output slot is needed for distinct window resizes and are drained
    // on the next owner poll without loss.
    {
        let mut resizes = OWNER_RESIZE_STATES.lock();
        for resize in resizes.iter_mut() {
            if out.is_full() {
                break;
            }
            if resize.owner == owner
                && let Some(event) = resize.pending.take()
            {
                let _ = out.push(Ui4InputEvent::Resize(event));
            }
        }
    }
    let mut queues = OWNER_QUEUES.lock();
    let Some(queue) = queues.iter_mut().find(|queue| queue.owner == owner) else {
        return out;
    };
    let mut events = Vec::new();
    core::mem::swap(&mut events, &mut queue.events);
    for event in events {
        if out.is_full() {
            let _ = queue.events.push(event);
        } else {
            let _ = out.push(event);
        }
    }
    out
}

pub(crate) fn focused_keyboard_state(
    owner: WindowOwner,
    window: WindowId,
) -> Option<crate::usb2::hid::hut::HidKeyboardState> {
    INPUT_BROKER
        .lock()
        .focused_keyboard_state(WindowTarget { owner, window })
}

pub(crate) fn window_input_routes(
    owner: WindowOwner,
    window: WindowId,
) -> Vec<Ui4WindowInputRoute, MAX_CURSOR_ROUTES> {
    INPUT_BROKER
        .lock()
        .window_input_routes(WindowTarget { owner, window })
}

/// Only a cursor with a live primary gesture in this frame may start a drag.
pub(super) fn drag_source(frame: super::CursorFrameKey) -> Option<Ui4CursorSource> {
    INPUT_BROKER
        .lock()
        .cursors
        .iter()
        .rev()
        .find(|route| {
            route.buttons_down & PRIMARY_BUTTON_MASK != 0
                && super::selected_frame_for_source(route.source) == Some(frame)
                && !route.absorb_select
        })
        .map(|route| route.source)
}

/// Drag-drop owns the rest of this primary gesture, even if it is cancelled.
pub(super) fn claim_drag_gesture(source: Ui4CursorSource) {
    if let Some(route) = INPUT_BROKER
        .lock()
        .cursors
        .iter_mut()
        .find(|route| route.source == source)
    {
        route.selection_anchor = None;
    }
}

 super::ContextMenuError> {
    super::context_menu::validate_request(&request)?;
    let (anchor, color) = {
        let mut broker = INPUT_BROKER.lock();
        let Some(route) = broker
            .cursors
            .iter_mut()
            .find(|route| route.source == source)
        else {
            return Err(super::ContextMenuError::NotFocused);
        };
        let selected = super::selected_frame_for_source(source);
        if selected != Some(super::CursorFrameKey::new(owner, window))
            || window_snapshot_for_target(WindowTarget { owner, window }).is_none()
        {
            return Err(super::ContextMenuError::NotFocused);
        }
        route.context_menu = None;
        route.context_menu_owns_gesture = false;
        route.context_menu_pressed_action = None;
        route.suppress_context_menu_open = false;
        ((route.x, route.y), route.color)
    };
    super::context_menu::open(source, owner, window, anchor, color, request)
}

pub(super) fn release_owner(owner: WindowOwner) -> (usize, usize) {
    let routes = INPUT_BROKER.lock().release_owner(owner);
    PHYSICAL_OWNER_QUEUES
        .lock()
        .retain(|queue| queue.owner != owner);
    let mut queued_events = {
        let mut queues = OWNER_QUEUES.lock();
        if let Some(index) = queues.iter().position(|queue| queue.owner == owner) {
            let queued_events = queues[index].events.len();
            queues.remove(index);
            queued_events
        } else {
            0
        }
    };
    let mut resize_events = OWNER_RESIZE_STATES.lock();
    let mut index = 0;
    while index < resize_events.len() {
        if resize_events[index].owner == owner {
            let removed = resize_events.remove(index);
            queued_events = queued_events.saturating_add(usize::from(removed.pending.is_some()));
        } else {
            index += 1;
        }
    }
    if routes != 0 || queued_events != 0 {
        SLOT4_VISUAL_CHANGE.signal(());
    }
    (routes, queued_events)
}

pub(crate) fn software_cursor_visuals() -> Vec<Ui4SoftwareCursorVisual, MAX_CURSOR_ROUTES> {
    INPUT_BROKER.lock().software_cursor_visuals()
}

pub(super) fn software_cursor_scale() -> u32 {
    if FAT_SOFTWARE_CURSORS.load(Ordering::Acquire) {
        3
    } else {
        1
    }
}

/// Select one ready UI4 frame for an existing cursor identity without
/// synthesizing any mouse button transition. Focus and selection-strip
/// ownership follow the same broker path as an absorb-select click, while no
/// Pointer or Button event is delivered to the application.
pub(crate) fn select_window_for_cursor(
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
) -> Result<bool, Ui4ProgrammaticSelectionError> {
    let snapshot = super::window_broker::window_snapshot(owner, window)
        .ok_or(Ui4ProgrammaticSelectionError::NotFound)?;
    if snapshot.state != WindowState::Ready || !snapshot.placement.visible {
        return Err(Ui4ProgrammaticSelectionError::NotReady);
    }
    let (screen_width, screen_height) =
        crate::ui4::output_dimensions().ok_or(Ui4ProgrammaticSelectionError::OutputUnavailable)?;
    if screen_width == 0 || screen_height == 0 {
        return Err(Ui4ProgrammaticSelectionError::OutputUnavailable);
    }
    let x =
        i64::from(snapshot.placement.x).clamp(0, i64::from(screen_width.saturating_sub(1))) as u32;
    let y =
        i64::from(snapshot.placement.y).clamp(0, i64::from(screen_height.saturating_sub(1))) as u32;
    let (combo_id, vcursor) = cursor_hut_metadata(source);
    let mut broker = INPUT_BROKER.lock();
    let index = broker.cursor_index(source, x, y);
    broker.cursors[index].x = x;
    broker.cursors[index].y = y;
    broker.cursors[index].visible_after_motion = true;
    Ok(broker.select_frame(index, Some(WindowTarget { owner, window }), combo_id, vcursor))
}

/// Transfer an existing cursor's selection to a replacement application frame
/// without moving the cursor. Preview applications use this after an input-
/// driven mode switch closes one broker session and opens the next.
pub(crate) fn reselect_window_for_cursor(
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
) -> Result<bool, Ui4ProgrammaticSelectionError> {
    let snapshot = super::window_broker::window_snapshot(owner, window)
        .ok_or(Ui4ProgrammaticSelectionError::NotFound)?;
    if snapshot.state != WindowState::Ready || !snapshot.placement.visible {
        return Err(Ui4ProgrammaticSelectionError::NotReady);
    }
    let (combo_id, vcursor) = cursor_hut_metadata(source);
    let mut broker = INPUT_BROKER.lock();
    let index = broker
        .cursors
        .iter()
        .position(|route| route.source == source)
        .ok_or(Ui4ProgrammaticSelectionError::NotFound)?;
    Ok(broker.select_frame(index, Some(WindowTarget { owner, window }), combo_id, vcursor))
}

/// Programmatically focus a ready frame while retaining a known cursor
/// position. Context-menu actions use their captured anchor so opening a
/// service frame never teleports the software cursor to the frame origin.
pub(super) fn select_window_for_cursor_at(
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
    x: u32,
    y: u32,
) -> Result<bool, Ui4ProgrammaticSelectionError> {
    let snapshot = super::window_broker::window_snapshot(owner, window)
        .ok_or(Ui4ProgrammaticSelectionError::NotFound)?;
    if snapshot.state != WindowState::Ready || !snapshot.placement.visible {
        return Err(Ui4ProgrammaticSelectionError::NotReady);
    }
    let (screen_width, screen_height) =
        crate::ui4::output_dimensions().ok_or(Ui4ProgrammaticSelectionError::OutputUnavailable)?;
    if screen_width == 0 || screen_height == 0 {
        return Err(Ui4ProgrammaticSelectionError::OutputUnavailable);
    }
    let x = x.min(screen_width - 1);
    let y = y.min(screen_height - 1);
    let (combo_id, vcursor) = cursor_hut_metadata(source);
    let mut broker = INPUT_BROKER.lock();
    let index = broker.cursor_index(source, x, y);
    broker.cursors[index].x = x;
    broker.cursors[index].y = y;
    broker.cursors[index].visible_after_motion = true;
    Ok(broker.select_frame(index, Some(WindowTarget { owner, window }), combo_id, vcursor))
}

pub(super) fn enqueue_window_resize(
    owner: WindowOwner,
    window: WindowId,
    resize_epoch: u64,
    old_width: u32,
    old_height: u32,
    width: u32,
    height: u32,
) {
    enqueue_owner_event(
        owner,
        Ui4InputEvent::Resize(Ui4ResizeEvent {
            window,
            resize_epoch,
            old_width,
            old_height,
            width,
            height,
        }),
    );
}

#[allow(clippy::too_many_arguments)]
fn enqueue_pan_event(
    target: WindowSnapshot,
    source: Ui4CursorSource,
    phase: Ui4PanPhase,
    x: u32,
    y: u32,
    local_x: i32,
    local_y: i32,
    dx: i32,
    dy: i32,
    combo_id: u32,
    vcursor: bool,
) {
    enqueue_owner_event(
        target.owner,
        Ui4InputEvent::Pan(Ui4PanEvent {
            source,
            window: target.id,
            phase,
            x,
            y,
            local_x,
            local_y,
            dx,
            dy,
            combo_id,
            vcursor,
        }),
    );
}

struct PhysicalOwnerQueue {
    owner: WindowOwner,
    events: Vec<Ui4KeyboardEvent, MAX_OWNER_EVENTS>,
    reset: bool,
}
static PHYSICAL_OWNER_QUEUES: Mutex<Vec<PhysicalOwnerQueue, MAX_OWNER_QUEUES>> =
    Mutex::new(Vec::new());

fn enqueue_physical_owner_event(owner: WindowOwner, event: Ui4KeyboardEvent) {
    if super::window_broker::collapse_state(owner, event.window) == Some(true) {
        return;
    }
    let mut queues = PHYSICAL_OWNER_QUEUES.lock();
    let index = if let Some(index) = queues.iter().position(|queue| queue.owner == owner) {
        index
    } else {
        if queues
            .push(PhysicalOwnerQueue {
                owner,
                events: Vec::new(),
                reset: false,
            })
            .is_err()
        {
            return;
        }
        queues.len() - 1
    };
    let queue = &mut queues[index];
    if queue.events.is_full() {
        queue.events.clear();
        queue.reset = true;
    }
    let _ = queue.events.push(event);
}

pub(super) fn take_physical_owner_events(
    owner: WindowOwner,
) -> (bool, Vec<Ui4KeyboardEvent, MAX_OWNER_EVENTS>) {
    let mut queues = PHYSICAL_OWNER_QUEUES.lock();
    let Some(queue) = queues.iter_mut().find(|queue| queue.owner == owner) else {
        return (false, Vec::new());
    };
    (core::mem::take(&mut queue.reset), core::mem::take(&mut queue.events))
}

fn enqueue_owner_event(owner: WindowOwner, event: Ui4InputEvent) {
    if let Ui4InputEvent::Keyboard(key) = event
        && super::window_broker::collapse_state(owner, key.window) == Some(true)
    {
        return;
    }
    if let Ui4InputEvent::Resize(incoming) = event {
        enqueue_owner_resize(owner, incoming);
        return;
    }
    let mut queues = OWNER_QUEUES.lock();
    let queue_index = if let Some(index) = queues.iter().position(|queue| queue.owner == owner) {
        index
    } else if queues
        .push(OwnerQueue {
            owner,
            events: Vec::new(),
        })
        .is_ok()
    {
        queues.len() - 1
    } else {
        note_owner_queue_drop(owner, event, "owner-capacity");
        return;
    };
    let queue = &mut queues[queue_index].events;
    if let Some(queued) = queue.last_mut() {
        if coalesce_owner_state_sample(queued, event) {
            return;
        }
    }
    if queue.push(event).is_ok() {
        return;
    }

    // UI4 keyboard events are ordered transitions. Never silently evict an
    // earlier transition to admit a newer event. Under pressure, discard one
    // replaceable pointer/pan sample first; consumers still receive the newest
    // absolute coordinates and accumulated deltas.  Wheel samples belong to
    // this class when no button changed: consumers such as Shell2's scalable
    // text frontend need the complete signed wheel distance, not every raw
    // HID report as a separately queued event.
    if !owner_event_is_state_sample(&event)
        && let Some(index) = queue.iter().position(owner_event_is_state_sample)
    {
        let evicted = queue.remove(index);
        note_owner_queue_drop(owner, evicted, "evict-state-for-transition");
        if queue.push(event).is_ok() {
            return;
        }
    }
    note_owner_queue_drop(owner, event, "queue-full-reject-newest");
}

fn enqueue_owner_resize(owner: WindowOwner, incoming: Ui4ResizeEvent) {
    let mut resize_events = OWNER_RESIZE_STATES.lock();
    if let Some(index) = resize_events
        .iter()
        .position(|current| current.owner == owner && current.window == incoming.window)
    {
        let latest_epoch = resize_events[index].latest_epoch;
        // Geometry mutations release the window-broker lock before entering
        // this queue. Preserve broker order even if two callers enqueue in
        // the opposite scheduler order after those serialized mutations. The
        // watermark survives delivery, so a delayed older caller cannot enter
        // an empty pending slot after the final event was already consumed.
        if resize_epoch_is_newer(incoming.resize_epoch, latest_epoch) {
            resize_events[index].latest_epoch = incoming.resize_epoch;
            resize_events[index].pending = Some(incoming);
        } else if incoming.resize_epoch == latest_epoch && resize_events[index].pending.is_some() {
            resize_events[index].pending = Some(incoming);
        }
        return;
    }
    if resize_events
        .push(OwnerResizeState {
            owner,
            window: incoming.window,
            latest_epoch: incoming.resize_epoch,
            pending: Some(incoming),
        })
        .is_err()
    {
        // Capacity equals the complete live broker registry. Generation churn
        // can leave closed-window notifications behind, so prove staleness and
        // purge those entries rather than ever evicting a live final target.
        let mut index = 0;
        while index < resize_events.len() {
            let queued = resize_events[index];
            let live = super::window_broker::window_snapshot(queued.owner, queued.window)
                .is_some_and(|snapshot| {
                    matches!(snapshot.state, WindowState::Pending | WindowState::Ready)
                });
            if !live {
                resize_events.remove(index);
            } else {
                index += 1;
            }
        }
        if resize_events
            .push(OwnerResizeState {
                owner,
                window: incoming.window,
                latest_epoch: incoming.resize_epoch,
                pending: Some(incoming),
            })
            .is_err()
        {
            note_owner_queue_drop(
                owner,
                Ui4InputEvent::Resize(incoming),
                "resize-capacity-live-invariant",
            );
        }
    }
}

const fn resize_epoch_is_newer(candidate: u64, current: u64) -> bool {
    candidate != current && candidate.wrapping_sub(current) < (1_u64 << 63)
}

fn owner_event_is_state_sample(event: &Ui4InputEvent) -> bool {
    match event {
        Ui4InputEvent::Pointer(pointer) => {
            pointer.buttons_pressed == 0 && pointer.buttons_released == 0
        }
        Ui4InputEvent::Pan(pan) => pan.phase == Ui4PanPhase::Update,
        _ => false,
    }
}

fn owner_event_kind(event: Ui4InputEvent) -> &'static str {
    match event {
        Ui4InputEvent::Pointer(_) => "pointer",
        Ui4InputEvent::Button(_) => "button",
        Ui4InputEvent::Pan(_) => "pan",
        Ui4InputEvent::Resize(_) => "resize",
        Ui4InputEvent::Keyboard(_) => "keyboard",
        
    }
}

fn coalesce_owner_state_sample(queued: &mut Ui4InputEvent, incoming: Ui4InputEvent) -> bool {
    match (queued, incoming) {
        (Ui4InputEvent::Pointer(previous), Ui4InputEvent::Pointer(mut next))
            if owner_event_is_state_sample(&Ui4InputEvent::Pointer(*previous))
                && owner_event_is_state_sample(&Ui4InputEvent::Pointer(next))
                && previous.source == next.source
                && previous.window == next.window
                && previous.buttons_down == next.buttons_down =>
        {
            next.dx = previous.dx.saturating_add(next.dx);
            next.dy = previous.dy.saturating_add(next.dy);
            // Preserve the total detent distance under input pressure.  This
            // keeps wheel-driven continuous scale deterministic while still
            // bounding an owner queue to its most recent pointer state.
            next.wheel = previous.wheel.saturating_add(next.wheel);
            *previous = next;
            true
        }
        (Ui4InputEvent::Pan(previous), Ui4InputEvent::Pan(mut next))
            if previous.phase == Ui4PanPhase::Update
                && next.phase == Ui4PanPhase::Update
                && previous.source == next.source
                && previous.window == next.window =>
        {
            next.dx = previous.dx.saturating_add(next.dx);
            next.dy = previous.dy.saturating_add(next.dy);
            *previous = next;
            true
        }
        _ => false,
    }
}

fn note_owner_queue_drop(owner: WindowOwner, event: Ui4InputEvent, reason: &'static str) {
    let drops = OWNER_QUEUE_DROPS.fetch_add(1, Ordering::Relaxed) + 1;
    if drops <= 8 || drops.is_power_of_two() {
        let keyboard_exact_once_violation = matches!(event, Ui4InputEvent::Keyboard(_));
        crate::log_warn!(target: "ui4";
            "ui4/input: owner-queue loss count={} owner={:?} event={} reason={} keyboard_exact_once_violation={}\n",
            drops,
            owner,
            owner_event_kind(event),
            reason,
            keyboard_exact_once_violation,
        );
    }
}

fn normalized_to_pixel(value: f64, extent: u32) -> u32 {
    if extent == 0 {
        return 0;
    }
    let last_pixel = extent - 1;
    (value.clamp(0.0, 1.0) * f64::from(last_pixel)) as u32
}

fn signed_delta(next: u32, previous: u32) -> i32 {
    i64::from(next)
        .saturating_sub(i64::from(previous))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn signed_local(pixel: u32, origin: i32) -> i32 {
    i64::from(pixel)
        .saturating_sub(i64::from(origin))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// Preserve a physical mouse's relative HID report while UI4 presents and
/// routes its cursor at the selected frame's center. Reconstructing motion
/// from the pinned coordinates would otherwise produce zero after the first
/// snapped event. Absolute pointer kinds continue to use coordinate deltas.
fn pointer_motion_delta(
    center_snapped: bool,
    relative_report: bool,
    relative_dx_bits: u16,
    relative_dy_bits: u16,
    x: u32,
    y: u32,
    previous_x: u32,
    previous_y: u32,
) -> (i32, i32) {
    if center_snapped && relative_report {
        return (i32::from(relative_dx_bits as i16), i32::from(relative_dy_bits as i16));
    }
    (signed_delta(x, previous_x), signed_delta(y, previous_y))
}

#[cfg(test)]
mod pointer_motion_delta_tests {
    use super::pointer_motion_delta;

    #[test]
    fn snapped_mouse_uses_relative_report_while_coordinates_stay_centered() {
        assert_eq!(
            pointer_motion_delta(true, true, 12, (-7i16) as u16, 960, 540, 960, 540),
            (12, -7),
        );
    }

    #[test]
    fn ordinary_pointer_uses_absolute_coordinate_delta() {
        assert_eq!(pointer_motion_delta(false, true, 99, 88, 113, 71, 100, 80), (13, -9),);
    }

    #[test]
    fn snapped_source_without_relative_payload_keeps_absolute_fallback() {
        assert_eq!(pointer_motion_delta(true, false, 0, 0, 210, 190, 200, 200), (10, -10),);
    }
}

/// Resolve only the slot-4 presentation. Raw cursor state, hit testing, and
/// every application-delivered pointer coordinate stay untouched.
fn cursor_visual_presentation(
    route: &CursorRoute,
) -> (u32, u32, super::Ui4CursorIcon, Option<Arc<super::Ui4CursorImage>>, Option<Ui4VisualRect>) {
    if let Some(key) = super::center_snapped_frame_for_source(route.source)
        && let Some(window) = super::window_broker::window_snapshot(key.owner, key.window)
    {
        let placement = window.presentation_placement;
        let x = (i64::from(placement.x) + i64::from(placement.width) / 2)
            .clamp(0, i64::from(u32::MAX)) as u32;
        let y = (i64::from(placement.y) + i64::from(placement.height) / 2)
            .clamp(0, i64::from(u32::MAX)) as u32;
        let icon = super::cursor_presentation_for_source(route.source)
            .map(|(_, icon, _)| icon)
            .unwrap_or_default();
        return (x, y, icon, super::cursor_image_for_source(route.source), None);
    }
    let Some((key, icon, step)) = super::cursor_presentation_for_source(route.source) else {
        return (route.x, route.y, super::Ui4CursorIcon::Default, None, None);
    };
    let custom_cursor = super::cursor_image_for_source(route.source);
    if icon != super::Ui4CursorIcon::CellOutline {
        return (route.x, route.y, icon, custom_cursor, None);
    }
    let Some(step) = step else {
        return (route.x, route.y, icon, custom_cursor, None);
    };
    let Some(window) = super::window_broker::window_snapshot(key.owner, key.window) else {
        return (route.x, route.y, icon, custom_cursor, None);
    };
    let placement = window.presentation_placement;
    if !placement_contains(placement, window.arc, route.x, route.y) {
        return (route.x, route.y, icon, custom_cursor, None);
    }
    let (Ok(local_x), Ok(local_y)) = (
        u32::try_from(signed_local(route.x, placement.x)),
        u32::try_from(signed_local(route.y, placement.y)),
    ) else {
        return (route.x, route.y, icon, custom_cursor, None);
    };
    let Some((left, top, width, height)) = step.cell_bounds_local(local_x, local_y) else {
        return (route.x, route.y, icon, custom_cursor, None);
    };
    let Some(cell) = stepped_cell_rect(placement, left, top, width, height) else {
        return (route.x, route.y, icon, custom_cursor, None);
    };
    (cell.x, cell.y, icon, custom_cursor, Some(cell))
}

fn stepped_cell_rect(
    placement: WindowPlacement,
    local_x: u32,
    local_y: u32,
    width: u32,
    height: u32,
) -> Option<Ui4VisualRect> {
    let left = i64::from(placement.x).saturating_add(i64::from(local_x));
    let top = i64::from(placement.y).saturating_add(i64::from(local_y));
    let right = left.saturating_add(i64::from(width));
    let bottom = top.saturating_add(i64::from(height));
    let max = i64::from(u32::MAX);
    let clipped_left = left.clamp(0, max);
    let clipped_top = top.clamp(0, max);
    let clipped_right = right.clamp(0, max);
    let clipped_bottom = bottom.clamp(0, max);
    (clipped_right > clipped_left && clipped_bottom > clipped_top).then_some(Ui4VisualRect {
        x: clipped_left as u32,
        y: clipped_top as u32,
        width: clipped_right.saturating_sub(clipped_left) as u32,
        height: clipped_bottom.saturating_sub(clipped_top) as u32,
    })
}

/// Motion and primary input are permitted; other clicks and scrolling discard the latch.
fn resize_latch_cancelled(armed_ms: u64, now_ms: u64, pressed: u32, wheel: i16) -> bool {
    now_ms.saturating_sub(armed_ms) >= 10_000 || pressed & !PRIMARY_BUTTON_MASK != 0 || wheel != 0
}

fn selection_rect_between(anchor: (u32, u32), point: (u32, u32)) -> Ui4VisualRect {
    Ui4VisualRect {
        x: anchor.0.min(point.0),
        y: anchor.1.min(point.1),
        width: anchor.0.abs_diff(point.0).saturating_add(1),
        height: anchor.1.abs_diff(point.1).saturating_add(1),
    }
}

pub(crate) fn context_menu_rect(
    anchor: (u32, u32),
    screen_width: u32,
    screen_height: u32,
) -> Ui4VisualRect {
    let menu_rows = DESKTOP_CONTEXT_MENU_ENTRY_COUNT.max(1);
    let menu_height = CONTEXT_MENU_BORDER_PX
        .saturating_mul(2)
        .saturating_add(DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX.saturating_mul(2))
        .saturating_add(CONTEXT_MENU_ROW_HEIGHT_PX.saturating_mul(menu_rows))
        .min(screen_height);
    Ui4VisualRect {
        x: anchor
            .0
            .saturating_add(CONTEXT_MENU_OFFSET_PX)
            .min(screen_width.saturating_sub(CONTEXT_MENU_WIDTH_PX)),
        y: anchor
            .1
            .saturating_add(CONTEXT_MENU_OFFSET_PX)
            .min(screen_height.saturating_sub(menu_height)),
        width: CONTEXT_MENU_WIDTH_PX.min(screen_width),
        height: menu_height,
    }
}

fn desktop_context_menu_action_at(
    menu: Ui4VisualRect,
    x: u32,
    y: u32,
) -> Option<DesktopContextMenuAction> {
    if !visual_rect_contains(menu, x, y) {
        return None;
    }
    match y
        .saturating_sub(menu.y)
        .saturating_sub(DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX)
        / CONTEXT_MENU_ROW_HEIGHT_PX
    {
        0 => Some(DesktopContextMenuAction::ColorPicker),
        1 => Some(DesktopContextMenuAction::Shell),
        2 => Some(DesktopContextMenuAction::Link),
        _ => None,
    }
}

fn visual_rect_contains(rect: Ui4VisualRect, x: u32, y: u32) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

fn point_travel_reached(origin: (u32, u32), point: (u32, u32), threshold: u32) -> bool {
    let dx = u64::from(origin.0.abs_diff(point.0));
    let dy = u64::from(origin.1.abs_diff(point.1));
    let threshold = u64::from(threshold);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
        >= threshold.saturating_mul(threshold)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct DockZoneMetrics {
    corner_width: u32,
    corner_height: u32,
    side_depth: u32,
    side_span: u32,
    top_span: u32,
    top_depth: u32,
}

/// Resolve physically stable edge targets from EDID. The proportional fallback
/// follows the supplied Full-HD layout when monitor size is unavailable.
fn dock_zone_metrics(
    screen_width: u32,
    screen_height: u32,
    physical_reference: Option<(u32, u32)>,
) -> DockZoneMetrics {
    let (corner_width, corner_height, side_depth, side_span, top_span, top_depth) =
        if let Some((reference_width, reference_height)) = physical_reference {
            (
                scale_reference(reference_width, DOCK_CORNER_MM, DOCK_REFERENCE_WIDTH_MM),
                scale_reference(reference_height, DOCK_CORNER_MM, DOCK_REFERENCE_HEIGHT_MM),
                scale_reference(reference_width, DOCK_EDGE_DEPTH_MM, DOCK_REFERENCE_WIDTH_MM),
                reference_height,
                reference_width,
                scale_reference(reference_height, DOCK_EDGE_DEPTH_MM, DOCK_REFERENCE_HEIGHT_MM),
            )
        } else {
            (
                screen_width / 20,
                screen_height / 11,
                screen_width / 40,
                screen_height / 7,
                screen_width / 8,
                screen_height / 24,
            )
        };
    DockZoneMetrics {
        corner_width: clamp_zone_metric(corner_width, screen_width / 4, screen_width),
        corner_height: clamp_zone_metric(corner_height, screen_height / 4, screen_height),
        side_depth: clamp_zone_metric(side_depth, screen_width / 4, screen_width),
        side_span: clamp_zone_metric(side_span, screen_height / 2, screen_height),
        top_span: clamp_zone_metric(top_span, screen_width / 2, screen_width),
        top_depth: clamp_zone_metric(top_depth, screen_height / 4, screen_height),
    }
}

fn scale_reference(pixels: u32, millimeters: u32, reference_millimeters: u32) -> u32 {
    ((u64::from(pixels)
        .saturating_mul(u64::from(millimeters))
        .saturating_add(u64::from(reference_millimeters / 2)))
        / u64::from(reference_millimeters.max(1)))
    .min(u64::from(u32::MAX)) as u32
}

fn clamp_zone_metric(value: u32, limit: u32, screen_extent: u32) -> u32 {
    if screen_extent == 0 {
        return 0;
    }
    value.max(1).min(limit.max(1)).min(screen_extent)
}

pub(super) fn dock_zones(screen_width: u32, screen_height: u32) -> [Ui4DockZone; 8] {
    let physical_reference =
        crate::intel::physical_extent_pixels(DOCK_REFERENCE_WIDTH_MM, DOCK_REFERENCE_HEIGHT_MM);
    dock_zones_with_reference(screen_width, screen_height, physical_reference)
}

fn dock_zones_with_reference(
    screen_width: u32,
    screen_height: u32,
    physical_reference: Option<(u32, u32)>,
) -> [Ui4DockZone; 8] {
    use super::WindowDockTarget;

    let metrics = dock_zone_metrics(screen_width, screen_height, physical_reference);
    let right_x = screen_width.saturating_sub(metrics.corner_width);
    let bottom_y = screen_height.saturating_sub(metrics.corner_height);
    let side_y = screen_height.saturating_sub(metrics.side_span) / 2;
    let top_x = screen_width.saturating_sub(metrics.top_span) / 2;
    [
        Ui4DockZone {
            target: WindowDockTarget::TopLeft,
            rect: Ui4VisualRect {
                x: 0,
                y: 0,
                width: metrics.corner_width,
                height: metrics.corner_height,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::TopRight,
            rect: Ui4VisualRect {
                x: right_x,
                y: 0,
                width: metrics.corner_width,
                height: metrics.corner_height,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::BottomLeft,
            rect: Ui4VisualRect {
                x: 0,
                y: bottom_y,
                width: metrics.corner_width,
                height: metrics.corner_height,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::BottomRight,
            rect: Ui4VisualRect {
                x: right_x,
                y: bottom_y,
                width: metrics.corner_width,
                height: metrics.corner_height,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::LeftHalf,
            rect: Ui4VisualRect {
                x: 0,
                y: side_y,
                width: metrics.side_depth,
                height: metrics.side_span,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::RightHalf,
            rect: Ui4VisualRect {
                x: screen_width.saturating_sub(metrics.side_depth),
                y: side_y,
                width: metrics.side_depth,
                height: metrics.side_span,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::Maximize,
            rect: Ui4VisualRect {
                x: top_x,
                y: 0,
                width: metrics.top_span,
                height: metrics.top_depth,
            },
        },
        Ui4DockZone {
            target: WindowDockTarget::GenericResize,
            rect: Ui4VisualRect {
                x: top_x,
                y: screen_height.saturating_sub(metrics.top_depth),
                width: metrics.top_span,
                height: metrics.top_depth,
            },
        },
    ]
}

fn dock_target_at(
    x: u32,
    y: u32,
    screen_width: u32,
    screen_height: u32,
) -> Option<super::WindowDockTarget> {
    dock_target_at_in_zones(x, y, &dock_zones(screen_width, screen_height))
}

fn dock_target_at_in_zones(
    x: u32,
    y: u32,
    zones: &[Ui4DockZone],
) -> Option<super::WindowDockTarget> {
    zones
        .iter()
        .find(|zone| dock_zone_contains(**zone, x, y))
        .map(|zone| zone.target)
}

pub(super) fn dock_zone_contains(zone: Ui4DockZone, x: u32, y: u32) -> bool {
    if !visual_rect_contains(zone.rect, x, y) {
        return false;
    }
    dock_zone_local_contains(
        zone.target,
        zone.rect.width,
        zone.rect.height,
        x.saturating_sub(zone.rect.x),
        y.saturating_sub(zone.rect.y),
    )
}

/// Return the exact horizontal coverage of one row of a dock field. Slot 4
/// builds its translucent mask from the same predicate used by hit-testing so
/// the visible curved field and the active pixels cannot drift apart.
pub(super) fn dock_zone_row_span(zone: Ui4DockZone, row: u32) -> Option<Ui4VisualRect> {
    if row >= zone.rect.height {
        return None;
    }

    use super::WindowDockTarget;

    let contains = |column| {
        dock_zone_local_contains(zone.target, zone.rect.width, zone.rect.height, column, row)
    };
    let (first, last) = match zone.target {
        WindowDockTarget::TopLeft | WindowDockTarget::BottomLeft | WindowDockTarget::LeftHalf => {
            let last = last_true_prefix(zone.rect.width, contains)?;
            (0, last)
        }
        WindowDockTarget::TopRight
        | WindowDockTarget::BottomRight
        | WindowDockTarget::RightHalf => {
            let first = first_true_suffix(zone.rect.width, contains)?;
            (first, zone.rect.width.saturating_sub(1))
        }
        WindowDockTarget::Maximize | WindowDockTarget::GenericResize => {
            let left_half = zone.rect.width / 2 + zone.rect.width % 2;
            let first = first_true_suffix(left_half, contains)?;
            (first, zone.rect.width.saturating_sub(1).saturating_sub(first))
        }
    };
    Some(Ui4VisualRect {
        x: zone.rect.x.saturating_add(first),
        y: zone.rect.y.saturating_add(row),
        width: last.saturating_sub(first).saturating_add(1),
        height: 1,
    })
}

/// Column equivalent of [`dock_zone_row_span`]. Tall side fields are emitted
/// column-wise so Slot 4's rectangle count follows the shorter dimension
/// instead of their much longer vertical diameter.
pub(super) fn dock_zone_column_span(zone: Ui4DockZone, column: u32) -> Option<Ui4VisualRect> {
    if column >= zone.rect.width {
        return None;
    }

    use super::WindowDockTarget;

    let contains =
        |row| dock_zone_local_contains(zone.target, zone.rect.width, zone.rect.height, column, row);
    let (first, last) = match zone.target {
        WindowDockTarget::TopLeft | WindowDockTarget::TopRight | WindowDockTarget::Maximize => {
            let last = last_true_prefix(zone.rect.height, contains)?;
            (0, last)
        }
        WindowDockTarget::BottomLeft
        | WindowDockTarget::BottomRight
        | WindowDockTarget::GenericResize => {
            let first = first_true_suffix(zone.rect.height, contains)?;
            (first, zone.rect.height.saturating_sub(1))
        }
        WindowDockTarget::LeftHalf | WindowDockTarget::RightHalf => {
            let top_half = zone.rect.height / 2 + zone.rect.height % 2;
            let first = first_true_suffix(top_half, contains)?;
            (first, zone.rect.height.saturating_sub(1).saturating_sub(first))
        }
    };
    Some(Ui4VisualRect {
        x: zone.rect.x.saturating_add(column),
        y: zone.rect.y.saturating_add(first),
        width: 1,
        height: last.saturating_sub(first).saturating_add(1),
    })
}

fn last_true_prefix(extent: u32, mut predicate: impl FnMut(u32) -> bool) -> Option<u32> {
    let mut low = 0;
    let mut high = extent;
    while low < high {
        let middle = low.saturating_add(high.saturating_sub(low) / 2);
        if predicate(middle) {
            low = middle.saturating_add(1);
        } else {
            high = middle;
        }
    }
    low.checked_sub(1)
}

fn first_true_suffix(extent: u32, mut predicate: impl FnMut(u32) -> bool) -> Option<u32> {
    let mut low = 0;
    let mut high = extent;
    while low < high {
        let middle = low.saturating_add(high.saturating_sub(low) / 2);
        if predicate(middle) {
            high = middle;
        } else {
            low = middle.saturating_add(1);
        }
    }
    (low < extent).then_some(low)
}

fn dock_zone_local_contains(
    target: super::WindowDockTarget,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
) -> bool {
    use super::WindowDockTarget;

    if width == 0 || height == 0 || x >= width || y >= height {
        return false;
    }

    let width = u64::from(width);
    let height = u64::from(height);
    let x = u64::from(x);
    let y = u64::from(y);
    match target {
        WindowDockTarget::TopLeft
        | WindowDockTarget::TopRight
        | WindowDockTarget::BottomLeft
        | WindowDockTarget::BottomRight => {
            let corner_x =
                if matches!(target, WindowDockTarget::TopRight | WindowDockTarget::BottomRight) {
                    width.saturating_sub(1).saturating_sub(x)
                } else {
                    x
                };
            let corner_y =
                if matches!(target, WindowDockTarget::BottomLeft | WindowDockTarget::BottomRight) {
                    height.saturating_sub(1).saturating_sub(y)
                } else {
                    y
                };
            normalized_ellipse_contains(
                corner_x.saturating_mul(2).saturating_add(1),
                width.saturating_mul(2),
                corner_y.saturating_mul(2).saturating_add(1),
                height.saturating_mul(2),
            )
        }
        WindowDockTarget::LeftHalf | WindowDockTarget::RightHalf => {
            let inward_x = if target == WindowDockTarget::RightHalf {
                width.saturating_sub(1).saturating_sub(x)
            } else {
                x
            };
            normalized_ellipse_contains(
                inward_x.saturating_mul(2).saturating_add(1),
                width.saturating_mul(2),
                y.saturating_mul(2).saturating_add(1).abs_diff(height),
                height,
            )
        }
        WindowDockTarget::GenericResize => normalized_ellipse_contains(
            x.saturating_mul(2).saturating_add(1).abs_diff(width),
            width,
            height
                .saturating_sub(1)
                .saturating_sub(y)
                .saturating_mul(2)
                .saturating_add(1),
            height.saturating_mul(2),
        ),
        WindowDockTarget::Maximize => normalized_ellipse_contains(
            x.saturating_mul(2).saturating_add(1).abs_diff(width),
            width,
            y.saturating_mul(2).saturating_add(1),
            height.saturating_mul(2),
        ),
    }
}

/// Pixel centers are expressed in doubled coordinates, then compared against
/// the ellipse equation using integer cross-products. This keeps the mask
/// deterministic at every DPI and avoids a floating-point boundary mismatch.
fn normalized_ellipse_contains(nx: u64, dx: u64, ny: u64, dy: u64) -> bool {
    if dx == 0 || dy == 0 {
        return false;
    }

    let nx = u128::from(nx);
    let dx = u128::from(dx);
    let ny = u128::from(ny);
    let dy = u128::from(dy);
    let nx_squared = nx.saturating_mul(nx);
    let dx_squared = dx.saturating_mul(dx);
    let ny_squared = ny.saturating_mul(ny);
    let dy_squared = dy.saturating_mul(dy);
    nx_squared
        .saturating_mul(dy_squared)
        .saturating_add(ny_squared.saturating_mul(dx_squared))
        <= dx_squared.saturating_mul(dy_squared)
}

const fn dock_target_label(target: super::WindowDockTarget) -> &'static str {
    match target {
        super::WindowDockTarget::GenericResize => "generic-resize",
        super::WindowDockTarget::Maximize => "maximize",
        super::WindowDockTarget::LeftHalf => "left-half",
        super::WindowDockTarget::RightHalf => "right-half",
        super::WindowDockTarget::TopLeft => "top-left",
        super::WindowDockTarget::TopRight => "top-right",
        super::WindowDockTarget::BottomLeft => "bottom-left",
        super::WindowDockTarget::BottomRight => "bottom-right",
    }
}

fn translated_frame_placement(
    mut placement: WindowPlacement,
    dx: i32,
    dy: i32,
    screen_width: u32,
    screen_height: u32,
) -> Option<WindowPlacement> {
    let max_x = i64::from(screen_width.saturating_sub(placement.width));
    let max_y = i64::from(screen_height.saturating_sub(placement.height));
    let next_x = i64::from(placement.x)
        .saturating_add(i64::from(dx))
        .clamp(0, max_x) as i32;
    let next_y = i64::from(placement.y)
        .saturating_add(i64::from(dy))
        .clamp(0, max_y) as i32;
    if next_x == placement.x && next_y == placement.y {
        return None;
    }
    placement.x = next_x;
    placement.y = next_y;
    Some(placement)
}

fn placement_contains(placement: WindowPlacement, arc: u16, x: u32, y: u32) -> bool {
    if !placement.visible {
        return false;
    }
    let x = i64::from(x);
    let y = i64::from(y);
    let left = i64::from(placement.x);
    let top = i64::from(placement.y);
    let in_bounds = x >= left
        && y >= top
        && x < left.saturating_add(i64::from(placement.width))
        && y < top.saturating_add(i64::from(placement.height));
    in_bounds
        && super::window_arc::contains(
            placement.width,
            placement.height,
            arc,
            (x - left) as u32,
            (y - top) as u32,
        )
}

fn topmost_window_at(x: u32, y: u32) -> Option<WindowSnapshot> {
    let output = OutputId::from_slot(0)?;
    super::visible_windows_for_output(output)
        .into_iter()
        .filter(|window| window.state == WindowState::Ready)
        .filter(|window| window.interaction.hit_testable)
        .filter(|window| placement_contains(window.presentation_placement, window.arc, x, y))
        // Plane slot is the hardware pipe-local stacking boundary. Only z
        // order windows against peers in the same slot; a later slot remains
        // above an earlier slot regardless of its local z value.
        .max_by_key(|window| (window.plane.slot(), window.placement.z, window.id))
}

fn window_snapshot_for_target(target: WindowTarget) -> Option<WindowSnapshot> {
    for output_slot in 0..super::OUTPUT_COUNT {
        let Some(output) = OutputId::from_slot(output_slot) else {
            continue;
        };
        if let Some(window) = super::visible_windows_for_output(output)
            .into_iter()
            .find(|window| {
                window.state == WindowState::Ready
                    && window.id == target.window
                    && window.owner == target.owner
            })
        {
            return Some(window);
        }
    }
    None
}

fn cursor_hut_metadata(source: Ui4CursorSource) -> (u32, bool) {
    let combo_id = crate::usb2::hid::hut::combos_snapshot()
        .into_iter()
        .find(|combo| {
            if source.hid_kind == crate::r::cursor::HID_KIND_TABLET {
                combo.tablet_controller_id == source.controller_id
                    && combo.tablet_slot_id == source.slot_id
                    && combo.tablet_ep_target == source.ep_target
            } else {
                combo.mouse_controller_id == source.controller_id
                    && combo.mouse_slot_id == source.slot_id
                    && combo.mouse_ep_target == source.ep_target
            }
        })
        .map(|combo| combo.combo_id)
        .unwrap_or(0);
    if source.hid_kind == crate::r::cursor::HID_KIND_TABLET {
        if let Some(tablet) = crate::usb2::hid::hut::tablets_snapshot()
            .into_iter()
            .find(|tablet| {
                tablet.controller_id == source.controller_id
                    && tablet.slot_id == source.slot_id
                    && tablet.ep_target == source.ep_target
            })
        {
            return (
                if combo_id != 0 {
                    combo_id
                } else {
                    tablet.combo_id
                },
                tablet.virtual_device,
            );
        }
    } else if let Some(mouse) = crate::usb2::hid::hut::mice_snapshot()
        .into_iter()
        .find(|mouse| {
            mouse.controller_id == source.controller_id
                && mouse.slot_id == source.slot_id
                && mouse.ep_target == source.ep_target
        })
    {
        return (
            if combo_id != 0 {
                combo_id
            } else {
                mouse.combo_id
            },
            mouse.virtual_device || source.hid_kind == crate::r::cursor::HID_KIND_VIRTUAL_CURSOR,
        );
    }
    (combo_id, source.hid_kind == crate::r::cursor::HID_KIND_VIRTUAL_CURSOR)
}

fn keyboard_hut_metadata(event: &crate::r::keyboard::TrueosKeyboardOutputEvent) -> (u32, bool) {
    let combo_id = crate::usb2::hid::hut::combos_snapshot()
        .into_iter()
        .find(|combo| {
            combo.keyboard_controller_id == event.controller_id
                && combo.keyboard_slot_id == event.slot_id
                && combo.keyboard_ep_target == event.ep_target
        })
        .map(|combo| combo.combo_id)
        .unwrap_or(0);
    crate::usb2::hid::hut::keyboards_snapshot()
        .into_iter()
        .find(|keyboard| {
            keyboard.controller_id == event.controller_id
                && keyboard.slot_id == event.slot_id
                && keyboard.ep_target == event.ep_target
        })
        .map(|keyboard| {
            (
                if combo_id != 0 {
                    combo_id
                } else {
                    keyboard.combo_id
                },
                keyboard.virtual_device,
            )
        })
        .unwrap_or((combo_id, event.controller_id == 0))
}

#[cfg(test)]
mod tests {
    use super::{
        DesktopContextMenuAction, Ui4CursorSource, Ui4InputEvent, Ui4PointerEvent, WindowId,
        coalesce_owner_state_sample, desktop_context_menu_action_at, dock_target_at_in_zones,
        dock_zone_column_span, dock_zone_contains, dock_zone_metrics, dock_zone_row_span,
        dock_zones_with_reference, owner_event_is_state_sample, resize_epoch_is_newer,
    };
    use crate::ui4::WindowDockTarget;

    fn pointer(wheel: i16) -> Ui4InputEvent {
        Ui4InputEvent::Pointer(Ui4PointerEvent {
            source: Ui4CursorSource {
                controller_id: 1,
                slot_id: 2,
                ep_target: 3,
                hid_kind: 4,
            },
            window: WindowId::from_raw(7).expect("non-zero test window"),
            x: 40,
            y: 50,
            local_x: 4,
            local_y: 5,
            dx: 2,
            dy: -3,
            wheel,
            buttons_down: 0,
            buttons_pressed: 0,
            buttons_released: 0,
            combo_id: 0,
            vcursor: false,
        })
    }

    #[test]
    fn wheel_samples_are_coalesced_without_losing_signed_distance() {
        let mut queued = pointer(3);
        assert!(owner_event_is_state_sample(&queued));
        assert!(coalesce_owner_state_sample(&mut queued, pointer(-1)));
        let Ui4InputEvent::Pointer(event) = queued else {
            panic!("pointer was replaced with another event kind");
        };
        assert_eq!(event.wheel, 2);
        assert_eq!((event.dx, event.dy), (4, -6));
    }

    #[test]
    fn wheel_accumulation_saturates_at_the_abi_limit() {
        let mut queued = pointer(i16::MAX);
        assert!(coalesce_owner_state_sample(&mut queued, pointer(1)));
        let Ui4InputEvent::Pointer(event) = queued else {
            panic!("pointer was replaced with another event kind");
        };
        assert_eq!(event.wheel, i16::MAX);
    }

    #[test]
    fn desktop_context_menu_exposes_link_as_the_third_entry() {
        let menu = super::Ui4VisualRect {
            x: 0,
            y: 0,
            width: 256,
            height: 256,
        };
        let y = super::DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX
            + super::CONTEXT_MENU_ROW_HEIGHT_PX * 2
            + 1;
        assert_eq!(
            desktop_context_menu_action_at(menu, 12, y),
            Some(DesktopContextMenuAction::Link)
        );
    }

    #[test]
    fn resize_epoch_order_survives_delivery_and_wrap() {
        assert!(resize_epoch_is_newer(3, 2));
        assert!(!resize_epoch_is_newer(2, 3));
        assert!(!resize_epoch_is_newer(3, 3));
        assert!(resize_epoch_is_newer(1, u64::MAX));
        assert!(!resize_epoch_is_newer(u64::MAX, 1));
    }

    #[test]
    fn dock_fields_follow_physical_monitor_scale() {
        let zones = dock_zones_with_reference(1_920, 1_080, Some((256, 160)));
        assert_eq!(
            (zones[0].rect.width, zones[0].rect.height),
            (96, 96),
            "24 mm corners at four pixels per millimetre"
        );
        assert_eq!(
            (zones[4].rect.x, zones[4].rect.y, zones[4].rect.width, zones[4].rect.height,),
            (0, 460, 48, 160)
        );
        assert_eq!(
            (zones[6].rect.x, zones[6].rect.y, zones[6].rect.width, zones[6].rect.height,),
            (832, 0, 256, 48)
        );
    }

    #[test]
    fn dock_hitboxes_map_corners_sides_and_top_center_without_whole_edge_latches() {
        let zones = dock_zones_with_reference(1_920, 1_080, Some((256, 160)));
        assert_eq!(dock_target_at_in_zones(0, 0, &zones), Some(WindowDockTarget::TopLeft));
        assert_eq!(
            dock_target_at_in_zones(1_919, 1_079, &zones),
            Some(WindowDockTarget::BottomRight)
        );
        assert_eq!(dock_target_at_in_zones(0, 540, &zones), Some(WindowDockTarget::LeftHalf));
        assert_eq!(dock_target_at_in_zones(1_919, 540, &zones), Some(WindowDockTarget::RightHalf));
        assert_eq!(dock_target_at_in_zones(960, 0, &zones), Some(WindowDockTarget::Maximize));
        assert_eq!(dock_target_at_in_zones(500, 0, &zones), None);
        assert_eq!(dock_target_at_in_zones(960, 200, &zones), None);
        assert_eq!(dock_target_at_in_zones(95, 95, &zones), None);
        assert_eq!(dock_target_at_in_zones(47, 460, &zones), None);
        assert_eq!(dock_target_at_in_zones(832, 47, &zones), None);
    }

    #[test]
    fn dock_field_spans_cover_exactly_the_curved_hitbox() {
        let zones = dock_zones_with_reference(1_920, 1_080, Some((256, 160)));
        for zone in zones {
            for row in 0..zone.rect.height {
                let span = dock_zone_row_span(zone, row);
                for column in 0..zone.rect.width {
                    let x = zone.rect.x.saturating_add(column);
                    let y = zone.rect.y.saturating_add(row);
                    let painted = span
                        .is_some_and(|span| x >= span.x && x < span.x.saturating_add(span.width));
                    assert_eq!(painted, dock_zone_contains(zone, x, y));
                }
            }
            for column in 0..zone.rect.width {
                let span = dock_zone_column_span(zone, column);
                for row in 0..zone.rect.height {
                    let x = zone.rect.x.saturating_add(column);
                    let y = zone.rect.y.saturating_add(row);
                    let painted = span
                        .is_some_and(|span| y >= span.y && y < span.y.saturating_add(span.height));
                    assert_eq!(painted, dock_zone_contains(zone, x, y));
                }
            }
        }
    }

    #[test]
    fn dock_fields_have_full_hd_proportional_fallback() {
        let metrics = dock_zone_metrics(1_920, 1_080, None);
        assert_eq!(
            (
                metrics.corner_width,
                metrics.corner_height,
                metrics.side_depth,
                metrics.side_span,
                metrics.top_span,
                metrics.top_depth,
            ),
            (96, 98, 48, 154, 240, 45)
        );
    }
}

fn absorb_selection_gesture(changed: bool, pressed: u32, primary_activation: bool) -> bool {
    changed && !(primary_activation && pressed == PRIMARY_BUTTON_MASK)
}

#[cfg(test)]
mod primary_activation_tests {
    use super::*;
    #[test]
    fn only_opted_in_primary_selection_is_delivered() {
        assert!(absorb_selection_gesture(true, PRIMARY_BUTTON_MASK, false));
        assert!(!absorb_selection_gesture(true, PRIMARY_BUTTON_MASK, true));
        assert!(absorb_selection_gesture(true, SECONDARY_BUTTON_MASK, true));
        assert!(absorb_selection_gesture(true, PRIMARY_BUTTON_MASK | SECONDARY_BUTTON_MASK, true));
        assert!(!absorb_selection_gesture(false, SECONDARY_BUTTON_MASK, true));
    }
}
