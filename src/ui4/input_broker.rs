//! UI4 input focus and delivery over the kernel HID rings.
//!
//! The HID/HUT layer owns device discovery and identity. UI4 starts at its
//! sequence rings: it hit-tests windows, keeps one selected-frame association
//! per cursor plus a most-recent application input focus, maintains pointer
//! capture per cursor source, associates keyboards through HUT combos, and
//! queues callbacks for the trusted `WindowOwner`. Consumers never drain a
//! global HID queue.

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
pub(super) const DESKTOP_CONTEXT_MENU_ENTRY_COUNT: u32 = 2;
pub(super) const DESKTOP_CONTEXT_MENU_HORIZONTAL_INSET_PX: u32 = 12;
pub(super) const DESKTOP_CONTEXT_MENU_VERTICAL_INSET_PX: u32 = 12;

static OWNER_QUEUE_DROPS: AtomicU32 = AtomicU32::new(0);
static KEYBOARD_TEXT_FORWARDS: AtomicU32 = AtomicU32::new(0);
static DESKTOP_SHELL_LAUNCH_SEQUENCE: AtomicU32 = AtomicU32::new(0);
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
    pub(crate) source: Ui4CursorSource,
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
    pub(crate) resize_epoch: u64,
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
pub(crate) struct Ui4DockPreview {
    pub(crate) target: super::WindowDockTarget,
    pub(crate) destination: Ui4VisualRect,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct Ui4DockZone {
    pub(super) target: super::WindowDockTarget,
    pub(super) rect: Ui4VisualRect,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4SoftwareCursorVisual {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) color: crate::graphics::primitives::Rgba8,
    pub(crate) icon: super::Ui4CursorIcon,
    pub(crate) stepped_cell: Option<Ui4VisualRect>,
    pub(crate) context_menu: Option<(u32, u32)>,
    pub(crate) selection: Option<Ui4VisualRect>,
    pub(crate) dock_fields_visible: bool,
    pub(crate) dock_preview: Option<Ui4DockPreview>,
}

#[derive(Clone, Debug)]
pub(crate) struct Ui4WindowInputRoute {
    pub(crate) source: Ui4CursorSource,
    pub(crate) combo_id: u32,
    pub(crate) color: crate::graphics::primitives::Rgba8,
    pub(crate) vcursor: bool,
    pub(crate) selected_for_window: bool,
    pub(crate) app_focus: bool,
    pub(crate) keyboard: Option<crate::usb2::hid::hut::HidKeyboardState>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4ProgrammaticSelectionError {
    NotFound,
    NotReady,
    OutputUnavailable,
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
    /// Suppress only the field a docked drag just restored from. Leaving it
    /// or jumping directly into a different field re-arms docking.
    secondary_restore_origin: Option<super::WindowDockTarget>,
    dock_fields_visible: bool,
    dock_preview: Option<Ui4DockPreview>,
    context_menu: Option<(u32, u32)>,
    context_menu_owns_gesture: bool,
    context_menu_pressed_action: Option<DesktopContextMenuAction>,
    requested_context_menu_gesture: Option<u64>,
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
            secondary_restore_origin: None,
            dock_fields_visible: false,
            dock_preview: None,
            context_menu: None,
            context_menu_owns_gesture: false,
            context_menu_pressed_action: None,
            requested_context_menu_gesture: None,
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
        self.secondary_restore_origin = None;
        self.dock_fields_visible = false;
        self.dock_preview = None;
        self.selection_anchor = None;
    }

    fn clear_window_interaction(&mut self) {
        self.clear_frame_interaction();
        self.context_menu = None;
        self.context_menu_owns_gesture = false;
        self.context_menu_pressed_action = None;
        self.requested_context_menu_gesture = None;
        self.suppress_context_menu_open = false;
    }

    /// The desktop menu owns presses inside its original visual bounds. An
    /// outside press dismisses it and remains available to ordinary routing.
    fn handle_context_menu_mouse_down(
        &mut self,
        x: u32,
        y: u32,
        screen_width: u32,
        screen_height: u32,
    ) {
        let Some(anchor) = self.context_menu else {
            return;
        };
        let menu = context_menu_rect(anchor, screen_width, screen_height);
        if visual_rect_contains(menu, x, y) {
            self.context_menu_owns_gesture = true;
            self.context_menu_pressed_action = desktop_context_menu_action_at(menu, x, y);
        } else {
            self.context_menu = None;
            self.context_menu_pressed_action = None;
            self.suppress_context_menu_open = true;
        }
    }
}

struct OwnerQueue {
    owner: WindowOwner,
    events: Vec<Ui4InputEvent, MAX_OWNER_EVENTS>,
}

#[derive(Copy, Clone)]
struct OwnerResizeState {
    owner: WindowOwner,
    window: WindowId,
    latest_epoch: u64,
    pending: Option<Ui4ResizeEvent>,
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
        super::context_menu::dismiss_for_source(self.cursors[index].source);
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
        if !change.changed && !change.focus_changed {
            return false;
        }
        super::context_menu::dismiss_for_source(source);
        if change.changed {
            // Pointer capture belongs to the selecting cursor. A different
            // cursor changing the singular keyboard focus must not cancel an
            // already selected frame's independent pointer gesture.
            self.cursors[cursor_index].clear_frame_interaction();
        }
        if change.focus_changed {
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
                // Focus is the hot signal. The frame the user just reached for
                // claims a hardware lease plane when one is free or revocable
                // and otherwise keeps presenting from the stack, so this can
                // neither fail nor gate the focus event below.
                let _ = super::window_broker::note_window_focused(next.owner, next.window);
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
        // Only a change to this cursor's own frame association is an
        // absorb-select. Merely restoring global keyboard focus to a frame
        // already owned by this cursor must leave its next drag gesture live.
        change.changed
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

    fn retire_hid_slot(&mut self, controller_id: u32, slot_id: u32) -> (usize, usize) {
        let mut retired_routes = 0usize;
        let mut index = 0usize;
        while index < self.cursors.len() {
            if self.cursors[index].source.controller_id == controller_id
                && self.cursors[index].source.slot_id == slot_id
            {
                let route = self.cursors.remove(index);
                super::context_menu::dismiss_for_source(route.source);
                super::cursor_frame_inout::cursor_retired(route.source);
                retired_routes = retired_routes.saturating_add(1);
            } else {
                index += 1;
            }
        }

        let mut retired_keyboards = 0usize;
        for route in &mut self.cursors {
            if route.keyboard_source.is_some_and(|source| {
                source.controller_id == controller_id && source.slot_id == slot_id
            }) {
                route.keyboard_source = None;
                retired_keyboards = retired_keyboards.saturating_add(1);
            }
        }
        (retired_routes, retired_keyboards)
    }

    fn process_cursor(&mut self, event: crate::usb2::hid::TrueosHidCursorEvent) {
        if event.flags & crate::usb2::hid::HID_CURSOR_EVENT_FLAG_DEVICE_LOST != 0 {
            let (routes, keyboards) = self.retire_hid_slot(event.controller_id, event.slot_id);
            crate::log_info!(target: "ui4";
                "ui4/input: hid device-lost consumed controller={} slot={} cursor_routes={} keyboard_routes={}\n",
                event.controller_id,
                event.slot_id,
                routes,
                keyboards
            );
            return;
        }
        let Some((width, height)) = crate::intel::active_scanout_dimensions() else {
            return;
        };
        let x = normalized_to_pixel(event.x, width);
        let y = normalized_to_pixel(event.y, height);
        let source = Ui4CursorSource::from_event(event);
        let (combo_id, vcursor) = cursor_hut_metadata(source);
        let index = self.cursor_index(source, x, y);
        let previous_buttons = self.cursors[index].buttons_down;
        let routed_button_mask = u32::MAX;
        let buttons_down = event.buttons_down & routed_button_mask;
        let previous_routed_buttons = previous_buttons & routed_button_mask;
        let pressed = buttons_down & !previous_routed_buttons;
        let released = previous_routed_buttons & !buttons_down;
        let dx = signed_delta(x, self.cursors[index].x);
        let dy = signed_delta(y, self.cursors[index].y);
        let hit = topmost_window_at(x, y);

        super::context_menu::pointer_moved(source, x, y, width, height);
        if dx != 0 || dy != 0 {
            self.cursors[index].visible_after_motion = true;
        }
        self.cursors[index].dock_fields_visible = false;
        self.cursors[index].dock_preview = None;

        if pressed != 0
            && let Some(serial) = super::context_menu::pointer_down(source, x, y, width, height)
        {
            self.cursors[index].requested_context_menu_gesture = Some(serial);
        }

        if let Some(serial) = self.cursors[index].requested_context_menu_gesture {
            self.cursors[index].x = x;
            self.cursors[index].y = y;
            self.cursors[index].buttons_down = event.buttons_down;
            if buttons_down == 0 {
                self.cursors[index].requested_context_menu_gesture = None;
                super::context_menu::pointer_up(source, serial, x, y, width, height);
                self.cursors[index].absorb_select = false;
                self.cursors[index].suppress_context_menu_open = false;
            }
            return;
        }

        if pressed != 0 {
            self.cursors[index].handle_context_menu_mouse_down(x, y, width, height);
        }

        if self.cursors[index].context_menu_owns_gesture {
            self.cursors[index].x = x;
            self.cursors[index].y = y;
            self.cursors[index].buttons_down = event.buttons_down;
            if buttons_down == 0 {
                let selected = self.cursors[index].context_menu.and_then(|anchor| {
                    let menu = context_menu_rect(anchor, width, height);
                    let released_action = desktop_context_menu_action_at(menu, x, y);
                    (released_action == self.cursors[index].context_menu_pressed_action)
                        .then_some((anchor, released_action?))
                });
                self.cursors[index].context_menu_owns_gesture = false;
                self.cursors[index].context_menu_pressed_action = None;
                self.cursors[index].absorb_select = false;
                self.cursors[index].suppress_context_menu_open = false;
                if let Some((anchor, action)) = selected {
                    self.cursors[index].context_menu = None;
                    match action {
                        DesktopContextMenuAction::ColorPicker => {
                            super::color_picker::request_open(source, anchor);
                        }
                        DesktopContextMenuAction::Shell => {
                            request_desktop_shell_launch(source, x, y)
                        }
                    }
                }
            }
            return;
        }

        if previous_routed_buttons == 0 && pressed != 0 {
            let next = hit.map(WindowTarget::from);
            let changed = self.select_frame(index, next, combo_id, vcursor);
            if absorb_selection_gesture(
                changed,
                pressed,
                hit.is_some_and(|window| window.interaction.primary_activation),
            ) {
                self.cursors[index].absorb_select = true;
            } else {
                let selected = super::selected_frame_for_source(source);
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
            self.cursors[index].secondary_restore_origin = None;
            self.cursors[index].context_menu = None;
            self.cursors[index].context_menu_pressed_action = None;
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
                // A frame owns the secondary-click gesture over its own pixels
                // whenever it has registered a menu. The broker's only job here
                // is to say where the click landed; it does not own the menu.
                // The kernel desktop menu is therefore the no-frame case, plus
                // frames which registered nothing.
                let color = self.cursors[index].color;
                let frame_menu = hit.and_then(|window| {
                    super::context_menu::registered_request(window.owner, window.id)
                        .map(|request| (window.owner, window.id, request))
                });
                match frame_menu {
                    Some((menu_owner, menu_window, request)) => {
                        if super::context_menu::open(
                            source,
                            menu_owner,
                            menu_window,
                            (x, y),
                            color,
                            request,
                        )
                        .is_err()
                        {
                            self.cursors[index].context_menu = Some((x, y));
                        }
                    }
                    None => self.cursors[index].context_menu = Some((x, y)),
                }
            }
            self.cursors[index].secondary_dragged = false;
        }

        // Pointer routing and capture follow this cursor's selected frame.
        // Global selection remains the singular keyboard/application focus.
        let selected = super::selected_frame_for_source(source);
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
            let mut restored_this_event = false;
            let mut frame_geometry_changed = false;
            if target.dock_target.is_some()
                && buttons_down & SECONDARY_BUTTON_MASK != 0
                && self.cursors[index].secondary_dragged
            {
                let restore_origin = target.dock_target;
                match super::restore_docked_window(target.owner, target.id, width, height, (x, y)) {
                    Ok(transition) => {
                        target.placement = transition.placement;
                        target.dock_target = transition.dock_target;
                        target.maximized = false;
                        // If this same gesture later docks elsewhere, that new
                        // dock must restore to the normal geometry we just
                        // recovered, not to the old dock rectangle captured on
                        // button-down.
                        self.cursors[index].secondary_start_placement = Some(transition.placement);
                        self.cursors[index].secondary_restore_origin = restore_origin;
                        restored_this_event = true;
                        frame_geometry_changed = true;
                        crate::log_info!(target: "ui4";
                            "ui4/input: frame-dock owner={:?} window={} plane={} state=restored old={}x{}@{},{} new={}x{}@{},{} cursor={}:{}:{} trigger=secondary-drag-begin\n",
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
                            "ui4/input: frame-dock-restore rejected owner={:?} window={} error={:?}\n",
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
                && !restored_this_event
            {
                if let Some(next) =
                    translated_frame_placement(target.placement, dx, dy, width, height)
                {
                    let next =
                        super::start_button::constrain_drag_placement(target.owner, next, x, width);
                    match super::move_window(target.owner, target.id, next) {
                        Ok(()) => {
                            target.placement = next;
                            frame_geometry_changed = true;
                            // Keep the lease alive for the whole gesture. The
                            // idle grace is shorter than a long drag, so
                            // without this refresh a continuously dragged
                            // frame would become revocable mid-motion.
                            let _ = super::window_broker::note_window_hot(target.owner, target.id);
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
            // Restoring a docked frame must not immediately relatch the field
            // it came from. Leaving that field, including a direct absolute
            // jump into another one, re-arms docking for the same gesture.
            if let Some(origin) = self.cursors[index].secondary_restore_origin
                && dock_target_at(x, y, width, height) != Some(origin)
            {
                self.cursors[index].secondary_restore_origin = None;
            }
            let docking_gesture = target.interaction.maximizable
                && target.dock_target.is_none()
                && self.cursors[index].secondary_restore_origin.is_none()
                && (self.cursors[index].secondary_dragged || secondary_drop);
            let dock_target = docking_gesture
                .then(|| dock_target_at(x, y, width, height))
                .flatten();
            if docking_gesture && buttons_down & SECONDARY_BUTTON_MASK != 0 {
                self.cursors[index].dock_fields_visible = true;
                if let Some(dock_target) = dock_target {
                    let preview = super::window_broker::docked_window_placement(
                        target.interaction,
                        target.placement,
                        dock_target,
                        width,
                        height,
                    );
                    self.cursors[index].dock_preview = Some(Ui4DockPreview {
                        target: dock_target,
                        destination: Ui4VisualRect {
                            x: preview.x.max(0) as u32,
                            y: preview.y.max(0) as u32,
                            width: preview.width,
                            height: preview.height,
                        },
                    });
                }
            }
            if secondary_drop && let Some(dock_target) = dock_target {
                match super::dock_window(
                    target.owner,
                    target.id,
                    dock_target,
                    width,
                    height,
                    self.cursors[index].secondary_start_placement,
                ) {
                    Ok(transition) => {
                        target.placement = transition.placement;
                        target.dock_target = transition.dock_target;
                        target.maximized =
                            transition.dock_target == Some(super::WindowDockTarget::Maximize);
                        frame_geometry_changed = true;
                        crate::log_info!(target: "ui4";
                            "ui4/input: frame-dock owner={:?} window={} plane={} target={} old={}x{}@{},{} new={}x{}@{},{} cursor={}:{}:{} trigger=secondary-drag-drop\n",
                            target.owner,
                            target.id.raw(),
                            target.plane.slot(),
                            dock_target_label(dock_target),
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
                            "ui4/input: frame-dock rejected owner={:?} window={} target={} error={:?}\n",
                            target.owner,
                            target.id.raw(),
                            dock_target_label(dock_target),
                            error,
                        );
                    }
                }
            }
            if target.interaction.receives_input {
                // While a dock resize is waiting for a producer replacement,
                // input follows the exact 1:1 pixels currently on screen rather
                // than the newer logical destination hidden behind them.
                let input_placement = if frame_geometry_changed {
                    window_snapshot_for_target(WindowTarget::from(target))
                        .map(|snapshot| snapshot.presentation_placement)
                        .unwrap_or(target.presentation_placement)
                } else {
                    target.presentation_placement
                };
                let local_x = signed_local(x, input_placement.x);
                let local_y = signed_local(y, input_placement.y);
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
            self.cursors[index].secondary_restore_origin = None;
            self.cursors[index].dock_fields_visible = false;
            self.cursors[index].dock_preview = None;
        }
        if buttons_down == 0 {
            self.cursors[index].capture = None;
            self.cursors[index].suppress_context_menu_open = false;
        }
    }

    fn retire_keyboard_source(
        &mut self,
        controller_id: u32,
        slot_id: u32,
        ep_target: u32,
    ) -> usize {
        let mut retired = 0usize;
        for route in &mut self.cursors {
            if route.keyboard_source.is_some_and(|source| {
                source.controller_id == controller_id
                    && source.slot_id == slot_id
                    && source.ep_target == ep_target
            }) {
                route.keyboard_source = None;
                retired = retired.saturating_add(1);
            }
        }
        retired
    }

    fn process_keyboard(&mut self, event: crate::r::keyboard::TrueosKeyboardOutputEvent) {
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_DEVICE_LOST {
            let routes =
                self.retire_keyboard_source(event.controller_id, event.slot_id, event.ep_target);
            crate::log_info!(target: "ui4";
                "ui4/input: keyboard device-lost consumed controller={} slot={} ep={} routes={}\n",
                event.controller_id,
                event.slot_id,
                event.ep_target,
                routes
            );
            return;
        }
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
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            && event.key_code == crate::r::keyboard::KEYBOARD_KEY_START
        {
            if event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_PRESS != 0 {
                super::request_start_button_reveal();
            }
            return;
        }
        let route_index = self.keyboard_route_index(&event, combo_id, true);
        let Some(route_index) = route_index else {
            return;
        };
        let route_source = self.cursors[route_index].source;
        let Some(key) = super::selected_frame_for_source(route_source) else {
            return;
        };
        let target = WindowTarget {
            owner: key.owner,
            window: key.window,
        };
        let Some(window) = window_snapshot_for_target(target) else {
            return;
        };
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            && event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE
            && matches!(
                super::window_escape_key_action(target.owner, target.window),
                Ok(super::Ui4FrameEscapeKeyAction::Close)
            )
        {
            // Escape is scoped to this keyboard/cursor's selected frame.  A
            // Blueprint frame ends its VM; a kernel-owned frame simply closes
            // that one UI4 window.  Either way it is consumed here, before
            // the owner queue, unless the frame explicitly reserved Escape.
            match target.owner {
                super::WindowOwner::Vm(vm_id) => {
                    let stop = crate::hv::stop(vm_id);
                    crate::log_info!(target: "ui4";
                        "ui4/input: Escape default-close selected_frame owner=vm{} window={} combo={} action=stop-vmx result={:?}\n",
                        vm_id, target.window.raw(), combo_id, stop);
                }
                owner => {
                    let close = super::close_window(owner, target.window);
                    crate::log_info!(target: "ui4";
                        "ui4/input: Escape default-close selected_frame owner={:?} window={} combo={} action=close-frame result={:?}\n",
                        owner, target.window.raw(), combo_id, close);
                }
            }
            return;
        }
        if !window.interaction.receives_input {
            return;
        }
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
            && matches!(
                event.key_code,
                crate::r::keyboard::KEYBOARD_KEY_F10
                    | crate::r::keyboard::KEYBOARD_KEY_PRINT_SCREEN
            )
        {
            crate::log_info!(target: "ui4";
                "ui4/input: printer shortcut forwarded named_key={} ring_seq={} device_seq={} controller={} slot={} ep={} owner={:?} window={} cursor={}:{}:{} combo={} virtual_keyboard={}\n",
                event.key_code,
                event.seq,
                event.device_seq,
                event.controller_id,
                event.slot_id,
                event.ep_target,
                target.owner,
                target.window.raw(),
                route_source.controller_id,
                route_source.slot_id,
                route_source.ep_target,
                combo_id,
                virtual_keyboard,
            );
        }
        // Keep the keyboard identity beside the cursor which participated in
        // selecting its frame. An exact HUT combo (including Lilly's paired
        // vCursor/vKeyboard) therefore cannot type into a more recently
        // selected frame owned by another cursor. Blueprint game consumers can
        // then
        // sample held HID usages without access to the global HUT list.
        self.cursors[route_index].keyboard_source = Some(event.into());
        if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT {
            let forwards = KEYBOARD_TEXT_FORWARDS.fetch_add(1, Ordering::Relaxed) + 1;
            if forwards <= 64 || forwards.is_multiple_of(120) {
                crate::log_info!(target: "ui4";
                    "ui4/input: text forwarded forward_seq={} ring_seq={} device_seq={} codepoint={} controller={} slot={} ep={} owner={:?} window={} combo={} virtual_keyboard={} path=keyboard-output-ring->selected-frame-owner-queue log_policy=first-64+each-120\n",
                    forwards,
                    event.seq,
                    event.device_seq,
                    event.codepoint,
                    event.controller_id,
                    event.slot_id,
                    event.ep_target,
                    target.owner,
                    target.window.raw(),
                    combo_id,
                    virtual_keyboard,
                );
            }
        }
        enqueue_owner_event(
            target.owner,
            Ui4InputEvent::Keyboard(Ui4KeyboardEvent {
                source: route_source,
                window: target.window,
                event,
                combo_id,
                virtual_keyboard,
            }),
        );
    }

    fn keyboard_route_index(
        &self,
        event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
        combo_id: u32,
        selected_only: bool,
    ) -> Option<usize> {
        let matched = self
            .cursors
            .iter()
            .enumerate()
            .filter_map(|(index, route)| {
                if selected_only && !super::cursor_frame_inout::source_selected(route.source) {
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
        matched.or_else(|| {
            if combo_id != 0 {
                return None;
            }
            self.cursors
                .iter()
                .enumerate()
                .filter_map(|(index, route)| {
                    (!selected_only || super::cursor_frame_inout::source_selected(route.source))
                        .then_some((route.selection_serial, index))
                })
                .max_by_key(|(serial, _)| *serial)
                .map(|(_, index)| index)
        })
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

    fn window_input_routes(
        &self,
        target: WindowTarget,
    ) -> Vec<Ui4WindowInputRoute, MAX_CURSOR_ROUTES> {
        if window_snapshot_for_target(target).is_none() {
            return Vec::new();
        }
        let keyboards = crate::usb2::hid::hut::keyboards_snapshot();
        let app_focus = super::selected_frame() == Some(target.cursor_frame_key());
        self.cursors
            .iter()
            .filter_map(|route| {
                let (combo_id, vcursor) = cursor_hut_metadata(route.source);
                let keyboard = if combo_id != 0 {
                    keyboards
                        .iter()
                        .find(|keyboard| keyboard.combo_id == combo_id)
                        .cloned()
                } else {
                    route.keyboard_source.and_then(|source| {
                        keyboards
                            .iter()
                            .find(|keyboard| {
                                keyboard.controller_id == source.controller_id
                                    && keyboard.slot_id == source.slot_id
                                    && keyboard.ep_target == source.ep_target
                            })
                            .cloned()
                    })
                };
                Some(Ui4WindowInputRoute {
                    source: route.source,
                    combo_id,
                    color: route.color,
                    vcursor,
                    selected_for_window: super::selected_frame_for_source(route.source)
                        == Some(target.cursor_frame_key()),
                    app_focus,
                    keyboard,
                })
            })
            .collect()
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
            let (x, y, icon, stepped_cell) = cursor_visual_presentation(route);
            let _ = visuals.push(Ui4SoftwareCursorVisual {
                x,
                y,
                color: route.color,
                icon,
                stepped_cell,
                context_menu: route.context_menu,
                dock_fields_visible: route.dock_fields_visible,
                dock_preview: route.dock_preview,
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
        "ui4/input: service online source=hid-sequence-rings cursor_wake=producer-signal keyboard_watchdog_hz={} selection=per-cursor-zero-or-one-frame+most-recent-input-focus first-click=absorb-select keyboard=global-hooks-before-ui4/hut-combo/exact-slot/recent-selector-fallback start_key=reveal-menu-button cursor=slot4-software/all-active-sources/per-frame-per-cursor hardware-cursor=preferred-physical-source/concurrent virtual=vcursor frame_drag=secondary-button/per-cursor-selected-frame-only dock=top-center-maximize+center-sides-halves+corners-quadrants/dpi-mm-first outline=primary-button/selected-frame-only desktop_menu=per-cursor/color-picker+shell owner_events=selected-frame-only screenshot=parked\n",
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
        let request = loop {
            let request = {
                let mut requests = DESKTOP_SHELL_LAUNCH_REQUESTS.lock();
                (!requests.is_empty()).then(|| requests.remove(0))
            };
            if let Some(request) = request {
                break request;
            }
            DESKTOP_SHELL_LAUNCH_SIGNAL.wait().await;
        };

        let target =
            crate::shell2::matrix_target_for_slot_name(crate::shell2::OUTPUT_SYSTEM_MASK, "");
        let token = DESKTOP_SHELL_LAUNCH_SEQUENCE
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_add(1)
            .max(1);
        let instance_name = alloc::format!("ui4_shell_{token}");
        {
            let mut intents = DESKTOP_SHELL_LAUNCH_INTENTS.lock();
            if intents.is_full() {
                intents.remove(0);
            }
            let _ = intents.push(DesktopShellLaunchIntent {
                token,
                launch: DesktopShellLaunch {
                    source: request.source,
                    x: request.x,
                    y: request.y,
                },
            });
        }
        match crate::shell2::cmds::run::submit_archive_name_to_target_from_app_db_with_instance_detached_ui_async(
            target,
            "shell.bp",
            alloc::vec::Vec::new(),
            crate::hv::BlueprintInstanceRequest::named(instance_name),
        )
        .await
        {
            Ok(_) => crate::log_info!(target: "ui4";
                "ui4/input: desktop shell launched cursor={}:{}:{} source=app.db archive=shell.bp policy=local-builtin-best-effort\n",
                request.source.controller_id,
                request.source.slot_id,
                request.source.ep_target,
            ),
            Err(error) => {
                DESKTOP_SHELL_LAUNCH_INTENTS
                    .lock()
                    .retain(|intent| intent.token != token);
                crate::log_warn!(target: "ui4";
                    "ui4/input: desktop shell launch failed cursor={}:{}:{} source=app.db archive=shell.bp policy=local-builtin-best-effort error={}\n",
                    request.source.controller_id,
                    request.source.slot_id,
                    request.source.ep_target,
                    error,
                );
            }
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

pub(crate) fn show_context_menu(
    source: Ui4CursorSource,
    owner: WindowOwner,
    window: WindowId,
    request: super::ContextMenuRequest,
) -> Result<(), super::ContextMenuError> {
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
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions()
        .ok_or(Ui4ProgrammaticSelectionError::OutputUnavailable)?;
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
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions()
        .ok_or(Ui4ProgrammaticSelectionError::OutputUnavailable)?;
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

fn enqueue_owner_event(owner: WindowOwner, event: Ui4InputEvent) {
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
        Ui4InputEvent::Focus(_) => "focus",
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

/// Resolve only the slot-4 presentation. Raw cursor state, hit testing, and
/// every application-delivered pointer coordinate stay untouched.
fn cursor_visual_presentation(
    route: &CursorRoute,
) -> (u32, u32, super::Ui4CursorIcon, Option<Ui4VisualRect>) {
    let Some((key, icon, step)) = super::cursor_presentation_for_source(route.source) else {
        return (route.x, route.y, super::Ui4CursorIcon::Default, None);
    };
    if icon != super::Ui4CursorIcon::CellOutline {
        return (route.x, route.y, icon, None);
    }
    let Some(step) = step else {
        return (route.x, route.y, icon, None);
    };
    let Some(window) = super::window_broker::window_snapshot(key.owner, key.window) else {
        return (route.x, route.y, icon, None);
    };
    let placement = window.presentation_placement;
    if !placement_contains(placement, route.x, route.y) {
        return (route.x, route.y, icon, None);
    }
    let (Ok(local_x), Ok(local_y)) = (
        u32::try_from(signed_local(route.x, placement.x)),
        u32::try_from(signed_local(route.y, placement.y)),
    ) else {
        return (route.x, route.y, icon, None);
    };
    let Some((left, top, width, height)) = step.cell_bounds_local(local_x, local_y) else {
        return (route.x, route.y, icon, None);
    };
    let Some(cell) = stepped_cell_rect(placement, left, top, width, height) else {
        return (route.x, route.y, icon, None);
    };
    (cell.x, cell.y, icon, Some(cell))
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

pub(super) fn dock_zones(screen_width: u32, screen_height: u32) -> [Ui4DockZone; 7] {
    let physical_reference =
        crate::intel::physical_extent_pixels(DOCK_REFERENCE_WIDTH_MM, DOCK_REFERENCE_HEIGHT_MM);
    dock_zones_with_reference(screen_width, screen_height, physical_reference)
}

fn dock_zones_with_reference(
    screen_width: u32,
    screen_height: u32,
    physical_reference: Option<(u32, u32)>,
) -> [Ui4DockZone; 7] {
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
        WindowDockTarget::Maximize => {
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
        WindowDockTarget::BottomLeft | WindowDockTarget::BottomRight => {
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
        .filter(|window| window.interaction.hit_testable)
        .filter(|window| placement_contains(window.presentation_placement, x, y))
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
        Ui4CursorSource, Ui4InputEvent, Ui4PointerEvent, WindowId, coalesce_owner_state_sample,
        dock_target_at_in_zones, dock_zone_column_span, dock_zone_contains, dock_zone_metrics,
        dock_zone_row_span, dock_zones_with_reference, owner_event_is_state_sample,
        resize_epoch_is_newer,
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
