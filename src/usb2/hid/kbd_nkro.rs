//! NKRO keyboard report helpers.
//!
//! This module is intentionally not wired into USB interface claiming yet. It
//! gives future HID drivers a small, shared adapter from bitmap-style keyboard
//! reports into the HUT keyboard state shape used by boot keyboards.

#![allow(dead_code)]

use super::hut;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct NkroBitmapLayout {
    pub report_id: Option<u8>,
    pub bitmap_offset: usize,
    pub first_usage: u8,
    pub usage_count: u16,
}

impl NkroBitmapLayout {
    pub const fn boot_page_bitmap(bitmap_offset: usize, usage_count: u16) -> Self {
        Self {
            report_id: None,
            bitmap_offset,
            first_usage: 0,
            usage_count,
        }
    }

    pub const fn with_report_id(
        report_id: u8,
        bitmap_offset: usize,
        first_usage: u8,
        usage_count: u16,
    ) -> Self {
        Self {
            report_id: Some(report_id),
            bitmap_offset,
            first_usage,
            usage_count,
        }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub struct NkroKeyboardState {
    pub key_down_bits: [u32; 8],
    pub modifiers: u8,
    pub boot_keys: [u8; 6],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum NkroParseError {
    ReportIdMismatch,
    ReportTooShort,
    UsageRangeOutsideKeyboardPage,
}

#[inline]
fn required_bitmap_bytes(usage_count: u16) -> usize {
    usize::from(usage_count).saturating_add(7) / 8
}

pub fn parse_bitmap_report(
    layout: NkroBitmapLayout,
    report: &[u8],
) -> Result<NkroKeyboardState, NkroParseError> {
    if let Some(report_id) = layout.report_id {
        if report.first().copied() != Some(report_id) {
            return Err(NkroParseError::ReportIdMismatch);
        }
    }

    let usage_end = u16::from(layout.first_usage).saturating_add(layout.usage_count);
    if usage_end > 256 {
        return Err(NkroParseError::UsageRangeOutsideKeyboardPage);
    }

    let byte_count = required_bitmap_bytes(layout.usage_count);
    let bitmap_end = layout.bitmap_offset.saturating_add(byte_count);
    if report.len() < bitmap_end {
        return Err(NkroParseError::ReportTooShort);
    }

    let bitmap = &report[layout.bitmap_offset..bitmap_end];
    let mut key_down_bits = [0u32; 8];
    for usage_idx in 0..layout.usage_count {
        let byte = bitmap[usize::from(usage_idx / 8)];
        if (byte & (1u8 << (usage_idx % 8))) == 0 {
            continue;
        }
        let usage = u16::from(layout.first_usage).saturating_add(usage_idx);
        hut::set_key_down_bit(&mut key_down_bits, usage as u8);
    }

    Ok(NkroKeyboardState {
        key_down_bits,
        modifiers: hut::keyboard_modifiers_from_key_down_bits(&key_down_bits),
        boot_keys: hut::keyboard_boot_keys_from_key_down_bits(&key_down_bits),
    })
}

pub fn upsert_hut_bitmap_report(
    controller_id: u32,
    slot_id: u32,
    ep_target: u32,
    layout: NkroBitmapLayout,
    report: &[u8],
    source_kind: hut::HidSourceKind,
    source_tag: &str,
) -> Result<NkroKeyboardState, NkroParseError> {
    let state = parse_bitmap_report(layout, report)?;
    hut::upsert_keyboard_nkro_state(
        controller_id,
        slot_id,
        ep_target,
        state.key_down_bits,
        source_kind,
        source_tag,
        false,
    );
    Ok(state)
}
