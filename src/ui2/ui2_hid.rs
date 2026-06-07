use super::*;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use trueos_gfx_core::Rgba8;

static UI2_CURSOR_CAP_DROP_COUNT: AtomicU32 = AtomicU32::new(0);
const UI2_WINDOW_CURSOR_EVENT_CAP: usize = 256;
pub(crate) const UI2_FUN_CURSOR_ICONS_ENABLED: bool = true;
const UI2_CURSOR_SPIRIT_DEFAULTS: [char; 6] = ['🦋', '🦊', '🦎', '🦁', '🦄', '🐕'];
const UI2_CURSOR_SPIRIT_CHOICES: [char; 24] = [
    '🦋', '🦊', '🦎', '🦁', '🦄', '🐕', '🐈', '🐇', '🐢', '🐙', '🐳', '🐬', '🐘', '🦕', '🦖', '🦉',
    '🦜', '🦚', '🦩', '🐝', '🐞', '🦀', '🐌', '🐧',
];
static UI2_CURSOR_SPIRIT_OVERRIDES: Mutex<[char; UI2_CURSOR_CAP]> =
    Mutex::new(['\0'; UI2_CURSOR_CAP]);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui2CursorColor {
    Blue,
    Red,
    Green,
    Amber,
    Violet,
    Cyan,
}

impl Ui2CursorColor {
    #[inline]
    pub(crate) const fn from_visual_ordinal(ordinal: u32) -> Self {
        match ordinal % 6 {
            0 => Self::Blue,
            1 => Self::Red,
            2 => Self::Green,
            3 => Self::Amber,
            4 => Self::Violet,
            _ => Self::Cyan,
        }
    }

    #[inline]
    pub(crate) const fn from_slot_id(slot_id: u32) -> Self {
        Self::from_visual_ordinal(slot_id.saturating_sub(1))
    }

    #[inline]
    pub(crate) const fn from_cursor_id(cursor_id: u32) -> Self {
        Self::from_visual_ordinal(cursor_id.saturating_sub(1))
    }

    #[inline]
    pub(crate) const fn rgba(self) -> (u8, u8, u8, u8) {
        match self {
            Self::Blue => (0x3B, 0x82, 0xF6, 0xFF),
            Self::Red => (0xEF, 0x44, 0x44, 0xFF),
            Self::Green => (0x10, 0xB9, 0x81, 0xFF),
            Self::Amber => (0xF5, 0x9E, 0x0B, 0xFF),
            Self::Violet => (0x8B, 0x5C, 0xF6, 0xFF),
            Self::Cyan => (0x06, 0xB6, 0xD4, 0xFF),
        }
    }

    #[inline]
    pub(crate) const fn rgba8(self) -> Rgba8 {
        let (r, g, b, a) = self.rgba();
        Rgba8::new(r, g, b, a)
    }

    #[inline]
    pub(crate) const fn spirit_glyph(self) -> char {
        UI2_CURSOR_SPIRIT_DEFAULTS[self as usize]
    }
}

#[inline]
pub(crate) fn cursor_spirit_choices() -> &'static [char] {
    &UI2_CURSOR_SPIRIT_CHOICES
}

#[inline]
fn cursor_spirit_override(slot_id: u32) -> Option<char> {
    let idx = usize::try_from(slot_id.checked_sub(1)?).ok()?;
    if idx >= UI2_CURSOR_CAP {
        return None;
    }
    let ch = UI2_CURSOR_SPIRIT_OVERRIDES.lock()[idx];
    (ch != '\0').then_some(ch)
}

pub(crate) fn set_cursor_spirit_glyph(slot_id: u32, glyph: char) -> bool {
    let Some(idx) = slot_id
        .checked_sub(1)
        .and_then(|idx| usize::try_from(idx).ok())
    else {
        return false;
    };
    if idx >= UI2_CURSOR_CAP || !UI2_CURSOR_SPIRIT_CHOICES.contains(&glyph) {
        return false;
    }
    UI2_CURSOR_SPIRIT_OVERRIDES.lock()[idx] = glyph;
    true
}

#[inline]
pub(crate) fn cursor_color(slot_id: u32) -> (u8, u8, u8, u8) {
    let color = cursor_color_rgba8(slot_id);
    (color.r, color.g, color.b, color.a)
}

#[inline]
pub(crate) fn cursor_color_rgba8(slot_id: u32) -> Rgba8 {
    Ui2CursorColor::from_slot_id(slot_id).rgba8()
}

#[inline]
pub(crate) fn cursor_spirit_glyph(slot_id: u32) -> Option<char> {
    if !UI2_FUN_CURSOR_ICONS_ENABLED {
        return None;
    }
    cursor_spirit_override(slot_id)
        .or_else(|| Some(Ui2CursorColor::from_slot_id(slot_id).spirit_glyph()))
}

#[inline]
pub(crate) fn cursor_color_for_cursor_id(cursor_id: u32) -> (u8, u8, u8, u8) {
    Ui2CursorColor::from_cursor_id(cursor_id).rgba()
}

#[inline]
pub(crate) fn cursor_color_rgba8_for_cursor_id(cursor_id: u32) -> Rgba8 {
    let (r, g, b, a) = cursor_color_for_cursor_id(cursor_id);
    Rgba8::new(r, g, b, a)
}

#[inline]
pub(crate) fn cursor_spirit_glyph_for_cursor_id(cursor_id: u32) -> Option<char> {
    UI2_FUN_CURSOR_ICONS_ENABLED.then_some(Ui2CursorColor::from_cursor_id(cursor_id).spirit_glyph())
}

#[inline]
fn should_log_input_diag(count: u32) -> bool {
    count <= UI2_INPUT_DIAG_LOG_FIRST
        || (count > UI2_INPUT_DIAG_LOG_FIRST && count.is_multiple_of(UI2_INPUT_DIAG_LOG_EVERY))
}

#[inline]
fn cursor_event_is_physical(event: &crate::usb2::hid::TrueosHidCursorEvent) -> bool {
    matches!(event.hid_kind, 2 | 3)
}

fn note_cursor_event_source(state: &mut Ui2State, event: &crate::usb2::hid::TrueosHidCursorEvent) {
    if cursor_event_is_physical(event) {
        return;
    }
    state.non_physical_cursor_event_count = state.non_physical_cursor_event_count.wrapping_add(1);
    let count = state.non_physical_cursor_event_count;
    if should_log_input_diag(count) {
        crate::log!(
            "ui2: cursor-input-source non-physical count={} seq={} kind={} ctrl={} slot={} ep={} buttons=0x{:X} wheel={} flags=0x{:X}\n",
            count,
            event.seq,
            event.hid_kind,
            event.controller_id,
            event.slot_id,
            event.ep_target,
            event.buttons_down,
            event.wheel,
            event.flags
        );
    }
}

fn note_keyboard_event_source(
    state: &mut Ui2State,
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) {
    if (event.flags & crate::r::keyboard::KEYBOARD_OUTPUT_FLAG_SYNTHETIC) == 0 {
        return;
    }
    state.synthetic_keyboard_event_count = state.synthetic_keyboard_event_count.wrapping_add(1);
    let count = state.synthetic_keyboard_event_count;
    if should_log_input_diag(count) {
        crate::log!(
            "ui2: keyboard-input-source synthetic count={} seq={} dev_seq={} ctrl={} slot={} ep={} kind={} key_code={} codepoint={} flags=0x{:X}\n",
            count,
            event.seq,
            event.device_seq,
            event.controller_id,
            event.slot_id,
            event.ep_target,
            event.kind,
            event.key_code,
            event.codepoint,
            event.flags
        );
    }
}

fn cursor_index(state: &Ui2State, slot_id: u32) -> Option<usize> {
    state
        .cursors
        .iter()
        .position(|cursor| cursor.slot_id == slot_id)
}

fn ensure_cursor_index(state: &mut Ui2State, slot_id: u32) -> Option<usize> {
    if let Some(idx) = cursor_index(state, slot_id) {
        return Some(idx);
    }
    if state.cursors.len() >= UI2_CURSOR_CAP {
        let drop_count = UI2_CURSOR_CAP_DROP_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
        if drop_count <= 8 || drop_count.is_multiple_of(32) {
            crate::log!(
                "ui2: cursor-cap drop slot={} cap={} tracked={} count={}\n",
                slot_id,
                UI2_CURSOR_CAP,
                state.cursors.len(),
                drop_count
            );
        }
        return None;
    }
    state.cursors.push(Ui2CursorState {
        slot_id,
        ..Ui2CursorState::default()
    });
    Some(state.cursors.len() - 1)
}

pub(super) fn note_selection_change(window: &mut Ui2Window) {
    window.chrome_titlebar_dirty = true;
    window.chrome_hover_clear_button = None;
    window.last_reason = "cursor-select";
}

fn set_cursor_selected_window(state: &mut Ui2State, slot_id: u32, next_window_id: u32) -> bool {
    let Some(cursor_idx) = ensure_cursor_index(state, slot_id) else {
        return false;
    };
    if state.cursors[cursor_idx].selected_window_id == next_window_id {
        return false;
    }

    let mut changed = false;
    let mut changed_window_ids = Vec::new();
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let mut raised = false;
    for window in &mut state.windows {
        if let Some(pos) = window
            .selected_cursor_slots
            .iter()
            .position(|selected_slot_id| *selected_slot_id == slot_id)
        {
            window.selected_cursor_slots.remove(pos);
            note_selection_change(window);
            changed_window_ids.push(window.id);
            changed = true;
        }
    }

    if next_window_id != 0 {
        if let Some(window) = window_mut(state, next_window_id) {
            if !window
                .selected_cursor_slots
                .iter()
                .any(|selected_slot_id| *selected_slot_id == slot_id)
            {
                window.selected_cursor_slots.push(slot_id);
                note_selection_change(window);
                changed_window_ids.push(window.id);
                changed = true;
            }
            let next_z = top_z.saturating_add(1);
            if window.z != next_z {
                window.z = next_z;
                window.last_reason = "cursor-select";
                raised = true;
            }
        }
    }

    state.cursors[cursor_idx].selected_window_id = next_window_id;
    if changed || raised {
        state.compose_reason = "cursor-select";
        if raised && next_window_id != 0 {
            refresh_window_hit_entries(state, next_window_id);
        }
        let titlebar_only = crate::gfx::is_intel_active() && state.first_compose_signaled;
        if titlebar_only {
            state.chrome_overlay_dirty = true;
        } else {
            for window_id in changed_window_ids {
                if let Some(window) = window_mut(state, window_id) {
                    window.dirty = true;
                    window.content_present_dirty = false;
                    window.content_present_dirty_rect = None;
                }
            }
            UI2_DIRTY.store(true, Ordering::Release);
        }
        crate::log!("ui2: cursor-select slot={} window={}\n", slot_id, next_window_id);
    }
    changed
}

fn queue_window_cursor_event(
    state: &mut Ui2State,
    window_id: u32,
    slot_id: u32,
    x: f32,
    y: f32,
    buttons_down: u32,
    wheel: i16,
    flags: u32,
) -> bool {
    if window_id == 0 {
        return false;
    }
    let Some(content) = state
        .windows
        .iter()
        .find(|window| window.id == window_id)
        .and_then(|window| window_content_rect(state, window))
    else {
        return false;
    };
    let Some(window) = window_mut(state, window_id) else {
        return false;
    };
    if window.cursor_events.len() >= UI2_WINDOW_CURSOR_EVENT_CAP {
        window.cursor_events.remove(0);
    }
    window.cursor_events.push(Ui2WindowCursorEvent {
        slot_id,
        x: x - content.x,
        y: y - content.y,
        buttons_down,
        wheel,
        flags,
    });
    true
}

fn selected_window_content_contains_cursor(
    state: &Ui2State,
    window_id: u32,
    x: f32,
    y: f32,
) -> bool {
    state
        .windows
        .iter()
        .find(|window| window.id == window_id)
        .and_then(|window| window_content_rect(state, window))
        .map(|content| rect_contains_point(content, x, y))
        .unwrap_or(false)
}

fn process_cursor_event(state: &mut Ui2State, event: crate::usb2::hid::TrueosHidCursorEvent) {
    if !cursor_event_is_physical(&event) {
        return;
    }

    let slot_id = event.slot_id;
    if slot_id == 0 {
        return;
    }

    let Some((px, py)) = ui2_cursor_px_for_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    ) else {
        return;
    };
    let press_hit = ui2_hit_for_cursor_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    )
    .map(|(_, _, hit)| hit);
    let release_hit = ui2_hit_for_cursor_source(
        state.view_w,
        state.view_h,
        event.controller_id,
        slot_id,
        event.ep_target,
        event.hid_kind,
    )
    .map(|(_, _, hit)| hit);
    let press_system_button_action = press_hit.and_then(|target| {
        if target.kind == Ui2HitKind::WindowDecoration {
            system_button_action_at(state, target.owner_window_id, px, py)
        } else {
            None
        }
    });
    let hover_decoration_button = decoration_hover_button_at(state, press_hit, px, py);
    let hover_offline_dock_play_rect = if press_hit.is_none() {
        ui2_win_register::offline_dock_play_button_hover_rect_at(px, py)
    } else {
        None
    };
    let press_window_id = if (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0 {
        press_hit.map(|target| target.owner_window_id).unwrap_or(0)
    } else {
        0
    };
    let release_window_id = release_hit
        .map(|target| target.owner_window_id)
        .unwrap_or(0);
    let Some(cursor_idx) = ensure_cursor_index(state, slot_id) else {
        return;
    };

    let mut select_window_id: Option<u32> = None;
    let mut window_button_action: Option<(u32, Ui2SystemButtonAction)> = None;
    let mut begin_move_drag = false;
    let mut begin_resize_drag: Option<(u32, u32)> = None;
    let mut begin_vertical_scroll_drag = false;
    let mut begin_horizontal_scroll_drag = false;
    let mut begin_scroll_pan_window_id = 0u32;
    let mut click_candidate_window_id = 0u32;
    let mut click_candidate_item_id = 0u32;
    let mut click_press_x = 0.0f32;
    let mut click_press_y = 0.0f32;
    let mut try_offline_dock = false;
    let mut cursor_capture_window_id = 0u32;
    let mut cursor_overlay_changed = false;
    let mut offline_dock_hover_changed = false;
    let mut hover_dirty_prev: Option<(u32, Ui2DecorationHoverButton)> = None;
    let mut hover_dirty_next: Option<(u32, Ui2DecorationHoverButton)> = None;
    {
        let cursor = &mut state.cursors[cursor_idx];
        let prev_buttons_down = cursor.buttons_down;
        let primary_was_down = (prev_buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let primary_is_down = (event.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0;
        let middle_was_down = (prev_buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;
        let middle_is_down = (event.buttons_down & UI2_MIDDLE_BUTTON_MASK) != 0;
        let prev_x = cursor.x;
        let prev_y = cursor.y;

        cursor.x = px;
        cursor.y = py;
        cursor.buttons_down = event.buttons_down;
        if prev_x != px || prev_y != py || prev_buttons_down != event.buttons_down {
            cursor_overlay_changed = true;
        }

        let prev_hover = cursor
            .hover_decoration_button
            .map(|button| (cursor.hover_window_id, button));
        if prev_hover != hover_decoration_button {
            hover_dirty_prev = prev_hover;
            hover_dirty_next = hover_decoration_button;
            if let Some((window_id, button)) = hover_decoration_button {
                cursor.hover_window_id = window_id;
                cursor.hover_decoration_button = Some(button);
            } else {
                cursor.hover_window_id = 0;
                cursor.hover_decoration_button = None;
            }
        }
        if cursor.hover_offline_dock_play_rect != hover_offline_dock_play_rect {
            cursor.hover_offline_dock_play_rect = hover_offline_dock_play_rect;
            offline_dock_hover_changed = true;
        }

        if !primary_was_down && primary_is_down {
            cursor.press_x = px;
            cursor.press_y = py;
            cursor.press_window_id = press_window_id;
            cursor.press_item_id = press_hit
                .filter(|target| target.kind == Ui2HitKind::BrowserInteractive)
                .map(|target| target.item_id)
                .unwrap_or(0);
            cursor.press_armed = press_window_id != 0;
            begin_move_drag = true;
        } else if primary_was_down && !primary_is_down {
            if cursor.selected_window_id != 0 && cursor.press_window_id == cursor.selected_window_id
            {
                cursor_capture_window_id = cursor.selected_window_id;
            }
            if cursor.press_armed
                && cursor.press_window_id != 0
                && cursor.press_window_id == release_window_id
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                click_candidate_window_id = release_window_id;
                click_candidate_item_id = release_hit
                    .filter(|target| {
                        target.kind == Ui2HitKind::BrowserInteractive
                            && target.item_id != 0
                            && target.item_id == cursor.press_item_id
                    })
                    .map(|target| target.item_id)
                    .unwrap_or(0);
                click_press_x = cursor.press_x;
                click_press_y = cursor.press_y;
            } else if cursor.press_window_id == 0
                && release_window_id == 0
                && is_simple_click(cursor.press_x, cursor.press_y, px, py)
            {
                try_offline_dock = true;
            }
            cursor.press_armed = false;
            cursor.press_window_id = 0;
            cursor.press_item_id = 0;
        }

        if cursor_capture_window_id == 0
            && cursor.selected_window_id != 0
            && cursor.press_window_id == cursor.selected_window_id
            && primary_is_down
        {
            cursor_capture_window_id = cursor.selected_window_id;
        }

        if !middle_was_down
            && middle_is_down
            && let Some(target) = press_hit
            && matches!(target.kind, Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive)
        {
            begin_scroll_pan_window_id = target.owner_window_id;
        }
    }

    if cursor_overlay_changed {
        note_cursor_overlay_dirty(state, "cursor-overlay");
    }
    if offline_dock_hover_changed {
        note_offline_dock_chrome_dirty(state, "offline-dock-button-hover");
    }
    if crate::gfx::is_intel_active() {
        if let Some((window_id, button)) = hover_dirty_prev
            && decoration_hover_button_rect(state, window_id, button).is_some()
        {
            let _ = note_window_chrome_hover_event(
                state,
                window_id,
                button,
                true,
                "decor-button-hover",
            );
        }
        if let Some((window_id, button)) = hover_dirty_next
            && decoration_hover_button_rect(state, window_id, button).is_some()
        {
            let _ = note_window_chrome_hover_event(
                state,
                window_id,
                button,
                false,
                "decor-button-hover",
            );
        }
    } else {
        if let Some((window_id, button)) = hover_dirty_prev {
            if decoration_hover_button_rect(state, window_id, button).is_some() {
                let _ = note_window_dirty(state, window_id, "decor-button-hover");
            }
        }
        if let Some((window_id, button)) = hover_dirty_next {
            if decoration_hover_button_rect(state, window_id, button).is_some() {
                let _ = note_window_dirty(state, window_id, "decor-button-hover");
            }
        }
    }

    let selected_window_id = state.cursors[cursor_idx].selected_window_id;
    let press_routes_to_window = begin_move_drag
        && press_window_id != 0
        && press_hit
            .map(|target| {
                matches!(target.kind, Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive)
            })
            .unwrap_or(false);
    if press_routes_to_window {
        cursor_capture_window_id = press_window_id;
        if selected_window_id != press_window_id {
            select_window_id = Some(press_window_id);
        }
    }
    let route_window_id = if cursor_capture_window_id != 0 {
        cursor_capture_window_id
    } else if selected_window_id != 0
        && selected_window_content_contains_cursor(state, selected_window_id, px, py)
    {
        selected_window_id
    } else {
        0
    };
    let _ = queue_window_cursor_event(
        state,
        route_window_id,
        slot_id,
        px,
        py,
        event.buttons_down,
        event.wheel,
        event.flags,
    );

    // Deferred offline dock click (after cursor borrow is released).
    if try_offline_dock {
        if !ui2_win_register::handle_offline_dock_click(state, px, py) {
            select_window_id = Some(0);
        } else {
            UI2_DIRTY.store(true, Ordering::Release);
        }
    }

    if begin_move_drag && let Some(target) = press_hit {
        match target.kind {
            Ui2HitKind::WindowResizeButton => {
                begin_resize_drag = Some((
                    target.owner_window_id,
                    UI2_WINDOW_RESIZE_RIGHT | UI2_WINDOW_RESIZE_BOTTOM,
                ));
            }
            Ui2HitKind::WindowVerticalScrollbar => {
                begin_vertical_scroll_drag = true;
            }
            Ui2HitKind::WindowHorizontalScrollbar => {
                begin_horizontal_scroll_drag = true;
            }
            Ui2HitKind::WindowDecoration => {
                if press_system_button_action.is_none() {
                    let _ =
                        begin_move_drag_for_cursor(state, slot_id, target.owner_window_id, px, py);
                }
            }
            Ui2HitKind::WindowBody | Ui2HitKind::BrowserInteractive => {}
        }
    }
    if let Some((window_id, edge_mask)) = begin_resize_drag {
        let _ = begin_window_resize_for_cursor(state, slot_id, window_id, edge_mask);
    }
    if begin_vertical_scroll_drag {
        let _ = begin_vertical_scroll_drag_for_cursor(state, slot_id, px, py);
    }
    if begin_horizontal_scroll_drag {
        let _ = begin_horizontal_scroll_drag_for_cursor(state, slot_id, px, py);
    }
    if begin_scroll_pan_window_id != 0 {
        let _ =
            begin_window_scroll_pan_for_cursor(state, slot_id, begin_scroll_pan_window_id, px, py);
        let _ = set_cursor_selected_window(state, slot_id, begin_scroll_pan_window_id);
    }

    update_move_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    update_resize_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    let _ = update_scroll_drag_for_cursor(state, slot_id, px, py, event.buttons_down);
    let _ = update_scroll_pan_for_cursor(state, slot_id, px, py, event.buttons_down);

    if click_candidate_window_id != 0 {
        let press_action =
            system_button_action_at(state, click_candidate_window_id, click_press_x, click_press_y);
        let release_action = system_button_action_at(state, click_candidate_window_id, px, py);
        if let (Some(press_action), Some(release_action)) = (press_action, release_action) {
            if press_action == release_action {
                window_button_action = Some((click_candidate_window_id, release_action));
            } else {
                select_window_id = Some(click_candidate_window_id);
            }
        } else {
            select_window_id = Some(click_candidate_window_id);
        }
        if click_candidate_item_id != 0 {
            let _ = note_window_item_click(
                state,
                click_candidate_window_id,
                click_candidate_item_id,
                slot_id,
            );
        }
    }

    if let Some(select_window_id) = select_window_id {
        let _ = set_cursor_selected_window(state, slot_id, select_window_id);
    }
    if let Some((window_id, action)) = window_button_action {
        let _ = handle_system_button_action(state, window_id, action);
    }

    if event.wheel != 0 {
        let selected_window_id = state.cursors[cursor_idx].selected_window_id;
        state.compose_reason = "wheel-input";
        UI2_DIRTY.store(true, Ordering::Release);
        let _ = forward_cursor_wheel_to_selected_window(state, selected_window_id, event.wheel);
    }
}

fn forward_cursor_wheel_to_selected_window(
    state: &mut Ui2State,
    window_id: u32,
    wheel: i16,
) -> bool {
    if window_id == 0 || wheel == 0 {
        return false;
    }
    let Some(window) = state.windows.iter().find(|window| window.id == window_id) else {
        return false;
    };
    if !window_content_participates_in_composition(window) {
        return false;
    }
    match window.kind {
        Ui2WindowKind::HostedBrowser => {
            let content_id = window_browser_instance_id(window);
            let snapshot = browser_surface_state_for_window(window);
            let scroll_delta = -(wheel as i32) * UI2_WHEEL_SCROLL_STEP_PX;
            let next_scroll = clamp_hosted_browser_scroll(
                &snapshot,
                i64::from(normalized_hosted_browser_scroll(&snapshot))
                    .saturating_add(i64::from(scroll_delta)),
            );
            if hosted_set_scroll_y(content_id, next_scroll) {
                state.compose_reason = "wheel-scroll";
                true
            } else {
                false
            }
        }
        Ui2WindowKind::HostedSurface => {
            let content_id = window_hosted_content_id(window);
            if content_id == 0 {
                return false;
            }
            let snapshot = hosted_surface_state_for_window(window);
            let scroll_delta = -(wheel as i32) * UI2_WHEEL_SCROLL_STEP_PX;
            let next_scroll = clamp_hosted_browser_scroll(
                &snapshot,
                i64::from(normalized_hosted_browser_scroll(&snapshot))
                    .saturating_add(i64::from(scroll_delta)),
            );
            if hosted_set_scroll_y(content_id, next_scroll) {
                state.compose_reason = "wheel-scroll";
                true
            } else {
                false
            }
        }
        Ui2WindowKind::Hosted3d => false,
    }
}

pub(super) fn pump_cursor_selection(state: &mut Ui2State) {
    let mut events = [crate::usb2::hid::TrueosHidCursorEvent::default(); UI2_CURSOR_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::usb2::hid::read_cursor_events_since(state.cursor_read_seq, &mut events);
        if dropped != 0 {
            crate::log!(
                "ui2: cursor-event-drop read_seq={} dropped={}\n",
                state.cursor_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.cursor_read_seq = next_seq;
        for event in events.iter().take(wrote) {
            note_cursor_event_source(state, event);
            process_cursor_event(state, *event);
        }
        if wrote < events.len() {
            break;
        }
    }
}

fn selected_window_id_for_keyboard(state: &Ui2State) -> Option<u32> {
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window.visible
            || window.composition_locked
            || window.state == Ui2WindowStateKind::Minimized
            || window.selected_cursor_slots.is_empty()
        {
            continue;
        }
        return Some(window.id);
    }
    None
}

fn selected_hosted_surface_window_ids_for_keyboard(state: &Ui2State) -> Vec<u32> {
    let mut out = Vec::new();
    for idx in sorted_window_indices(state).into_iter().rev() {
        let window = &state.windows[idx];
        if !window.visible
            || window.composition_locked
            || window.state == Ui2WindowStateKind::Minimized
            || window.selected_cursor_slots.is_empty()
            || window.kind != Ui2WindowKind::HostedSurface
        {
            continue;
        }
        out.push(window.id);
    }
    out
}

fn keyboard_output_modifiers_to_browser_mask(modifiers: u8) -> u8 {
    let mut out = 0u8;
    if (modifiers & ((1 << 1) | (1 << 5))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_SHIFT;
    }
    if (modifiers & ((1 << 0) | (1 << 4))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_CTRL;
    }
    if (modifiers & ((1 << 2) | (1 << 6))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_ALT;
    }
    if (modifiers & ((1 << 3) | (1 << 7))) != 0 {
        out |= HOSTED_KEYBOARD_MOD_META;
    }
    out
}

fn keyboard_output_key_name(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> Option<String> {
    let named = match event.key_code {
        crate::r::keyboard::KEYBOARD_KEY_BACKSPACE => Some("Backspace"),
        crate::r::keyboard::KEYBOARD_KEY_TAB => Some("Tab"),
        crate::r::keyboard::KEYBOARD_KEY_ENTER => Some("Enter"),
        crate::r::keyboard::KEYBOARD_KEY_ESCAPE => Some("Escape"),
        crate::r::keyboard::KEYBOARD_KEY_SPACE => Some("Space"),
        crate::r::keyboard::KEYBOARD_KEY_DELETE => Some("Delete"),
        crate::r::keyboard::KEYBOARD_KEY_INSERT => Some("Insert"),
        crate::r::keyboard::KEYBOARD_KEY_HOME => Some("Home"),
        crate::r::keyboard::KEYBOARD_KEY_END => Some("End"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_UP => Some("PageUp"),
        crate::r::keyboard::KEYBOARD_KEY_PAGE_DOWN => Some("PageDown"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_UP => Some("ArrowUp"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_DOWN => Some("ArrowDown"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_LEFT => Some("ArrowLeft"),
        crate::r::keyboard::KEYBOARD_KEY_ARROW_RIGHT => Some("ArrowRight"),
        crate::r::keyboard::KEYBOARD_KEY_F1 => Some("F1"),
        crate::r::keyboard::KEYBOARD_KEY_F2 => Some("F2"),
        crate::r::keyboard::KEYBOARD_KEY_F3 => Some("F3"),
        crate::r::keyboard::KEYBOARD_KEY_F4 => Some("F4"),
        crate::r::keyboard::KEYBOARD_KEY_F5 => Some("F5"),
        crate::r::keyboard::KEYBOARD_KEY_F6 => Some("F6"),
        crate::r::keyboard::KEYBOARD_KEY_F7 => Some("F7"),
        crate::r::keyboard::KEYBOARD_KEY_F8 => Some("F8"),
        crate::r::keyboard::KEYBOARD_KEY_F9 => Some("F9"),
        crate::r::keyboard::KEYBOARD_KEY_F10 => Some("F10"),
        crate::r::keyboard::KEYBOARD_KEY_F11 => Some("F11"),
        crate::r::keyboard::KEYBOARD_KEY_F12 => Some("F12"),
        _ => None,
    };
    if let Some(name) = named {
        return Some(String::from(name));
    }
    char::from_u32(event.codepoint).map(|ch| {
        let mut value = String::new();
        value.push(ch);
        value
    })
}

fn browser_keyboard_event_from_output(
    event: crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> Option<UiHostedKeyboardEvent> {
    match event.kind {
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_TEXT => {
            let utf8_len = (event.utf8_len as usize).min(event.utf8.len());
            if utf8_len == 0 {
                return char::from_u32(event.codepoint).map(|ch| {
                    let mut text = String::new();
                    text.push(ch);
                    UiHostedKeyboardEvent::Text { text }
                });
            }
            let text = core::str::from_utf8(&event.utf8[..utf8_len]).ok()?;
            if text.is_empty() {
                return None;
            }
            Some(UiHostedKeyboardEvent::Text {
                text: String::from(text),
            })
        }
        crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY => {
            let key = keyboard_output_key_name(&event)?;
            Some(UiHostedKeyboardEvent::Key {
                key,
                modifiers: keyboard_output_modifiers_to_browser_mask(event.modifiers),
            })
        }
        _ => None,
    }
}

pub(super) fn pump_keyboard_input(state: &mut Ui2State) {
    let selected_window_id = selected_window_id_for_keyboard(state);
    let mut raw_events =
        [crate::r::keyboard::TrueosKeyboardOutputEvent::default(); UI2_KEYBOARD_EVENT_BATCH];
    loop {
        let (next_seq, dropped, wrote) =
            crate::r::keyboard::read_output_events_since(state.keyboard_read_seq, &mut raw_events);
        if dropped != 0 {
            crate::log!(
                "ui2: keyboard-event-drop read_seq={} dropped={}\n",
                state.keyboard_read_seq,
                dropped
            );
        }
        if wrote == 0 {
            break;
        }
        state.keyboard_read_seq = next_seq;

        if let Some(window_id) = selected_window_id {
            let window_kind = state
                .windows
                .iter()
                .find(|window| window.id == window_id)
                .map(|window| window.kind);
            match window_kind {
                Some(Ui2WindowKind::HostedBrowser) => {
                    let mut events = Vec::new();
                    for event in raw_events.iter().take(wrote).copied() {
                        note_keyboard_event_source(state, &event);
                        if let Some(next) = browser_keyboard_event_from_output(event) {
                            events.push(next);
                        }
                    }
                    let content_id = state
                        .windows
                        .iter()
                        .find(|window| window.id == window_id)
                        .map(window_browser_instance_id)
                        .unwrap_or(0);
                    if !events.is_empty()
                        && !hosted_queue_keyboard_events(content_id, events.as_slice())
                    {
                        crate::log!(
                            "ui2: keyboard-forward-drop window={} count={}\n",
                            window_id,
                            events.len()
                        );
                    }
                }
                Some(Ui2WindowKind::HostedSurface) => {
                    let selected_surface_window_ids =
                        selected_hosted_surface_window_ids_for_keyboard(state);
                    for event in raw_events.iter().take(wrote).copied() {
                        note_keyboard_event_source(state, &event);
                        for target_window_id in selected_surface_window_ids.iter().copied() {
                            if crate::tst::ui2::gboi_demo::queue_keyboard_event(
                                target_window_id,
                                event,
                            ) {
                                continue;
                            }
                            if crate::shell2::queue_ui2_shell_keyboard_event(
                                target_window_id,
                                event,
                            ) {
                                continue;
                            }
                            let _ = note_window_keyboard_event(state, target_window_id, event);
                        }
                    }
                }
                Some(Ui2WindowKind::Hosted3d) => {
                    for event in raw_events.iter().take(wrote) {
                        note_keyboard_event_source(state, event);
                    }
                }
                None => {}
            }
        } else {
            for event in raw_events.iter().take(wrote) {
                note_keyboard_event_source(state, event);
            }
        }

        if wrote < raw_events.len() {
            break;
        }
    }
}

#[inline]
fn window_uses_live_resize(window: &Ui2Window) -> bool {
    match window.resize_mode {
        Ui2WindowResizeMode::Auto => !matches!(window.kind, Ui2WindowKind::HostedBrowser),
        Ui2WindowResizeMode::Live => true,
        Ui2WindowResizeMode::PreviewCommit => false,
    }
}

pub(super) fn pick_drag_cursor_slot(state: &Ui2State, window: &Ui2Window) -> Option<u32> {
    for slot_id in &window.selected_cursor_slots {
        if let Some(cursor) = state
            .cursors
            .iter()
            .find(|cursor| cursor.slot_id == *slot_id)
            && (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0
        {
            return Some(*slot_id);
        }
    }
    for cursor in &state.cursors {
        if cursor.selected_window_id == window.id
            && (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) != 0
        {
            return Some(cursor.slot_id);
        }
    }
    for cursor in &state.cursors {
        if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
            continue;
        }
        if rect_contains_point(window.rect, cursor.x, cursor.y) {
            return Some(cursor.slot_id);
        }
    }
    None
}

fn begin_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let mut next_rect = window.rect;
    if window.state == Ui2WindowStateKind::Maximized {
        let view_w = (state.view_w as f32).max(1.0);
        let view_h = (state.view_h as f32).max(1.0);
        let restored = if window.restore_rect.w > 0.0 && window.restore_rect.h > 0.0 {
            normalized_window_rect_for_window_for_view(
                state.view_w,
                state.view_h,
                &window,
                window.restore_rect,
            )
        } else {
            Ui2Rect::new(0.0, 0.0, (view_w * 0.75).max(1.0), (view_h * 0.75).max(1.0))
        };
        let cursor_ratio_x = ((cursor_x - window.rect.x) / window.rect.w.max(1.0)).clamp(0.0, 1.0);
        let grab_dy =
            (cursor_y - window.rect.y).clamp(0.0, UI2_TITLE_H.min(restored.h).max(1.0) - 1.0);
        next_rect = restored;
        next_rect.x =
            (cursor_x - (next_rect.w * cursor_ratio_x)).clamp(0.0, (view_w - next_rect.w).max(0.0));
        next_rect.y = (cursor_y - grab_dy).clamp(0.0, (view_h - next_rect.h).max(0.0));
    }
    let was_maximized = window.state == Ui2WindowStateKind::Maximized;
    let raise_on_move = window.z < top_z;
    if let Some(window_mut) = window_mut(state, window_id) {
        if was_maximized {
            window_mut.rect = next_rect;
            window_mut.restore_rect = next_rect;
            window_mut.state = Ui2WindowStateKind::Normal;
        }
    }
    let _ = set_cursor_selected_window(state, slot_id, window_id);
    let needs_full_compose = was_maximized;
    if needs_full_compose {
        let _ = note_window_dirty(state, window_id, "begin-window-move");
        if was_maximized {
            let _ = note_window_viewport_sync_needed(state, window_id);
        }
    }
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_move_drag(
        state,
        Ui2WindowMoveDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            grab_dx: cursor_x - next_rect.x,
            grab_dy: cursor_y - next_rect.y,
            start_rect: next_rect,
            raise_on_move,
            edge_actions_armed: window_edge_drop_action(state, cursor_x, cursor_y).is_none(),
        },
    );
    if needs_full_compose {
        state.compose_reason = "begin-window-move";
        refresh_window_hit_entries(state, window_id);
    }
    true
}

pub(super) fn begin_window_resize_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    edge_mask: u32,
) -> bool {
    let edge_mask = edge_mask
        & (UI2_WINDOW_RESIZE_LEFT
            | UI2_WINDOW_RESIZE_TOP
            | UI2_WINDOW_RESIZE_RIGHT
            | UI2_WINDOW_RESIZE_BOTTOM);
    if !is_valid_resize_edge_mask(edge_mask) {
        return false;
    }
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
        .cloned()
    else {
        return false;
    };
    if window.state != Ui2WindowStateKind::Normal {
        return false;
    }
    let Some(cursor) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .copied()
    else {
        return false;
    };
    if (cursor.buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        return false;
    }

    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_resize_drag(
        state,
        Ui2WindowResizeDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            live_apply: window_uses_live_resize(&window),
            edge_mask,
            start_cursor_x: cursor.x,
            start_cursor_y: cursor.y,
            start_rect: window.rect,
            preview_rect: window.rect,
        },
    );
    let top_z = state
        .windows
        .iter()
        .map(|window| window.z)
        .max()
        .unwrap_or(0);
    let raised = window.z < top_z;
    if raised && let Some(window_mut) = window_mut(state, window_id) {
        window_mut.z = top_z.saturating_add(1);
    }
    if raised {
        state.compose_reason = "begin-window-resize";
        let noted = note_window_dirty(state, window_id, "begin-window-resize");
        refresh_window_hit_entries(state, window_id);
        return noted;
    }
    true
}

fn browser_vertical_scrollbar_metrics(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<(Ui2Rect, f32, f32, u32)> {
    let Some(snapshot) = window_scroll_snapshot(window) else {
        return None;
    };
    let track = window_vertical_scrollbar_rect(state, window)?;
    let viewport_h = snapshot.viewport_height.max(1);
    let content_h = snapshot.content_height.max(viewport_h);
    let scroll_range = hosted_browser_scroll_max(&snapshot);
    let thumb_h =
        libm::fmaxf(10.0, (track.h * (viewport_h as f32 / content_h as f32)).min(track.h));
    let thumb_y = if scroll_range > 0 {
        let avail = (track.h - thumb_h).max(0.0);
        track.y
            + (avail * (normalized_hosted_browser_scroll(&snapshot) as f32 / scroll_range as f32))
    } else {
        track.y
    };
    Some((track, thumb_h, thumb_y, scroll_range))
}

fn browser_horizontal_scrollbar_metrics(
    state: &Ui2State,
    window: &Ui2Window,
) -> Option<(Ui2Rect, f32, f32, u32)> {
    let Some(snapshot) = window_scroll_snapshot(window) else {
        return None;
    };
    let track = window_horizontal_scrollbar_rect(state, window)?;
    let viewport_w = snapshot.viewport_width.max(1);
    let content_w = snapshot.content_width.max(viewport_w);
    let scroll_range = hosted_browser_scroll_x_max(&snapshot);
    let thumb_w =
        libm::fmaxf(10.0, (track.w * (viewport_w as f32 / content_w as f32)).min(track.w));
    let thumb_x = if scroll_range > 0 {
        let avail = (track.w - thumb_w).max(0.0);
        track.x
            + (avail * (normalized_hosted_browser_scroll_x(&snapshot) as f32 / scroll_range as f32))
    } else {
        track.x
    };
    Some((track, thumb_w, thumb_x, scroll_range))
}

fn begin_vertical_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window_id) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .map(|cursor| cursor.press_window_id)
        .filter(|id| *id != 0)
    else {
        return false;
    };
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    let Some((track, thumb_h, thumb_y, _)) = browser_vertical_scrollbar_metrics(state, window)
    else {
        return false;
    };
    let thumb_rect = Ui2Rect::new(track.x, thumb_y, track.w, thumb_h);
    let grab_offset = if rect_contains_point(thumb_rect, cursor_x, cursor_y) {
        (cursor_y - thumb_y).clamp(0.0, thumb_h)
    } else {
        thumb_h * 0.5
    };
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_scroll_drag(
        state,
        Ui2WindowScrollDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            horizontal: false,
            track_rect: track,
            thumb_extent: thumb_h,
            grab_offset,
        },
    );
    update_scroll_drag_for_cursor(state, slot_id, cursor_x, cursor_y, UI2_PRIMARY_BUTTON_MASK)
}

fn begin_horizontal_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window_id) = state
        .cursors
        .iter()
        .find(|cursor| cursor.slot_id == slot_id)
        .map(|cursor| cursor.press_window_id)
    else {
        return false;
    };
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    let Some((track, thumb_w, thumb_x, _)) = browser_horizontal_scrollbar_metrics(state, window)
    else {
        return false;
    };
    let thumb_rect = Ui2Rect::new(thumb_x, track.y, thumb_w, track.h);
    let grab_offset = if rect_contains_point(thumb_rect, cursor_x, cursor_y) {
        (cursor_x - thumb_x).clamp(0.0, thumb_w)
    } else {
        thumb_w * 0.5
    };
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_scroll_drag(
        state,
        Ui2WindowScrollDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            horizontal: true,
            track_rect: track,
            thumb_extent: thumb_w,
            grab_offset,
        },
    );
    update_scroll_drag_for_cursor(state, slot_id, cursor_x, cursor_y, UI2_PRIMARY_BUTTON_MASK)
}

fn begin_window_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    window_id: u32,
    cursor_x: f32,
    cursor_y: f32,
) -> bool {
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == window_id && window_is_renderable(window))
    else {
        return false;
    };
    if window_scroll_snapshot(window).is_none() {
        return false;
    }
    clear_window_drag_claims(state, window_id);
    clear_other_drag_modes_for_slot(state, slot_id);
    upsert_scroll_pan_drag(
        state,
        Ui2WindowScrollPanDrag {
            active: true,
            window_id,
            cursor_slot_id: slot_id,
            last_cursor_x: cursor_x,
            last_cursor_y: cursor_y,
        },
    );
    state.compose_reason = "begin-scroll-pan";
    true
}

fn update_scroll_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    let Some(drag_idx) = state
        .scroll_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return false;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        state.scroll_drags.remove(drag_idx);
        return false;
    }
    let drag = state.scroll_drags[drag_idx];
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id && window_is_renderable(window))
    else {
        state.scroll_drags.remove(drag_idx);
        return false;
    };
    let metrics = if drag.horizontal {
        browser_horizontal_scrollbar_metrics(state, window)
    } else {
        browser_vertical_scrollbar_metrics(state, window)
    };
    let Some((track, _thumb_extent, _thumb_origin, scroll_range)) = metrics else {
        state.scroll_drags.remove(drag_idx);
        return false;
    };
    state.scroll_drags[drag_idx].track_rect = track;
    if scroll_range == 0 {
        return false;
    }
    let track_origin = if drag.horizontal { track.x } else { track.y };
    let cursor_origin = if drag.horizontal { cursor_x } else { cursor_y };
    let track_extent = if drag.horizontal { track.w } else { track.h };
    let avail = (track_extent - state.scroll_drags[drag_idx].thumb_extent).max(0.0);
    if avail <= 0.0 {
        return false;
    }
    let thumb_origin = (cursor_origin - state.scroll_drags[drag_idx].grab_offset)
        .clamp(track_origin, track_origin + avail);
    let ratio = ((thumb_origin - track_origin) / avail).clamp(0.0, 1.0);
    let snapshot = hosted_surface_state_for_window(window);
    let next_scroll = libm::roundf(ratio * scroll_range as f32) as i64;
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id)
    else {
        return false;
    };
    let content_id = window_hosted_content_id(window);
    if content_id == 0 {
        return false;
    }
    let scrolled = if drag.horizontal {
        let next_scroll_x = clamp_hosted_browser_scroll_x(&snapshot, next_scroll);
        hosted_set_scroll(content_id, next_scroll_x, normalized_hosted_browser_scroll(&snapshot))
    } else {
        let next_scroll_y = clamp_hosted_browser_scroll(&snapshot, next_scroll);
        hosted_set_scroll_y(content_id, next_scroll_y)
    };
    if scrolled {
        state.compose_reason = "scrollbar-drag";
        true
    } else {
        false
    }
}

fn update_scroll_pan_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) -> bool {
    let Some(drag_idx) = state
        .scroll_pan_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return false;
    };
    if (buttons_down & UI2_MIDDLE_BUTTON_MASK) == 0 {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    }
    let drag = state.scroll_pan_drags[drag_idx];
    let Some(window) = state.windows.iter().find(|window| {
        window.id == drag.window_id && window_content_participates_in_composition(window)
    }) else {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    };
    if window_scroll_snapshot(window).is_none() {
        state.scroll_pan_drags.remove(drag_idx);
        return false;
    }

    let dx = cursor_x - drag.last_cursor_x;
    let dy = cursor_y - drag.last_cursor_y;
    state.scroll_pan_drags[drag_idx].last_cursor_x = cursor_x;
    state.scroll_pan_drags[drag_idx].last_cursor_y = cursor_y;

    let dx_px = libm::roundf(dx) as i32;
    let dy_px = libm::roundf(dy) as i32;
    if dx_px == 0 && dy_px == 0 {
        return false;
    }

    let snapshot = hosted_surface_state_for_window(window);
    let next_scroll_x = clamp_hosted_browser_scroll_x(
        &snapshot,
        i64::from(normalized_hosted_browser_scroll_x(&snapshot)).saturating_sub(i64::from(dx_px)),
    );
    let next_scroll_y = clamp_hosted_browser_scroll(
        &snapshot,
        i64::from(normalized_hosted_browser_scroll(&snapshot)).saturating_sub(i64::from(dy_px)),
    );
    let content_id = window_hosted_content_id(window);
    if content_id == 0 {
        return false;
    }
    if hosted_set_scroll(content_id, next_scroll_x, next_scroll_y) {
        state.compose_reason = "scroll-pan";
        true
    } else {
        false
    }
}

fn update_move_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    let Some(drag_idx) = state
        .move_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        let drag = state.move_drags[drag_idx];
        let moved = state
            .windows
            .iter()
            .find(|window| window.id == drag.window_id)
            .map(|window| window.rect != drag.start_rect)
            .unwrap_or(false);
        if moved {
            state.compose_reason = "window-move-commit";
            let _ = commit_window_geometry_change(state, drag.window_id, "window-move-commit");
            let _ = note_window_content_present_after_geometry(
                state,
                drag.window_id,
                "window-move-commit",
            );
        }
        state.move_drags.remove(drag_idx);
        return;
    }
    let edge_action = window_edge_drop_action(state, cursor_x, cursor_y);
    if !state.move_drags[drag_idx].edge_actions_armed {
        if edge_action.is_none() {
            state.move_drags[drag_idx].edge_actions_armed = true;
        }
    } else if let Some(action) = edge_action {
        let window_id = state.move_drags[drag_idx].window_id;
        state.move_drags.remove(drag_idx);
        let _ = apply_window_edge_drop_action(state, window_id, action);
        return;
    }
    let next_x = cursor_x - state.move_drags[drag_idx].grab_dx;
    let next_y = cursor_y - state.move_drags[drag_idx].grab_dy;
    let window_id = state.move_drags[drag_idx].window_id;
    let raise_on_move = state.move_drags[drag_idx].raise_on_move;
    let Some(window) = window_mut(state, window_id) else {
        state.move_drags.remove(drag_idx);
        return;
    };
    let mut moved = false;
    if window.rect.x != next_x || window.rect.y != next_y {
        window.rect.x = next_x;
        window.rect.y = next_y;
        window.restore_rect = window.rect;
        moved = true;
    }
    if moved && raise_on_move {
        let top_z = state
            .windows
            .iter()
            .map(|window| window.z)
            .max()
            .unwrap_or(0);
        if let Some(window) = window_mut(state, window_id) {
            window.z = top_z.saturating_add(1);
        }
        state.move_drags[drag_idx].raise_on_move = false;
    }
    if moved {
        state.compose_reason = "window-drag";
        let _ = note_window_dirty(state, window_id, "window-drag");
        let _ = note_window_viewport_sync_needed(state, window_id);
        refresh_window_hit_entries(state, window_id);
    }
}

fn update_resize_drag_for_cursor(
    state: &mut Ui2State,
    slot_id: u32,
    cursor_x: f32,
    cursor_y: f32,
    buttons_down: u32,
) {
    let Some(drag_idx) = state
        .resize_drags
        .iter()
        .position(|drag| drag.active && drag.cursor_slot_id == slot_id)
    else {
        return;
    };
    if (buttons_down & UI2_PRIMARY_BUTTON_MASK) == 0 {
        let drag = state.resize_drags[drag_idx];
        if drag.active {
            if !drag.live_apply && drag.preview_rect != drag.start_rect {
                if let Some(window) = window_mut(state, drag.window_id) {
                    window.rect = drag.preview_rect;
                    window.restore_rect = drag.preview_rect;
                }
                state.compose_reason = "window-resize-commit";
                let _ =
                    commit_window_geometry_change(state, drag.window_id, "window-resize-commit");
                let _ = note_window_content_present_after_geometry(
                    state,
                    drag.window_id,
                    "window-resize-commit",
                );
            } else if drag.live_apply {
                let resized = state
                    .windows
                    .iter()
                    .find(|window| window.id == drag.window_id)
                    .map(|window| window.rect != drag.start_rect)
                    .unwrap_or(false);
                if resized {
                    state.compose_reason = "window-resize-commit";
                    let _ = commit_window_geometry_change(
                        state,
                        drag.window_id,
                        "window-resize-commit",
                    );
                    let _ = note_window_content_present_after_geometry(
                        state,
                        drag.window_id,
                        "window-resize-commit",
                    );
                }
            }
        }
        state.resize_drags.remove(drag_idx);
        return;
    }

    let drag = state.resize_drags[drag_idx];
    let Some(window) = state
        .windows
        .iter()
        .find(|window| window.id == drag.window_id)
    else {
        state.resize_drags.remove(drag_idx);
        return;
    };
    if window.state == Ui2WindowStateKind::Maximized {
        state.resize_drags.remove(drag_idx);
        return;
    }

    let mut next = drag.start_rect;
    let (min_w, min_h) = ui2_window_min_size(window);
    let dx = cursor_x - drag.start_cursor_x;
    let dy = cursor_y - drag.start_cursor_y;
    let right = drag.start_rect.x + drag.start_rect.w;
    let bottom = drag.start_rect.y + drag.start_rect.h;

    if (drag.edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 {
        let max_x = right - min_w;
        next.x = libm::fminf(drag.start_rect.x + dx, max_x);
        next.w = (right - next.x).max(min_w);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
        next.w = (drag.start_rect.w + dx).max(min_w);
    }

    if (drag.edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 {
        let max_y = bottom - min_h;
        next.y = libm::fminf(drag.start_rect.y + dy, max_y);
        next.h = (bottom - next.y).max(min_h);
    } else if (drag.edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
        next.h = (drag.start_rect.h + dy).max(min_h);
    }

    if window.resize_maintain_aspect {
        let mut start_window = window.clone();
        start_window.rect = drag.start_rect;
        let Some(start_content) = window_content_rect(state, &start_window) else {
            return;
        };
        if start_content.w > 0.0 && start_content.h > 0.0 {
            let aspect = start_content.w / start_content.h;
            let outer_left = drag.start_rect.x;
            let outer_top = drag.start_rect.y;
            let outer_right = drag.start_rect.x + drag.start_rect.w;
            let outer_bottom = drag.start_rect.y + drag.start_rect.h;
            let content_right = start_content.x + start_content.w;
            let content_bottom = start_content.y + start_content.h;
            let inset_left = start_content.x - outer_left;
            let inset_top = start_content.y - outer_top;
            let inset_right = outer_right - content_right;
            let inset_bottom = outer_bottom - content_bottom;
            let min_content_w = (min_w - inset_left - inset_right).max(1.0);
            let min_content_h = (min_h - inset_top - inset_bottom).max(1.0);

            let mut raw_left = start_content.x;
            let mut raw_right = content_right;
            let mut raw_top = start_content.y;
            let mut raw_bottom = content_bottom;

            if (drag.edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 {
                raw_left = next.x + inset_left;
            } else if (drag.edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 {
                raw_right = next.x + next.w - inset_right;
            }

            if (drag.edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 {
                raw_top = next.y + inset_top;
            } else if (drag.edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 {
                raw_bottom = next.y + next.h - inset_bottom;
            }

            let raw_w = (raw_right - raw_left).max(min_content_w);
            let raw_h = (raw_bottom - raw_top).max(min_content_h);
            let has_horizontal =
                (drag.edge_mask & (UI2_WINDOW_RESIZE_LEFT | UI2_WINDOW_RESIZE_RIGHT)) != 0;
            let has_vertical =
                (drag.edge_mask & (UI2_WINDOW_RESIZE_TOP | UI2_WINDOW_RESIZE_BOTTOM)) != 0;

            let mut next_content_w = start_content.w.max(min_content_w);
            let mut next_content_h = start_content.h.max(min_content_h);
            if has_horizontal && has_vertical {
                next_content_w = libm::fminf(raw_w, raw_h * aspect).max(min_content_w);
                next_content_h = (next_content_w / aspect).max(min_content_h);
                if next_content_h > raw_h {
                    next_content_h = raw_h.max(min_content_h);
                    next_content_w = (next_content_h * aspect).max(min_content_w);
                }
            } else if has_horizontal {
                next_content_w = raw_w.max(min_content_w);
                next_content_h = (next_content_w / aspect).max(min_content_h);
            } else if has_vertical {
                next_content_h = raw_h.max(min_content_h);
                next_content_w = (next_content_h * aspect).max(min_content_w);
            }

            let start_center_x = start_content.x + start_content.w * 0.5;
            let start_center_y = start_content.y + start_content.h * 0.5;
            let next_content_x = if (drag.edge_mask & UI2_WINDOW_RESIZE_LEFT) != 0 && has_horizontal
            {
                content_right - next_content_w
            } else if (drag.edge_mask & UI2_WINDOW_RESIZE_RIGHT) != 0 && has_horizontal {
                start_content.x
            } else {
                start_center_x - next_content_w * 0.5
            };
            let next_content_y = if (drag.edge_mask & UI2_WINDOW_RESIZE_TOP) != 0 && has_vertical {
                content_bottom - next_content_h
            } else if (drag.edge_mask & UI2_WINDOW_RESIZE_BOTTOM) != 0 && has_vertical {
                start_content.y
            } else {
                start_center_y - next_content_h * 0.5
            };

            next = Ui2Rect::new(
                next_content_x - inset_left,
                next_content_y - inset_top,
                next_content_w + inset_left + inset_right,
                next_content_h + inset_top + inset_bottom,
            );
        }
    }

    if drag.live_apply {
        if window.rect != next {
            if let Some(window) = window_mut(state, drag.window_id) {
                window.rect = next;
                window.restore_rect = next;
            }
            state.compose_reason = "window-resize-drag";
            let _ = note_window_dirty(state, drag.window_id, "window-resize-drag");
            let _ = note_window_viewport_sync_needed(state, drag.window_id);
            refresh_window_hit_entries(state, drag.window_id);
        }
    } else if drag.preview_rect != next {
        state.resize_drags[drag_idx].preview_rect = next;
        state.compose_reason = "window-resize-preview";
        let _ = note_window_dirty(state, drag.window_id, "window-resize-preview");
    }
}
