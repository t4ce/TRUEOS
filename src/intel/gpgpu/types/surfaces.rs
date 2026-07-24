#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuPoint {
    pub(crate) x: i32,
    pub(crate) y: i32,
}

impl GpgpuPoint {
    pub(crate) const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuRect {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl GpgpuRect {
    pub(crate) const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub(crate) const fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) enum GpgpuRgba8StorageOrder {
    /// Bytes in increasing memory order are R, G, B, A.
    #[default]
    Rgba,
    /// Bytes in increasing memory order are B, G, R, A while shader-facing
    /// channels remain logical RGBA. Intel ARGB cursor scanout needs this.
    Bgra,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) storage_order: GpgpuRgba8StorageOrder,
}

impl GpgpuRgba8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            storage_order: GpgpuRgba8StorageOrder::Rgba,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn new_bgra(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let mut surface = Self::new(phys, gpu, bytes, width, height, pitch_bytes)?;
        surface.storage_order = GpgpuRgba8StorageOrder::Bgra;
        Some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u32>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }

    pub(crate) const fn bounds(self) -> GpgpuRect {
        GpgpuRect::new(0, 0, self.width, self.height)
    }
}

/// Persistent linear RGBA8 storage owned by one GPU-side producer.
///
/// Gridpaper uses this for its immutable page base: geometry and static glyph
/// instances are rendered once through ordinary PAT0/WB mappings, then the C++
/// copy kernel reads it through the same cache policy to seed each PAT3/UC
/// scanout buffer before animated font instances are composited.
pub(crate) struct GpgpuOwnedRgba8Surface {
    surface: GpgpuRgba8Surface,
    virt: *mut u8,
}

unsafe impl Send for GpgpuOwnedRgba8Surface {}
unsafe impl Sync for GpgpuOwnedRgba8Surface {}

impl GpgpuOwnedRgba8Surface {
    pub(crate) const fn surface(&self) -> GpgpuRgba8Surface {
        self.surface
    }
}

impl Drop for GpgpuOwnedRgba8Surface {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.surface.bytes);
        recycle_font_coverage_gpu_va(self.surface.gpu, self.surface.bytes);
    }
}

/// Decoder-owned Xe media Tile64 NV12 storage mapped read-only by convention into the
/// compositor's private PPGTT.  The media engine's VA is only an opaque alias;
/// direct RCS installs its own PTEs for the same physical picture.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuNv12Tile64Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
    pub(crate) uv_offset: u32,
}

impl GpgpuNv12Tile64Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
        uv_offset: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
            uv_offset,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.phys == 0
            || self.gpu == 0
            || !self.phys.is_multiple_of(4096)
            || !self.gpu.is_multiple_of(4096)
            || self.width == 0
            || self.height == 0
            || self.pitch_bytes < self.width
            || !self.pitch_bytes.is_multiple_of(256)
            || self.uv_offset == 0
            || !self.uv_offset.is_multiple_of(self.pitch_bytes)
        {
            return false;
        }
        let chroma_row = self.uv_offset / self.pitch_bytes;
        if !chroma_row.is_multiple_of(256) {
            return false;
        }
        let Some(total_rows) = chroma_row
            .checked_add(self.height.div_ceil(2))
            .map(|rows| rows.next_multiple_of(256))
        else {
            return false;
        };
        let Some(required) = u64::from(total_rows).checked_mul(u64::from(self.pitch_bytes)) else {
            return false;
        };
        required <= self.bytes as u64
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgb565Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

impl GpgpuRgb565Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        if surface.is_valid() {
            Some(surface)
        } else {
            None
        }
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let min_pitch = self
            .width
            .saturating_mul(core::mem::size_of::<u16>() as u32);
        if self.pitch_bytes < min_pitch {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(min_pitch as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMask8Surface {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

pub(crate) struct GpgpuOwnedMask8Surface {
    surface: GpgpuMask8Surface,
    virt: *mut u8,
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuMask8Audit {
    pub(crate) nonzero_pixels: usize,
    pub(crate) bounds: GpgpuRect,
}

unsafe impl Send for GpgpuOwnedMask8Surface {}
unsafe impl Sync for GpgpuOwnedMask8Surface {}

impl GpgpuOwnedMask8Surface {
    pub(crate) const fn surface(&self) -> GpgpuMask8Surface {
        self.surface
    }

    /// Read back the persistent mask once after generation.  This is a cold
    /// path integrity check, not part of frame composition.
    pub(crate) fn nonzero_audit(&self) -> Option<GpgpuMask8Audit> {
        if !self.surface.is_valid() || self.virt.is_null() {
            return None;
        }
        super::dma_flush(self.virt, self.surface.bytes);
        let mut min_x = self.surface.width;
        let mut min_y = self.surface.height;
        let mut max_x = 0u32;
        let mut max_y = 0u32;
        let mut nonzero_pixels = 0usize;
        for y in 0..self.surface.height {
            let row_offset = (y as usize).checked_mul(self.surface.pitch_bytes as usize)?;
            for x in 0..self.surface.width {
                let offset = row_offset.checked_add(x as usize)?;
                let coverage = unsafe { core::ptr::read_volatile(self.virt.add(offset)) };
                if coverage == 0 {
                    continue;
                }
                nonzero_pixels = nonzero_pixels.saturating_add(1);
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
        (nonzero_pixels != 0).then_some(GpgpuMask8Audit {
            nonzero_pixels,
            bounds: GpgpuRect::new(
                min_x as i32,
                min_y as i32,
                max_x.saturating_sub(min_x).saturating_add(1),
                max_y.saturating_sub(min_y).saturating_add(1),
            ),
        })
    }
}

impl Drop for GpgpuOwnedMask8Surface {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.surface.bytes);
        recycle_font_coverage_gpu_va(self.surface.gpu, self.surface.bytes);
    }
}

pub(crate) const GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS: usize = 64;
pub(crate) const GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES: usize =
    GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS * core::mem::size_of::<u32>();
pub(crate) const GPGPU_FONT_INSTANCE_MAX_LAYERS: usize = 64;

const GPGPU_FONT_INSTANCE_MAGIC: u32 = 0x3149_5446;
const GPGPU_FONT_INSTANCE_FLAG_ENABLED: u32 = 1 << 0;
const GPGPU_FONT_INSTANCE_FLAG_BACKGROUND: u32 = 1 << 1;
const GPGPU_FONT_INSTANCE_FLAG_COLOR_ANIMATION: u32 = 1 << 2;
const GPGPU_FONT_INSTANCE_FLAG_TIMING_SINE: u32 = 1 << 3;
const GPGPU_FONT_INSTANCE_ITERATION_SHIFT: u32 = 4;
const GPGPU_FONT_INSTANCE_CHANNELS_SHIFT: u32 = 8;
const GPGPU_FONT_INSTANCE_FRAME_COUNT_SHIFT: u32 = 16;
const GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM: u32 = 1 << 20;
const GPGPU_FONT_INSTANCE_FLAG_MOTION: u32 = 1 << 21;

/// One immutable-layout font presentation record consumed directly by the
/// C++/IGC font-instance kernel. The complete record is copied into persistent
/// GPU-visible storage only when its style program changes.
#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuFontInstanceDescriptor {
    dwords: [u32; GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS],
}

impl GpgpuFontInstanceDescriptor {
    pub(crate) fn new(
        mask: GpgpuMask8Surface,
        mask_rect: GpgpuRect,
        foreground_rgba: u32,
    ) -> Option<Self> {
        if !mask.is_valid()
            || mask_rect.x < 0
            || mask_rect.y < 0
            || mask_rect.width == 0
            || mask_rect.height == 0
            || (mask_rect.x as u32).checked_add(mask_rect.width)? > mask.width
            || (mask_rect.y as u32).checked_add(mask_rect.height)? > mask.height
        {
            return None;
        }
        let mut dwords = [0u32; GPGPU_FONT_INSTANCE_DESCRIPTOR_DWORDS];
        dwords[0] = GPGPU_FONT_INSTANCE_MAGIC;
        dwords[1] = GPGPU_FONT_INSTANCE_FLAG_ENABLED;
        dwords[2] = mask.pitch_bytes;
        dwords[3] = mask_rect.x as u32;
        dwords[4] = mask_rect.y as u32;
        dwords[5] = mask_rect.width;
        dwords[6] = mask_rect.height;
        dwords[7] = 1.0f32.to_bits();
        dwords[8] = 0.0f32.to_bits();
        dwords[9] = 1.0f32.to_bits();
        dwords[10] = foreground_rgba;
        dwords[11] = 0;
        dwords[12] = 1.0f32.to_bits();
        dwords[13] = 1.0f32.to_bits();
        Some(Self { dwords })
    }

    pub(crate) fn set_transform(&mut self, scale: f32, rotation_radians: f32, opacity: f32) {
        let scale = scale.clamp(0.125, 8.0);
        let rotation = rotation_radians.clamp(-core::f32::consts::PI, core::f32::consts::PI);
        self.dwords[7] = scale.to_bits();
        self.dwords[8] = rotation.to_bits();
        self.dwords[9] = opacity.clamp(0.0, 1.0).to_bits();
        if (scale - 1.0).abs() > f32::EPSILON || rotation.abs() > f32::EPSILON {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM;
        } else {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_AFFINE_TRANSFORM;
        }
    }

    pub(crate) fn set_background(&mut self, rgba: u32) {
        self.dwords[11] = rgba;
        if rgba >> 24 == 0 {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_BACKGROUND;
        } else {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_BACKGROUND;
        }
    }

    pub(crate) fn set_motion(
        &mut self,
        period_seconds: f32,
        phase_cycles: f32,
        rotation_amplitude_radians: f32,
        scale_amplitude: f32,
        opacity_amplitude: f32,
        translation_amplitude_px: [f32; 2],
    ) {
        self.dwords[13] = period_seconds.clamp(0.016, 600.0).to_bits();
        self.dwords[14] = phase_cycles.to_bits();
        self.dwords[15] = rotation_amplitude_radians
            .clamp(-core::f32::consts::PI, core::f32::consts::PI)
            .to_bits();
        self.dwords[16] = scale_amplitude.clamp(-0.875, 4.0).to_bits();
        self.dwords[17] = opacity_amplitude.clamp(-1.0, 1.0).to_bits();
        self.dwords[18] = translation_amplitude_px[0].clamp(-4096.0, 4096.0).to_bits();
        self.dwords[19] = translation_amplitude_px[1].clamp(-4096.0, 4096.0).to_bits();
        if rotation_amplitude_radians.abs() > f32::EPSILON
            || scale_amplitude.abs() > f32::EPSILON
            || opacity_amplitude.abs() > f32::EPSILON
            || translation_amplitude_px[0].abs() > f32::EPSILON
            || translation_amplitude_px[1].abs() > f32::EPSILON
        {
            self.dwords[1] |= GPGPU_FONT_INSTANCE_FLAG_MOTION;
        } else {
            self.dwords[1] &= !GPGPU_FONT_INSTANCE_FLAG_MOTION;
        }
    }

    pub(crate) fn set_color_animation(
        &mut self,
        channels: u8,
        timing_sine: bool,
        iteration: u8,
        duration_seconds: f32,
        frames: &[(u16, u32)],
    ) -> bool {
        if channels == 0
            || channels & !0x0F != 0
            || iteration > 2
            || !(2..=8).contains(&frames.len())
            || frames[0].0 != 0
            || frames[frames.len() - 1].0 != 1_000
            || frames.windows(2).any(|pair| pair[1].0 <= pair[0].0)
        {
            return false;
        }
        let mut flags = self.dwords[1]
            | GPGPU_FONT_INSTANCE_FLAG_COLOR_ANIMATION
            | (u32::from(iteration) << GPGPU_FONT_INSTANCE_ITERATION_SHIFT)
            | (u32::from(channels) << GPGPU_FONT_INSTANCE_CHANNELS_SHIFT)
            | ((frames.len() as u32) << GPGPU_FONT_INSTANCE_FRAME_COUNT_SHIFT);
        if timing_sine {
            flags |= GPGPU_FONT_INSTANCE_FLAG_TIMING_SINE;
        }
        self.dwords[1] = flags;
        self.dwords[12] = duration_seconds.clamp(0.016, 600.0).to_bits();
        for (index, &(offset, rgba)) in frames.iter().enumerate() {
            self.dwords[32 + index * 2] = u32::from(offset);
            self.dwords[33 + index * 2] = rgba;
        }
        true
    }
}

/// Persistent VM-backed descriptor storage shared by all font walkers in one
/// retained page. Its PPGTT range is unique for the allocation lifetime.
pub(crate) struct GpgpuOwnedFontInstanceState {
    phys: u64,
    gpu: u64,
    bytes: usize,
    virt: *mut u8,
    capacity: usize,
}

unsafe impl Send for GpgpuOwnedFontInstanceState {}
unsafe impl Sync for GpgpuOwnedFontInstanceState {}

impl GpgpuOwnedFontInstanceState {
    pub(crate) const fn phys(&self) -> u64 {
        self.phys
    }

    pub(crate) const fn gpu(&self) -> u64 {
        self.gpu
    }

    pub(crate) const fn bytes(&self) -> usize {
        self.bytes
    }

    pub(crate) const fn capacity(&self) -> usize {
        self.capacity
    }

    pub(crate) fn descriptor_gpu(&self, index: usize) -> Option<u64> {
        (index < self.capacity)
            .then_some(self.gpu + (index * GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES) as u64)
    }

    pub(crate) fn write(&self, index: usize, descriptor: &GpgpuFontInstanceDescriptor) -> bool {
        if index >= self.capacity || self.virt.is_null() {
            return false;
        }
        let offset = index * GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES;
        unsafe {
            core::ptr::copy_nonoverlapping(
                descriptor.dwords.as_ptr() as *const u8,
                self.virt.add(offset),
                GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES,
            );
        }
        super::dma_flush(unsafe { self.virt.add(offset) }, GPGPU_FONT_INSTANCE_DESCRIPTOR_BYTES);
        true
    }
}

impl Drop for GpgpuOwnedFontInstanceState {
    fn drop(&mut self) {
        crate::dma::dealloc(self.virt, self.bytes);
        recycle_font_coverage_gpu_va(self.gpu, self.bytes);
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GpgpuFontInstanceLayer {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst_center: [f32; 2],
    pub(crate) dispatch_rect: GpgpuRect,
    pub(crate) descriptor_index: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuFontInstanceBatchResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) requested_layers: usize,
    pub(crate) active_walkers: usize,
    pub(crate) submits: usize,
}

impl GpgpuMask8Surface {
    pub(crate) fn new(
        phys: u64,
        gpu: u64,
        bytes: usize,
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Option<Self> {
        let surface = Self {
            phys,
            gpu,
            bytes,
            width,
            height,
            pitch_bytes,
        };
        surface.is_valid().then_some(surface)
    }

    pub(crate) fn is_valid(self) -> bool {
        if self.width == 0 || self.height == 0 || self.pitch_bytes < self.width {
            return false;
        }
        if (self.phys & 0xFFF) != 0 {
            return false;
        }
        let Some(last_row) = (self.height as usize)
            .checked_sub(1)
            .and_then(|row| row.checked_mul(self.pitch_bytes as usize))
        else {
            return false;
        };
        let Some(min_bytes) = last_row.checked_add(self.width as usize) else {
            return false;
        };
        min_bytes <= self.bytes
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBlit {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst: GpgpuRgba8Surface,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

/// One independently positioned/colorized persistent R8 coverage layer.
/// The destination is supplied once for the complete scene-level batch.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskLayer {
    pub(crate) mask: GpgpuMask8Surface,
    pub(crate) mask_rect: GpgpuRect,
    pub(crate) dst_xy: GpgpuPoint,
    pub(crate) color_rgba: u32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuGlyphMaskBatchResult {
    pub(crate) ok: bool,
    /// True once the command buffer reached the hardware submission boundary.
    /// An incomplete submitted batch must not be replayed over the same target.
    pub(crate) submitted: bool,
    pub(crate) requested_layers: usize,
    pub(crate) active_walkers: usize,
    pub(crate) submits: usize,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuSubmitStats {
    pub(crate) spans: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) total_ms: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuSubmissionProbe {
    /// Entry into the queue function through a fully encoded, DMA-visible batch.
    pub(crate) queue_prepare_us: u64,
    /// Entry into the queue function through accepted GuC admission.
    pub(crate) queue_total_us: u64,
    pub(crate) forcewake_us: u64,
    pub(crate) state_map_us: u64,
    pub(crate) ppgtt_init_us: u64,
    pub(crate) kernel_map_us: u64,
    pub(crate) source_map_us: u64,
    pub(crate) destination_map_us: u64,
    pub(crate) batch_encode_us: u64,
    pub(crate) admission_us: u64,
    /// Accepted GuC admission through host observation of the post marker.
    pub(crate) submit_to_marker_us: u64,
    pub(crate) completion_polls: u64,
    /// Command-stream timestamp frequency resolved from the Gen11+ GT clock
    /// configuration for this boot.
    pub(crate) gpu_timestamp_frequency_hz: u64,
    /// Host and PIPE_CONTROL samples from the same 36-bit RCS timestamp
    /// domain, ordered across the complete video submission.
    pub(crate) guc_h2g_publish_sequence: u64,
    pub(crate) gpu_host_pre_submit_timestamp: u64,
    /// First host observation that GuC advanced the H2G head through this
    /// submission. This can lag actual consumption by one completion poll.
    pub(crate) gpu_h2g_consumed_observe_timestamp: u64,
    pub(crate) gpu_batch_enter_timestamp: u64,
    pub(crate) gpu_pre_walker_timestamp: u64,
    pub(crate) gpu_post_walker_timestamp: u64,
    pub(crate) gpu_post_release_timestamp: u64,
    pub(crate) gpu_host_observe_timestamp: u64,
    pub(crate) gpu_pre_submit_to_batch_ticks: u64,
    pub(crate) gpu_pre_submit_to_batch_us: u64,
    pub(crate) gpu_pre_submit_to_h2g_consumed_ticks: u64,
    pub(crate) gpu_pre_submit_to_h2g_consumed_us: u64,
    pub(crate) gpu_h2g_consumed_to_batch_ticks: u64,
    pub(crate) gpu_h2g_consumed_to_batch_us: u64,
    pub(crate) gpu_batch_to_walker_ticks: u64,
    pub(crate) gpu_batch_to_walker_us: u64,
    pub(crate) gpu_walker_ticks: u64,
    pub(crate) gpu_walker_us: u64,
    pub(crate) gpu_walker_to_release_ticks: u64,
    pub(crate) gpu_walker_to_release_us: u64,
    pub(crate) gpu_release_to_observe_ticks: u64,
    pub(crate) gpu_release_to_observe_us: u64,
    pub(crate) gpu_pre_submit_to_observe_ticks: u64,
    pub(crate) gpu_pre_submit_to_observe_us: u64,
    pub(crate) gpu_walker_timestamp_valid: bool,
    pub(crate) gpu_phase_timestamps_valid: bool,
    pub(crate) gpu_h2g_split_valid: bool,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpgpuWorklistSubmitStats {
    pub(crate) descs: usize,
    pub(crate) walkers: usize,
    pub(crate) submits: usize,
    pub(crate) submit_ms: u64,
    pub(crate) probe: GpgpuSubmissionProbe,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpgpuSubmissionOutcome {
    /// The request failed before crossing the hardware submission boundary.
    Unavailable,
    /// The post marker retired, so all destination writes are complete.
    Complete,
    /// Hardware accepted the request but its post marker did not retire.
    SubmittedIncomplete,
}

impl Default for GpgpuSubmissionOutcome {
    fn default() -> Self {
        Self::Unavailable
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuWorklistSubmitResult {
    pub(crate) stats: GpgpuWorklistSubmitStats,
    pub(crate) outcome: GpgpuSubmissionOutcome,
}

/// Opaque serial and executor submission for one slot in the persistent UI4
/// compositor ring.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Ui4CompositorSubmission {
    serial: u64,
    gpu: crate::gpu::executor::KernelSubmission,
}

impl Ui4CompositorSubmission {
    /// Create a future for the exact vGPU timeline point backing this UI4 job.
    pub(crate) fn fence(self) -> crate::gpu::executor::GpuFence {
        self.gpu.fence()
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorSubmitError {
    Busy,
    Unavailable,
    InvalidWorklist,
    SubmissionRejected,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui4CompositorCompletion {
    Pending,
    Complete(GpgpuWorklistSubmitStats),
    Failed,
    InvalidSubmission,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4VideoFrameCompletion {
    Pending,
    Complete {
        stats: GpgpuWorklistSubmitStats,
        release: GpgpuRgba8ReleaseFence,
    },
    Failed,
}

#[derive(Copy, Clone, Debug)]
pub(crate) enum Ui4SpriteSceneCompletion {
    Pending,
    Complete {
        stats: GpgpuWorklistSubmitStats,
        release: GpgpuRgba8ReleaseFence,
    },
    Failed,
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuShellMandel64WorklistResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) requested: usize,
    pub(crate) descriptors: usize,
    pub(crate) walkers: usize,
    pub(crate) pixels: usize,
    pub(crate) submit_ms: u64,
    pub(crate) desc_gpu: u64,
    pub(crate) last_src_xy: GpgpuPoint,
    pub(crate) last_dst_xy: GpgpuPoint,
    /// Present only for a complete direct-scanout render whose final
    /// PIPE_CONTROL and post-sync marker retired for this exact allocation.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Common result for a full-surface compute node that does not own
/// presentation. UI4 consumers decide whether and when to publish the frame.
#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuRgba8KernelResult {
    pub(crate) ok: bool,
    pub(crate) submitted: bool,
    pub(crate) marker: u32,
    pub(crate) submit_ms: u64,
    /// Exact-surface producer release, minted only after the kernel's final
    /// cache-draining PIPE_CONTROL and post-sync marker have retired.
    pub(crate) release: Option<GpgpuRgba8ReleaseFence>,
}

/// Proof that one full-surface compute dispatch retired its producer-release
/// packet for one exact allocation. The fields stay private so consumers
/// cannot manufacture display eligibility from an address alone.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpgpuRgba8ReleaseFence {
    phys: u64,
    byte_len: usize,
    sequence: u64,
}

impl GpgpuRgba8ReleaseFence {
    pub(crate) const fn matches(self, phys: u64, byte_len: usize) -> bool {
        self.phys == phys && self.byte_len == byte_len
    }

    pub(crate) const fn sequence(self) -> u64 {
        self.sequence
    }
}

static GPGPU_RGBA8_RELEASE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Default)]
struct DirectRcsDispatchOutcome {
    submitted: bool,
    observed: u32,
}

/// Establish the final producer-to-display boundary for an RGBA8 allocation.
///
/// Earlier resolve/coverage/decorations may use different completion packets;
/// this dedicated batch remaps the exact destination PAT3/UC, drains HDC/L3
/// and render-target writes, and proves retirement with an ordered
/// PIPE_CONTROL post-sync cookie. No pixel shader or surface copy runs here.
pub(crate) fn release_rgba8_surface_for_scanout(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid() {
        return GpgpuRgba8KernelResult::default();
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let prepared = direct_rcs_forcewake(dev)
        && direct_rcs_map_state(dev, state)
        && direct_rcs_init_ppgtt(state)
        && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes)
        && direct_rcs_encode_rgba8_scanout_release_batch(state);
    let submitted = prepared && direct_rcs_submit_batch(dev, state);
    let marker = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            RGBA8_SCANOUT_RELEASE_MARKER_SLOT,
            RGBA8_SCANOUT_RELEASE_MARKER,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == RGBA8_SCANOUT_RELEASE_MARKER;
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct GpgpuMandel64Placement {
    pub(crate) src_x: i32,
    pub(crate) src_y: i32,
    pub(crate) dst_x: i32,
    pub(crate) dst_y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) view_height: u32,
    pub(crate) mirror_at_center: bool,
    pub(crate) iterations: u32,
}

#[derive(Copy, Clone, Debug)]
struct GpgpuRectWorklistDescBuffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for GpgpuRectWorklistDescBuffer {}
unsafe impl Sync for GpgpuRectWorklistDescBuffer {}
