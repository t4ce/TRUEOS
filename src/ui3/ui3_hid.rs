use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use trueos_gfx_core::Rgba8;

static UI3_CURSOR_CAP_DROP_COUNT: AtomicU32 = AtomicU32::new(0);
const UI3_CURSOR_CAP: usize = 32;
const UI3_WINDOW_CURSOR_EVENT_CAP: usize = 256;
pub(crate) const UI3_FUN_CURSOR_ICONS_ENABLED: bool = true;
const UI3_CURSOR_SPIRIT_DEFAULTS: [char; 6] = ['🦋', '🦊', '🦎', '🦁', '🦄', '🐕'];
const UI3_CURSOR_SPIRIT_CHOICES: [char; 24] = [
    '🦋', '🦊', '🦎', '🦁', '🦄', '🐕', '🐈', '🐇', '🐢', '🐙', '🐳', '🐬', '🐘', '🦕', '🦖', '🦉',
    '🦜', '🦚', '🦩', '🐝', '🐞', '🦀', '🐌', '🐧',
];
static UI3_CURSOR_SPIRIT_OVERRIDES: Mutex<[char; UI3_CURSOR_CAP]> =
    Mutex::new(['\0'; UI3_CURSOR_CAP]);
pub(crate) const UI3_CURSOR_BUTTON_LEFT: u32 = 1 << 0;
pub(crate) const UI3_CURSOR_BUTTON_RIGHT: u32 = 1 << 1;
pub(crate) const UI3_CURSOR_EVENT_FLAG_MOTION: u32 = 1 << 0;
pub(crate) const UI3_CURSOR_EVENT_FLAG_WHEEL: u32 = 1 << 1;
pub(crate) const UI3_CURSOR_EVENT_FLAG_BUTTONS: u32 = 1 << 2;
const UI3_CURSOR_CROSS_HALF_SPAN: u32 = 9;
const UI3_CURSOR_CROSS_THICKNESS: u32 = 2;
const UI3_CONTEXT_MENU_WIDTH: u32 = 220;
const UI3_CONTEXT_MENU_HEIGHT: u32 = 132;
const UI3_CONTEXT_MENU_BG: Rgba8 = Rgba8::new(248, 250, 252, 238);
const UI3_CONTEXT_MENU_BORDER: Rgba8 = Rgba8::new(15, 23, 42, 255);
const UI3_CONTEXT_MENU_RULE: Rgba8 = Rgba8::new(148, 163, 184, 180);
const UI3_SELECTION_RGBA: Rgba8 = Rgba8::new(59, 130, 246, 72);

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3CursorEventDrain {
    pub(crate) read_seq: u64,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3CursorEventRead {
    pub(crate) next_seq: u64,
    pub(crate) dropped: u32,
    pub(crate) wrote: usize,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct Ui3CursorSnapshot {
    pub(crate) cursor_id: u32,
    pub(crate) slot_id: u32,
    pub(crate) x_norm: f64,
    pub(crate) y_norm: f64,
    pub(crate) x_px: u32,
    pub(crate) y_px: u32,
    pub(crate) buttons_down: u32,
    pub(crate) color: Rgba8,
}

impl Default for Ui3CursorSnapshot {
    fn default() -> Self {
        Self {
            cursor_id: 0,
            slot_id: 0,
            x_norm: 0.0,
            y_norm: 0.0,
            x_px: 0,
            y_px: 0,
            buttons_down: 0,
            color: Rgba8::new(0, 0, 0, 0),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui3CursorColor {
    Blue,
    Red,
    Green,
    Amber,
    Violet,
    Cyan,
}

impl Ui3CursorColor {
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
        UI3_CURSOR_SPIRIT_DEFAULTS[self as usize]
    }
}

#[inline]
pub(crate) fn cursor_spirit_choices() -> &'static [char] {
    &UI3_CURSOR_SPIRIT_CHOICES
}

#[inline]
fn cursor_spirit_override(slot_id: u32) -> Option<char> {
    let idx = usize::try_from(slot_id.checked_sub(1)?).ok()?;
    if idx >= UI3_CURSOR_CAP {
        return None;
    }
    let ch = UI3_CURSOR_SPIRIT_OVERRIDES.lock()[idx];
    (ch != '\0').then_some(ch)
}

pub(crate) fn set_cursor_spirit_glyph(slot_id: u32, glyph: char) -> bool {
    let Some(idx) = slot_id
        .checked_sub(1)
        .and_then(|idx| usize::try_from(idx).ok())
    else {
        return false;
    };
    if idx >= UI3_CURSOR_CAP || !UI3_CURSOR_SPIRIT_CHOICES.contains(&glyph) {
        return false;
    }
    UI3_CURSOR_SPIRIT_OVERRIDES.lock()[idx] = glyph;
    true
}

#[inline]
pub(crate) fn cursor_color(slot_id: u32) -> (u8, u8, u8, u8) {
    let color = cursor_color_rgba8(slot_id);
    (color.r, color.g, color.b, color.a)
}

#[inline]
pub(crate) fn cursor_color_rgba8(slot_id: u32) -> Rgba8 {
    Ui3CursorColor::from_slot_id(slot_id).rgba8()
}

#[inline]
pub(crate) fn cursor_spirit_glyph(slot_id: u32) -> Option<char> {
    if !UI3_FUN_CURSOR_ICONS_ENABLED {
        return None;
    }
    cursor_spirit_override(slot_id)
        .or_else(|| Some(Ui3CursorColor::from_slot_id(slot_id).spirit_glyph()))
}

#[inline]
pub(crate) fn cursor_color_for_cursor_id(cursor_id: u32) -> (u8, u8, u8, u8) {
    Ui3CursorColor::from_cursor_id(cursor_id).rgba()
}

#[inline]
pub(crate) fn cursor_color_rgba8_for_cursor_id(cursor_id: u32) -> Rgba8 {
    let (r, g, b, a) = cursor_color_for_cursor_id(cursor_id);
    Rgba8::new(r, g, b, a)
}

#[inline]
pub(crate) fn cursor_spirit_glyph_for_cursor_id(cursor_id: u32) -> Option<char> {
    UI3_FUN_CURSOR_ICONS_ENABLED.then_some(Ui3CursorColor::from_cursor_id(cursor_id).spirit_glyph())
}

#[inline]
pub(crate) fn cursor_event_cap_drop_count() -> u32 {
    UI3_CURSOR_CAP_DROP_COUNT.load(Ordering::Acquire)
}

pub(crate) fn drain_cursor_events(
    drain: &mut Ui3CursorEventDrain,
    out: &mut [crate::usb2::hid::TrueosHidCursorEvent],
) -> Ui3CursorEventRead {
    let cap = out.len().min(UI3_WINDOW_CURSOR_EVENT_CAP);
    let (next_seq, dropped, wrote) =
        crate::usb2::hid::read_cursor_events_since(drain.read_seq, &mut out[..cap]);
    drain.read_seq = next_seq;
    if dropped != 0 {
        UI3_CURSOR_CAP_DROP_COUNT.fetch_add(dropped, Ordering::AcqRel);
    }
    Ui3CursorEventRead {
        next_seq,
        dropped,
        wrote,
    }
}

pub(crate) fn ordered_cursor_snapshots(
    viewport_width: u32,
    viewport_height: u32,
) -> Vec<Ui3CursorSnapshot> {
    let mut out = Vec::new();
    let cursors = crate::r::cursor::ordered_cursor_snapshot_with_slot_buttons();
    for (idx, (slot_id, x_norm, y_norm, buttons_down)) in cursors.into_iter().enumerate() {
        let cursor_id = (idx as u32).saturating_add(1);
        out.push(cursor_snapshot_from_norm(
            cursor_id,
            slot_id,
            x_norm,
            y_norm,
            buttons_down,
            viewport_width,
            viewport_height,
        ));
        if out.len() >= UI3_CURSOR_CAP {
            break;
        }
    }
    out
}

pub(crate) fn preferred_cursor_snapshot(
    viewport_width: u32,
    viewport_height: u32,
) -> Option<Ui3CursorSnapshot> {
    let (slot_id, x_norm, y_norm, buttons_down) =
        crate::r::cursor::preferred_kernel_hw_cursor_snapshot_with_slot_buttons()?;
    Some(cursor_snapshot_from_norm(
        1,
        slot_id,
        x_norm,
        y_norm,
        buttons_down,
        viewport_width,
        viewport_height,
    ))
}

pub(crate) fn push_software_cursor_rects(
    rects: &mut Vec<crate::intel::LiveOverlayRect>,
    viewport_width: u32,
    viewport_height: u32,
) {
    let cursors = ordered_cursor_snapshots(viewport_width, viewport_height);
    for cursor in cursors {
        push_cursor_cross(rects, cursor);
    }
}

pub(crate) fn push_context_menu_rects(
    rects: &mut Vec<crate::intel::LiveOverlayRect>,
    x: u32,
    y: u32,
    viewport_width: u32,
    viewport_height: u32,
) {
    let menu_w = UI3_CONTEXT_MENU_WIDTH.min(viewport_width);
    let menu_h = UI3_CONTEXT_MENU_HEIGHT.min(viewport_height);
    if menu_w == 0 || menu_h == 0 {
        return;
    }
    let x = x.min(viewport_width.saturating_sub(menu_w));
    let y = y.min(viewport_height.saturating_sub(menu_h));
    rects.push(crate::intel::LiveOverlayRect::new(x, y, menu_w, menu_h, UI3_CONTEXT_MENU_BG));
    push_outline_rect(rects, x, y, menu_w, menu_h, 2, UI3_CONTEXT_MENU_BORDER);
    let rule_y = y
        .saturating_add(42)
        .min(y.saturating_add(menu_h.saturating_sub(1)));
    rects.push(crate::intel::LiveOverlayRect::new(
        x.saturating_add(10),
        rule_y,
        menu_w.saturating_sub(20),
        1,
        UI3_CONTEXT_MENU_RULE,
    ));
}

pub(crate) fn push_outline_rect(
    rects: &mut Vec<crate::intel::LiveOverlayRect>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    thickness: u32,
    color: Rgba8,
) {
    if width == 0 || height == 0 || thickness == 0 {
        return;
    }
    let t = thickness.min(width).min(height);
    rects.push(crate::intel::LiveOverlayRect::new(x, y, width, t, color));
    rects.push(crate::intel::LiveOverlayRect::new(
        x,
        y.saturating_add(height.saturating_sub(t)),
        width,
        t,
        color,
    ));
    rects.push(crate::intel::LiveOverlayRect::new(x, y, t, height, color));
    rects.push(crate::intel::LiveOverlayRect::new(
        x.saturating_add(width.saturating_sub(t)),
        y,
        t,
        height,
        color,
    ));
}

pub(crate) fn push_merged_selection_rects(
    rects: &mut Vec<crate::intel::LiveOverlayRect>,
    source: &[(u32, u32, u32, u32)],
) {
    let mut merged: Vec<crate::intel::LiveOverlayRect> = Vec::new();
    for &(x, y, width, height) in source {
        if width == 0 || height == 0 {
            continue;
        }
        let mut did_merge = false;
        for existing in &mut merged {
            let same_band = existing.y == y && existing.height == height;
            let existing_end = existing.x.saturating_add(existing.width);
            let next_end = x.saturating_add(width);
            if same_band && x <= existing_end.saturating_add(1) && next_end >= existing.x {
                let new_x = existing.x.min(x);
                existing.width = existing_end.max(next_end).saturating_sub(new_x);
                existing.x = new_x;
                did_merge = true;
                break;
            }
        }
        if !did_merge {
            merged.push(crate::intel::LiveOverlayRect::new(
                x,
                y,
                width,
                height,
                UI3_SELECTION_RGBA,
            ));
        }
    }
    rects.extend(merged);
}

pub(crate) fn push_drag_selection_probe_rects(
    rects: &mut Vec<crate::intel::LiveOverlayRect>,
    start_x: u32,
    start_y: u32,
    current_x: u32,
    current_y: u32,
) {
    let x0 = start_x.min(current_x);
    let y0 = start_y.min(current_y);
    let x1 = start_x.max(current_x);
    let y1 = start_y.max(current_y);
    let width = x1.saturating_sub(x0).max(1);
    let height = y1.saturating_sub(y0).max(1);

    let first_w = (width / 3).max(1);
    let second_w = (width.saturating_sub(first_w) / 2).max(1);
    let third_x = x0.saturating_add(first_w).saturating_add(second_w);
    let third_w = x0.saturating_add(width).saturating_sub(third_x).max(1);
    let pieces = [
        (x0, y0, first_w, height),
        (x0.saturating_add(first_w), y0, second_w, height),
        (third_x, y0, third_w, height),
    ];
    push_merged_selection_rects(rects, &pieces);
    push_outline_rect(rects, x0, y0, width, height, 1, Ui3CursorColor::Blue.rgba8());
}

#[inline]
pub(crate) fn event_has_right_button(event: crate::usb2::hid::TrueosHidCursorEvent) -> bool {
    (event.buttons_down & UI3_CURSOR_BUTTON_RIGHT) != 0
}

#[inline]
pub(crate) fn event_has_button_change(event: crate::usb2::hid::TrueosHidCursorEvent) -> bool {
    (event.flags & UI3_CURSOR_EVENT_FLAG_BUTTONS) != 0
}

#[inline]
pub(crate) fn event_wheel_delta(event: crate::usb2::hid::TrueosHidCursorEvent) -> i32 {
    event.wheel as i32
}

pub(crate) fn event_position_px(
    event: crate::usb2::hid::TrueosHidCursorEvent,
    viewport_width: u32,
    viewport_height: u32,
) -> (u32, u32) {
    (norm_to_px(event.x, viewport_width), norm_to_px(event.y, viewport_height))
}

fn cursor_snapshot_from_norm(
    cursor_id: u32,
    slot_id: u32,
    x_norm: f64,
    y_norm: f64,
    buttons_down: u32,
    viewport_width: u32,
    viewport_height: u32,
) -> Ui3CursorSnapshot {
    let x_px = norm_to_px(x_norm, viewport_width);
    let y_px = norm_to_px(y_norm, viewport_height);
    Ui3CursorSnapshot {
        cursor_id,
        slot_id,
        x_norm,
        y_norm,
        x_px,
        y_px,
        buttons_down,
        color: cursor_color_rgba8_for_cursor_id(cursor_id),
    }
}

fn push_cursor_cross(rects: &mut Vec<crate::intel::LiveOverlayRect>, cursor: Ui3CursorSnapshot) {
    let color = cursor.color;
    let span = UI3_CURSOR_CROSS_HALF_SPAN
        .saturating_mul(2)
        .saturating_add(1);
    let x = cursor.x_px;
    let y = cursor.y_px;
    rects.push(crate::intel::LiveOverlayRect::new(
        x.saturating_sub(UI3_CURSOR_CROSS_HALF_SPAN),
        y.saturating_sub(UI3_CURSOR_CROSS_THICKNESS / 2),
        span,
        UI3_CURSOR_CROSS_THICKNESS,
        color,
    ));
    rects.push(crate::intel::LiveOverlayRect::new(
        x.saturating_sub(UI3_CURSOR_CROSS_THICKNESS / 2),
        y.saturating_sub(UI3_CURSOR_CROSS_HALF_SPAN),
        UI3_CURSOR_CROSS_THICKNESS,
        span,
        color,
    ));
    rects.push(crate::intel::LiveOverlayRect::new(x, y, 2, 2, Rgba8::new(0, 0, 0, 255)));
}

fn norm_to_px(value: f64, extent: u32) -> u32 {
    if extent == 0 || !value.is_finite() {
        return 0;
    }
    let max_px = extent.saturating_sub(1) as f64;
    libm::round(value.clamp(0.0, 1.0) * max_px).clamp(0.0, max_px) as u32
}
