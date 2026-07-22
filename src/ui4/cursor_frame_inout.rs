//! Cursor/frame input-output handoff for UI4.
//!
//! # UI4 selection contract
//!
//! Across every UI4 screen, output, and hardware plane there is **exactly zero
//! or one selected frame**. Only that selected frame may receive application
//! keyboard or pointer input, and only that frame may activate cursor
//! overrides. A click which changes the selected frame is an absorb-select:
//! its down, motion, and up transitions are never delivered to an application.
//!
//! Keyboard hooks are global and run before selected-frame routing. A hook may
//! consume an event so it never reaches UI4, or explicitly pass it through.
//!
//! Cursor identity is not singular: the kernel retains up to N independent
//! cursor sources. Each frame owns one fallback cursor plus optional overrides
//! for individual sources, and the selected frame remembers every cursor which
//! selected it so slot 4 can paint their colored ownership segments.

use heapless::Vec;
use spin::Mutex;

use super::{OutputId, WindowId, WindowOwner, WindowSessionId, WindowState};

const MAX_TRACKED_FRAMES: usize = super::window_broker::MAX_WINDOWS;
const MAX_CURSOR_SOURCES: usize = 32;
const MAX_GLOBAL_KEYBOARD_HOOKS: usize = 16;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CursorSource {
    pub(crate) controller_id: u32,
    pub(crate) slot_id: u32,
    pub(crate) ep_target: u32,
    pub(crate) hid_kind: u8,
}

impl Ui4CursorSource {
    pub(crate) fn from_event(event: crate::usb2::hid::TrueosHidCursorEvent) -> Self {
        Self {
            controller_id: event.controller_id,
            slot_id: event.slot_id,
            ep_target: event.ep_target,
            hid_kind: event.hid_kind,
        }
    }
}

/// Kernel-provided cursor sprites. `AppOwned` is the escape hatch for a frame
/// such as Blueprint Tactics which already paints a cursor into its own pixels.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum Ui4CursorIcon {
    #[default]
    Default = 0,
    Loading = 1,
    ResizeHorizontal = 2,
    ResizeVertical = 3,
    ResizeDiagonal = 4,
    AppOwned = 5,
}

impl Ui4CursorIcon {
    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            0 => Some(Self::Default),
            1 => Some(Self::Loading),
            2 => Some(Self::ResizeHorizontal),
            3 => Some(Self::ResizeVertical),
            4 => Some(Self::ResizeDiagonal),
            5 => Some(Self::AppOwned),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorFrameKey {
    pub(crate) owner: WindowOwner,
    pub(crate) window: WindowId,
}

impl CursorFrameKey {
    pub(crate) const fn new(owner: WindowOwner, window: WindowId) -> Self {
        Self { owner, window }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct CursorOverride {
    source: Ui4CursorSource,
    icon: Ui4CursorIcon,
}

struct FrameCursorState {
    key: CursorFrameKey,
    session: WindowSessionId,
    fallback: Ui4CursorIcon,
    overrides: Vec<CursorOverride, MAX_CURSOR_SOURCES>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SelectingCursor {
    source: Ui4CursorSource,
    color: crate::graphics::primitives::Rgba8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectionChange {
    pub(crate) previous: Option<CursorFrameKey>,
    pub(crate) selected: Option<CursorFrameKey>,
    pub(crate) changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorSelectionStrip {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) colors: Vec<crate::graphics::primitives::Rgba8, MAX_CURSOR_SOURCES>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum CursorFrameError {
    NotFound,
    Capacity,
}

struct CursorFrameRig {
    frames: Vec<FrameCursorState, MAX_TRACKED_FRAMES>,
    selected: Option<CursorFrameKey>,
    selecting_cursors: Vec<SelectingCursor, MAX_CURSOR_SOURCES>,
}

impl CursorFrameRig {
    const fn new() -> Self {
        Self {
            frames: Vec::new(),
            selected: None,
            selecting_cursors: Vec::new(),
        }
    }

    fn frame_opened(
        &mut self,
        key: CursorFrameKey,
        session: WindowSessionId,
    ) -> Result<(), CursorFrameError> {
        if let Some(frame) = self.frames.iter_mut().find(|frame| frame.key == key) {
            frame.session = session;
            frame.fallback = Ui4CursorIcon::Default;
            frame.overrides.clear();
            return Ok(());
        }
        self.frames
            .push(FrameCursorState {
                key,
                session,
                fallback: Ui4CursorIcon::Default,
                overrides: Vec::new(),
            })
            .map_err(|_| CursorFrameError::Capacity)
    }

    fn frame_closed(&mut self, key: CursorFrameKey) -> bool {
        let previous_len = self.frames.len();
        self.frames.retain(|frame| frame.key != key);
        let mut changed = self.frames.len() != previous_len;
        if self.selected == Some(key) {
            self.selected = None;
            self.selecting_cursors.clear();
            changed = true;
        }
        changed
    }

    fn session_closed(&mut self, owner: WindowOwner, session: WindowSessionId) -> bool {
        let selected_was_closed = self.selected.is_some_and(|selected| {
            self.frames.iter().any(|frame| {
                frame.key == selected && frame.key.owner == owner && frame.session == session
            })
        });
        let previous_len = self.frames.len();
        self.frames
            .retain(|frame| frame.key.owner != owner || frame.session != session);
        let mut changed = self.frames.len() != previous_len;
        if selected_was_closed {
            self.selected = None;
            self.selecting_cursors.clear();
            changed = true;
        }
        changed
    }

    fn owner_closed(&mut self, owner: WindowOwner) -> bool {
        let previous_len = self.frames.len();
        self.frames.retain(|frame| frame.key.owner != owner);
        let mut changed = self.frames.len() != previous_len;
        if self
            .selected
            .is_some_and(|selected| selected.owner == owner)
        {
            self.selected = None;
            self.selecting_cursors.clear();
            changed = true;
        }
        changed
    }

    fn select(
        &mut self,
        selected: Option<CursorFrameKey>,
        source: Ui4CursorSource,
        color: crate::graphics::primitives::Rgba8,
    ) -> (SelectionChange, bool) {
        let previous = self.selected;
        let changed = previous != selected;
        let mut visual_changed = changed;
        if changed {
            self.selected = selected;
            self.selecting_cursors.clear();
        }
        if selected.is_some()
            && !self
                .selecting_cursors
                .iter()
                .any(|cursor| cursor.source == source)
        {
            visual_changed |= self
                .selecting_cursors
                .push(SelectingCursor { source, color })
                .is_ok();
        }
        (
            SelectionChange {
                previous,
                selected,
                changed,
            },
            visual_changed,
        )
    }

    fn set_cursor(
        &mut self,
        key: CursorFrameKey,
        source: Option<Ui4CursorSource>,
        icon: Ui4CursorIcon,
    ) -> Result<bool, CursorFrameError> {
        let frame = self
            .frames
            .iter_mut()
            .find(|frame| frame.key == key)
            .ok_or(CursorFrameError::NotFound)?;
        let changed = if let Some(source) = source {
            if let Some(cursor) = frame
                .overrides
                .iter_mut()
                .find(|cursor| cursor.source == source)
            {
                let changed = cursor.icon != icon;
                cursor.icon = icon;
                changed
            } else {
                frame
                    .overrides
                    .push(CursorOverride { source, icon })
                    .map_err(|_| CursorFrameError::Capacity)?;
                true
            }
        } else {
            let changed = frame.fallback != icon;
            frame.fallback = icon;
            changed
        };
        Ok(changed && self.selected == Some(key))
    }

    fn cursor_icon(&self, key: CursorFrameKey, source: Ui4CursorSource) -> Ui4CursorIcon {
        if self.selected != Some(key) {
            return Ui4CursorIcon::Default;
        }
        let Some(frame) = self.frames.iter().find(|frame| frame.key == key) else {
            return Ui4CursorIcon::Default;
        };
        frame
            .overrides
            .iter()
            .find(|cursor| cursor.source == source)
            .map(|cursor| cursor.icon)
            .unwrap_or(frame.fallback)
    }

    fn source_selected(&self, source: Ui4CursorSource) -> bool {
        self.selecting_cursors
            .iter()
            .any(|cursor| cursor.source == source)
    }

    fn cursor_retired(&mut self, source: Ui4CursorSource) -> bool {
        let previous_len = self.selecting_cursors.len();
        self.selecting_cursors
            .retain(|cursor| cursor.source != source);
        self.selecting_cursors.len() != previous_len
    }
}

static CURSOR_FRAME_RIG: Mutex<CursorFrameRig> = Mutex::new(CursorFrameRig::new());

pub(super) fn frame_opened(
    owner: WindowOwner,
    session: WindowSessionId,
    window: WindowId,
) -> Result<(), CursorFrameError> {
    CURSOR_FRAME_RIG
        .lock()
        .frame_opened(CursorFrameKey::new(owner, window), session)
}

pub(super) fn frame_closed(owner: WindowOwner, window: WindowId) {
    if CURSOR_FRAME_RIG
        .lock()
        .frame_closed(CursorFrameKey::new(owner, window))
    {
        signal_visual_change();
    }
}

pub(super) fn session_closed(owner: WindowOwner, session: WindowSessionId) {
    if CURSOR_FRAME_RIG.lock().session_closed(owner, session) {
        signal_visual_change();
    }
}

pub(super) fn owner_closed(owner: WindowOwner) {
    if CURSOR_FRAME_RIG.lock().owner_closed(owner) {
        signal_visual_change();
    }
}

pub(crate) fn selected_frame() -> Option<CursorFrameKey> {
    CURSOR_FRAME_RIG.lock().selected
}

pub(crate) fn select_frame(
    selected: Option<CursorFrameKey>,
    source: Ui4CursorSource,
    color: crate::graphics::primitives::Rgba8,
) -> SelectionChange {
    let (change, visual_changed) = CURSOR_FRAME_RIG.lock().select(selected, source, color);
    if visual_changed {
        signal_visual_change();
    }
    change
}

pub(crate) fn source_selected(source: Ui4CursorSource) -> bool {
    CURSOR_FRAME_RIG.lock().source_selected(source)
}

pub(super) fn cursor_retired(source: Ui4CursorSource) {
    if CURSOR_FRAME_RIG.lock().cursor_retired(source) {
        signal_visual_change();
    }
}

pub(crate) fn cursor_icon_for(key: CursorFrameKey, source: Ui4CursorSource) -> Ui4CursorIcon {
    CURSOR_FRAME_RIG.lock().cursor_icon(key, source)
}

pub(crate) fn set_window_custom_cursor(
    owner: WindowOwner,
    window: WindowId,
    enabled: bool,
) -> Result<(), CursorFrameError> {
    set_window_cursor_icon(
        owner,
        window,
        None,
        if enabled {
            Ui4CursorIcon::AppOwned
        } else {
            Ui4CursorIcon::Default
        },
    )
}

pub(crate) fn set_window_cursor_icon(
    owner: WindowOwner,
    window: WindowId,
    source: Option<Ui4CursorSource>,
    icon: Ui4CursorIcon,
) -> Result<(), CursorFrameError> {
    let selected_visual_changed =
        CURSOR_FRAME_RIG
            .lock()
            .set_cursor(CursorFrameKey::new(owner, window), source, icon)?;
    if selected_visual_changed {
        signal_visual_change();
    }
    Ok(())
}

pub(crate) fn selection_strip(
    output: OutputId,
    screen_width: u32,
    screen_height: u32,
) -> Option<CursorSelectionStrip> {
    let (selected, colors) = {
        let rig = CURSOR_FRAME_RIG.lock();
        let selected = rig.selected?;
        let mut colors = Vec::new();
        for cursor in &rig.selecting_cursors {
            let _ = colors.push(cursor.color);
        }
        (selected, colors)
    };
    if colors.is_empty() {
        return None;
    }
    let window = super::window_broker::window_snapshot(selected.owner, selected.window)?;
    if window.output != output
        || window.state != WindowState::Ready
        || !window.placement.visible
        || window.placement.y <= 0
    {
        return None;
    }
    let top = window.placement.y - 1;
    if top < 0 || top as u32 >= screen_height {
        return None;
    }
    let left = i64::from(window.placement.x).clamp(0, i64::from(screen_width));
    let right = i64::from(window.placement.x)
        .saturating_add(i64::from(window.placement.width))
        .clamp(0, i64::from(screen_width));
    if right <= left {
        return None;
    }
    Some(CursorSelectionStrip {
        x: left as u32,
        y: top as u32,
        width: (right - left) as u32,
        colors,
    })
}

pub(super) fn frame_visual_changed(owner: WindowOwner, window: WindowId) {
    if selected_frame() == Some(CursorFrameKey::new(owner, window)) {
        signal_visual_change();
    }
}

fn signal_visual_change() {
    super::input_broker::notify_slot4_visual_change();
}

pub(crate) fn cursor_color(source: Ui4CursorSource) -> crate::graphics::primitives::Rgba8 {
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

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GlobalKeyboardDisposition {
    Consume,
    PassThrough,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GlobalKeyboardHookId(u32);

#[derive(Copy, Clone)]
struct GlobalKeyboardHook {
    id: GlobalKeyboardHookId,
    priority: u8,
    callback: fn(&crate::r::keyboard::TrueosKeyboardOutputEvent) -> GlobalKeyboardDisposition,
}

struct GlobalKeyboardRegistry {
    next_id: u32,
    hooks: Vec<GlobalKeyboardHook, MAX_GLOBAL_KEYBOARD_HOOKS>,
}

impl GlobalKeyboardRegistry {
    const fn new() -> Self {
        Self {
            next_id: 0,
            hooks: Vec::new(),
        }
    }
}

static GLOBAL_KEYBOARD_REGISTRY: Mutex<GlobalKeyboardRegistry> =
    Mutex::new(GlobalKeyboardRegistry::new());

#[allow(dead_code)]
pub(crate) fn register_global_keyboard_hook(
    priority: u8,
    callback: fn(&crate::r::keyboard::TrueosKeyboardOutputEvent) -> GlobalKeyboardDisposition,
) -> Result<GlobalKeyboardHookId, CursorFrameError> {
    let mut registry = GLOBAL_KEYBOARD_REGISTRY.lock();
    registry.next_id = registry.next_id.wrapping_add(1).max(1);
    let id = GlobalKeyboardHookId(registry.next_id);
    registry
        .hooks
        .push(GlobalKeyboardHook {
            id,
            priority,
            callback,
        })
        .map_err(|_| CursorFrameError::Capacity)?;
    registry
        .hooks
        .as_mut_slice()
        .sort_unstable_by_key(|hook| core::cmp::Reverse(hook.priority));
    Ok(id)
}

#[allow(dead_code)]
pub(crate) fn unregister_global_keyboard_hook(id: GlobalKeyboardHookId) -> bool {
    let mut registry = GLOBAL_KEYBOARD_REGISTRY.lock();
    let Some(index) = registry.hooks.iter().position(|hook| hook.id == id) else {
        return false;
    };
    registry.hooks.remove(index);
    true
}

/// Returns true only when every global hook explicitly permits UI4 delivery.
pub(crate) fn global_keyboard_passes(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> bool {
    let hooks = GLOBAL_KEYBOARD_REGISTRY.lock().hooks.clone();
    hooks
        .iter()
        .all(|hook| matches!((hook.callback)(event), GlobalKeyboardDisposition::PassThrough))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graphics::primitives::Rgba8;

    fn source(slot_id: u32) -> Ui4CursorSource {
        Ui4CursorSource {
            controller_id: 1,
            slot_id,
            ep_target: 3,
            hid_kind: 1,
        }
    }

    #[test]
    fn selection_is_global_while_selectors_are_plural() {
        let mut rig = CursorFrameRig::new();
        let first = CursorFrameKey::new(WindowOwner::KernelApp(1), WindowId::from_raw(1).unwrap());
        let second = CursorFrameKey::new(WindowOwner::KernelApp(2), WindowId::from_raw(2).unwrap());
        let session = WindowSessionId::from_raw(1).unwrap();
        rig.frame_opened(first, session).unwrap();
        rig.frame_opened(second, session).unwrap();

        let (change, _) = rig.select(Some(first), source(1), Rgba8::new(1, 2, 3, 255));
        assert!(change.changed);
        let (change, _) = rig.select(Some(first), source(2), Rgba8::new(4, 5, 6, 255));
        assert!(!change.changed);
        assert_eq!(rig.selecting_cursors.len(), 2);

        let (change, _) = rig.select(Some(second), source(3), Rgba8::new(7, 8, 9, 255));
        assert_eq!(change.previous, Some(first));
        assert_eq!(rig.selected, Some(second));
        assert_eq!(rig.selecting_cursors.len(), 1);
    }

    #[test]
    fn cursor_overrides_are_frame_and_source_scoped() {
        let mut rig = CursorFrameRig::new();
        let frame = CursorFrameKey::new(WindowOwner::KernelApp(1), WindowId::from_raw(1).unwrap());
        let session = WindowSessionId::from_raw(1).unwrap();
        rig.frame_opened(frame, session).unwrap();
        rig.set_cursor(frame, None, Ui4CursorIcon::Loading).unwrap();
        rig.set_cursor(frame, Some(source(2)), Ui4CursorIcon::ResizeHorizontal)
            .unwrap();
        rig.select(Some(frame), source(1), Rgba8::new(1, 2, 3, 255));

        assert_eq!(rig.cursor_icon(frame, source(1)), Ui4CursorIcon::Loading);
        assert_eq!(rig.cursor_icon(frame, source(2)), Ui4CursorIcon::ResizeHorizontal);
    }
}
