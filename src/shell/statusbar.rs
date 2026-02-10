use super::matrix;

pub const INDICATOR_COUNT: usize = matrix::STATUS_INDICATORS;
pub const TEXT_WIDTH: usize = matrix::STATUS_TEXT_LEN;

pub type IndicatorCode = u8;

pub use super::matrix::StatusBarSnapshot;

#[inline]
pub fn set_active_slot(slot_id: u8) -> bool {
    matrix::set_active_status_slot(slot_id)
}

#[inline]
pub fn active_slot() -> Option<u8> {
    matrix::active_status_slot()
}

#[inline]
pub fn clear_active_slot() {
    matrix::clear_active_status_slot()
}

#[inline]
pub fn set_left(slot_id: u8, text: &str) -> bool {
    matrix::status_set_left(slot_id, text)
}

#[inline]
pub fn set_right(slot_id: u8, text: &str) -> bool {
    matrix::status_set_right(slot_id, text)
}

#[inline]
pub fn set_indicator(slot_id: u8, index: usize, color_code: IndicatorCode) -> bool {
    matrix::status_set_indicator(slot_id, index, color_code)
}

#[inline]
pub fn set_left_active(text: &str) -> bool {
    matrix::status_set_left_active(text)
}

#[inline]
pub fn set_right_active(text: &str) -> bool {
    matrix::status_set_right_active(text)
}

#[inline]
pub fn set_indicator_active(index: usize, color_code: IndicatorCode) -> bool {
    matrix::status_set_indicator_active(index, color_code)
}

#[inline]
pub fn snapshot_active() -> Option<StatusBarSnapshot> {
    matrix::active_status_snapshot()
}

