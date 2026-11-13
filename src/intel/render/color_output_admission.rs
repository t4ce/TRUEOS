//! Admission for a color-only direct target matching the physical output.
//! Compatibility scratch, MSAA, depth and Blueprint extent policies remain
//! independent of this kernel producer's active-output exception.

pub(super) fn active_color_extent_admitted(
    width: u32,
    height: u32,
    active_output: Option<(u32, u32)>,
    direct_limit: (u32, u32),
    pci_identity: u32,
) -> bool {
    matches!(
        pci_identity,
        0x9A49_8086 | 0x4680_8086 | 0x46D1_8086 | 0xA780_8086
    ) && width != 0
        && height != 0
        && width <= direct_limit.0
        && height <= direct_limit.1
        && active_output == Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::active_color_extent_admitted as admitted;
    const LIMIT: (u32, u32) = (3840, 2160);

    #[test]
    fn active_4k_is_admitted_on_known_tgl_adl_and_rpl_devices() {
        for device in [0x9A49_8086, 0x4680_8086, 0x46D1_8086, 0xA780_8086] {
            assert!(admitted(3840, 2160, Some((3840, 2160)), LIMIT, device));
            assert!(admitted(1920, 1080, Some((1920, 1080)), LIMIT, device));
        }
    }

    #[test]
    fn rejects_unclaimed_or_different_output_unknown_hardware_and_oversized_targets() {
        let device = 0xA780_8086;
        assert!(!admitted(3840, 2160, None, LIMIT, device));
        assert!(!admitted(3840, 2160, Some((2560, 1440)), LIMIT, device));
        assert!(!admitted(2560, 1440, Some((3840, 2160)), LIMIT, device));
        assert!(!admitted(
            3840,
            2160,
            Some((3840, 2160)),
            LIMIT,
            0xA780_1234
        ));
        assert!(!admitted(
            3840,
            2160,
            Some((3840, 2160)),
            LIMIT,
            0xFFFF_8086
        ));
        for extent in [(0, 0), (3840, 0), (0, 2160), (4096, 2160), (3840, 2161)] {
            assert!(!admitted(extent.0, extent.1, Some(extent), LIMIT, device));
        }
    }
}
