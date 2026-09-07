//! Xe-LP single-level, single-layer, single-sample D32 auxiliary contract.
//! Derived from Mesa ISL HIZ (8x4 pixels / 128-bit element, 16x16-element
//! tiles occupying 128x32 bytes), gen120.xml and BLORP's gfx8+ HiZ sequence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Layout {
    pub pitch: u32,
    pub qpitch: u32,
    pub bytes: usize,
    pub width: u32,
    pub height: u32,
}
pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) fn layout(device: u16, width: usize, height: usize) -> Option<Layout> {
    if !matches!(device, 0xa780 | 0x4680)
        || width == 0
        || height == 0
        || width > 2560
        || height > 1440
    {
        return None;
    }
    let pitch = width.div_ceil(128) * 128;
    let rows = height.div_ceil(64) * 32;
    let bytes = pitch * rows;
    if bytes > MAX_BYTES {
        return None;
    }
    Some(Layout {
        pitch: pitch as u32,
        qpitch: (height.div_ceil(16) * 4) as u32,
        bytes,
        width: (width.div_ceil(8) * 8) as u32,
        height: (height.div_ceil(4) * 4) as u32,
    })
}
pub(crate) fn clear_packet(layout: Layout) -> [u32; 5] {
    [
        0x7852_0003,
        (1 << 30) | (1 << 25),
        0,
        layout.width | (layout.height << 16),
        0xffff,
    ]
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn maximum_and_odd_extents_fit_owned_storage() {
        assert_eq!(
            layout(0xa780, 2560, 1440),
            Some(Layout {
                pitch: 2560,
                qpitch: 360,
                bytes: 1_884_160,
                width: 2560,
                height: 1440
            })
        );
        assert_eq!(
            layout(0x4680, 784, 441),
            Some(Layout {
                pitch: 896,
                qpitch: 112,
                bytes: 200_704,
                width: 784,
                height: 444
            })
        );
        for w in [1, 7, 8, 127, 128, 129, 2559, 2560] {
            for h in [1, 3, 4, 63, 64, 65, 1439, 1440] {
                let l = layout(0xa780, w, h).unwrap();
                assert_eq!(l.bytes % 4096, 0);
                assert!(l.bytes <= MAX_BYTES && l.width as usize >= w && l.height as usize >= h);
            }
        }
    }
    #[test]
    fn unsupported_layouts_never_enable_hiz() {
        for (d, w, h) in [
            (0x56a0, 784, 441),
            (0xa780, 0, 1),
            (0xa780, 2561, 1440),
            (0xa780, 1, 1441),
        ] {
            assert!(layout(d, w, h).is_none());
        }
    }
    #[test]
    fn fast_clear_is_full_surface_not_a_resolve_or_stencil_operation() {
        assert_eq!(
            clear_packet(layout(0xa780, 785, 443).unwrap()),
            [0x78520003, 0x42000000, 0, (444 << 16) | 792, 0xffff]
        );
    }
}
