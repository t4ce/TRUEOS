//! Address reservations for the single native 3840x2160 Tiger Lake panel.
//!
//! GGTT scanout addresses and compositor-private PPGTT addresses are distinct.
//! Keep the retained panel probe at E0000000 intact, including after a failed
//! handoff. These helpers do not publish readiness or imply GPU execution.

pub(super) const SURFACE_SLOT_BYTES: u64 = 0x0400_0000;
pub(super) const GGTT_BASE: u64 = 0xD000_0000;
const PROBE_GPU: u64 = 0xE000_0000;
const COMPOSE_BASE: u64 = 0x2000_0000;
const GGTT_LIMIT: u64 = 0x1_0000_0000;
const COMPOSE_LIMIT: u64 = 0x4000_0000;

pub(super) const fn primary_gpu() -> u64 {
    GGTT_BASE
}

pub(super) const fn primary_swap_gpu(index: usize) -> Option<u64> {
    if index >= 2 {
        return None;
    }
    Some(GGTT_BASE + (1 + index as u64) * SURFACE_SLOT_BYTES)
}

pub(super) const fn overlay_gpu(plane_slot: usize, index: usize) -> Option<u64> {
    if plane_slot < 1 || plane_slot > 4 || index >= 2 {
        return None;
    }
    // Slots 0..2 hold primary + its two swap surfaces. Skip slot 4, the
    // still-retained E0000000 native-panel probe, rather than remapping it.
    let packed_slot = 3 + (plane_slot as u64 - 1) * 2 + index as u64;
    let slot = if packed_slot >= 4 { packed_slot + 1 } else { packed_slot };
    Some(GGTT_BASE + slot * SURFACE_SLOT_BYTES)
}

pub(super) const fn compose_gpu(plane_slot: usize, index: usize) -> Option<u64> {
    // Only application slots 0..3 use this isolated compositor PPGTT.
    // Slot 4 remains the independently CPU-authored interaction overlay.
    if plane_slot >= 4 || index >= 2 {
        return None;
    }
    Some(COMPOSE_BASE + (plane_slot as u64 * 2 + index as u64) * SURFACE_SLOT_BYTES)
}

const _: () = {
    let guarded_bytes = (3840u64 + 64) * 4 * (2160 + 64);
    assert!(guarded_bytes > 0x0200_0000);
    assert!(guarded_bytes <= SURFACE_SLOT_BYTES);
    assert!(GGTT_BASE % 4096 == 0);
    assert!(GGTT_BASE + 4 * SURFACE_SLOT_BYTES == PROBE_GPU);
    assert!(GGTT_BASE + 12 * SURFACE_SLOT_BYTES == GGTT_LIMIT);
    assert!(COMPOSE_BASE + 8 * SURFACE_SLOT_BYTES == COMPOSE_LIMIT);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guarded_native_surface_fits_without_overrunning_its_slot() {
        let pitch = ((3840u64 + 64) * 4 + 63) & !63;
        let bytes = pitch * (2160 + 64);
        assert_eq!(pitch, 15_616);
        assert_eq!(bytes, 34_729_984);
        assert!(bytes > 0x0200_0000);
        assert!(bytes <= SURFACE_SLOT_BYTES);
    }

    #[test]
    fn all_scanout_reservations_are_disjoint_and_leave_probe_intact() {
        let mut addresses = vec![primary_gpu(), primary_swap_gpu(0).unwrap(), primary_swap_gpu(1).unwrap()];
        for plane in 1..=4 {
            for index in 0..2 {
                addresses.push(overlay_gpu(plane, index).unwrap());
            }
        }
        assert_eq!(addresses.len(), 11);
        addresses.push(PROBE_GPU);
        addresses.sort_unstable();
        for pair in addresses.windows(2) {
            assert!(pair[0] + SURFACE_SLOT_BYTES <= pair[1]);
        }
        assert_eq!(addresses[0], GGTT_BASE);
        assert_eq!(addresses.last().unwrap() + SURFACE_SLOT_BYTES, GGTT_LIMIT);
    }

    #[test]
    fn compositor_slots_are_private_nonoverlapping_and_below_one_gib() {
        let mut previous_end = COMPOSE_BASE;
        for plane in 0..4 {
            for index in 0..2 {
                let address = compose_gpu(plane, index).unwrap();
                assert_eq!(address, previous_end);
                previous_end = address + SURFACE_SLOT_BYTES;
            }
        }
        assert_eq!(previous_end, COMPOSE_LIMIT);
        assert!(previous_end <= 1 << 30);
    }

    #[test]
    fn invalid_slots_do_not_alias_valid_buffers() {
        assert_eq!(primary_swap_gpu(2), None);
        assert_eq!(overlay_gpu(0, 0), None);
        assert_eq!(overlay_gpu(5, 0), None);
        assert_eq!(overlay_gpu(1, 2), None);
        assert_eq!(compose_gpu(4, 0), None);
        assert_eq!(compose_gpu(0, 2), None);
    }
}
