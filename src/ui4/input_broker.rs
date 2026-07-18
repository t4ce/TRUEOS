//! UI4 input focus and delivery over the kernel HID rings.
//!
//! The HID/HUT layer owns device discovery and identity. UI4 starts at its
//! sequence rings: it hit-tests windows, keeps one focus/capture per cursor
//! source, associates keyboards through HUT combos, and queues callbacks for
//! the trusted `WindowOwner`. Consumers never drain a global HID queue.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use heapless::Vec;
use spin::Mutex;

use super::{OutputId, WindowId, WindowOwner, WindowPlacement, WindowSnapshot, WindowState};

const MAX_CURSOR_ROUTES: usize = 32;
const MAX_OWNER_QUEUES: usize = 64;
const MAX_OWNER_EVENTS: usize = 256;
const CURSOR_BATCH: usize = 64;
const KEYBOARD_BATCH: usize = 64;
const INPUT_PUMP_PERIOD_MS: u64 = 4;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const SECONDARY_BUTTON_MASK: u32 = 1 << 1;
const MIDDLE_BUTTON_MASK: u32 = 1 << 2;
const SCREENSHOT_BUTTON_MASK: u32 = (1 << 3) | (1 << 4);
const FRAME_DRAG_GESTURE_MIN_TRAVEL_PX: u32 = 8;
const MAXIMIZE_CURSOR_REARM_TRAVEL_PX: u32 = 48;
const MAXIMIZE_LATCH_TOP_PX: u32 = 48;

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
    pub(crate) context_menu: Option<(u32, u32)>,
    pub(crate) selection: Option<Ui4VisualRect>,
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
    secondary_anchor: Option<(u32, u32)>,
    secondary_start_placement: Option<WindowPlacement>,
    secondary_dragged: bool,
    maximize_rearm_origin: Option<(u32, u32)>,
    context_menu: Option<(u32, u32)>,
    selection_anchor: Option<(u32, u32)>,
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
            secondary_anchor: None,
            secondary_start_placement: None,
            secondary_dragged: false,
            maximize_rearm_origin: None,
            context_menu: None,
            selection_anchor: None,
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

    fn release_owner(&mut self, owner: WindowOwner) -> usize {
        let mut released = 0usize;
        for route in &mut self.cursors {
            let owned_focus = route.focus.is_some_and(|target| target.owner == owner);
            let owned_capture = route.capture.is_some_and(|target| target.owner == owner);
            if !owned_focus && !owned_capture {
                continue;
            }
            if owned_focus {
                route.focus = None;
            }
            if owned_capture {
                route.capture = None;
            }
            route.secondary_anchor = None;
            route.secondary_start_placement = None;
            route.secondary_dragged = false;
            route.maximize_rearm_origin = None;
            route.context_menu = None;
            route.selection_anchor = None;
            released = released.saturating_add(1);
        }
        released
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
        let screenshot_pressed = event.buttons_down & !previous_buttons & SCREENSHOT_BUTTON_MASK;
        if screenshot_pressed != 0 {
            // Coalesce a simultaneous button-4/button-5 transition into one
            // global capture request. Side buttons are consumed by UI4 and do
            // not change application focus or pointer capture.
            super::screenshot::request_capture(screenshot_pressed.trailing_zeros() as u8 + 1);
        }
        let buttons_down = event.buttons_down & !SCREENSHOT_BUTTON_MASK;
        let previous_routed_buttons = previous_buttons & !SCREENSHOT_BUTTON_MASK;
        let pressed = buttons_down & !previous_routed_buttons;
        let released = previous_routed_buttons & !buttons_down;
        let dx = signed_delta(x, self.cursors[index].x);
        let dy = signed_delta(y, self.cursors[index].y);
        let hit = topmost_window_at(x, y);

        if dx != 0 || dy != 0 {
            self.cursors[index].visible_after_motion = true;
        }
        if self.cursors[index]
            .maximize_rearm_origin
            .is_some_and(|origin| {
                point_travel_reached(origin, (x, y), MAXIMIZE_CURSOR_REARM_TRAVEL_PX)
            })
        {
            self.cursors[index].maximize_rearm_origin = None;
        }
        if pressed & PRIMARY_BUTTON_MASK != 0 {
            self.cursors[index].selection_anchor = Some((x, y));
        }
        if released & PRIMARY_BUTTON_MASK != 0 {
            self.cursors[index].selection_anchor = None;
        }
        if pressed & SECONDARY_BUTTON_MASK != 0 {
            self.cursors[index].secondary_anchor = Some((x, y));
            self.cursors[index].secondary_start_placement = hit.map(|window| window.placement);
            self.cursors[index].secondary_dragged = false;
            self.cursors[index].context_menu = None;
        }
        if buttons_down & SECONDARY_BUTTON_MASK != 0
            && self.cursors[index].secondary_anchor.is_some_and(|anchor| {
                point_travel_reached(anchor, (x, y), FRAME_DRAG_GESTURE_MIN_TRAVEL_PX)
            })
        {
            self.cursors[index].secondary_dragged = true;
        }
        let secondary_released = released & SECONDARY_BUTTON_MASK != 0;
        let secondary_drop = secondary_released
            && self.cursors[index].secondary_anchor.is_some()
            && self.cursors[index].secondary_dragged;
        if secondary_released {
            let had_anchor = self.cursors[index].secondary_anchor.take().is_some();
            if had_anchor && !secondary_drop {
                self.cursors[index].context_menu = Some((x, y));
            }
            self.cursors[index].secondary_dragged = false;
        }

        if previous_routed_buttons == 0 && pressed != 0 {
            let focus = hit.map(WindowTarget::from);
            self.set_focus(index, focus, combo_id, vcursor);
            self.cursors[index].capture = focus;
        }

        let target = self.cursors[index]
            .capture
            .and_then(window_snapshot_for_target)
            .or(hit);
        if let Some(mut target) = target {
            if buttons_down & SECONDARY_BUTTON_MASK != 0
                && self.cursors[index].secondary_dragged
                && (dx != 0 || dy != 0)
            {
                if let Some(next) =
                    translated_frame_placement(target.placement, dx, dy, width, height)
                {
                    match super::set_window_placement(target.owner, target.id, next) {
                        Ok(()) => {
                            target.placement = next;
                            crate::log_trace!(target: "ui4";
                                "ui4/input: frame-drag owner={:?} window={} plane={} dx={} dy={} placement={},{} trigger=secondary-button\n",
                                target.owner,
                                target.id.raw(),
                                target.plane.slot(),
                                dx,
                                dy,
                                next.x,
                                next.y,
                            );
                        }
                        Err(error) => {
                            crate::log_warn!(target: "ui4";
                                "ui4/input: frame-drag rejected owner={:?} window={} error={:?}\n",
                                target.owner,
                                target.id.raw(),
                                error,
                            );
                        }
                    }
                }
            }
            let maximize_latched = secondary_drop
                && self.cursors[index].maximize_rearm_origin.is_none()
                && (target.maximized || maximize_latch_contains(x, y, width, height));
            if maximize_latched {
                match super::toggle_window_maximized(
                    target.owner,
                    target.id,
                    width,
                    height,
                    self.cursors[index].secondary_start_placement,
                ) {
                    Ok(transition) => {
                        target.placement = transition.placement;
                        target.maximized = transition.maximized;
                        self.cursors[index].maximize_rearm_origin = Some((x, y));
                        self.cursors[index].context_menu = None;
                        crate::log_info!(target: "ui4";
                            "ui4/input: frame-maximize-toggle owner={:?} window={} plane={} state={} old={}x{}@{},{} new={}x{}@{},{} cursor={}:{}:{} rearm_travel_px={} trigger=secondary-drag-drop\n",
                            target.owner,
                            target.id.raw(),
                            target.plane.slot(),
                            if transition.maximized { "maximized" } else { "restored" },
                            transition.previous.width,
                            transition.previous.height,
                            transition.previous.x,
                            transition.previous.y,
                            transition.placement.width,
                            transition.placement.height,
                            transition.placement.x,
                            transition.placement.y,
                            source.controller_id,
                            source.slot_id,
                            source.ep_target,
                            MAXIMIZE_CURSOR_REARM_TRAVEL_PX,
                        );
                    }
                    Err(error) => {
                        crate::log_warn!(target: "ui4";
                            "ui4/input: frame-maximize-toggle rejected owner={:?} window={} error={:?}\n",
                            target.owner,
                            target.id.raw(),
                            error,
                        );
                    }
                }
            }
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
                    buttons_down,
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
                        buttons_down,
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
                        buttons_down,
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
            if buttons_down & MIDDLE_BUTTON_MASK != 0 && (dx != 0 || dy != 0) {
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
        if secondary_released {
            self.cursors[index].secondary_start_placement = None;
        }
        if buttons_down == 0 {
            self.cursors[index].capture = None;
        }
    }

    fn process_keyboard(&self, event: crate::r::keyboard::TrueosKeyboardOutputEvent) {
        let (combo_id, virtual_keyboard) = keyboard_hut_metadata(&event);
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            && event.key_code == crate::r::keyboard::KEYBOARD_KEY_F1
        {
            let route = self
                .cursors
                .iter()
                .filter(|route| {
                    if combo_id != 0 {
                        cursor_hut_metadata(route.source).0 == combo_id
                    } else {
                        route.source.controller_id == event.controller_id
                            && route.source.slot_id == event.slot_id
                    }
                })
                .max_by_key(|route| route.focus_serial)
                .or_else(|| self.cursors.iter().max_by_key(|route| route.focus_serial));
            let Some(route) = route else {
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: F1 ignored reason=no-ui4-cursor keyboard_ctrl={} keyboard_slot={} combo={}\n",
                    event.controller_id,
                    event.slot_id,
                    combo_id,
                );
                return;
            };
            let Some(window) = topmost_window_at(route.x, route.y) else {
                crate::log_warn!(target: "ui4/screenshot";
                    "ui4/screenshot: F1 ignored reason=no-window-below-cursor cursor={},{} combo={}\n",
                    route.x,
                    route.y,
                    combo_id,
                );
                return;
            };
            super::screenshot::request_window_capture(window, route.x, route.y);
            return;
        }
        let matched_target = self
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
        let target = matched_target.or_else(|| {
            if combo_id != 0 {
                return None;
            }
            self.cursors
                .iter()
                .filter_map(|route| {
                    let focus = route.focus?;
                    window_snapshot_for_target(focus)
                        .is_some()
                        .then_some((route.focus_serial, focus))
                })
                .max_by_key(|(serial, _)| *serial)
                .map(|(_, focus)| focus)
        });
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

    fn pump(&mut self) -> bool {
        let mut cursor_activity = false;
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
            cursor_activity = true;
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
        cursor_activity
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
                context_menu: route.context_menu,
                selection: route.selection_anchor.and_then(|anchor| {
                    (route.buttons_down & PRIMARY_BUTTON_MASK != 0)
                        .then(|| selection_rect_between(anchor, (route.x, route.y)))
                }),
            });
        }
        visuals
    }
}

static INPUT_BROKER: Mutex<InputBroker> = Mutex::new(InputBroker::new());
static OWNER_QUEUES: Mutex<Vec<OwnerQueue, MAX_OWNER_QUEUES>> = Mutex::new(Vec::new());
static SLOT4_VISUAL_CHANGE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();

#[embassy_executor::task]
pub(crate) async fn ui4_input_service_task() {
    crate::log_info!(target: "ui4";
        "ui4/input: service online source=hid-sequence-rings focus=per-cursor keyboard=hut-combo/exact-slot/recent-focus-fallback virtual=vcursor frame_drag=secondary-button/broker-placement maximize=top-center-drop/restore-next-drop/per-cursor-rearm-48px selection=primary-button/active-outline screenshot=F1/topmost-window-below-cursor+mouse-buttons-4-or-5/composition\n"
    );
    loop {
        if INPUT_BROKER.lock().pump() {
            SLOT4_VISUAL_CHANGE.signal(());
        }
        Timer::after(Duration::from_millis(INPUT_PUMP_PERIOD_MS)).await;
    }
}

pub(super) async fn wait_slot4_visual_change() {
    SLOT4_VISUAL_CHANGE.wait().await;
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

pub(super) fn release_owner(owner: WindowOwner) -> (usize, usize) {
    let routes = INPUT_BROKER.lock().release_owner(owner);
    let queued_events = {
        let mut queues = OWNER_QUEUES.lock();
        if let Some(index) = queues.iter().position(|queue| queue.owner == owner) {
            let queued_events = queues[index].events.len();
            queues.remove(index);
            queued_events
        } else {
            0
        }
    };
    if routes != 0 || queued_events != 0 {
        SLOT4_VISUAL_CHANGE.signal(());
    }
    (routes, queued_events)
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

fn selection_rect_between(anchor: (u32, u32), point: (u32, u32)) -> Ui4VisualRect {
    Ui4VisualRect {
        x: anchor.0.min(point.0),
        y: anchor.1.min(point.1),
        width: anchor.0.abs_diff(point.0).saturating_add(1),
        height: anchor.1.abs_diff(point.1).saturating_add(1),
    }
}

fn point_travel_reached(origin: (u32, u32), point: (u32, u32), threshold: u32) -> bool {
    let dx = u64::from(origin.0.abs_diff(point.0));
    let dy = u64::from(origin.1.abs_diff(point.1));
    let threshold = u64::from(threshold);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
        >= threshold.saturating_mul(threshold)
}

/// Small drop target centered at the 50% horizontal point on the top edge.
/// A maximized window accepts its restore drop anywhere because it cannot be
/// translated while it already covers the output.
fn maximize_latch_contains(x: u32, y: u32, screen_width: u32, screen_height: u32) -> bool {
    if screen_width == 0 || screen_height == 0 {
        return false;
    }
    let half_width = (screen_width / 16).clamp(48, 160).min(screen_width / 2);
    let center = screen_width / 2;
    y < MAXIMIZE_LATCH_TOP_PX.min(screen_height) && x.abs_diff(center) <= half_width
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
        .filter(|window| window.state == WindowState::Ready)
        .filter(|window| placement_contains(window.placement, x, y))
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
