//! Reservations and pipe-A contracts for the adopted 3840x2160 Tiger Lake panel.
//!
//! Logical interaction slot 4 uses physical index 6. Extra application slot 5
//! uses physical index 4. Logical slot 6 maps to unused physical index 5. Scanout
//! GGTT and compositor PPGTT are separate; neither remaps the retained native
//! panel probe at E0000000.

// Keep the extra-plane experiment parked while comparing scanout against
// five carriers. Native 4K addresses, capacities and width-aware DBUF/WM
// remain selected independently; this is not a return to desktop geometry.
pub(super) const EXPANDED_PLANES_ENABLED: bool = false;

pub(super) const fn plane_count(expanded: bool) -> usize {
    if expanded { 7 } else { 5 }
}

pub(super) const fn application_mask(expanded: bool) -> u8 {
    if expanded { 0x2F } else { 0x0F }
}

pub(super) const fn profile_physical_plane_active(physical: usize, expanded: bool) -> bool {
    match profile_logical_slot(physical, expanded) {
        Some(logical) => logical == 4 || application_mask(expanded) & (1 << logical) != 0,
        None => false,
    }
}

pub(super) const fn profile_physical_slot(logical: usize, expanded: bool) -> Option<usize> {
    if expanded {
        physical_slot(logical)
    } else if logical < 5 {
        Some(logical)
    } else {
        None
    }
}

pub(super) const fn profile_logical_slot(physical: usize, expanded: bool) -> Option<usize> {
    if expanded {
        logical_slot(physical)
    } else if physical < 5 {
        Some(physical)
    } else {
        None
    }
}

pub(super) const PRIMARY_SLOT_BYTES: u64 = 0x0400_0000;
pub(super) const OVERLAY_SLOT_BYTES: u64 = 0x0200_0000;
pub(super) const GGTT_BASE: u64 = 0xD000_0000;
const PROBE_GPU: u64 = 0xE000_0000;
const OVERLAY_BASE: u64 = 0xDC00_0000;
const EXTRA_DIRECT_BASE: u64 = 0xF800_0000;
const EXTRA_DIRECT_ALIAS_COUNT: usize = 4;
const COMPOSE_BASE: u64 = 0x2000_0000;
const OVERLAY_COMPOSE_BASE: u64 = COMPOSE_BASE + 2 * PRIMARY_SLOT_BYTES;
const PRIMARY_SOURCE_GPU: u64 = 0x3800_0000;
const GGTT_LIMIT: u64 = 0x1_0000_0000;
const COMPOSE_LIMIT: u64 = 0x4000_0000;

pub(super) const DBUF_CTL_S2: usize = 0x44FE8;
pub(super) const DBUF_POWER_REQUEST: u32 = 1 << 31;
pub(super) const DBUF_POWER_STATE: u32 = 1 << 30;
const WATERMARK_LINES: u32 = 8;

pub(super) const fn physical_slot(logical_slot: usize) -> Option<usize> {
    match logical_slot {
        0..=3 => Some(logical_slot),
        4 => Some(6),
        5 => Some(4),
        6 => Some(5),
        _ => None,
    }
}

pub(super) const fn logical_slot(physical_slot: usize) -> Option<usize> {
    match physical_slot {
        0..=3 => Some(physical_slot),
        4 => Some(5),
        5 => Some(6),
        6 => Some(4),
        _ => None,
    }
}

/// Both TGL DBUF slices belong to pipe A while it is the only enabled pipe.
/// Preserve Spirit's cursor reservations 1008..1023 and 2032..2047.
///
/// Intel TGL Display PRM pp. 297-300 allows A to use S1+S2 alone, but A+C
/// confines A to S1. Capture must reject this contract until it can reallocate
/// both pipes' stacks; enabling a mirror must not borrow these ranges.
/// https://cdrdv2-public.intel.com/705833/intel-gfx-prm-osrc-tgl-vol-12-display-engine.pdf
pub(super) const fn dbuf_range(logical_slot: usize) -> Option<(u16, u16)> {
    match logical_slot {
        0 => Some((0, 274)),
        1 => Some((275, 549)),
        2 => Some((550, 824)),
        3 => Some((1024, 1298)),
        5 => Some((1299, 1573)),
        4 => Some((1574, 1848)),
        _ => None,
    }
}

/// Eight-scanline linear RGBA8 contract, sized for the actual plane width.
/// TGL Display PRM pp. 423-424: blocks/line=ceil(width*4/512)+1, then add one
/// result block. Full-width planes need 249.
/// Allocations strictly exceed those blocks plus the rounded-up 10% margin.
pub(super) const fn watermark(logical_slot: usize) -> Option<u32> {
    let width: u32 = match logical_slot {
        0..=5 => 3840,
        _ => return None,
    };
    let blocks_per_line = (width * 4).div_ceil(512) + 1;
    Some((1 << 31) | (WATERMARK_LINES << 14) | (WATERMARK_LINES * blocks_per_line + 1))
}

pub(super) const fn primary_gpu() -> u64 {
    GGTT_BASE
}

pub(super) const fn primary_swap_gpu(index: usize) -> Option<u64> {
    if index >= 2 {
        return None;
    }
    Some(GGTT_BASE + (1 + index as u64) * PRIMARY_SLOT_BYTES)
}

/// Linear overlays need 32 MiB each; only the primary needs guarded 64 MiB
/// buffers. Unused logical slot 6 has no allocation.
pub(super) const fn overlay_gpu(logical_slot: usize, index: usize) -> Option<u64> {
    if logical_slot < 1 || logical_slot > 5 || index >= 2 {
        return None;
    }
    let packed = OVERLAY_BASE + ((logical_slot as u64 - 1) * 2 + index as u64) * OVERLAY_SLOT_BYTES;
    Some(if packed >= PROBE_GPU {
        packed + PRIMARY_SLOT_BYTES
    } else {
        packed
    })
}

/// Four display-owned imports for the extra application on adopted pipe A.
/// Existing desktop/per-output import reservations stay unchanged below D000.
pub(super) const fn extra_direct_scanout_gpu(index: usize) -> Option<u64> {
    if index >= EXTRA_DIRECT_ALIAS_COUNT {
        return None;
    }
    Some(EXTRA_DIRECT_BASE + index as u64 * OVERLAY_SLOT_BYTES)
}

pub(super) const fn compose_gpu(logical_slot: usize, index: usize) -> Option<u64> {
    if index >= 2 {
        return None;
    }
    let overlay_index = match logical_slot {
        0 => return Some(COMPOSE_BASE + index as u64 * PRIMARY_SLOT_BYTES),
        1..=3 => logical_slot - 1,
        5 => 3,
        _ => return None,
    };
    Some(OVERLAY_COMPOSE_BASE + (overlay_index as u64 * 2 + index as u64) * OVERLAY_SLOT_BYTES)
}

pub(super) const fn primary_source_gpu() -> u64 {
    PRIMARY_SOURCE_GPU
}

const _: () = {
    let guarded_bytes = (3840u64 + 64) * 4 * (2160 + 64);
    assert!(guarded_bytes > OVERLAY_SLOT_BYTES);
    assert!(guarded_bytes <= PRIMARY_SLOT_BYTES);
    assert!(3840 * 2160 * 4 <= OVERLAY_SLOT_BYTES);
    assert!(GGTT_BASE % 4096 == 0);
    assert!(GGTT_BASE + 3 * PRIMARY_SLOT_BYTES == OVERLAY_BASE);
    assert!(OVERLAY_BASE + 2 * OVERLAY_SLOT_BYTES == PROBE_GPU);
    assert!(OVERLAY_BASE + 10 * OVERLAY_SLOT_BYTES + PRIMARY_SLOT_BYTES <= EXTRA_DIRECT_BASE);
    assert!(EXTRA_DIRECT_BASE + EXTRA_DIRECT_ALIAS_COUNT as u64 * OVERLAY_SLOT_BYTES == GGTT_LIMIT);
    assert!(OVERLAY_COMPOSE_BASE + 8 * OVERLAY_SLOT_BYTES == PRIMARY_SOURCE_GPU);
    assert!(PRIMARY_SOURCE_GPU + PRIMARY_SLOT_BYTES <= COMPOSE_LIMIT);
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn physical_order_keeps_apps_below_interaction_and_unused_plane_disabled() {
        for (physical, logical) in [0, 1, 2, 3, 5, 6, 4].into_iter().enumerate() {
            assert_eq!(physical_slot(logical), Some(physical));
            assert_eq!(logical_slot(physical), Some(logical));
        }
        assert_eq!(physical_slot(7), None);
        assert_eq!(logical_slot(7), None);
        for physical in 0..8 {
            assert_eq!(profile_physical_plane_active(physical, false), physical < 5);
            assert_eq!(profile_physical_plane_active(physical, true),
                physical < 7 && physical != 5);
        }
    }

    #[test]
    fn actual_native_geometries_fit_their_reservations() {
        let pitch = ((3840u64 + 64) * 4 + 63) & !63;
        let guarded = pitch * (2160 + 64);
        assert_eq!(pitch, 15_616);
        assert_eq!(guarded, 34_729_984);
        assert!(guarded > OVERLAY_SLOT_BYTES && guarded <= PRIMARY_SLOT_BYTES);
        let overlay = ((3840u64 * 4 + 63) & !63) * 2160;
        assert_eq!(overlay, 33_177_600);
        assert!(overlay <= OVERLAY_SLOT_BYTES);
    }

    #[test]
    fn scanout_imports_and_retained_probe_never_overlap() {
        let mut ranges = vec![
            (primary_gpu(), PRIMARY_SLOT_BYTES),
            (primary_swap_gpu(0).unwrap(), PRIMARY_SLOT_BYTES),
            (primary_swap_gpu(1).unwrap(), PRIMARY_SLOT_BYTES),
            (PROBE_GPU, PRIMARY_SLOT_BYTES),
        ];
        for plane in 1..=5 {
            for index in 0..2 {
                ranges.push((overlay_gpu(plane, index).unwrap(), OVERLAY_SLOT_BYTES));
            }
        }
        for index in 0..EXTRA_DIRECT_ALIAS_COUNT {
            ranges.push((extra_direct_scanout_gpu(index).unwrap(), OVERLAY_SLOT_BYTES));
        }
        assert_eq!(ranges.len(), 18);
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            assert!(pair[0].0 + pair[0].1 <= pair[1].0);
            assert_eq!(pair[0].0 % 4096, 0);
        }
        assert_eq!(ranges[0].0, GGTT_BASE);
        let (last, bytes) = *ranges.last().unwrap();
        assert_eq!(last + bytes, GGTT_LIMIT);
        assert_eq!(overlay_gpu(1, 0), Some(0xDC00_0000));
        assert_eq!(overlay_gpu(2, 0), Some(0xE400_0000));
        assert_eq!(overlay_gpu(5, 1), Some(0xF200_0000));
    }

    #[test]
    fn compositor_destinations_and_primary_source_fit_below_one_gib() {
        let mut end = COMPOSE_BASE;
        for plane in [0, 1, 2, 3, 5] {
            let bytes = if plane == 0 {
                PRIMARY_SLOT_BYTES
            } else {
                OVERLAY_SLOT_BYTES
            };
            for index in 0..2 {
                assert_eq!(compose_gpu(plane, index), Some(end));
                end += bytes;
            }
        }
        assert_eq!(end, primary_source_gpu());
        assert!(primary_source_gpu() + PRIMARY_SLOT_BYTES <= COMPOSE_LIMIT);
        assert!(COMPOSE_LIMIT <= 1 << 30);
    }

    #[test]
    fn dbuf_ranges_preserve_every_cursor_reservation() {
        let mut ranges = Vec::new();
        for logical in 0..6 {
            let (start, end) = dbuf_range(logical).unwrap();
            assert!(start <= end && end < 2048);
            for (cursor_start, cursor_end) in [(1008, 1023), (2032, 2047)] {
                assert!(end < cursor_start || start > cursor_end);
            }
            ranges.push((start, end));
        }
        ranges.sort_unstable();
        for pair in ranges.windows(2) {
            assert!(pair[0].1 < pair[1].0);
        }
    }

    #[test]
    fn actual_width_watermarks_fit_with_strict_buffer_margin() {
        for logical in 0..6 {
            let raw = watermark(logical).unwrap();
            let blocks = raw & 0x7FF;
            assert_ne!(raw & (1 << 31), 0);
            assert_eq!((raw >> 14) & 0x1F, 8);
            let width = 3840u32;
            assert_eq!(blocks, 8 * ((width * 4).div_ceil(512) + 1) + 1);
            assert_eq!(blocks, 249);
            let (start, end) = dbuf_range(logical).unwrap();
            assert!(blocks + blocks.div_ceil(10) < u32::from(end - start + 1));
        }
    }

    #[test]
    fn invalid_roles_and_buffer_indices_cannot_alias_valid_surfaces() {
        assert_eq!(primary_swap_gpu(2), None);
        assert_eq!(overlay_gpu(0, 0), None);
        assert_eq!(overlay_gpu(6, 0), None);
        assert_eq!(overlay_gpu(1, 2), None);
        assert_eq!(compose_gpu(4, 0), None);
        assert_eq!(compose_gpu(6, 0), None);
        assert_eq!(compose_gpu(5, 2), None);
        assert_eq!(extra_direct_scanout_gpu(4), None);
        assert_eq!(dbuf_range(7), None);
        assert_eq!(watermark(7), None);
        assert_eq!(dbuf_range(6), None);
        assert_eq!(watermark(6), None);
    }
}
