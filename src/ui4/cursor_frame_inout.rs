//! Cursor/frame input-output handoff for UI4.
//!
//! # UI4 selection contract
//!
//! Every cursor source owns **exactly zero or one selected frame**. UI4 also
//! retains the most recently selected frame as its application input focus.
//! A click which changes either association is an absorb-select: its down,
//! motion, and up transitions are never delivered to an application.
//!
//! Keyboard hooks are global and run before selected-frame routing. A hook may
//! consume an event so it never reaches UI4, or explicitly pass it through.
//!
//! Cursor identity is not singular: the kernel retains up to N independent
//! cursor sources. Each frame owns one fallback cursor plus optional overrides
//! for individual sources, and every cursor/frame association remains present
//! so slot 4 can paint colored ownership segments on several frames at once.

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

/// Kernel-provided cursor presentations. `AppOwned` is the escape hatch for a
/// frame such as Blueprint Tactics which already paints a cursor into its own
/// pixels.
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
    /// A cell-sized outline rendered by the slot-4 software-cursor plane.
    CellOutline = 6,
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
            6 => Some(Self::CellOutline),
            _ => None,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorFrameKey {
    pub(crate) owner: WindowOwner,
    pub(crate) window: WindowId,
}

/// Static grid spacing for a frame-scoped software cursor. Origins are
/// frame-local pixels and cell advances are 1/1024 pixel units, preserving
/// fractional terminal glyph advances. Application input coordinates are not
/// modified by this presentation policy.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CursorStep {
    pub(crate) origin_x: u32,
    pub(crate) origin_y: u32,
    pub(crate) cell_width_subpx: u32,
    pub(crate) cell_height_subpx: u32,
}

impl Ui4CursorStep {
    pub(crate) const fn is_valid(self) -> bool {
        self.cell_width_subpx != 0 && self.cell_height_subpx != 0
    }

    pub(crate) fn cell_bounds_local(self, x: u32, y: u32) -> Option<(u32, u32, u32, u32)> {
        let (left, right) = cell_axis_bounds(x, self.origin_x, self.cell_width_subpx)?;
        let (top, bottom) = cell_axis_bounds(y, self.origin_y, self.cell_height_subpx)?;
        Some((left, top, right.saturating_sub(left), bottom.saturating_sub(top)))
    }
}

fn cell_axis_bounds(value: u32, origin: u32, step_subpx: u32) -> Option<(u32, u32)> {
    const SUBPIXELS_PER_PIXEL: u64 = 1_024;
    if value < origin || step_subpx == 0 {
        return None;
    }
    let offset_subpx = u64::from(value.saturating_sub(origin)).saturating_mul(SUBPIXELS_PER_PIXEL);
    let cell = offset_subpx / u64::from(step_subpx);
    let origin_subpx = u64::from(origin).saturating_mul(SUBPIXELS_PER_PIXEL);
    let left_subpx = origin_subpx.saturating_add(cell.saturating_mul(u64::from(step_subpx)));
    let right_subpx = left_subpx.saturating_add(u64::from(step_subpx));
    let round = |subpx: u64| subpx.saturating_add(SUBPIXELS_PER_PIXEL / 2) / SUBPIXELS_PER_PIXEL;
    let left = u32::try_from(round(left_subpx)).ok()?;
    let right = u32::try_from(round(right_subpx)).ok()?;
    (right > left).then_some((left, right))
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
    cursor_step: Option<Ui4CursorStep>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct SelectingCursor {
    source: Ui4CursorSource,
    selected: CursorFrameKey,
    color: crate::graphics::primitives::Rgba8,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct SelectionChange {
    pub(crate) previous: Option<CursorFrameKey>,
    pub(crate) selected: Option<CursorFrameKey>,
    /// The calling cursor changed its own selected-frame association.
    pub(crate) changed: bool,
    /// The most recently selected application input-focus frame changed.
    pub(crate) focus_changed: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct CursorSelectionStrip {
    pub(crate) x: u32,
    pub(crate) y: u32,
    pub(crate) width: u32,
    pub(crate) color: crate::graphics::primitives::Rgba8,
}

struct GroupedCursorSelectionStrip {
    key: CursorFrameKey,
    colors: Vec<crate::graphics::primitives::Rgba8, MAX_CURSOR_SOURCES>,
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
            frame.cursor_step = None;
            return Ok(());
        }
        self.frames
            .push(FrameCursorState {
                key,
                session,
                fallback: Ui4CursorIcon::Default,
                overrides: Vec::new(),
                cursor_step: None,
            })
            .map_err(|_| CursorFrameError::Capacity)
    }

    fn frame_closed(&mut self, key: CursorFrameKey) -> bool {
        let previous_len = self.frames.len();
        self.frames.retain(|frame| frame.key != key);
        let mut changed = self.frames.len() != previous_len;
        let previous_selectors = self.selecting_cursors.len();
        self.selecting_cursors
            .retain(|cursor| cursor.selected != key);
        changed |= self.selecting_cursors.len() != previous_selectors;
        if self.selected == Some(key) {
            self.selected = None;
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
        let frames = &self.frames;
        let previous_selectors = self.selecting_cursors.len();
        self.selecting_cursors
            .retain(|cursor| frames.iter().any(|frame| frame.key == cursor.selected));
        changed |= self.selecting_cursors.len() != previous_selectors;
        if selected_was_closed {
            self.selected = None;
            changed = true;
        }
        changed
    }

    fn owner_closed(&mut self, owner: WindowOwner) -> bool {
        let previous_len = self.frames.len();
        self.frames.retain(|frame| frame.key.owner != owner);
        let mut changed = self.frames.len() != previous_len;
        let previous_selectors = self.selecting_cursors.len();
        self.selecting_cursors
            .retain(|cursor| cursor.selected.owner != owner);
        changed |= self.selecting_cursors.len() != previous_selectors;
        if self
            .selected
            .is_some_and(|selected| selected.owner == owner)
        {
            self.selected = None;
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
        let focus_changed = previous != selected;
        if focus_changed {
            self.selected = selected;
        }
        let source_previous = self.selected_frame_for_source(source);
        let changed = source_previous != selected;
        let mut visual_changed = focus_changed || changed;
        match selected {
            Some(selected) => {
                if let Some(cursor) = self
                    .selecting_cursors
                    .iter_mut()
                    .find(|cursor| cursor.source == source)
                {
                    visual_changed |= cursor.color != color;
                    cursor.selected = selected;
                    cursor.color = color;
                } else {
                    visual_changed |= self
                        .selecting_cursors
                        .push(SelectingCursor {
                            source,
                            selected,
                            color,
                        })
                        .is_ok();
                }
            }
            None => {
                self.selecting_cursors
                    .retain(|cursor| cursor.source != source);
            }
        }
        (
            SelectionChange {
                previous,
                selected,
                changed,
                focus_changed,
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
        Ok(changed
            && self
                .selecting_cursors
                .iter()
                .any(|cursor| cursor.selected == key))
    }

    fn set_cursor_step(
        &mut self,
        key: CursorFrameKey,
        step: Option<Ui4CursorStep>,
    ) -> Result<bool, CursorFrameError> {
        let frame = self
            .frames
            .iter_mut()
            .find(|frame| frame.key == key)
            .ok_or(CursorFrameError::NotFound)?;
        let changed = frame.cursor_step != step;
        frame.cursor_step = step;
        Ok(changed
            && self
                .selecting_cursors
                .iter()
                .any(|cursor| cursor.selected == key))
    }

    #[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
    fn cursor_icon(&self, key: CursorFrameKey, source: Ui4CursorSource) -> Ui4CursorIcon {
        if self.selected_frame_for_source(source) != Some(key) {
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
        self.selected_frame_for_source(source)
            .is_some_and(|selected| self.selected == Some(selected))
    }

    fn selected_frame_for_source(&self, source: Ui4CursorSource) -> Option<CursorFrameKey> {
        self.selecting_cursors
            .iter()
            .find(|cursor| cursor.source == source)
            .map(|cursor| cursor.selected)
    }

    fn cursor_presentation_for_source(
        &self,
        source: Ui4CursorSource,
    ) -> Option<(CursorFrameKey, Ui4CursorIcon, Option<Ui4CursorStep>)> {
        let key = self.selected_frame_for_source(source)?;
        let frame = self.frames.iter().find(|frame| frame.key == key)?;
        let icon = frame
            .overrides
            .iter()
            .find(|cursor| cursor.source == source)
            .map(|cursor| cursor.icon)
            .unwrap_or(frame.fallback);
        Some((key, icon, frame.cursor_step))
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

pub(crate) fn selected_frame_for_source(source: Ui4CursorSource) -> Option<CursorFrameKey> {
    CURSOR_FRAME_RIG.lock().selected_frame_for_source(source)
}

/// Resolve the selected frame's cursor icon and optional presentation-only
/// stepping policy for one independent cursor source.
pub(crate) fn cursor_presentation_for_source(
    source: Ui4CursorSource,
) -> Option<(CursorFrameKey, Ui4CursorIcon, Option<Ui4CursorStep>)> {
    CURSOR_FRAME_RIG
        .lock()
        .cursor_presentation_for_source(source)
}

pub(super) fn cursor_retired(source: Ui4CursorSource) {
    if CURSOR_FRAME_RIG.lock().cursor_retired(source) {
        signal_visual_change();
    }
}

#[expect(dead_code, reason = "baseline archived in tools/warnings_last")]
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

/// Configure or clear a selected frame's software-cursor presentation grid.
pub(crate) fn set_window_cursor_step(
    owner: WindowOwner,
    window: WindowId,
    step: Option<Ui4CursorStep>,
) -> Result<(), CursorFrameError> {
    let selected_visual_changed = CURSOR_FRAME_RIG
        .lock()
        .set_cursor_step(CursorFrameKey::new(owner, window), step)?;
    if selected_visual_changed {
        signal_visual_change();
    }
    Ok(())
}

pub(crate) fn selection_strips(
    output: OutputId,
    screen_width: u32,
    screen_height: u32,
) -> alloc::vec::Vec<CursorSelectionStrip> {
    let selectors = CURSOR_FRAME_RIG.lock().selecting_cursors.clone();
    let windows = super::window_broker::visible_windows_for_output(output);
    let mut grouped: Vec<GroupedCursorSelectionStrip, MAX_CURSOR_SOURCES> = Vec::new();
    for selector in selectors {
        if let Some(strip) = grouped
            .iter_mut()
            .find(|strip| strip.key == selector.selected)
        {
            let _ = strip.colors.push(selector.color);
            continue;
        }
        let mut colors = Vec::new();
        let _ = colors.push(selector.color);
        let _ = grouped.push(GroupedCursorSelectionStrip {
            key: selector.selected,
            colors,
        });
    }

    let mut output_strips = alloc::vec::Vec::new();
    for strip in grouped {
        let Some(window) = windows
            .iter()
            .find(|window| CursorFrameKey::new(window.owner, window.id) == strip.key)
        else {
            continue;
        };
        let placement = window.presentation_placement;
        if window.output != output
            || window.state != WindowState::Ready
            || !placement.visible
            || placement.opacity == 0
            || placement.y <= 0
        {
            continue;
        }
        let top = placement.y - 1;
        if top < 0 || top as u32 >= screen_height {
            continue;
        }
        let left = i64::from(placement.x).clamp(0, i64::from(screen_width));
        let right = i64::from(placement.x)
            .saturating_add(i64::from(placement.width))
            .clamp(0, i64::from(screen_width));
        if right <= left {
            continue;
        }
        let left = left as u32;
        let right = right as u32;
        let strip_y = top as u32;
        let target_stack = window_stack_key(*window);
        let mut occluders = windows
            .iter()
            .copied()
            .filter(|candidate| candidate.id != window.id || candidate.owner != window.owner)
            .filter(|candidate| window_stack_key(*candidate) > target_stack)
            .filter_map(|candidate| {
                let candidate = candidate.presentation_placement;
                if !candidate.visible || candidate.opacity == 0 {
                    return None;
                }
                let candidate_top = i64::from(candidate.y);
                let candidate_bottom = candidate_top.saturating_add(i64::from(candidate.height));
                let strip_y = i64::from(strip_y);
                if strip_y < candidate_top || strip_y >= candidate_bottom {
                    return None;
                }
                let occluder_left = i64::from(candidate.x).clamp(i64::from(left), i64::from(right));
                let occluder_right = i64::from(candidate.x)
                    .saturating_add(i64::from(candidate.width))
                    .clamp(i64::from(left), i64::from(right));
                (occluder_right > occluder_left)
                    .then_some((occluder_left as u32, occluder_right as u32))
            })
            .collect::<alloc::vec::Vec<_>>();
        merge_horizontal_occluders(&mut occluders);

        let color_count = strip.colors.len() as u64;
        let strip_width = u64::from(right - left);
        for (index, color) in strip.colors.iter().copied().enumerate() {
            let color_left = u64::from(left)
                .saturating_add(strip_width.saturating_mul(index as u64) / color_count)
                as u32;
            let color_right = u64::from(left)
                .saturating_add(strip_width.saturating_mul(index as u64 + 1) / color_count)
                as u32;
            for (visible_left, visible_right) in
                visible_horizontal_spans(color_left, color_right, &occluders)
            {
                output_strips.push(CursorSelectionStrip {
                    x: visible_left,
                    y: strip_y,
                    width: visible_right - visible_left,
                    color,
                });
            }
        }
    }
    output_strips
}

fn window_stack_key(window: super::WindowSnapshot) -> (usize, i32, WindowId) {
    (window.plane.slot(), window.placement.z, window.id)
}

fn merge_horizontal_occluders(occluders: &mut alloc::vec::Vec<(u32, u32)>) {
    occluders.sort_unstable();
    let mut write = 0usize;
    for read in 0..occluders.len() {
        let (left, right) = occluders[read];
        if write != 0 && left <= occluders[write - 1].1 {
            occluders[write - 1].1 = occluders[write - 1].1.max(right);
            continue;
        }
        occluders[write] = (left, right);
        write += 1;
    }
    occluders.truncate(write);
}

fn visible_horizontal_spans(
    left: u32,
    right: u32,
    occluders: &[(u32, u32)],
) -> alloc::vec::Vec<(u32, u32)> {
    let mut spans = alloc::vec::Vec::new();
    let mut cursor = left;
    for &(occluder_left, occluder_right) in occluders {
        if occluder_right <= cursor {
            continue;
        }
        if occluder_left >= right {
            break;
        }
        if occluder_left > cursor {
            spans.push((cursor, occluder_left.min(right)));
        }
        cursor = cursor.max(occluder_right);
        if cursor >= right {
            return spans;
        }
    }
    if cursor < right {
        spans.push((cursor, right));
    }
    spans
}

pub(super) fn frame_visual_changed(owner: WindowOwner, window: WindowId) {
    let key = CursorFrameKey::new(owner, window);
    if CURSOR_FRAME_RIG
        .lock()
        .selecting_cursors
        .iter()
        .any(|cursor| cursor.selected == key)
    {
        signal_visual_change();
    }
}

/// Wake slot 4 when application geometry or hardware-plane ownership changes
/// can expose or cover part of a selected frame's one-pixel strip.
pub(super) fn selection_strip_stack_changed() {
    if !CURSOR_FRAME_RIG.lock().selecting_cursors.is_empty() {
        signal_visual_change();
    }
}

fn signal_visual_change() {
    super::input_broker::notify_slot4_visual_change();
}

pub(crate) fn cursor_color(source: Ui4CursorSource) -> crate::graphics::primitives::Rgba8 {
    // A virtual device may deliberately carry a bespoke presentation color.
    // Otherwise the VLayer InputCombo owns the stable visual identity shared
    // by all independently clocked devices in that collection.
    if source.hid_kind == crate::r::cursor::HID_KIND_VIRTUAL_CURSOR
        && let Some(color) =
            crate::r::services::mouse_motion_service::cursor_visual_color(source.slot_id)
    {
        return color;
    }
    if let Some(color_id) = crate::usb2::hid::hut::combo_color_for_cursor(
        source.controller_id,
        source.slot_id,
        source.ep_target,
        source.hid_kind == crate::r::cursor::HID_KIND_TABLET,
    ) {
        return input_combo_color_rgba(color_id);
    }

    let hash = source.controller_id.wrapping_mul(0x9E37_79B9)
        ^ source.slot_id.rotate_left(11)
        ^ source.ep_target.rotate_left(19)
        ^ u32::from(source.hid_kind);
    input_combo_color_rgba((hash % u32::from(v::vinput::InputComboColor::COUNT)) as u8)
}

fn input_combo_color_rgba(color_id: u8) -> crate::graphics::primitives::Rgba8 {
    let [r, g, b, a] = v::vinput::InputComboColor::from_index(color_id).rgba();
    crate::graphics::primitives::Rgba8::new(r, g, b, a)
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
    fn cursor_selections_survive_another_cursor_changing_focus() {
        let mut rig = CursorFrameRig::new();
        let first = CursorFrameKey::new(WindowOwner::KernelApp(1), WindowId::from_raw(1).unwrap());
        let second = CursorFrameKey::new(WindowOwner::KernelApp(2), WindowId::from_raw(2).unwrap());
        let session = WindowSessionId::from_raw(1).unwrap();
        rig.frame_opened(first, session).unwrap();
        rig.frame_opened(second, session).unwrap();

        let (change, _) = rig.select(Some(first), source(1), Rgba8::new(1, 2, 3, 255));
        assert!(change.changed);
        let (change, _) = rig.select(Some(first), source(2), Rgba8::new(4, 5, 6, 255));
        assert!(change.changed);
        assert!(!change.focus_changed);
        assert_eq!(rig.selecting_cursors.len(), 2);

        let (change, _) = rig.select(Some(second), source(3), Rgba8::new(7, 8, 9, 255));
        assert_eq!(change.previous, Some(first));
        assert_eq!(rig.selected, Some(second));
        assert_eq!(rig.selecting_cursors.len(), 3);
        assert_eq!(rig.selected_frame_for_source(source(1)), Some(first));
        assert_eq!(rig.selected_frame_for_source(source(2)), Some(first));
        assert_eq!(rig.selected_frame_for_source(source(3)), Some(second));
        assert!(!rig.source_selected(source(1)));
        assert!(rig.source_selected(source(3)));
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
        rig.select(Some(frame), source(2), Rgba8::new(4, 5, 6, 255));

        assert_eq!(rig.cursor_icon(frame, source(1)), Ui4CursorIcon::Loading);
        assert_eq!(rig.cursor_icon(frame, source(2)), Ui4CursorIcon::ResizeHorizontal);
    }

    #[test]
    fn cursor_step_is_selected_frame_scoped_and_presentation_only() {
        let mut rig = CursorFrameRig::new();
        let frame = CursorFrameKey::new(WindowOwner::KernelApp(1), WindowId::from_raw(1).unwrap());
        let session = WindowSessionId::from_raw(1).unwrap();
        let step = Ui4CursorStep {
            origin_x: 12,
            origin_y: 8,
            cell_width_subpx: 9 * 1_024,
            cell_height_subpx: 16 * 1_024,
        };
        rig.frame_opened(frame, session).unwrap();
        rig.set_cursor_step(frame, Some(step)).unwrap();
        rig.select(Some(frame), source(1), Rgba8::new(1, 2, 3, 255));

        rig.set_cursor(frame, None, Ui4CursorIcon::CellOutline)
            .unwrap();
        assert_eq!(
            rig.cursor_presentation_for_source(source(1)),
            Some((frame, Ui4CursorIcon::CellOutline, Some(step)))
        );
        assert_eq!(step.cell_bounds_local(11, 7), None);
        assert_eq!(step.cell_bounds_local(28, 41), Some((21, 40, 9, 16)));
        let fractional_step = Ui4CursorStep {
            cell_width_subpx: 14_746,
            cell_height_subpx: 26 * 1_024,
            ..step
        };
        assert_eq!(fractional_step.cell_bounds_local(60, 40), Some((55, 34, 15, 26)));

        rig.set_cursor(frame, None, Ui4CursorIcon::Default).unwrap();
        assert_eq!(
            rig.cursor_presentation_for_source(source(1)),
            Some((frame, Ui4CursorIcon::Default, Some(step)))
        );
    }

    #[test]
    fn strip_visibility_can_keep_left_right_both_or_nothing() {
        assert_eq!(visible_horizontal_spans(10, 90, &[]), [(10, 90)]);
        assert_eq!(visible_horizontal_spans(10, 90, &[(0, 25)]), [(25, 90)]);
        assert_eq!(visible_horizontal_spans(10, 90, &[(75, 100)]), [(10, 75)]);
        assert_eq!(visible_horizontal_spans(10, 90, &[(35, 55)]), [(10, 35), (55, 90)]);
        assert!(visible_horizontal_spans(10, 90, &[(0, 100)]).is_empty());
    }

    #[test]
    fn overlapping_occluders_merge_before_strip_clipping() {
        let mut occluders = alloc::vec![(50, 70), (20, 40), (35, 60), (80, 90)];
        merge_horizontal_occluders(&mut occluders);
        assert_eq!(occluders, [(20, 70), (80, 90)]);
        assert_eq!(visible_horizontal_spans(10, 100, &occluders), [(10, 20), (70, 80), (90, 100)]);
    }
}
