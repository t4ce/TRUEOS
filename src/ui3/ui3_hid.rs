use super::*;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use trueos_gfx_core::Rgba8;

static UI3_CURSOR_CAP_DROP_COUNT: AtomicU32 = AtomicU32::new(0);
const UI3_WINDOW_CURSOR_EVENT_CAP: usize = 256;
pub(crate) const UI3_FUN_CURSOR_ICONS_ENABLED: bool = true;
const UI3_CURSOR_SPIRIT_DEFAULTS: [char; 6] = ['🦋', '🦊', '🦎', '🦁', '🦄', '🐕'];
const UI3_CURSOR_SPIRIT_CHOICES: [char; 24] = [
    '🦋', '🦊', '🦎', '🦁', '🦄', '🐕', '🐈', '🐇', '🐢', '🐙', '🐳', '🐬', '🐘', '🦕', '🦖', '🦉',
    '🦜', '🦚', '🦩', '🐝', '🐞', '🦀', '🐌', '🐧',
];
static UI3_CURSOR_SPIRIT_OVERRIDES: Mutex<[char; UI3_CURSOR_CAP]> =
    Mutex::new(['\0'; UI3_CURSOR_CAP]);

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