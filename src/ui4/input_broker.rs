//! UI4 input focus and delivery over the kernel HID rings.
//!
//! The HID/HUT layer owns device discovery and identity. UI4 starts at its
//! sequence rings: it hit-tests windows, keeps one focus/capture per cursor
//! source, associates keyboards through HUT combos, and queues callbacks for
//! the trusted `WindowOwner`. Consumers never drain a global HID queue.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_time::{Duration, Timer};
use heapless::Vec;
use spin::Mutex;

use super::{OutputId, WindowId, WindowOwner, WindowPlacement, WindowSnapshot};

const MAX_CURSOR_ROUTES: usize = 32;
const MAX_OWNER_QUEUES: usize = 64;
const MAX_OWNER_EVENTS: usize = 256;
const CURSOR_BATCH: usize = 64;
const KEYBOARD_BATCH: usize = 64;
const INPUT_PUMP_PERIOD_MS: u64 = 4;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const SECONDARY_BUTTON_MASK: u32 = 1 << 1;
const MIDDLE_BUTTON_MASK: u32 = 1 << 2;

static OWNER_QUEUE_DROPS: AtomicU32 = AtomicU32::new(0);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CursorSource {
    pub(crate) controller_id: u32,
    pub(crate) slot_id: u32,
    pub(crate) ep_target: u32,
    pub(crate) hid_kind: u8,
}

impl Ui4CursorSource {
    fn from_event(event: crate::usb2::hid::TrueosHidCursorEvent) -> Self {
        Self {
            controller_id: event.controller_id,
            slot_id: event.slot_id,
            ep_target: event.ep_target,
            hid_kind: event.hid_kind,
        }
    }
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
    pub(crate) combo_id: u32,
    pub(crate) virtual_keyboard: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4FocusEvent {
    pub(crate) source: Ui4CursorSource,
    pub(crate) window: WindowId,
    pub(crate) focused: bool,
    pub(crate) combo_id: u32,
    pub(crate) vcursor: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4ButtonPhase {
    Down,
    Up,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4ButtonEvent {
    pub(crate) source: Ui4CursorSource,
    pub(crate) window: WindowId,
    pub(crate) phase: Ui4ButtonPhase,
    /// All buttons which made this transition together.
    pub(crate) changed_buttons: u32,
    /// Complete coherent button state after the transition.
    pub(crate) buttons_down: u32,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) local_x: i32,
    pub(crate) local_y: i32,
    pub(crate) combo_id: u32,
    pub(crate) vcursor: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4PanPhase {
    Begin,
    Update,
    End,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4PanEvent {
    pub(crate) source: Ui4CursorSource,
    pub(crate) window: WindowId,
    pub(crate) phase: Ui4PanPhase,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) local_x: i32,
    pub(crate) local_y: i32,
    pub(crate) dx: i32,
    pub(crate) dy: i32,
    pub(crate) combo_id: u32,
    pub(crate) vcursor: bool,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4ResizeEvent {
    pub(crate) window: WindowId,
    pub(crate) old_width: u32,
    pub(crate) old_height: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4InputEvent {
    Pointer(Ui4PointerEvent),
    Button(Ui4ButtonEvent),
    Pan(Ui4PanEvent),
    Resize(Ui4ResizeEvent),
    Keyboard(Ui4KeyboardEvent),
    Focus(Ui4FocusEvent),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4VisualRect {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4SoftwareCursorVisual {
    pub(crate) source: Ui4CursorSource,
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) color: crate::graphics::primitives::Rgba8,
    pub(crate) draw_cursor: bool,
    pub(crate) selection: Option<Ui4VisualRect>,
    pub(crate) context_menu: Option<(u32, u32)>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct WindowTarget {
    owner: WindowOwner,
    window: WindowId,
}

impl From<WindowSnapshot> for WindowTarget {
    fn from(window: WindowSnapshot) -> Self {
        Self {
            owner: window.owner,
            window: window.id,
        }
    }
}

#[derive(Copy, Clone)]
struct CursorRoute {
    source: Ui4CursorSource,
    x: u32,
    y: u32,
    buttons_down: u32,
    focus: Option<WindowTarget>,
    capture: Option<WindowTarget>,
    focus_serial: u64,
    visible_after_motion: bool,
    color: crate::graphics::primitives::Rgba8,
    selection_anchor: Option<(u32, u32)>,
    selection: Option<Ui4VisualRect>,
    secondary_anchor: Option<(u32, u32)>,
    secondary_dragged: bool,
    context_menu: Option<(u32, u32)>,
}

impl CursorRoute {
    fn new(source: Ui4CursorSource, x: u32, y: u32, buttons_down: u32) -> Self {
        Self {
            source,
            x,
            y,
            buttons_down,
            focus: None,
            capture: None,
            focus_serial: 0,
            visible_after_motion: false,
            color: software_cursor_color(source),
            selection_anchor: None,
            selection: None,
            secondary_anchor: None,
            secondary_dragged: false,
            context_menu: None,
        }
    }
}

struct OwnerQueue {
    owner: WindowOwner,
    events: Vec<Ui4InputEvent, MAX_OWNER_EVENTS>,
}

struct InputBroker {
    cursor_read_seq: u64,
    keyboard_read_seq: u64,
    focus_serial: u64,
    cursors: Vec<CursorRoute, MAX_CURSOR_ROUTES>,
}

impl InputBroker {
    const fn new() -> Self {
        Self {
            cursor_read_seq: 0,
            keyboard_read_seq: 0,
            focus_serial: 0,
            cursors: Vec::new(),
        }
    }

    fn cursor_index(&mut self, source: Ui4CursorSource, x: u32, y: u32) -> usize {
        if let Some(index) = self.cursors.iter().position(|route| route.source == source) {
            return index;
        }
        if self.cursors.push(CursorRoute::new(source, x, y, 0)).is_ok() {
            return self.cursors.len() - 1;
        }
        let index = self
            .cursors
            .iter()
            .enumerate()
            .min_by_key(|(_, route)| route.focus_serial)
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.cursors[index] = CursorRoute::new(source, x, y, 0);
        index
    }

    fn set_focus(
        &mut self,
        cursor_index: usize,
        next: Option<WindowTarget>,
        combo_id: u32,
        vcursor: bool,
    ) {
        let previous = self.cursors[cursor_index].focus;
        if previous == next {
            return;
        }
        self.focus_serial = self.focus_serial.wrapping_add(1).max(1);
        self.cursors[cursor_index].focus = next;
        self.cursors[cursor_index].focus_serial = self.focus_serial;
        let source = self.cursors[cursor_index].source;
        if let Some(previous) = previous {
            enqueue_owner_event(
                previous.owner,
                Ui4InputEvent::Focus(Ui4FocusEvent {
                    source,
                    window: previous.window,
                    focused: false,
                    combo_id,
                    vcursor,
                }),
            );
        }
        if let Some(next) = next {
            enqueue_owner_event(
                next.owner,
                Ui4InputEvent::Focus(Ui4FocusEvent {
                    source,
                    window: next.window,
                    focused: true,
                    combo_id,
                    vcursor,
                }),
            );
        }
    }

    fn process_cursor(&mut self, event: crate::usb2::hid::TrueosHidCursorEvent) {
        let Some((width, height)) = crate::intel::active_scanout_dimensions() else {
            return;
        };
        let x = normalized_to_pixel(event.x, width);
        let y = normalized_to_pixel(event.y, height);
        let source = Ui4CursorSource::from_event(event);
        let (combo_id, vcursor) = cursor_hut_metadata(source);
        let index = self.cursor_index(source, x, y);
        let previous_buttons = self.cursors[index].buttons_down;
        let pressed = event.buttons_down & !previous_buttons;
        let released = previous_buttons & !event.buttons_down;
        let dx = signed_delta(x, self.cursors[index].x);
        let dy = signed_delta(y, self.cursors[index].y);
        let hit = topmost_window_at(x, y);

        if dx != 0 || dy != 0 {
            self.cursors[index].visible_after_motion = true;
        }
        if pressed & PRIMARY_BUTTON_MASK != 0 {
            self.cursors[index].selection_anchor = Some((x, y));
            self.cursors[index].selection = None;
            self.cursors[index].context_menu = None;
        }
        if event.buttons_down & PRIMARY_BUTTON_MASK != 0 {
            if let Some(anchor) = self.cursors[index].selection_anchor {
                self.cursors[index].selection = Some(visual_rect_between(anchor, (x, y)));
            }
        }
        if released & PRIMARY_BUTTON_MASK != 0 {
            if let Some(anchor) = self.cursors[index].selection_anchor.take() {
                let rect = visual_rect_between(anchor, (x, y));
                self.cursors[index].selection =
                    (rect.width >= 4 && rect.height >= 4).then_some(rect);
            }
        }
        if pressed & SECONDARY_BUTTON_MASK != 0 {
            self.cursors[index].selection_anchor = None;
            self.cursors[index].selection = None;
            self.cursors[index].secondary_anchor = Some((x, y));
            self.cursors[index].secondary_dragged = false;
            self.cursors[index].context_menu = None;
        }
        if event.buttons_down & SECONDARY_BUTTON_MASK != 0 && (dx != 0 || dy != 0) {
            self.cursors[index].secondary_dragged = true;
        }
        if released & SECONDARY_BUTTON_MASK != 0 {
            if self.cursors[index].secondary_anchor.take().is_some()
                && !self.cursors[index].secondary_dragged
            {
                self.cursors[index].context_menu = Some((x, y));
            }
            self.cursors[index].secondary_dragged = false;
        }

        if previous_buttons == 0 && pressed != 0 {
            let focus = hit.map(WindowTarget::from);
            self.set_focus(index, focus, combo_id, vcursor);
            self.cursors[index].capture = focus;
        }

        let target = self.cursors[index]
            .capture
            .and_then(window_snapshot_for_target)
            .or(hit);
        if let Some(target) = target {
            let local_x = signed_local(x, target.placement.x);
            let local_y = signed_local(y, target.placement.y);
            enqueue_owner_event(
                target.owner,
                Ui4InputEvent::Pointer(Ui4PointerEvent {
                    source,
                    window: target.id,
                    x,
                    y,
                    local_x,
                    local_y,
                    dx,
                    dy,
                    wheel: event.wheel,
                    buttons_down: event.buttons_down,
                    buttons_pressed: pressed,
                    buttons_released: released,
                    combo_id,
                    vcursor,
                }),
            );
            if pressed != 0 {
                enqueue_owner_event(
                    target.owner,
                    Ui4InputEvent::Button(Ui4ButtonEvent {
                        source,
                        window: target.id,
                        phase: Ui4ButtonPhase::Down,
                        changed_buttons: pressed,
                        buttons_down: event.buttons_down,
                        x,
                        y,
                        local_x,
                        local_y,
                        combo_id,
                        vcursor,
                    }),
                );
            }
            if released != 0 {
                enqueue_owner_event(
                    target.owner,
                    Ui4InputEvent::Button(Ui4ButtonEvent {
                        source,
                        window: target.id,
                        phase: Ui4ButtonPhase::Up,
                        changed_buttons: released,
                        buttons_down: event.buttons_down,
                        x,
                        y,
                        local_x,
                        local_y,
                        combo_id,
                        vcursor,
                    }),
                );
            }
            if pressed & MIDDLE_BUTTON_MASK != 0 {
                enqueue_pan_event(
                    target,
                    source,
                    Ui4PanPhase::Begin,
                    x,
                    y,
                    local_x,
                    local_y,
                    0,
                    0,
                    combo_id,
                    vcursor,
                );
            }
            if event.buttons_down & MIDDLE_BUTTON_MASK != 0 && (dx != 0 || dy != 0) {
                enqueue_pan_event(
                    target,
                    source,
                    Ui4PanPhase::Update,
                    x,
                    y,
                    local_x,
                    local_y,
                    dx,
                    dy,
                    combo_id,
                    vcursor,
                );
            }
            if released & MIDDLE_BUTTON_MASK != 0 {
                enqueue_pan_event(
                    target,
                    source,
                    Ui4PanPhase::End,
                    x,
                    y,
                    local_x,
                    local_y,
                    0,
                    0,
                    combo_id,
                    vcursor,
                );
            }
        }

        self.cursors[index].x = x;
        self.cursors[index].y = y;
        self.cursors[index].buttons_down = event.buttons_down;
        if event.buttons_down == 0 {
            self.cursors[index].capture = None;
        }
    }

    fn process_keyboard(&self, event: crate::r::keyboard::TrueosKeyboardOutputEvent) {
        let (combo_id, virtual_keyboard) = keyboard_hut_metadata(&event);
        let target = self
            .cursors
            .iter()
            .filter_map(|route| {
                let focus = route.focus?;
                if window_snapshot_for_target(focus).is_none() {
                    return None;
                }
                let matches = if combo_id != 0 {
                    cursor_hut_metadata(route.source).0 == combo_id
                } else {
                    route.source.controller_id == event.controller_id
                        && route.source.slot_id == event.slot_id
                };
                matches.then_some((route.focus_serial, focus))
            })
            .max_by_key(|(serial, _)| *serial)
            .map(|(_, focus)| focus);
        let Some(target) = target else {
            return;
        };
        enqueue_owner_event(
            target.owner,
            Ui4InputEvent::Keyboard(Ui4KeyboardEvent {
                window: target.window,
                event,
                combo_id,
                virtual_keyboard,
            }),
        );
    }

    fn pump(&mut self) {
        let mut cursor_events = [crate::usb2::hid::TrueosHidCursorEvent::default(); CURSOR_BATCH];
        loop {
            let (next, dropped, wrote) = crate::usb2::hid::read_cursor_events_since(
                self.cursor_read_seq,
                &mut cursor_events,
            );
            if dropped != 0 {
                crate::log_warn!(target: "ui4";
                    "ui4/input: cursor-ring overrun read_seq={} dropped={}\n",
                    self.cursor_read_seq,
                    dropped
                );
            }
            if wrote == 0 {
                break;
            }
            self.cursor_read_seq = next;
            for event in cursor_events.iter().take(wrote).copied() {
                self.process_cursor(event);
            }
            if wrote < cursor_events.len() {
                break;
            }
        }

        let mut keyboard_events =
            [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); KEYBOARD_BATCH];
        loop {
            let (next, dropped, wrote) = crate::r::keyboard::read_output_events_since(
                self.keyboard_read_seq,
                &mut keyboard_events,
            );
            if dropped != 0 {
                crate::log_warn!(target: "ui4";
                    "ui4/input: keyboard-ring overrun read_seq={} dropped={}\n",
                    self.keyboard_read_seq,
                    dropped
                );
            }
            if wrote == 0 {
                break;
            }
            self.keyboard_read_seq = next;
            for event in keyboard_events.iter().take(wrote).copied() {
                self.process_keyboard(event);
            }
            if wrote < keyboard_events.len() {
                break;
            }
        }
    }

    fn software_cursor_visuals(&self) -> Vec<Ui4SoftwareCursorVisual, MAX_CURSOR_ROUTES> {
        let mut visuals = Vec::new();
        let hardware_cursor_slot =
            crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()
                .map(|(slot_id, _, _, _)| slot_id);
        for route in &self.cursors {
            if !route.visible_after_motion {
                continue;
            }
            let _ = visuals.push(Ui4SoftwareCursorVisual {
                source: route.source,
                x: route.x,
                y: route.y,
                color: route.color,
                draw_cursor: hardware_cursor_slot != Some(route.source.slot_id),
                selection: route.selection,
                context_menu: route.context_menu,
            });
        }
        visuals
    }
}

static INPUT_BROKER: Mutex<InputBroker> = Mutex::new(InputBroker::new());
static OWNER_QUEUES: Mutex<Vec<OwnerQueue, MAX_OWNER_QUEUES>> = Mutex::new(Vec::new());

#[embassy_executor::task]
pub(crate) async fn ui4_input_service_task() {
    crate::log_info!(target: "ui4";
        "ui4/input: service online source=hid-sequence-rings focus=per-cursor keyboard=hut-combo/exact-slot virtual=vcursor\n"
    );
    loop {
        INPUT_BROKER.lock().pump();
        Timer::after(Duration::from_millis(INPUT_PUMP_PERIOD_MS)).await;
    }
}

pub(crate) fn take_owner_input_events(owner: WindowOwner) -> Vec<Ui4InputEvent, MAX_OWNER_EVENTS> {
    let mut queues = OWNER_QUEUES.lock();
    let Some(queue) = queues.iter_mut().find(|queue| queue.owner == owner) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    core::mem::swap(&mut out, &mut queue.events);
    out
}

pub(crate) fn software_cursor_visuals() -> Vec<Ui4SoftwareCursorVisual, MAX_CURSOR_ROUTES> {
    INPUT_BROKER.lock().software_cursor_visuals()
}

pub(super) fn enqueue_window_resize(
    owner: WindowOwner,
    window: WindowId,
    old_width: u32,
    old_height: u32,
    width: u32,
    height: u32,
) {
    enqueue_owner_event(
        owner,
        Ui4InputEvent::Resize(Ui4ResizeEvent {
            window,
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

fn enqueue_owner_event(owner: WindowOwner, event: Ui4InputEvent) {
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
        note_owner_queue_drop();
        return;
    };
    let queue = &mut queues[queue_index].events;
    if queue.push(event).is_err() {
        let _ = queue.remove(0);
        if queue.push(event).is_err() {
            note_owner_queue_drop();
        }
    }
}

fn note_owner_queue_drop() {
    let drops = OWNER_QUEUE_DROPS.fetch_add(1, Ordering::Relaxed) + 1;
    if drops <= 8 || drops.is_power_of_two() {
        crate::log_warn!(target: "ui4";
            "ui4/input: owner-queue drop count={}\n",
            drops
        );
    }
}

fn normalized_to_pixel(value: f64, extent: u32) -> u32 {
    if extent == 0 {
        return 0;
    }
    ((value.clamp(0.0, 1.0) * f64::from(extent)) as u32).min(extent - 1)
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

fn visual_rect_between(a: (u32, u32), b: (u32, u32)) -> Ui4VisualRect {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    Ui4VisualRect {
        x,
        y,
        width: a.0.max(b.0).saturating_sub(x).saturating_add(1),
        height: a.1.max(b.1).saturating_sub(y).saturating_add(1),
    }
}

fn software_cursor_color(source: Ui4CursorSource) -> crate::graphics::primitives::Rgba8 {
    use crate::graphics::primitives::Rgba8;

    const COLORS: [Rgba8; 16] = [
        Rgba8::new(255, 64, 64, 255),
        Rgba8::new(32, 168, 255, 255),
        Rgba8::new(32, 224, 128, 255),
        Rgba8::new(255, 190, 32, 255),
        Rgba8::new(220, 80, 255, 255),
        Rgba8::new(255, 112, 32, 255),
        Rgba8::new(32, 224, 224, 255),
        Rgba8::new(152, 112, 255, 255),
        Rgba8::new(192, 240, 48, 255),
        Rgba8::new(255, 64, 176, 255),
        Rgba8::new(64, 112, 255, 255),
        Rgba8::new(48, 192, 96, 255),
        Rgba8::new(255, 224, 64, 255),
        Rgba8::new(176, 80, 224, 255),
        Rgba8::new(255, 128, 160, 255),
        Rgba8::new(96, 224, 255, 255),
    ];
    let hash = source.controller_id.wrapping_mul(0x9E37_79B9)
        ^ source.slot_id.rotate_left(11)
        ^ source.ep_target.rotate_left(19)
        ^ u32::from(source.hid_kind);
    COLORS[(hash as usize) % COLORS.len()]
}

fn placement_contains(placement: WindowPlacement, x: u32, y: u32) -> bool {
    if !placement.visible {
        return false;
    }
    let x = i64::from(x);
    let y = i64::from(y);
    let left = i64::from(placement.x);
    let top = i64::from(placement.y);
    x >= left
        && y >= top
        && x < left.saturating_add(i64::from(placement.width))
        && y < top.saturating_add(i64::from(placement.height))
}

fn topmost_window_at(x: u32, y: u32) -> Option<WindowSnapshot> {
    let output = OutputId::from_slot(0)?;
    super::visible_windows_for_output(output)
        .into_iter()
        .rev()
        .find(|window| placement_contains(window.placement, x, y))
}

fn window_snapshot_for_target(target: WindowTarget) -> Option<WindowSnapshot> {
    for output_slot in 0..super::OUTPUT_COUNT {
        let Some(output) = OutputId::from_slot(output_slot) else {
            continue;
        };
        if let Some(window) = super::visible_windows_for_output(output)
            .into_iter()
            .find(|window| window.id == target.window && window.owner == target.owner)
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
