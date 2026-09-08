//! Two independently published surfaces, one window and one stacking position.
//!
//! The current display backend reserves slots 1/2 for background/foreground.
//! Ordinary windows below the pair compose on slot 0; those above compose on
//! slot 3. This preserves arbitrary window order without splitting the pair.
//! Slot 4 remains exclusively owned by interaction chrome.

pub(super) const BACKGROUND_SLOT: usize = 1;
pub(super) const FOREGROUND_SLOT: usize = 2;
pub(super) const REQUIRED_PLANE_MASK: u8 = 0b1111;

pub(super) const fn ordinary_slot(above_pair: bool) -> usize {
    if above_pair { 3 } else { 0 }
}

/// Render-target tokens have their own namespace within the existing u32
/// surface ABI. Broker window slots occupy only low words 1..=256. Bit 15
/// identifies the second surface without consuming a second WindowRecord or
/// weakening the full 16-bit generation check. Tokens are never input IDs.
pub(super) const BACKGROUND_TARGET_BIT: u32 = 1 << 15;

pub(super) const fn background_target(window: u32) -> u32 {
    window | BACKGROUND_TARGET_BIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_is_adjacent_and_every_ordinary_window_stays_outside_it() {
        assert_eq!(FOREGROUND_SLOT, BACKGROUND_SLOT + 1);
        assert!(ordinary_slot(false) < BACKGROUND_SLOT);
        assert!(ordinary_slot(true) > FOREGROUND_SLOT);
        assert_eq!(REQUIRED_PLANE_MASK & (1 << 4), 0);
    }

    #[test]
    fn surface_namespace_preserves_generation_and_cannot_alias_a_window() {
        for generation in [1u32, 2, 32768, 65535] {
            for slot in 1..=256 {
                let window = generation << 16 | slot;
                let target = background_target(window);
                assert_eq!(target >> 16, generation);
                assert!(target as u16 > 256);
                assert_eq!(target & !BACKGROUND_TARGET_BIT, window);
            }
        }
    }
}
