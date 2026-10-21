



#[cfg(test)]
mod rgba16_float_volume_tests {
    use super::{
        GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL, GpgpuRgba16FloatVolume3d,
        GpgpuVolumePhysicalBacking,
    };

    const PHYS: u64 = 0x0000_0000_1234_5000;
    const GPU: u64 = 0x0000_0000_4000_0000;

    #[test]
    fn tight_volume_has_the_expected_linear_layout() {
        let width = 16u32;
        let height = 8u32;
        let depth = 4u32;
        let bytes = width as usize
            * height as usize
            * depth as usize
            * GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL;
        let volume = GpgpuRgba16FloatVolume3d::tight(PHYS, GPU, bytes, width, height, depth)
            .expect("tightly packed rgba16f volume");

        assert_eq!(volume.row_pitch_bytes, 128);
        assert_eq!(volume.slice_pitch_bytes, 1024);
        assert_eq!(volume.required_bytes(), Some(4096));
        assert_eq!(bytes, 4096);
        assert_eq!(
            volume.voxel_byte_offset(width - 1, height - 1, depth - 1),
            Some(bytes - GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL)
        );
        assert_eq!(bytes * 2, 8192);

        let paged = GpgpuRgba16FloatVolume3d::from_exact_ppgtt_pages(
            GPU,
            bytes,
            width,
            height,
            depth,
            128,
            1024,
        )
        .expect("exact-page PPGTT volume");
        assert_eq!(paged.backing, GpgpuVolumePhysicalBacking::ExactPpgttPages);
        assert_eq!(paged.required_bytes(), Some(bytes));
    }

    #[test]
    fn pitched_volume_keeps_slice_and_row_addressing_explicit() {
        let volume = GpgpuRgba16FloatVolume3d::new(
            PHYS,
            GPU,
            16 * 1024,
            8,
            4,
            3,
            128,
            1024,
        )
        .expect("pitched rgba16f volume");
        assert_eq!(volume.voxel_byte_offset(2, 3, 1), Some(1024 + 384 + 16));
    }

    #[test]
    fn rejects_ambiguous_or_undersized_layouts() {
        assert!(GpgpuRgba16FloatVolume3d::tight(PHYS + 1, GPU, 4096, 4, 4, 4).is_none());
        assert!(GpgpuRgba16FloatVolume3d::tight(PHYS, GPU + 1, 4096, 4, 4, 4).is_none());
        assert!(GpgpuRgba16FloatVolume3d::new(PHYS, GPU, 4096, 8, 4, 3, 32, 128).is_none());
        assert!(GpgpuRgba16FloatVolume3d::new(PHYS, GPU, 4096, 8, 4, 3, 64, 128).is_none());
        assert!(GpgpuRgba16FloatVolume3d::tight(PHYS, GPU, 511, 4, 4, 4).is_none());
        assert!(
            GpgpuRgba16FloatVolume3d::from_exact_ppgtt_pages(
                GPU, 4095, 4, 4, 4, 32, 128,
            )
            .is_none()
        );
    }
}
