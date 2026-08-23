/// Bytes occupied by one RGBA16F voxel: four IEEE-754 binary16 channels.
#[allow(dead_code)]
pub(crate) const GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL: usize = 8;

/// Linear, page-backed 3D RGBA16F storage admitted to the direct-RCS PPGTT.
///
/// This is intentionally a memory-layout contract rather than an Intel sampler
/// surface-state encoding. Cloud compute can therefore start with the proven
/// stateful/stateless buffer path and software trilinear filtering, while a
/// later sampler bring-up can bind the exact same allocation as a 3D sampled
/// surface without changing simulation ownership or the JSON-derived ABI.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuRgba16FloatVolume3d {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) depth: u32,
    pub(crate) row_pitch_bytes: u32,
    pub(crate) slice_pitch_bytes: u32,
}

#[allow(dead_code)]
impl GpgpuRgba16FloatVolume3d {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        depth: u32,
        row_pitch_bytes: u32,
        slice_pitch_bytes: u32,
    ) -> Option<Self> {
        let volume = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            depth,
            row_pitch_bytes,
            slice_pitch_bytes,
        };
        volume.is_valid().then_some(volume)
    }

    /// Construct a tightly packed linear volume.
    pub(crate) fn tight(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        depth: u32,
    ) -> Option<Self> {
        let row_pitch_bytes = width.checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL as u32)?;
        let slice_pitch_bytes = row_pitch_bytes.checked_mul(height)?;
        Self::new(
            phys,
            gpu,
            bytes,
            width,
            height,
            depth,
            row_pitch_bytes,
            slice_pitch_bytes,
        )
    }

    pub(crate) fn required_bytes(self) -> Option<usize> {
        if self.width == 0 || self.height == 0 || self.depth == 0 {
            return None;
        }
        let row_bytes = (self.width as usize).checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL)?;
        let last_slice = (self.depth as usize)
            .checked_sub(1)?
            .checked_mul(self.slice_pitch_bytes as usize)?;
        let last_row = (self.height as usize)
            .checked_sub(1)?
            .checked_mul(self.row_pitch_bytes as usize)?;
        last_slice.checked_add(last_row)?.checked_add(row_bytes)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.phys == 0
            || self.gpu == 0
            || !self.phys.is_multiple_of(4096)
            || !self.gpu.is_multiple_of(4096)
            || self.width == 0
            || self.height == 0
            || self.depth == 0
        {
            return false;
        }

        let Some(row_bytes) = self
            .width
            .checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL as u32)
        else {
            return false;
        };
        if self.row_pitch_bytes < row_bytes
            || !self
                .row_pitch_bytes
                .is_multiple_of(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL as u32)
        {
            return false;
        }

        let Some(min_slice_pitch) = self.row_pitch_bytes.checked_mul(self.height) else {
            return false;
        };
        if self.slice_pitch_bytes < min_slice_pitch
            || !self.slice_pitch_bytes.is_multiple_of(self.row_pitch_bytes)
        {
            return false;
        }

        self.required_bytes().is_some_and(|required| required <= self.bytes)
    }

    /// Byte offset of one voxel inside this linear allocation.
    ///
    /// Useful for cold-path population, deterministic probes, and later
    /// software-vs-hardware sampler comparisons. Runtime kernels still index
    /// the mapped GPU address directly.
    pub(crate) fn voxel_byte_offset(self, x: u32, y: u32, z: u32) -> Option<usize> {
        if !self.is_valid() || x >= self.width || y >= self.height || z >= self.depth {
            return None;
        }
        (z as usize)
            .checked_mul(self.slice_pitch_bytes as usize)?
            .checked_add((y as usize).checked_mul(self.row_pitch_bytes as usize)?)?
            .checked_add((x as usize).checked_mul(GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL)?)
    }
}

#[cfg(test)]
mod rgba16_float_volume_tests {
    use super::{GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL, GpgpuRgba16FloatVolume3d};

    const PHYS: u64 = 0x0000_0000_1234_5000;
    const GPU: u64 = 0x0000_0000_4000_0000;

    #[test]
    fn cloud_reference_extent_has_the_expected_linear_layout() {
        let width = 96u32;
        let height = 48u32;
        let depth = 96u32;
        let bytes = width as usize
            * height as usize
            * depth as usize
            * GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL;
        let volume = GpgpuRgba16FloatVolume3d::tight(PHYS, GPU, bytes, width, height, depth)
            .expect("96x48x96 rgba16f volume");

        assert_eq!(volume.row_pitch_bytes, 768);
        assert_eq!(volume.slice_pitch_bytes, 36_864);
        assert_eq!(volume.required_bytes(), Some(3_538_944));
        assert_eq!(bytes, 3_538_944);
        assert_eq!(
            volume.voxel_byte_offset(width - 1, height - 1, depth - 1),
            Some(bytes - GPGPU_RGBA16_FLOAT_BYTES_PER_VOXEL)
        );
        assert_eq!(bytes * 2, 7_077_888, "ping-pong pair remains 6.75 MiB");
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
    }
}
