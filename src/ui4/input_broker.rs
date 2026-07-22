//! UI4 input focus and delivery over the kernel HID rings.
//!
//! The HID/HUT layer owns device discovery and identity. UI4 starts at its
//! sequence rings: it hit-tests windows, enforces the one global selected
//! frame, keeps pointer capture per cursor source, associates keyboards through
//! HUT combos, and queues callbacks for the trusted `WindowOwner`. Consumers
//! never drain a global HID queue.

use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::Timer;
use heapless::Vec;
use spin::Mutex;

use super::{
    OutputId, Ui4CursorSource, WindowId, WindowOwner, WindowPlacement, WindowSnapshot, WindowState,
};

const MAX_CURSOR_ROUTES: usize = 32;
const MAX_OWNER_QUEUES: usize = 64;
const MAX_OWNER_EVENTS: usize = 256;
const CURSOR_BATCH: usize = 64;
const KEYBOARD_BATCH: usize = 64;
const PRIMARY_BUTTON_MASK: u32 = 1 << 0;
const SECONDARY_BUTTON_MASK: u32 = 1 << 1;
const MIDDLE_BUTTON_MASK: u32 = 1 << 2;
const SCREENSHOT_BUTTON_MASK: u32 = (1 << 3) | (1 << 4);
// The screenshot worker remains intentionally parked during frame/window
// reintegration. Do not consume F1 or side buttons into an undrained capture
// queue; this switch can move with the worker when that producer returns.
const INTERACTIVE_SCREENSHOT_ENABLED: bool = false;
const FRAME_DRAG_GESTURE_MIN_TRAVEL_PX: u32 = 8;
const MAXIMIZE_LATCH_TOP_PX: u32 = 48;
const CONTEXT_MENU_OFFSET_PX: u32 = 14;
const CONTEXT_MENU_WIDTH_PX: u32 = 196;
const CONTEXT_MENU_HEIGHT_PX: u32 = 116;

static OWNER_QUEUE_DROPS: AtomicU32 = AtomicU32::new(0);

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
    pub(crate) buttons_down: u32,
    pub(crate) icon: super::Ui4CursorIcon,
    pub(crate) context_menu: Option<(u32, u32)>,
    pub(crate) selection: Option<Ui4VisualRect>,
    pub(crate) maximize_preview: Option<Ui4VisualRect>,
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

impl WindowTarget {
    const fn cursor_frame_key(self) -> super::CursorFrameKey {
        super::CursorFrameKey::new(self.owner, self.window)
    }
}

impl From<super::CursorFrameKey> for WindowTarget {
    fn from(key: super::CursorFrameKey) -> Self {
        Self {
            owner: key.owner,
            window: key.window,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct KeyboardSource {
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
}

impl From<crate::r::keyboard::TrueosKeyboardOutputEvent> for KeyboardSource {
    fn from(event: crate::r::keyboard::TrueosKeyboardOutputEvent) -> Self {
        Self {
            controller_id: event.controller_id,
            slot_id: event.slot_id,
            ep_target: event.ep_target,
        }
    }
}

#[derive(Copy, Clone)]
struct CursorRoute {
    source: Ui4CursorSource,
    x: u32,
    y: u32,
    buttons_down: u32,
    capture: Option<WindowTarget>,
    keyboard_source: Option<KeyboardSource>,
    selection_serial: u64,
    visible_after_motion: bool,
    color: crate::graphics::primitives::Rgba8,
    secondary_anchor: Option<(u32, u32)>,
    secondary_start_placement: Option<WindowPlacement>,
    secondary_dragged: bool,
    secondary_restored_from_maximize: bool,
    maximize_preview: Option<Ui4VisualRect>,
    context_menu: Option<(u32, u32)>,
    context_menu_owns_gesture: bool,
    suppress_context_menu_open: bool,
    selection_anchor: Option<(u32, u32)>,
    absorb_select: bool,
}

impl CursorRoute {
    fn new(source: Ui4CursorSource, x: u32, y: u32, buttons_down: u32) -> Self {
        Self {
            source,
            x,
            y,
            buttons_down,
            capture: None,
            keyboard_source: None,
            selection_serial: 0,
            visible_after_motion: false,
            color: super::cursor_frame_inout::cursor_color(source),
            secondary_anchor: None,
            secondary_start_placement: None,
            secondary_dragged: false,
            secondary_restored_from_maximize: false,
            maximize_preview: None,
            context_menu: None,
            context_menu_owns_gesture: false,
            suppress_context_menu_open: false,
            selection_anchor: None,
            absorb_select: false,
        }
    }

    fn clear_frame_interaction(&mut self) {
        self.capture = None;
        self.keyboard_source = None;
        self.secondary_anchor = None;
        self.secondary_start_placement = None;
        self.secondary_dragged = false;
        self.secondary_restored_from_maximize = false;
        self.maximize_preview = None;
        self.selection_anchor = None;
    }

    fn clear_window_interaction(&mut self) {
        self.clear_frame_interaction();
        self.context_menu = None;
        self.context_menu_owns_gesture = false;
        self.suppress_context_menu_open = false;
    }

    /// Returns true when the menu owns this mouse-down. An outside press
    /// dismisses this route's menu but remains available to normal UI4 input.
    fn handle_context_menu_mouse_down(
        &mut self,
        x: u32,
        y: u32,
        screen_width: u32,
        screen_height: u32,
    ) -> bool {
        let Some(anchor) = self.context_menu else {
            return false;
        };
        if visual_rect_contains(context_menu_rect(anchor, screen_width, screen_height), x, y) {
            self.context_menu_owns_gesture = true;
            true
        } else {
            self.context_menu = None;
            self.suppress_context_menu_open = true;
            false
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
    selection_serial: u64,
    cursors: Vec<CursorRoute, MAX_CURSOR_ROUTES>,
}

impl InputBroker {
    const fn new() -> Self {
        Self {
            cursor_read_seq: 0,
            keyboard_read_seq: 0,
            selection_serial: 0,
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
            .min_by_key(|(_, route)| route.selection_serial)
            .map(|(index, _)| index)
            .unwrap_or(0);
        super::cursor_frame_inout::cursor_retired(self.cursors[index].source);
        self.cursors[index] = CursorRoute::new(source, x, y, 0);
        index
    }

    fn select_frame(
        &mut self,
        cursor_index: usize,
        next: Option<WindowTarget>,
        combo_id: u32,
        vcursor: bool,
    ) -> bool {
        let source = self.cursors[cursor_index].source;
        let color = self.cursors[cursor_index].color;
        let change = super::cursor_frame_inout::select_frame(
            next.map(WindowTarget::cursor_frame_key),
            source,
            color,
        );
        self.selection_serial = self.selection_serial.wrapping_add(1).max(1);
        self.cursors[cursor_index].selection_serial = self.selection_serial;
        if !change.changed {
            return false;
        }
        for route in &mut self.cursors {
            // Context menus belong to cursor routes, not to the globally
            // selected frame. The cursor performing the selection dismisses
            // its own menu in `process_cursor`; other cursors keep theirs.
            route.clear_frame_interaction();
            route.absorb_select |= route.buttons_down != 0;
        }
        if let Some(previous) = change.previous.map(WindowTarget::from) {
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
        if let Some(next) = change.selected.map(WindowTarget::from) {
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
        true
    }

    fn release_owner(&mut self, owner: WindowOwner) -> usize {
        let mut released = 0usize;
        for route in &mut self.cursors {
            let owned_capture = route.capture.is_some_and(|target| target.owner == owner);
            if !owned_capture {
                continue;
            }
            route.clear_window_interaction();
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
        if INTERACTIVE_SCREENSHOT_ENABLED && screenshot_pressed != 0 {
            // Coalesce a simultaneous button-4/button-5 transition into one
            // global capture request. Side buttons are consumed by UI4 and do
            // not change application focus or pointer capture.
            super::screenshot::request_capture(screenshot_pressed.trailing_zeros() as u8 + 1);
        }
        let routed_button_mask = if INTERACTIVE_SCREENSHOT_ENABLED {
            !SCREENSHOT_BUTTON_MASK
        } else {
            u32::MAX
        };
        let buttons_down = event.buttons_down & routed_button_mask;
        let previous_routed_buttons = previous_buttons & routed_button_mask;
        let pressed = buttons_down & !previous_routed_buttons;
        let released = previous_routed_buttons & !buttons_down;
        let dx = signed_delta(x, self.cursors[index].x);
        let dy = signed_delta(y, self.cursors[index].y);
        let hit = topmost_window_at(x, y);

        if dx != 0 || dy != 0 {
            self.cursors[index].visible_after_motion = true;
        }
        self.cursors[index].maximize_preview = None;

        if pressed != 0 {
            // Menu actions are not wired yet. An inside click belongs to this
            // cursor's menu and cannot leak through to the frame below. An
            // outside click dismisses only this cursor's menu; a secondary
            // click is suppressed from reopening it on the matching release.
            self.cursors[index].handle_context_menu_mouse_down(x, y, width, height);
        }

        if self.cursors[index].context_menu_owns_gesture {
            self.cursors[index].x = x;
            self.cursors[index].y = y;
            self.cursors[index].buttons_down = event.buttons_down;
            if buttons_down == 0 {
                self.cursors[index].context_menu_owns_gesture = false;
                // A different cursor may have changed global selection while
                // this menu-owned gesture was held. Both absorption latches
                // end on this same release.
                self.cursors[index].absorb_select = false;
                self.cursors[index].suppress_context_menu_open = false;
            }
            return;
        }

        if previous_routed_buttons == 0 && pressed != 0 {
            let next = hit.map(WindowTarget::from);
            if self.select_frame(index, next, combo_id, vcursor) {
                self.cursors[index].absorb_select = true;
            } else {
                let selected = super::selected_frame();
                self.cursors[index].capture = hit
                    .filter(|window| {
                        selected == Some(WindowTarget::from(*window).cursor_frame_key())
                            && (window.interaction.movable
                                || window.interaction.maximizable
                                || window.interaction.receives_input)
                    })
                    .map(WindowTarget::from);
            }
        }

        // A selection-changing click is a complete absorbed gesture. Keeping
        // the latch through button-up prevents a release (or drag motion)
        // leaking into the newly selected application.
        if self.cursors[index].absorb_select {
            self.cursors[index].x = x;
            self.cursors[index].y = y;
            self.cursors[index].buttons_down = event.buttons_down;
            if buttons_down == 0 {
                self.cursors[index].absorb_select = false;
                self.cursors[index].suppress_context_menu_open = false;
            }
            return;
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
            self.cursors[index].secondary_restored_from_maximize = false;
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
            if had_anchor && !secondary_drop && !self.cursors[index].suppress_context_menu_open {
                self.cursors[index].context_menu = Some((x, y));
            }
            self.cursors[index].secondary_dragged = false;
        }

        let selected = super::selected_frame();
        let target = self.cursors[index]
            .capture
            .and_then(window_snapshot_for_target)
            .filter(|window| selected == Some(WindowTarget::from(*window).cursor_frame_key()))
            .or_else(|| {
                hit.filter(|window| {
                    selected == Some(WindowTarget::from(*window).cursor_frame_key())
                })
            });
        if let Some(mut target) = target {
            if target.maximized
                && buttons_down & SECONDARY_BUTTON_MASK != 0
                && self.cursors[index].secondary_dragged
            {
                match super::toggle_window_maximized(target.owner, target.id, width, height, None) {
                    Ok(transition) => {
                        target.placement = transition.placement;
                        target.maximized = transition.maximized;
                        self.cursors[index].secondary_restored_from_maximize = true;
                        self.cursors[index].context_menu = None;
                        crate::log_info!(target: "ui4";
                            "ui4/input: frame-maximize-toggle owner={:?} window={} plane={} state=restored old={}x{}@{},{} new={}x{}@{},{} cursor={}:{}:{} trigger=secondary-drag-begin\n",
                            target.owner,
                            target.id.raw(),
                            target.plane.slot(),
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
                        );
                    }
                    Err(error) => {
                        crate::log_warn!(target: "ui4";
                            "ui4/input: frame-maximize-restore rejected owner={:?} window={} error={:?}\n",
                            target.owner,
                            target.id.raw(),
                            error,
                        );
                    }
                }
            }
            if target.interaction.movable
                && buttons_down & SECONDARY_BUTTON_MASK != 0
                && self.cursors[index].secondary_dragged
                && (dx != 0 || dy != 0)
            {
                if let Some(next) =
                    translated_frame_placement(target.placement, dx, dy, width, height)
                {
                    match super::move_window(target.owner, target.id, next) {
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
            if target.interaction.maximizable
                && !target.maximized
                && !self.cursors[index].secondary_restored_from_maximize
                && buttons_down & SECONDARY_BUTTON_MASK != 0
                && self.cursors[index].secondary_dragged
                && maximize_latch_contains(x, y, width, height)
            {
                let preview = super::window_broker::maximized_window_placement(
                    target.interaction,
                    target.placement,
                    width,
                    height,
                );
                self.cursors[index].maximize_preview = Some(Ui4VisualRect {
                    x: preview.x.max(0) as u32,
                    y: preview.y.max(0) as u32,
                    width: preview.width,
                    height: preview.height,
                });
            }
            let maximize_latched = target.interaction.maximizable
                && secondary_drop
                && !self.cursors[index].secondary_restored_from_maximize
                && maximize_latch_contains(x, y, width, height);
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
                        self.cursors[index].context_menu = None;
                        crate::log_info!(target: "ui4";
                            "ui4/input: frame-maximize-toggle owner={:?} window={} plane={} state={} old={}x{}@{},{} new={}x{}@{},{} cursor={}:{}:{} trigger=secondary-drag-drop\n",
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
            if target.interaction.receives_input {
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
        }

        self.cursors[index].x = x;
        self.cursors[index].y = y;
        self.cursors[index].buttons_down = event.buttons_down;
        if secondary_released {
            self.cursors[index].secondary_start_placement = None;
            self.cursors[index].secondary_restored_from_maximize = false;
            self.cursors[index].maximize_preview = None;
        }
        if buttons_down == 0 {
            self.cursors[index].capture = None;
            self.cursors[index].suppress_context_menu_open = false;
        }
    }

    fn process_keyboard(&mut self, event: crate::r::keyboard::TrueosKeyboardOutputEvent) {
        if !super::cursor_frame_inout::global_keyboard_passes(&event) {
            return;
        }
        let (combo_id, virtual_keyboard) = keyboard_hut_metadata(&event);
        if INTERACTIVE_SCREENSHOT_ENABLED
            && event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
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
                .max_by_key(|route| route.selection_serial)
                .or_else(|| {
                    self.cursors
                        .iter()
                        .max_by_key(|route| route.selection_serial)
                });
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
        let Some(target) = super::selected_frame().map(WindowTarget::from) else {
            return;
        };
        let Some(window) = window_snapshot_for_target(target) else {
            return;
        };
        if !window.interaction.receives_input {
            return;
        }
        let matched_route = self
            .cursors
            .iter()
            .enumerate()
            .filter_map(|(index, route)| {
                if !super::cursor_frame_inout::source_selected(route.source) {
                    return None;
                }
                let matches = if combo_id != 0 {
                    cursor_hut_metadata(route.source).0 == combo_id
                } else {
                    route.source.controller_id == event.controller_id
                        && route.source.slot_id == event.slot_id
                };
                matches.then_some((route.selection_serial, index))
            })
            .max_by_key(|(serial, _)| *serial)
            .map(|(_, index)| index);
        let route_index = matched_route.or_else(|| {
            if combo_id != 0 {
                return None;
            }
            self.cursors
                .iter()
                .enumerate()
                .filter_map(|(index, route)| {
                    super::cursor_frame_inout::source_selected(route.source)
                        .then_some((route.selection_serial, index))
                })
                .max_by_key(|(serial, _)| *serial)
                .map(|(_, index)| index)
        });
        let Some(route_index) = route_index else {
            return;
        };
        // Keep the keyboard identity beside the cursor which participated in
        // selecting the one global frame. Blueprint game consumers can then
        // sample held HID usages without access to the global HUT list.
        self.cursors[route_index].keyboard_source = Some(event.into());
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

    fn focused_keyboard_state(
        &self,
        target: WindowTarget,
    ) -> Option<crate::usb2::hid::hut::HidKeyboardState> {
        if super::selected_frame() != Some(target.cursor_frame_key())
            || window_snapshot_for_target(target).is_none()
        {
            return None;
        }
        let source = self
            .cursors
            .iter()
            .filter(|route| super::cursor_frame_inout::source_selected(route.source))
            .filter_map(|route| {
                route
                    .keyboard_source
                    .map(|source| (route.selection_serial, source))
            })
            .max_by_key(|(serial, _)| *serial)
            .map(|(_, source)| source)?;
        crate::usb2::hid::hut::keyboards_snapshot()
            .into_iter()
            .find(|keyboard| {
                keyboard.controller_id == source.controller_id
                    && keyboard.slot_id == source.slot_id
                    && keyboard.ep_target == source.ep_target
            })
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
        for route in &self.cursors {
            if !route.visible_after_motion {
                continue;
            }
            let icon = topmost_window_at(route.x, route.y)
                .filter(|window| {
                    super::selected_frame() == Some(WindowTarget::from(*window).cursor_frame_key())
                })
                .map(|window| {
                    super::cursor_icon_for(
                        WindowTarget::from(window).cursor_frame_key(),
                        route.source,
                    )
                })
                .unwrap_or(super::Ui4CursorIcon::Default);
            let _ = visuals.push(Ui4SoftwareCursorVisual {
                source: route.source,
                x: route.x,
                y: route.y,
                color: route.color,
                buttons_down: route.buttons_down,
                icon,
                context_menu: route.context_menu,
                maximize_preview: route.maximize_preview,
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
        "ui4/input: service online source=hid-sequence-rings pump_hz={} pump_clock=absolute-fractional selection=global-zero-or-one-frame first-click=absorb-select keyboard=global-hooks-before-ui4/hut-combo/exact-slot/recent-selector-fallback cursor=slot4-software/all-active-sources/per-frame-per-cursor hardware-cursor=preferred-physical-source/concurrent virtual=vcursor frame_drag=secondary-button/selected-frame-only maximize=interaction-capability-gated outline=primary-button/selected-frame-only owner_events=selected-frame-only screenshot=parked\n",
        super::INTERACTION_CADENCE_HZ,
    );
    let mut cadence = super::InteractionCadence::new();
    loop {
        if INPUT_BROKER.lock().pump() {
            SLOT4_VISUAL_CHANGE.signal(());
        }
        Timer::at(cadence.next_deadline()).await;
    }
}

pub(super) async fn wait_slot4_visual_change() {
    SLOT4_VISUAL_CHANGE.wait().await;
}

pub(super) fn notify_slot4_visual_change() {
    SLOT4_VISUAL_CHANGE.signal(());
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

pub(crate) fn focused_keyboard_state(
    owner: WindowOwner,
    window: WindowId,
) -> Option<crate::usb2::hid::hut::HidKeyboardState> {
    INPUT_BROKER
        .lock()
        .focused_keyboard_state(WindowTarget { owner, window })
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

pub(crate) fn context_menu_rect(
    anchor: (u32, u32),
    screen_width: u32,
    screen_height: u32,
) -> Ui4VisualRect {
    Ui4VisualRect {
        x: anchor
            .0
            .saturating_add(CONTEXT_MENU_OFFSET_PX)
            .min(screen_width.saturating_sub(CONTEXT_MENU_WIDTH_PX)),
        y: anchor
            .1
            .saturating_add(CONTEXT_MENU_OFFSET_PX)
            .min(screen_height.saturating_sub(CONTEXT_MENU_HEIGHT_PX)),
        width: CONTEXT_MENU_WIDTH_PX.min(screen_width),
        height: CONTEXT_MENU_HEIGHT_PX.min(screen_height),
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

/// Monitor-wide top-edge drop target. The dedicated slot-4 plane previews the
/// result while the cursor remains inside this narrow activation band.
fn maximize_latch_contains(_x: u32, y: u32, screen_width: u32, screen_height: u32) -> bool {
    if screen_width == 0 || screen_height == 0 {
        return false;
    }
    y < MAXIMIZE_LATCH_TOP_PX.min(screen_height)
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
