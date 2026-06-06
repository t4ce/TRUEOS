extern crate alloc;

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use libm::{ceilf, floorf};
use trueos_gfx_core::{
    BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, Command, CommandBuffer, DeviceCaps,
    Error, FenceId, GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, ImageRegion,
    MemoryType, PipelineDesc, PipelineId, Result, SamplerDesc, SamplerFilter, SamplerWrap,
    ScissorRect, ShaderDesc, ShaderId, SwapchainDesc, TexCoordFormat, TexVertexF32,
    read_rgb_vertex_f32_bytes, read_tex_vertex_f32_bytes,
};

const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;
const TEX_PIPELINE_FS_PARTICLE_TAG_RAW: u32 = 0x5052_5443;
const RCS_PRESENT_RETRY_COOLDOWN_PRESENTS: u32 = 600;
const IMAGE_GPU_VA_BASE: u64 = 0x0800_0000;
const IMAGE_GPU_VA_ALIGN: u64 = 0x0040_0000;
const MAX_BACKEND_IMAGE_DIM: u32 = 8192;
const MAX_BACKEND_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const ENABLE_IMAGE_GPGPU_CLEAR: bool = false;
const ENABLE_HW_PLANE_ALPHA_OVERLAY: bool = false;

static PRESENT_COMPLETED_SEQ: AtomicU32 = AtomicU32::new(0);

#[inline]
pub(crate) fn present_completed_seq() -> u32 {
    PRESENT_COMPLETED_SEQ.load(Ordering::Acquire)
}

#[inline]
fn mark_present_completed(seq: u32) {
    PRESENT_COMPLETED_SEQ.store(seq, Ordering::Release);
}

#[derive(Clone)]
struct BufferEntry {
    bytes: Vec<u8>,
    usage: BufferUsage,
    memory: MemoryType,
}

#[derive(Clone)]
struct ImageDmaSurface {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
    pitch_bytes: u32,
}

#[derive(Clone)]
struct ImageEntry {
    width: u32,
    height: u32,
    format: ImageFormat,
    gpu_addr: u64,
    mask_gpu_addr: u64,
    dma: ImageDmaSurface,
    mask_dma: Option<ImageDmaSurface>,
    gpu_dirty: bool,
    rgba: Vec<u8>,
}

impl ImageEntry {
    fn gpgpu_surface(&self) -> Option<crate::intel::gpgpu::GpgpuRgba8Surface> {
        crate::intel::gpgpu::GpgpuRgba8Surface::new(
            self.dma.phys,
            self.gpu_addr,
            self.dma.bytes,
            self.width,
            self.height,
            self.dma.pitch_bytes,
        )
    }

    fn gpgpu_mask_surface(&self) -> Option<crate::intel::gpgpu::GpgpuMask8Surface> {
        let dma = self.mask_dma.as_ref()?;
        crate::intel::gpgpu::GpgpuMask8Surface::new(
            dma.phys,
            self.mask_gpu_addr,
            dma.bytes,
            self.width,
            self.height,
            dma.pitch_bytes,
        )
    }

    fn copy_cpu_to_dma(&self) {
        if self.dma.virt.is_null() || self.rgba.is_empty() {
            return;
        }
        let len = self.rgba.len().min(self.dma.bytes);
        unsafe {
            core::ptr::copy_nonoverlapping(self.rgba.as_ptr(), self.dma.virt, len);
        }
        crate::intel::dma_cache_flush_range(self.dma.virt, len);
    }

    fn copy_dma_to_cpu(&mut self) {
        if self.dma.virt.is_null() || self.rgba.is_empty() {
            return;
        }
        let len = self.rgba.len().min(self.dma.bytes);
        crate::intel::dma_cache_flush_range(self.dma.virt, len);
        unsafe {
            core::ptr::copy_nonoverlapping(self.dma.virt, self.rgba.as_mut_ptr(), len);
        }
    }

    fn dma_rgba_slice(&self) -> Option<&[u8]> {
        if self.dma.virt.is_null() || self.rgba.is_empty() {
            return None;
        }
        let len = self.rgba.len().min(self.dma.bytes);
        crate::intel::dma_cache_flush_range(self.dma.virt, len);
        Some(unsafe { core::slice::from_raw_parts(self.dma.virt as *const u8, len) })
    }

    fn rebuild_mask_dma(&mut self) {
        let Some(mask_dma) = self.mask_dma.as_ref() else {
            return;
        };
        if mask_dma.virt.is_null() || self.rgba.is_empty() {
            return;
        }
        let pixels = (self.width as usize).saturating_mul(self.height as usize);
        let len = pixels.min(mask_dma.bytes);
        unsafe {
            let dst = core::slice::from_raw_parts_mut(mask_dma.virt, len);
            for (idx, coverage) in dst.iter_mut().enumerate() {
                let src = idx.saturating_mul(4);
                if src + 4 > self.rgba.len() {
                    break;
                }
                let alpha = self.rgba[src + 3];
                *coverage = if alpha < 0xFF { alpha } else { self.rgba[src] };
            }
        }
        crate::intel::dma_cache_flush_range(mask_dma.virt, len);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PipelineKind {
    Rgb,
    TexMask,
    TexRgba,
    TexParticle,
    Mandelbrot,
    Julia,
    BurningShip,
}

#[derive(Clone)]
struct PipelineEntry {
    desc: PipelineDesc,
    kind: PipelineKind,
}

#[derive(Clone, Copy)]
enum RenderTarget {
    Screen,
    Image(ImageId),
}

#[derive(Clone, Copy)]
enum CpuPresentMode {
    PrimarySurface,
    LimineFramebuffer,
}

#[derive(Clone, Copy, Debug)]
struct TextureCopyQuad {
    src_x: u32,
    src_y: u32,
    dst_x: i32,
    dst_y: i32,
    width: u32,
    height: u32,
}

#[derive(Clone, Copy, Debug)]
struct TextureScaleQuad {
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    dst_x: i32,
    dst_y: i32,
    dst_w: u32,
    dst_h: u32,
}

#[derive(Clone, Copy, Debug)]
struct RgbFillQuad {
    dst_x: i32,
    dst_y: i32,
    width: u32,
    height: u32,
    color_rgba: u32,
}

impl CpuPresentMode {
    fn label(self) -> &'static str {
        match self {
            Self::PrimarySurface => "cpu-primary-surface",
            Self::LimineFramebuffer => "cpu-limine-framebuffer",
        }
    }
}

pub struct IntelGfxBackend {
    swapchain_desc: SwapchainDesc,
    framebuffer_ptr: *mut u8,
    framebuffer_pitch: usize,
    framebuffer_width: u32,
    framebuffer_height: u32,
    screen_rgba: Vec<u8>,
    screen_rgba_gpu_dirty: bool,
    buffers: Vec<Option<BufferEntry>>,
    images: Vec<Option<ImageEntry>>,
    pipelines: Vec<Option<PipelineEntry>>,
    next_shader_raw: u32,
    next_fence_raw: u64,
    submit_seq: u32,
    present_seq: u32,
    rcs_retry_after_present_seq: u32,
    rcs_present_failures: u32,
    next_image_gpu_addr: u64,
}

unsafe impl Send for IntelGfxBackend {}

impl IntelGfxBackend {
    pub fn init(framebuffers: Option<&'static crate::limine::FramebufferResponse>) -> Option<Self> {
        if !crate::intel::has_claimed_device() {
            return None;
        }

        crate::log!("intel/gfx-backend: init starting\n");

        let fb = framebuffers.and_then(|response| response.framebuffers().first().copied());
        let mut framebuffer_ptr = core::ptr::null_mut();
        let mut framebuffer_pitch = 0usize;
        let mut framebuffer_width = 0u32;
        let mut framebuffer_height = 0u32;
        if let Some(fb) = fb {
            if fb.memory_model == ::limine::framebuffer::FRAMEBUFFER_RGB
                && fb.bpp == 32
                && !fb.address().is_null()
            {
                let width = fb.width as u32;
                let height = fb.height as u32;
                let pitch = fb.pitch as usize;
                if width != 0 && height != 0 && pitch >= width as usize * 4 {
                    framebuffer_ptr = fb.address() as *mut u8;
                    framebuffer_pitch = pitch;
                    framebuffer_width = width;
                    framebuffer_height = height;
                } else {
                    crate::log!(
                        "intel/gfx-backend: limine framebuffer ignored bad geometry size={}x{} pitch=0x{:X}\n",
                        width,
                        height,
                        pitch
                    );
                }
            } else {
                crate::log!(
                    "intel/gfx-backend: limine framebuffer ignored model={} bpp={} addr_null={}\n",
                    fb.memory_model,
                    fb.bpp,
                    fb.address().is_null() as u8
                );
            }
        } else {
            crate::log!(
                "intel/gfx-backend: init without limine framebuffer; primary-surface fallback only\n"
            );
        }

        let (width, height) = crate::intel::active_scanout_dimensions().or_else(|| {
            (framebuffer_width != 0 && framebuffer_height != 0)
                .then_some((framebuffer_width, framebuffer_height))
        })?;
        if width == 0 || height == 0 {
            return None;
        }

        let swapchain_desc = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: trueos_gfx_core::Extent2D { width, height },
        };
        let screen_len = rgba_len(width, height)?;
        crate::log!(
            "intel/gfx-backend: init ok size={}x{} limine_fb={} pitch=0x{:X}\n",
            width,
            height,
            (!framebuffer_ptr.is_null()) as u8,
            framebuffer_pitch
        );
        Some(Self {
            swapchain_desc,
            framebuffer_ptr,
            framebuffer_pitch,
            framebuffer_width,
            framebuffer_height,
            screen_rgba: alloc::vec![0; screen_len],
            screen_rgba_gpu_dirty: false,
            buffers: Vec::new(),
            images: Vec::new(),
            pipelines: Vec::new(),
            next_shader_raw: 1,
            next_fence_raw: 1,
            submit_seq: 0,
            present_seq: 0,
            rcs_retry_after_present_seq: 0,
            rcs_present_failures: 0,
            next_image_gpu_addr: IMAGE_GPU_VA_BASE,
        })
    }

    fn ensure_screen_rgba(&mut self) -> Result<()> {
        let len = rgba_len(self.swapchain_desc.extent.width, self.swapchain_desc.extent.height)
            .ok_or(Error::Invalid)?;
        if self.screen_rgba.len() != len {
            self.screen_rgba.resize(len, 0);
            self.screen_rgba_gpu_dirty = false;
        }
        Ok(())
    }

    fn sync_screen_rgba_from_gpu(&mut self) {
        if !self.screen_rgba_gpu_dirty || self.screen_rgba.is_empty() {
            return;
        }
        crate::intel::dma_cache_flush_range(self.screen_rgba.as_ptr(), self.screen_rgba.len());
        self.screen_rgba_gpu_dirty = false;
    }

    fn sync_image_rgba_from_gpu(&mut self, id: ImageId) {
        let Some(image) = self.image_mut(id) else {
            return;
        };
        if !image.gpu_dirty || image.rgba.is_empty() {
            return;
        }
        image.copy_dma_to_cpu();
        image.gpu_dirty = false;
    }

    fn alloc_slot<T>(slots: &mut Vec<Option<T>>, value: T) -> u32 {
        if let Some((idx, slot)) = slots
            .iter_mut()
            .enumerate()
            .find(|(_, slot)| slot.is_none())
        {
            *slot = Some(value);
            return idx as u32 + 1;
        }
        slots.push(Some(value));
        slots.len() as u32
    }

    fn buffer(&self, id: BufferId) -> Option<&BufferEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.buffers.get(idx)?.as_ref()
    }

    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut BufferEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.buffers.get_mut(idx)?.as_mut()
    }

    fn image(&self, id: ImageId) -> Option<&ImageEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.images.get(idx)?.as_ref()
    }

    pub(crate) fn image_gpgpu_surface(
        &self,
        id: ImageId,
    ) -> Option<crate::intel::gpgpu::GpgpuRgba8Surface> {
        self.image(id)?.gpgpu_surface()
    }

    pub(crate) fn image_gpgpu_mask_surface(
        &mut self,
        id: ImageId,
    ) -> Option<crate::intel::gpgpu::GpgpuMask8Surface> {
        let _ = self.ensure_image_mask_dma(id);
        self.image(id)?.gpgpu_mask_surface()
    }

    fn image_mut(&mut self, id: ImageId) -> Option<&mut ImageEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.images.get_mut(idx)?.as_mut()
    }

    fn alloc_image_dma(width: u32, height: u32, len: usize) -> Result<ImageDmaSurface> {
        let pitch_bytes = width
            .checked_mul(core::mem::size_of::<u32>() as u32)
            .ok_or(Error::Invalid)?;
        let expected = (pitch_bytes as usize)
            .checked_mul(height as usize)
            .ok_or(Error::Invalid)?;
        if expected != len {
            return Err(Error::Invalid);
        }
        let bytes = align_up_usize(len, crate::intel::WARM_ALIGN).ok_or(Error::Invalid)?;
        let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN).ok_or_else(|| {
            crate::log!(
                "intel/gfx-backend: image dma alloc failed size={}x{} bytes=0x{:X}\n",
                width,
                height,
                bytes
            );
            Error::OutOfMemory
        })?;
        unsafe {
            core::ptr::write_bytes(virt, 0, bytes);
        }
        crate::intel::dma_cache_flush_range(virt, bytes);
        Ok(ImageDmaSurface {
            phys,
            virt,
            bytes,
            pitch_bytes,
        })
    }

    fn alloc_mask_dma(width: u32, height: u32) -> Result<ImageDmaSurface> {
        let len = (width as usize)
            .checked_mul(height as usize)
            .ok_or(Error::Invalid)?;
        let bytes = align_up_usize(len, crate::intel::WARM_ALIGN).ok_or(Error::Invalid)?;
        let (phys, virt) = crate::dma::alloc(bytes, crate::intel::WARM_ALIGN).ok_or_else(|| {
            crate::log!(
                "intel/gfx-backend: mask dma alloc failed size={}x{} bytes=0x{:X}\n",
                width,
                height,
                bytes
            );
            Error::OutOfMemory
        })?;
        unsafe {
            core::ptr::write_bytes(virt, 0, bytes);
        }
        crate::intel::dma_cache_flush_range(virt, bytes);
        Ok(ImageDmaSurface {
            phys,
            virt,
            bytes,
            pitch_bytes: width,
        })
    }

    fn dealloc_image_dma(dma: &ImageDmaSurface) {
        crate::dma::dealloc(dma.virt, dma.bytes);
    }

    fn ensure_image_mask_dma(&mut self, id: ImageId) -> bool {
        let Some(image) = self.image_mut(id) else {
            return false;
        };
        if image.mask_dma.is_none() {
            let Ok(mask_dma) = Self::alloc_mask_dma(image.width, image.height) else {
                return false;
            };
            image.mask_dma = Some(mask_dma);
        }
        image.rebuild_mask_dma();
        image.gpgpu_mask_surface().is_some()
    }

    fn pipeline(&self, id: PipelineId) -> Option<&PipelineEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.pipelines.get(idx)?.as_ref()
    }

    fn with_target_mut<R>(
        &mut self,
        target: RenderTarget,
        f: impl FnOnce(&mut [u8], u32, u32) -> R,
    ) -> Result<R> {
        match target {
            RenderTarget::Screen => {
                self.ensure_screen_rgba()?;
                let width = self.swapchain_desc.extent.width;
                let height = self.swapchain_desc.extent.height;
                Ok(f(self.screen_rgba.as_mut_slice(), width, height))
            }
            RenderTarget::Image(id) => {
                self.sync_image_rgba_from_gpu(id);
                let image = self.image_mut(id).ok_or(Error::NotFound)?;
                let out = f(image.rgba.as_mut_slice(), image.width, image.height);
                image.copy_cpu_to_dma();
                image.gpu_dirty = false;
                Ok(out)
            }
        }
    }

    fn present_screen_primary_surface_cpu(&self) -> bool {
        if self.screen_rgba.is_empty() {
            return false;
        }
        let width = self.swapchain_desc.extent.width;
        let height = self.swapchain_desc.extent.height;
        crate::intel::present_rgba_primary_flip_y(
            self.screen_rgba.as_slice(),
            width,
            height,
            width as usize * 4,
            "gfx-cpu-primary-surface-flip-y",
        )
    }

    fn present_scanout_extent(&self) -> (usize, usize) {
        let (dst_w, dst_h) = crate::intel::active_scanout_dimensions()
            .or_else(|| {
                (self.framebuffer_width != 0 && self.framebuffer_height != 0)
                    .then_some((self.framebuffer_width, self.framebuffer_height))
            })
            .unwrap_or((self.swapchain_desc.extent.width, self.swapchain_desc.extent.height));
        (
            self.swapchain_desc.extent.width.min(dst_w) as usize,
            self.swapchain_desc.extent.height.min(dst_h) as usize,
        )
    }

    fn present_screen_limine_framebuffer_cpu(&mut self) -> Result<()> {
        self.ensure_screen_rgba()?;
        if self.framebuffer_ptr.is_null() || self.framebuffer_pitch == 0 {
            return Err(Error::Invalid);
        }

        let copy_w = self.swapchain_desc.extent.width.min(self.framebuffer_width) as usize;
        let copy_h = self
            .swapchain_desc
            .extent
            .height
            .min(self.framebuffer_height) as usize;
        for y in 0..copy_h {
            let src_row = y
                .saturating_mul(self.swapchain_desc.extent.width as usize)
                .saturating_mul(4);
            let dst_row = y.saturating_mul(self.framebuffer_pitch);
            for x in 0..copy_w {
                let src_off = src_row + x.saturating_mul(4);
                if src_off + 4 > self.screen_rgba.len() {
                    break;
                }
                let pixel = pack_xrgb(
                    self.screen_rgba[src_off],
                    self.screen_rgba[src_off + 1],
                    self.screen_rgba[src_off + 2],
                );
                let dst_ptr =
                    unsafe { self.framebuffer_ptr.add(dst_row + x.saturating_mul(4)) as *mut u32 };
                unsafe { core::ptr::write_volatile(dst_ptr, pixel) };
            }
        }

        Ok(())
    }

    fn present_screen(&mut self) -> Result<()> {
        let present_start_ms = embassy_time::Instant::now().as_millis();
        self.ensure_screen_rgba()?;
        let (copy_w, copy_h) = self.present_scanout_extent();
        self.present_seq = self.present_seq.wrapping_add(1);
        if self.screen_rgba_gpu_dirty {
            self.screen_rgba_gpu_dirty = false;
            mark_present_completed(self.present_seq);
            if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
                let elapsed_ms = embassy_time::Instant::now()
                    .as_millis()
                    .saturating_sub(present_start_ms);
                crate::log!(
                    "intel/gfx-backend: present seq={} mode=primary-already-updated size={}x{} elapsed_ms={}\n",
                    self.present_seq,
                    copy_w,
                    copy_h,
                    elapsed_ms
                );
            }
            return Ok(());
        }
        mark_present_completed(self.present_seq);
        if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(present_start_ms);
            crate::log!(
                "intel/gfx-backend: present seq={} mode=no-primary-damage size={}x{} elapsed_ms={}\n",
                self.present_seq,
                copy_w,
                copy_h,
                elapsed_ms
            );
        }
        Ok(())
    }

    fn present_image_target(&mut self, id: ImageId) -> Result<()> {
        let present_start_ms = embassy_time::Instant::now().as_millis();
        let image = self.image(id).ok_or(Error::NotFound)?.clone();
        if image.width == 0 || image.height == 0 {
            return Err(Error::Invalid);
        }
        let Some(src_rgba) = image.dma_rgba_slice() else {
            return Err(Error::Invalid);
        };

        self.present_seq = self.present_seq.wrapping_add(1);

        let allow_rcs_retry = self.present_seq >= self.rcs_retry_after_present_seq;
        if allow_rcs_retry {
            if crate::intel::rcs_present_rgba_frame(
                src_rgba,
                image.width as usize,
                image.height as usize,
            ) {
                self.rcs_retry_after_present_seq = 0;
                self.rcs_present_failures = 0;
                mark_present_completed(self.present_seq);
                if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
                    let elapsed_ms = embassy_time::Instant::now()
                        .as_millis()
                        .saturating_sub(present_start_ms);
                    crate::log!(
                        "intel/gfx-backend: present seq={} complete_seq={} mode=rcs-execlist-image-dma-target size={}x{} src_gpu=0x{:X} elapsed_ms={}\n",
                        self.present_seq,
                        present_completed_seq(),
                        image.width,
                        image.height,
                        image.gpu_addr,
                        elapsed_ms
                    );
                }
                return Ok(());
            }

            self.rcs_present_failures = self.rcs_present_failures.saturating_add(1);
            self.rcs_retry_after_present_seq = self
                .present_seq
                .saturating_add(RCS_PRESENT_RETRY_COOLDOWN_PRESENTS);
            crate::log!(
                "intel/gfx-backend: present seq={} image-rcs-present-failed failures={} cooldown_until_seq={} size={}x{} src_gpu=0x{:X}\n",
                self.present_seq,
                self.rcs_present_failures,
                self.rcs_retry_after_present_seq,
                image.width,
                image.height,
                image.gpu_addr
            );
        }

        if crate::intel::present_rgba_primary(
            src_rgba,
            image.width,
            image.height,
            image.width as usize * 4,
            "gfx-cpu-primary-image-target",
        ) {
            mark_present_completed(self.present_seq);
            if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
                let elapsed_ms = embassy_time::Instant::now()
                    .as_millis()
                    .saturating_sub(present_start_ms);
                crate::log!(
                    "intel/gfx-backend: present seq={} complete_seq={} fallback=cpu-primary-image-dma-target rcs_retry_ready={} size={}x{} src_gpu=0x{:X} elapsed_ms={}\n",
                    self.present_seq,
                    present_completed_seq(),
                    allow_rcs_retry as u8,
                    image.width,
                    image.height,
                    image.gpu_addr,
                    elapsed_ms
                );
            }
            return Ok(());
        }

        Err(Error::Invalid)
    }

    fn draw_rgb(
        &mut self,
        target: RenderTarget,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
    ) -> Result<()> {
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        let start = byte_offset as usize + first_vertex as usize * trueos_gfx_core::RGB_VERTEX_SIZE;
        let need = vertex_count as usize * trueos_gfx_core::RGB_VERTEX_SIZE;
        if start > buffer.bytes.len() || start.saturating_add(need) > buffer.bytes.len() {
            return Err(Error::Invalid);
        }
        let verts = buffer.bytes[start..start + need].to_vec();
        if let RenderTarget::Image(dst_id) = target
            && let Some((quads, spans, dst_gpu_addr)) =
                self.rgb_quads_to_image(dst_id, verts.as_slice(), scissor, blend)
        {
            if let Some(image) = self.image_mut(dst_id) {
                image.gpu_dirty = true;
            }
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                crate::log!(
                    "intel/gfx-backend: draw-rgb mode=gpgpu-fill-quads quads={} spans={} dst_gpu=0x{:X}\n",
                    quads,
                    spans,
                    dst_gpu_addr
                );
            }
            return Ok(());
        }
        let fast_ok = match target {
            RenderTarget::Screen => {
                let screen_surface_gpu =
                    crate::intel::primary_present_surface_gpu_addr().unwrap_or(0x0200_0000);
                crate::intel::rcs_draw_rgba_rgb_triangles(
                    self.screen_rgba.as_slice(),
                    verts.as_slice(),
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    screen_surface_gpu,
                    scissor,
                    blend,
                )
                .then_some((
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    screen_surface_gpu,
                ))
            }
            RenderTarget::Image(id) => self.image(id).and_then(|image| {
                crate::intel::rcs_draw_rgba_rgb_triangles(
                    image.rgba.as_slice(),
                    verts.as_slice(),
                    image.width,
                    image.height,
                    image.gpu_addr,
                    scissor,
                    blend,
                )
                .then_some((image.width, image.height, image.gpu_addr))
            }),
        };
        if let Some((target_w, target_h, target_gpu_addr)) = fast_ok {
            match target {
                RenderTarget::Screen => self.screen_rgba_gpu_dirty = true,
                RenderTarget::Image(id) => {
                    if let Some(image) = self.image_mut(id) {
                        image.gpu_dirty = true;
                    }
                }
            }
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                crate::log!(
                    "intel/gfx-backend: draw-rgb mode=rcs-store triangles={} size={}x{} gpu=0x{:X}\n",
                    verts.len() / (3 * trueos_gfx_core::RGB_VERTEX_SIZE),
                    target_w,
                    target_h,
                    target_gpu_addr
                );
            }
            return Ok(());
        }

        if matches!(target, RenderTarget::Screen) {
            self.sync_screen_rgba_from_gpu();
        }
        self.with_target_mut(target, |rgba, width, height| {
            let mut off = 0usize;
            while off + (3 * trueos_gfx_core::RGB_VERTEX_SIZE) <= verts.len() {
                let Some(v0) = read_rgb_vertex_f32_bytes(&verts, off) else {
                    break;
                };
                let Some(v1) =
                    read_rgb_vertex_f32_bytes(&verts, off + trueos_gfx_core::RGB_VERTEX_SIZE)
                else {
                    break;
                };
                let Some(v2) =
                    read_rgb_vertex_f32_bytes(&verts, off + 2 * trueos_gfx_core::RGB_VERTEX_SIZE)
                else {
                    break;
                };
                draw_rgb_triangle_rgba(rgba, width, height, scissor, blend, v0, v1, v2);
                off += 3 * trueos_gfx_core::RGB_VERTEX_SIZE;
            }
        })?;
        Ok(())
    }

    fn draw_tex(
        &mut self,
        target: RenderTarget,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        source: ImageId,
        pipeline_kind: PipelineKind,
        sampler: SamplerDesc,
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
    ) -> Result<()> {
        let draw_start_ms = embassy_time::Instant::now().as_millis();
        if matches!(target, RenderTarget::Screen) {
            self.sync_screen_rgba_from_gpu();
        }
        self.sync_image_rgba_from_gpu(source);
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        let source_id = source;
        let start = byte_offset as usize + first_vertex as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        let need = vertex_count as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        if start > buffer.bytes.len() || start.saturating_add(need) > buffer.bytes.len() {
            return Err(Error::Invalid);
        }
        let verts = buffer.bytes[start..start + need].to_vec();
        let sample_kind = match pipeline_kind {
            PipelineKind::TexMask => SampleKind::Mask,
            PipelineKind::TexRgba | PipelineKind::TexParticle => SampleKind::Rgba,
            PipelineKind::Mandelbrot | PipelineKind::Julia | PipelineKind::BurningShip => {
                return Err(Error::Unsupported);
            }
            PipelineKind::Rgb => return Err(Error::Invalid),
        };
        if sample_kind == SampleKind::Mask {
            let _ = self.ensure_image_mask_dma(source_id);
        }
        let source = self.image(source_id).ok_or(Error::NotFound)?.clone();
        if let RenderTarget::Image(dst_id) = target
            && let Some((quads, spans, submits, dst_gpu_addr)) = self.copy_tex_quads_to_image(
                dst_id,
                &source,
                verts.as_slice(),
                sampler,
                blend,
                sample_kind,
                scissor,
            )
        {
            if let Some(image) = self.image_mut(dst_id) {
                image.gpu_dirty = true;
            }
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(draw_start_ms);
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode=gpgpu-copy-rect target=image quads={} spans={} submits={} elapsed_ms={} dst_gpu=0x{:X} src_gpu=0x{:X}\n",
                    quads,
                    spans,
                    submits,
                    elapsed_ms,
                    dst_gpu_addr,
                    source.gpu_addr
                );
            }
            return Ok(());
        }
        if let RenderTarget::Image(dst_id) = target
            && let Some((quads, spans, submits, dst_gpu_addr)) = self.mask_tex_quads_to_image(
                dst_id,
                &source,
                verts.as_slice(),
                sampler,
                blend,
                sample_kind,
                scissor,
            )
        {
            if let Some(image) = self.image_mut(dst_id) {
                image.gpu_dirty = true;
            }
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(draw_start_ms);
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode=gpgpu-glyph-mask target=image quads={} spans={} submits={} elapsed_ms={} dst_gpu=0x{:X} mask_gpu=0x{:X}\n",
                    quads,
                    spans,
                    submits,
                    elapsed_ms,
                    dst_gpu_addr,
                    source.mask_gpu_addr
                );
            }
            return Ok(());
        }
        if let RenderTarget::Image(dst_id) = target
            && let Some((quads, spans, submits, dst_gpu_addr)) = self.alpha_tex_quads_to_image(
                dst_id,
                &source,
                verts.as_slice(),
                sampler,
                blend,
                sample_kind,
                scissor,
            )
        {
            if let Some(image) = self.image_mut(dst_id) {
                image.gpu_dirty = true;
            }
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(draw_start_ms);
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode=gpgpu-alpha-over target=image quads={} spans={} submits={} elapsed_ms={} dst_gpu=0x{:X} src_gpu=0x{:X}\n",
                    quads,
                    spans,
                    submits,
                    elapsed_ms,
                    dst_gpu_addr,
                    source.gpu_addr
                );
            }
            return Ok(());
        }
        if matches!(target, RenderTarget::Screen)
            && let Some((quads, bytes, mode)) = self.copy_tex_quads_to_screen(
                &source,
                verts.as_slice(),
                sampler,
                blend,
                sample_kind,
                scissor,
            )
        {
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(draw_start_ms);
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode={} target=screen quads={} bytes=0x{:X} source={}x{} target={}x{} elapsed_ms={} samples=[0x{:08X},0x{:08X},0x{:08X}]\n",
                    mode,
                    quads,
                    bytes,
                    source.width,
                    source.height,
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    elapsed_ms,
                    sample_rgba_word(
                        self.screen_rgba.as_slice(),
                        self.swapchain_desc.extent.width,
                        0,
                        0,
                    ),
                    sample_rgba_word(
                        self.screen_rgba.as_slice(),
                        self.swapchain_desc.extent.width,
                        self.swapchain_desc.extent.width.saturating_div(2),
                        self.swapchain_desc.extent.height.saturating_div(2),
                    ),
                    sample_rgba_word(
                        self.screen_rgba.as_slice(),
                        self.swapchain_desc.extent.width,
                        self.swapchain_desc.extent.width.saturating_sub(1),
                        self.swapchain_desc.extent.height.saturating_sub(1),
                    )
                );
            }
            return Ok(());
        }
        if matches!(target, RenderTarget::Screen)
            && (self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120))
        {
            crate::log!(
                "intel/gfx-backend: draw-tex cpu-copy-rect-miss target=screen sample={} blend_enabled={} sampler=({:?},{:?},{:?},{:?}) verts={} source={}x{} target={}x{} scissor={}\n",
                match sample_kind {
                    SampleKind::Mask => "mask",
                    SampleKind::Rgba => "rgba",
                },
                blend.enabled as u8,
                sampler.wrap_s,
                sampler.wrap_t,
                sampler.min_filter,
                sampler.mag_filter,
                verts.len() / trueos_gfx_core::TEX_VERTEX_SIZE,
                source.width,
                source.height,
                self.swapchain_desc.extent.width,
                self.swapchain_desc.extent.height,
                scissor.is_some() as u8
            );
        }
        let screen_surface_gpu =
            crate::intel::primary_present_surface_gpu_addr().unwrap_or(0x0200_0000);
        if matches!(target, RenderTarget::Screen)
            && crate::intel::rcs_draw_screen_tex_triangles(
                self.screen_rgba.as_slice(),
                source.rgba.as_slice(),
                source.width,
                source.height,
                verts.as_slice(),
                self.swapchain_desc.extent.width,
                self.swapchain_desc.extent.height,
                screen_surface_gpu,
                scissor,
                blend,
                sampler,
                match sample_kind {
                    SampleKind::Mask => crate::intel::TextureStoreSampleKind::Mask,
                    SampleKind::Rgba => crate::intel::TextureStoreSampleKind::Rgba,
                },
            )
        {
            self.screen_rgba_gpu_dirty = true;
            let elapsed_ms = embassy_time::Instant::now()
                .as_millis()
                .saturating_sub(draw_start_ms);
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode=rcs-store target=screen sample={} triangles={} source={}x{} target={}x{} elapsed_ms={} gpu=0x{:X}\n",
                    match sample_kind {
                        SampleKind::Mask => "mask",
                        SampleKind::Rgba => "rgba",
                    },
                    verts.len() / (3 * trueos_gfx_core::TEX_VERTEX_SIZE),
                    source.width,
                    source.height,
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    elapsed_ms,
                    screen_surface_gpu
                );
            }
            return Ok(());
        }
        let RenderTarget::Image(_) = target else {
            return Err(Error::Unsupported);
        };
        self.with_target_mut(target, |rgba, width, height| {
            let mut off = 0usize;
            while off + (3 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
                let Some(v0) = read_tex_vertex_f32_bytes(&verts, off) else {
                    break;
                };
                let Some(v1) =
                    read_tex_vertex_f32_bytes(&verts, off + trueos_gfx_core::TEX_VERTEX_SIZE)
                else {
                    break;
                };
                let Some(v2) =
                    read_tex_vertex_f32_bytes(&verts, off + 2 * trueos_gfx_core::TEX_VERTEX_SIZE)
                else {
                    break;
                };
                draw_tex_triangle_rgba(
                    rgba,
                    width,
                    height,
                    scissor,
                    blend,
                    sampler,
                    sample_kind,
                    &source,
                    v0,
                    v1,
                    v2,
                );
                off += 3 * trueos_gfx_core::TEX_VERTEX_SIZE;
            }
        })?;
        let elapsed_ms = embassy_time::Instant::now()
            .as_millis()
            .saturating_sub(draw_start_ms);
        if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || elapsed_ms >= 20 {
            crate::log!(
                "intel/gfx-backend: draw-tex mode=cpu target=image sample={} triangles={} source={}x{} elapsed_ms={}\n",
                match sample_kind {
                    SampleKind::Mask => "mask",
                    SampleKind::Rgba => "rgba",
                },
                verts.len() / (3 * trueos_gfx_core::TEX_VERTEX_SIZE),
                source.width,
                source.height,
                elapsed_ms
            );
        }
        Ok(())
    }

    fn copy_tex_quads_to_screen(
        &mut self,
        source: &ImageEntry,
        verts: &[u8],
        sampler: SamplerDesc,
        blend: BlendDesc,
        sample_kind: SampleKind,
        scissor: Option<ScissorRect>,
    ) -> Option<(usize, usize, &'static str)> {
        if sample_kind != SampleKind::Rgba
            || !(blend == BlendDesc::disabled() || blend == BlendDesc::straight_alpha())
            || sampler.wrap_s != SamplerWrap::ClampToEdge
            || sampler.wrap_t != SamplerWrap::ClampToEdge
            || !sampler_filter_is_texel_center_equivalent(sampler)
            || verts.is_empty()
            || !verts
                .len()
                .is_multiple_of(6 * trueos_gfx_core::TEX_VERTEX_SIZE)
        {
            return None;
        }

        self.ensure_screen_rgba().ok()?;
        let dst_width = self.swapchain_desc.extent.width;
        let dst_height = self.swapchain_desc.extent.height;
        let full_scene_alpha_candidate = blend == BlendDesc::straight_alpha()
            && source.width == dst_width
            && source.height == dst_height
            && scissor.is_none()
            && verts.len() == 6 * trueos_gfx_core::TEX_VERTEX_SIZE;
        let full_scene_alpha_quad_ok =
            tex_quad_is_full_target_white_rgb(verts, dst_width, dst_height);
        if full_scene_alpha_candidate
            && (self.submit_seq <= 16 || self.submit_seq.is_multiple_of(60))
        {
            let src = source.dma_rgba_slice().unwrap_or(source.rgba.as_slice());
            let src_pitch = source.dma.pitch_bytes as usize;
            let alpha = rgba_alpha_summary(src, source.width, source.height, src_pitch);
            crate::log!(
                "intel/gfx-backend: scene-present probe source={}x{} target={}x{} pitch=0x{:X} rgba_len={} dma_bytes={} gpu_dirty={} surface={} decode_alpha={} full_target={} alpha_min={} alpha_max={} alpha_zero={} alpha_mid={} alpha_opaque={} first_nonzero={} {}x{}:0x{:08X} first_opaque={} {}x{}:0x{:08X} samples=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
                source.width,
                source.height,
                dst_width,
                dst_height,
                src_pitch,
                source.rgba.len(),
                source.dma.bytes,
                source.gpu_dirty as u8,
                source.gpgpu_surface().is_some() as u8,
                decode_alpha_tex_quad(verts, 0, source.width, source.height, dst_width, dst_height)
                    .is_some() as u8,
                full_scene_alpha_quad_ok as u8,
                alpha.min,
                alpha.max,
                alpha.zero,
                alpha.mid,
                alpha.opaque,
                alpha.first_nonzero_valid as u8,
                alpha.first_nonzero_x,
                alpha.first_nonzero_y,
                alpha.first_nonzero,
                alpha.first_opaque_valid as u8,
                alpha.first_opaque_x,
                alpha.first_opaque_y,
                alpha.first_opaque,
                alpha.tl,
                alpha.center,
                alpha.br,
                alpha.q1,
                alpha.q3
            );
            if full_scene_alpha_quad_ok {
                crate::intel::log_display_plane_ladder_probe("gfx-full-scene-alpha-candidate");
            }
        }
        if ENABLE_HW_PLANE_ALPHA_OVERLAY && full_scene_alpha_candidate && full_scene_alpha_quad_ok {
            if self.submit_seq <= 16 || self.submit_seq.is_multiple_of(60) {
                crate::log!(
                    "intel/gfx-backend: scene-present skip stage=hw-plane-alpha-overlay reason=ladder-log-only size={}x{}\n",
                    source.width,
                    source.height
                );
            }
        } else if full_scene_alpha_candidate
            && (self.submit_seq <= 16 || self.submit_seq.is_multiple_of(60))
        {
            crate::log!(
                "intel/gfx-backend: scene-present skip stage=hw-plane-alpha-overlay reason={} source={}x{} target={}x{}\n",
                if ENABLE_HW_PLANE_ALPHA_OVERLAY {
                    "quad-decode"
                } else {
                    "disabled"
                },
                source.width,
                source.height,
                dst_width,
                dst_height
            );
        }
        let prefer_cpu_alpha_rect_primary =
            blend == BlendDesc::straight_alpha() && !full_scene_alpha_candidate;
        if !prefer_cpu_alpha_rect_primary && let Some(src_surface) = source.gpgpu_surface() {
            if full_scene_alpha_candidate && full_scene_alpha_quad_ok {
                if let Some(stats) = crate::intel::gpgpu::alpha_blend_rgba8_over_primary_stats(
                    src_surface,
                    crate::intel::gpgpu::GpgpuRect::new(0, 0, source.width, source.height),
                    crate::intel::gpgpu::GpgpuPoint::new(0, 0),
                ) {
                    self.screen_rgba_gpu_dirty = true;
                    if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                        crate::log!(
                            "intel/gfx-backend: draw-tex mode=gpgpu-full-alpha-over-primary target=screen spans={} submits={} source={}x{} target={}x{}\n",
                            stats.spans,
                            stats.submits,
                            source.width,
                            source.height,
                            dst_width,
                            dst_height
                        );
                        crate::intel::log_display_plane_ladder_probe(
                            "gfx-after-gpgpu-full-alpha-over-primary",
                        );
                    }
                    return Some((
                        1,
                        (source.width as usize)
                            .saturating_mul(source.height as usize)
                            .saturating_mul(4),
                        "gpgpu-full-alpha-over-primary",
                    ));
                }
            }
            let mut quads = 0usize;
            let mut submits = 0usize;
            let mut spans = 0usize;
            let mut pixels = 0usize;
            let mut off = 0usize;
            let mode = if blend == BlendDesc::straight_alpha() {
                "gpgpu-alpha-over-primary"
            } else {
                "gpgpu-primary-xrgb-rect"
            };
            while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
                let Some(quad) = (if blend == BlendDesc::straight_alpha() {
                    decode_alpha_tex_quad(
                        verts,
                        off,
                        source.width,
                        source.height,
                        dst_width,
                        dst_height,
                    )
                } else {
                    decode_copy_tex_quad(
                        verts,
                        off,
                        source.width,
                        source.height,
                        dst_width,
                        dst_height,
                    )
                }) else {
                    if full_scene_alpha_candidate {
                        crate::log!(
                            "intel/gfx-backend: scene-present miss stage=decode mode={} off={}\n",
                            mode,
                            off
                        );
                    }
                    return None;
                };
                let Some(quad) = clip_texture_copy_quad_to_scissor(quad, scissor) else {
                    if full_scene_alpha_candidate {
                        crate::log!(
                            "intel/gfx-backend: scene-present miss stage=scissor mode={} off={}\n",
                            mode,
                            off
                        );
                    }
                    return None;
                };
                let src_rect = crate::intel::gpgpu::GpgpuRect::new(
                    quad.src_x as i32,
                    quad.src_y as i32,
                    quad.width,
                    quad.height,
                );
                let dst_xy = crate::intel::gpgpu::GpgpuPoint::new(quad.dst_x, quad.dst_y);
                let Some(stats) = (if blend == BlendDesc::straight_alpha() {
                    crate::intel::gpgpu::alpha_blend_rgba8_over_primary_stats(
                        src_surface,
                        src_rect,
                        dst_xy,
                    )
                } else {
                    crate::intel::gpgpu::present_rgba8_rect_to_primary_xrgb_stats(
                        src_surface,
                        src_rect,
                        dst_xy,
                    )
                }) else {
                    if full_scene_alpha_candidate {
                        crate::log!(
                            "intel/gfx-backend: scene-present miss stage=submit mode={} src_rect={}x{}@{},{} dst={},{}\n",
                            mode,
                            src_rect.width,
                            src_rect.height,
                            src_rect.x,
                            src_rect.y,
                            dst_xy.x,
                            dst_xy.y
                        );
                    }
                    return None;
                };
                quads = quads.saturating_add(1);
                submits = submits.saturating_add(stats.submits);
                spans = spans.saturating_add(stats.spans);
                pixels = pixels
                    .saturating_add((quad.width as usize).saturating_mul(quad.height as usize));
                off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
            }
            if quads > 0 && submits > 0 {
                self.screen_rgba_gpu_dirty = true;
                if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                    crate::log!(
                        "intel/gfx-backend: draw-tex mode={} target=screen quads={} spans={} submits={} pixels={} source={}x{} target={}x{}\n",
                        mode,
                        quads,
                        spans,
                        submits,
                        pixels,
                        source.width,
                        source.height,
                        dst_width,
                        dst_height
                    );
                }
                return Some((quads, pixels.saturating_mul(4), mode));
            }
            if full_scene_alpha_candidate {
                crate::log!(
                    "intel/gfx-backend: scene-present miss stage=no-submits mode={} quads={} spans={} submits={}\n",
                    mode,
                    quads,
                    spans,
                    submits
                );
            }
        }

        if blend == BlendDesc::straight_alpha() && !full_scene_alpha_candidate {
            let src = source.dma_rgba_slice().unwrap_or(source.rgba.as_slice());
            let src_pitch = source.dma.pitch_bytes as usize;
            let mut quads = 0usize;
            let mut bytes = 0usize;
            let mut off = 0usize;
            while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
                let Some(quad) = decode_tex_quad_bounds(
                    verts,
                    off,
                    source.width,
                    source.height,
                    dst_width,
                    dst_height,
                ) else {
                    if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                        crate::log!(
                            "intel/gfx-backend: draw-tex cpu-alpha-rect-miss stage=decode off={} source={}x{} target={}x{}\n",
                            off,
                            source.width,
                            source.height,
                            dst_width,
                            dst_height
                        );
                    }
                    return None;
                };
                let Some(quad) = clip_texture_scale_quad_to_scissor(quad, scissor) else {
                    if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                        crate::log!(
                            "intel/gfx-backend: draw-tex cpu-alpha-rect-miss stage=scissor off={} dst={}x{}@{},{}\n",
                            off,
                            quad.dst_w,
                            quad.dst_h,
                            quad.dst_x,
                            quad.dst_y
                        );
                    }
                    return None;
                };
                if crate::intel::blend_rgba_primary_rect_scaled(
                    src,
                    source.width,
                    source.height,
                    src_pitch,
                    quad.src_x,
                    quad.src_y,
                    quad.src_w,
                    quad.src_h,
                    quad.dst_x,
                    quad.dst_y,
                    quad.dst_w,
                    quad.dst_h,
                    "gfx-alpha-rect-primary",
                ) {
                    quads = quads.saturating_add(1);
                    bytes = bytes.saturating_add(
                        (quad.dst_w as usize)
                            .saturating_mul(quad.dst_h as usize)
                            .saturating_mul(4),
                    );
                } else {
                    if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                        crate::log!(
                            "intel/gfx-backend: draw-tex cpu-alpha-rect-miss stage=blend off={} src={}x{}@{},{} dst={}x{}@{},{} pitch=0x{:X} src_len=0x{:X}\n",
                            off,
                            quad.src_w,
                            quad.src_h,
                            quad.src_x,
                            quad.src_y,
                            quad.dst_w,
                            quad.dst_h,
                            quad.dst_x,
                            quad.dst_y,
                            src_pitch,
                            src.len()
                        );
                    }
                    return None;
                }
                off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
            }
            if quads > 0 {
                self.screen_rgba_gpu_dirty = true;
                if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                    crate::log!(
                        "intel/gfx-backend: draw-tex mode=cpu-alpha-rect-primary target=screen quads={} bytes=0x{:X} source={}x{} target={}x{}\n",
                        quads,
                        bytes,
                        source.width,
                        source.height,
                        dst_width,
                        dst_height
                    );
                }
                return Some((quads, bytes, "cpu-alpha-rect-primary"));
            }
        }

        if source.width == dst_width
            && source.height == dst_height
            && scissor.is_none()
            && verts.len() == 6 * trueos_gfx_core::TEX_VERTEX_SIZE
            && tex_quad_is_full_target_untinted(verts, dst_width, dst_height)
        {
            let src = source.dma_rgba_slice().unwrap_or(source.rgba.as_slice());
            let bytes = self.screen_rgba.len().min(src.len());
            self.screen_rgba[..bytes].copy_from_slice(&src[..bytes]);
            self.screen_rgba_gpu_dirty = false;
            return Some((1, bytes, "cpu-shadow-full"));
        }
        if full_scene_alpha_candidate {
            let src = source.dma_rgba_slice().unwrap_or(source.rgba.as_slice());
            if crate::intel::present_rgba_primary(
                src,
                source.width,
                source.height,
                source.dma.pitch_bytes as usize,
                "gfx-full-scene-primary-recovery",
            ) {
                self.screen_rgba_gpu_dirty = true;
                if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                    crate::log!(
                        "intel/gfx-backend: draw-tex mode=cpu-primary-recovery target=screen source={}x{} target={}x{}\n",
                        source.width,
                        source.height,
                        dst_width,
                        dst_height
                    );
                }
                return Some((1, src.len(), "cpu-primary-recovery"));
            }
        }

        let mut quads = 0usize;
        let mut bytes = 0usize;
        let mut off = 0usize;
        while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
            let quad = decode_copy_tex_quad(
                verts,
                off,
                source.width,
                source.height,
                dst_width,
                dst_height,
            )?;
            let quad = clip_texture_copy_quad_to_scissor(quad, scissor)?;
            if quad.dst_x < 0 || quad.dst_y < 0 {
                return None;
            }
            let dst_x = quad.dst_x as usize;
            let dst_y = quad.dst_y as usize;
            let src_x = quad.src_x as usize;
            let src_y = quad.src_y as usize;
            let width = quad.width as usize;
            let height = quad.height as usize;
            if dst_x.saturating_add(width) > dst_width as usize
                || dst_y.saturating_add(height) > dst_height as usize
                || src_x.saturating_add(width) > source.width as usize
                || src_y.saturating_add(height) > source.height as usize
            {
                return None;
            }

            let row_bytes = width.checked_mul(4)?;
            let src_pitch = (source.width as usize).checked_mul(4)?;
            let dst_pitch = (dst_width as usize).checked_mul(4)?;
            for row in 0..height {
                let src_off = src_y
                    .checked_add(row)?
                    .checked_mul(src_pitch)?
                    .checked_add(src_x.checked_mul(4)?)?;
                let dst_off = dst_y
                    .checked_add(row)?
                    .checked_mul(dst_pitch)?
                    .checked_add(dst_x.checked_mul(4)?)?;
                let src_end = src_off.checked_add(row_bytes)?;
                let dst_end = dst_off.checked_add(row_bytes)?;
                if src_end > source.rgba.len() || dst_end > self.screen_rgba.len() {
                    return None;
                }
                self.screen_rgba[dst_off..dst_end].copy_from_slice(&source.rgba[src_off..src_end]);
                bytes = bytes.saturating_add(row_bytes);
            }
            quads = quads.saturating_add(1);
            off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
        }

        if quads == 0 {
            if full_scene_alpha_candidate {
                crate::log!("intel/gfx-backend: scene-present miss stage=cpu-copy-empty\n");
            }
            return None;
        }
        self.screen_rgba_gpu_dirty = false;
        Some((quads, bytes, "cpu-copy-rect"))
    }

    fn rgb_quads_to_image(
        &self,
        dst_id: ImageId,
        verts: &[u8],
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
    ) -> Option<(usize, usize, u64)> {
        if blend != BlendDesc::disabled()
            || verts.is_empty()
            || !verts
                .len()
                .is_multiple_of(6 * trueos_gfx_core::RGB_VERTEX_SIZE)
        {
            return None;
        }
        let dst = self.image(dst_id)?;
        let dst_surface = dst.gpgpu_surface()?;
        let mut total_spans = 0usize;
        let mut quads = 0usize;
        let mut off = 0usize;
        while off + (6 * trueos_gfx_core::RGB_VERTEX_SIZE) <= verts.len() {
            let quad = decode_rgb_fill_quad_shape(verts, off, dst.width, dst.height)?;
            let quad = clip_rgb_fill_quad_to_scissor(quad, scissor)?;
            let spans = crate::intel::gpgpu::fill_rect_worklist_rgba8(
                dst_surface,
                crate::intel::gpgpu::GpgpuRect::new(
                    quad.dst_x,
                    quad.dst_y,
                    quad.width,
                    quad.height,
                ),
                quad.color_rgba,
            );
            if spans == 0 {
                return None;
            }
            total_spans = total_spans.saturating_add(spans);
            quads = quads.saturating_add(1);
            off = off.saturating_add(6 * trueos_gfx_core::RGB_VERTEX_SIZE);
        }
        (quads > 0).then_some((quads, total_spans, dst.gpu_addr))
    }

    fn copy_tex_quads_to_image(
        &self,
        dst_id: ImageId,
        source: &ImageEntry,
        verts: &[u8],
        sampler: SamplerDesc,
        blend: BlendDesc,
        sample_kind: SampleKind,
        scissor: Option<ScissorRect>,
    ) -> Option<(usize, usize, usize, u64)> {
        if sample_kind != SampleKind::Rgba
            || blend != BlendDesc::disabled()
            || sampler.wrap_s != SamplerWrap::ClampToEdge
            || sampler.wrap_t != SamplerWrap::ClampToEdge
            || !sampler_filter_is_texel_center_equivalent(sampler)
            || verts.is_empty()
            || !verts
                .len()
                .is_multiple_of(6 * trueos_gfx_core::TEX_VERTEX_SIZE)
        {
            return None;
        }
        let dst = self.image(dst_id)?;
        if dst.gpu_addr == source.gpu_addr {
            return None;
        }
        let src_surface = source.gpgpu_surface()?;
        let dst_surface = dst.gpgpu_surface()?;
        let mut ops = Vec::new();
        let mut off = 0usize;
        while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
            let quad = decode_copy_tex_quad(
                verts,
                off,
                source.width,
                source.height,
                dst.width,
                dst.height,
            )?;
            let quad = clip_texture_copy_quad_to_scissor(quad, scissor)?;
            ops.push(crate::intel::gpgpu::GpgpuCompositeRect {
                src: src_surface,
                src_rect: crate::intel::gpgpu::GpgpuRect::new(
                    quad.src_x as i32,
                    quad.src_y as i32,
                    quad.width,
                    quad.height,
                ),
                dst: dst_surface,
                dst_xy: crate::intel::gpgpu::GpgpuPoint::new(quad.dst_x, quad.dst_y),
                mode: crate::intel::gpgpu::GpgpuCompositeMode::Copy,
            });
            off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
        }
        if ops.is_empty() {
            return None;
        }
        let stats = crate::intel::gpgpu::composite_rects_rgba8_stats(ops.as_slice());
        if stats.submits == 0 || stats.spans == 0 {
            return None;
        }
        Some((ops.len(), stats.spans, stats.submits, dst.gpu_addr))
    }

    fn alpha_tex_quads_to_image(
        &self,
        dst_id: ImageId,
        source: &ImageEntry,
        verts: &[u8],
        sampler: SamplerDesc,
        blend: BlendDesc,
        sample_kind: SampleKind,
        scissor: Option<ScissorRect>,
    ) -> Option<(usize, usize, usize, u64)> {
        if sample_kind != SampleKind::Rgba
            || blend != BlendDesc::straight_alpha()
            || sampler.wrap_s != SamplerWrap::ClampToEdge
            || sampler.wrap_t != SamplerWrap::ClampToEdge
            || !sampler_filter_is_texel_center_equivalent(sampler)
            || verts.is_empty()
            || !verts
                .len()
                .is_multiple_of(6 * trueos_gfx_core::TEX_VERTEX_SIZE)
        {
            return None;
        }
        let dst = self.image(dst_id)?;
        if dst.gpu_addr == source.gpu_addr {
            return None;
        }
        let src_surface = source.gpgpu_surface()?;
        let dst_surface = dst.gpgpu_surface()?;
        let mut total_spans = 0usize;
        let mut total_submits = 0usize;
        let mut quads = 0usize;
        let mut off = 0usize;
        while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
            let quad = decode_copy_tex_quad(
                verts,
                off,
                source.width,
                source.height,
                dst.width,
                dst.height,
            )?;
            let quad = clip_texture_copy_quad_to_scissor(quad, scissor)?;
            let stats = crate::intel::gpgpu::composite_rect_rgba8_stats(
                crate::intel::gpgpu::GpgpuCompositeRect {
                    src: src_surface,
                    src_rect: crate::intel::gpgpu::GpgpuRect::new(
                        quad.src_x as i32,
                        quad.src_y as i32,
                        quad.width,
                        quad.height,
                    ),
                    dst: dst_surface,
                    dst_xy: crate::intel::gpgpu::GpgpuPoint::new(quad.dst_x, quad.dst_y),
                    mode: crate::intel::gpgpu::GpgpuCompositeMode::SrcOver,
                },
            );
            if stats.submits == 0 || stats.spans == 0 {
                return None;
            }
            total_spans = total_spans.saturating_add(stats.spans);
            total_submits = total_submits.saturating_add(stats.submits);
            quads = quads.saturating_add(1);
            off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
        }
        (quads > 0).then_some((quads, total_spans, total_submits, dst.gpu_addr))
    }

    fn mask_tex_quads_to_image(
        &self,
        dst_id: ImageId,
        source: &ImageEntry,
        verts: &[u8],
        sampler: SamplerDesc,
        blend: BlendDesc,
        sample_kind: SampleKind,
        scissor: Option<ScissorRect>,
    ) -> Option<(usize, usize, usize, u64)> {
        if sample_kind != SampleKind::Mask
            || blend != BlendDesc::straight_alpha()
            || sampler.wrap_s != SamplerWrap::ClampToEdge
            || sampler.wrap_t != SamplerWrap::ClampToEdge
            || !sampler_filter_is_texel_center_equivalent(sampler)
            || verts.is_empty()
            || !verts
                .len()
                .is_multiple_of(6 * trueos_gfx_core::TEX_VERTEX_SIZE)
        {
            return None;
        }
        let dst = self.image(dst_id)?;
        if dst.gpu_addr == source.gpu_addr {
            return None;
        }
        let mask_surface = source.gpgpu_mask_surface()?;
        let dst_surface = dst.gpgpu_surface()?;
        let mut total_spans = 0usize;
        let mut total_submits = 0usize;
        let mut quads = 0usize;
        let mut off = 0usize;
        while off + (6 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
            let (quad, color_rgba) = decode_mask_tex_quad(
                verts,
                off,
                source.width,
                source.height,
                dst.width,
                dst.height,
            )?;
            let quad = clip_texture_copy_quad_to_scissor(quad, scissor)?;
            let stats = crate::intel::gpgpu::glyph_mask_rgba8_stats(
                crate::intel::gpgpu::GpgpuGlyphMaskBlit {
                    mask: mask_surface,
                    mask_rect: crate::intel::gpgpu::GpgpuRect::new(
                        quad.src_x as i32,
                        quad.src_y as i32,
                        quad.width,
                        quad.height,
                    ),
                    dst: dst_surface,
                    dst_xy: crate::intel::gpgpu::GpgpuPoint::new(quad.dst_x, quad.dst_y),
                    color_rgba,
                },
            );
            if stats.submits == 0 || stats.spans == 0 {
                return None;
            }
            total_spans = total_spans.saturating_add(stats.spans);
            total_submits = total_submits.saturating_add(stats.submits);
            quads = quads.saturating_add(1);
            off = off.saturating_add(6 * trueos_gfx_core::TEX_VERTEX_SIZE);
        }
        (quads > 0).then_some((quads, total_spans, total_submits, dst.gpu_addr))
    }

    fn draw_mandelbrot(
        &mut self,
        target: RenderTarget,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        scissor: Option<ScissorRect>,
        blend: BlendDesc,
        pipeline_kind: PipelineKind,
    ) -> Result<()> {
        if matches!(target, RenderTarget::Screen) {
            self.sync_screen_rgba_from_gpu();
        }
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        let start = byte_offset as usize + first_vertex as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        let need = vertex_count as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        if start > buffer.bytes.len() || start.saturating_add(need) > buffer.bytes.len() {
            return Err(Error::Invalid);
        }
        let verts = buffer.bytes[start..start + need].to_vec();
        let should_log = self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120);
        self.with_target_mut(target, |rgba, width, height| {
            if should_log {
                crate::log!(
                    "intel/gfx-backend: draw-mandelbrot mode=simd16-mask triangles={} size={}x{} target={}\n",
                    verts.len() / (3 * trueos_gfx_core::TEX_VERTEX_SIZE),
                    width,
                    height,
                    match target {
                        RenderTarget::Screen => "screen",
                        RenderTarget::Image(_) => "image",
                    }
                );
            }
            let mut off = 0usize;
            while off + (3 * trueos_gfx_core::TEX_VERTEX_SIZE) <= verts.len() {
                let Some(v0) = read_tex_vertex_f32_bytes(&verts, off) else {
                    break;
                };
                let Some(v1) =
                    read_tex_vertex_f32_bytes(&verts, off + trueos_gfx_core::TEX_VERTEX_SIZE)
                else {
                    break;
                };
                let Some(v2) =
                    read_tex_vertex_f32_bytes(&verts, off + 2 * trueos_gfx_core::TEX_VERTEX_SIZE)
                else {
                    break;
                };
                draw_mandelbrot_triangle_rgba(
                    rgba,
                    width,
                    height,
                    scissor,
                    blend,
                    pipeline_kind,
                    v0,
                    v1,
                    v2,
                );
                off += 3 * trueos_gfx_core::TEX_VERTEX_SIZE;
            }
        })?;
        Ok(())
    }
}

impl GfxDevice for IntelGfxBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            supports_rgbx8888: true,
            supports_host_visible_buffers: true,
            supports_scissor: true,
        }
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        let size = usize::try_from(desc.size).map_err(|_| Error::Invalid)?;
        let id = Self::alloc_slot(
            &mut self.buffers,
            BufferEntry {
                bytes: alloc::vec![0; size],
                usage: desc.usage,
                memory: desc.memory,
            },
        );
        Ok(BufferId::from_raw(id))
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        if let Some(slot) = id
            .raw()
            .checked_sub(1)
            .and_then(|idx| self.buffers.get_mut(idx as usize))
        {
            *slot = None;
        }
    }

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId> {
        let id = self.next_shader_raw;
        self.next_shader_raw = self.next_shader_raw.wrapping_add(1).max(1);
        Ok(ShaderId::from_raw(id))
    }

    fn destroy_shader(&mut self, _id: ShaderId) {}

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        let kind = if desc.vertex_layout.texcoord_format == TexCoordFormat::None {
            PipelineKind::Rgb
        } else {
            match desc.fs.map(|id| id.raw()) {
                Some(TEX_PIPELINE_FS_MASK_TAG_RAW) => PipelineKind::TexMask,
                Some(TEX_PIPELINE_FS_RGBA_TAG_RAW) => PipelineKind::TexRgba,
                Some(TEX_PIPELINE_FS_PARTICLE_TAG_RAW) => PipelineKind::TexParticle,
                Some(crate::gfx::mandelbrot::MANDELBROT_PIPELINE_FS_TAG_RAW) => {
                    PipelineKind::Mandelbrot
                }
                Some(crate::gfx::mandelbrot::JULIA_PIPELINE_FS_TAG_RAW) => PipelineKind::Julia,
                Some(crate::gfx::mandelbrot::BURNING_SHIP_PIPELINE_FS_TAG_RAW) => {
                    PipelineKind::BurningShip
                }
                _ => PipelineKind::TexRgba,
            }
        };
        let id = Self::alloc_slot(&mut self.pipelines, PipelineEntry { desc, kind });
        Ok(PipelineId::from_raw(id))
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        if let Some(slot) = id
            .raw()
            .checked_sub(1)
            .and_then(|idx| self.pipelines.get_mut(idx as usize))
        {
            *slot = None;
        }
    }

    fn create_image(&mut self, desc: ImageDesc) -> Result<ImageId> {
        let len = rgba_len(desc.width, desc.height).ok_or(Error::Invalid)?;
        if desc.width > MAX_BACKEND_IMAGE_DIM
            || desc.height > MAX_BACKEND_IMAGE_DIM
            || len > MAX_BACKEND_IMAGE_BYTES
        {
            crate::log!(
                "intel/gfx-backend: reject image create size={}x{} bytes={} format={:?}\n",
                desc.width,
                desc.height,
                len,
                desc.format
            );
            return Err(Error::Invalid);
        }
        let gpu_addr = self.next_image_gpu_addr;
        let alloc_span = u64::try_from(len).map_err(|_| Error::Invalid)?;
        let mask_len = (desc.width as usize)
            .checked_mul(desc.height as usize)
            .ok_or(Error::Invalid)?;
        let mask_span = u64::try_from(mask_len).map_err(|_| Error::Invalid)?;
        let mask_gpu_addr =
            align_up_u64(gpu_addr.saturating_add(alloc_span), crate::intel::WARM_ALIGN as u64);
        self.next_image_gpu_addr = align_up_u64(
            mask_gpu_addr
                .saturating_add(mask_span)
                .saturating_add(IMAGE_GPU_VA_ALIGN.saturating_sub(1)),
            IMAGE_GPU_VA_ALIGN,
        );
        let id = Self::alloc_slot(
            &mut self.images,
            ImageEntry {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                gpu_addr,
                mask_gpu_addr,
                dma: Self::alloc_image_dma(desc.width, desc.height, len)?,
                mask_dma: None,
                gpu_dirty: false,
                rgba: alloc::vec![0; len],
            },
        );
        Ok(ImageId::from_raw(id))
    }

    fn destroy_image(&mut self, id: ImageId) {
        if let Some(slot) = id
            .raw()
            .checked_sub(1)
            .and_then(|idx| self.images.get_mut(idx as usize))
        {
            if let Some(image) = slot.as_ref() {
                Self::dealloc_image_dma(&image.dma);
                if let Some(mask_dma) = image.mask_dma.as_ref() {
                    Self::dealloc_image_dma(mask_dma);
                }
            }
            *slot = None;
        }
    }

    fn write_image(&mut self, id: ImageId, data: &[u8]) -> Result<()> {
        let image = self.image_mut(id).ok_or(Error::NotFound)?;
        let len = rgba_len(image.width, image.height).ok_or(Error::Invalid)?;
        if data.len() < len {
            return Err(Error::Invalid);
        }
        image.rgba[..len].copy_from_slice(&data[..len]);
        if image.format == ImageFormat::Rgbx8888 {
            force_opaque_alpha(image.rgba.as_mut_slice());
        }
        image.copy_cpu_to_dma();
        image.rebuild_mask_dma();
        image.gpu_dirty = false;
        Ok(())
    }

    fn write_image_region(&mut self, id: ImageId, region: ImageRegion, data: &[u8]) -> Result<()> {
        let image = self.image_mut(id).ok_or(Error::NotFound)?;
        if region.width == 0
            || region.height == 0
            || region.x.saturating_add(region.width) > image.width
            || region.y.saturating_add(region.height) > image.height
        {
            return Err(Error::Invalid);
        }
        let need = rgba_len(region.width, region.height).ok_or(Error::Invalid)?;
        if data.len() < need {
            return Err(Error::Invalid);
        }
        for row in 0..region.height as usize {
            let src_off = row.saturating_mul(region.width as usize).saturating_mul(4);
            let dst_off = ((region.y as usize + row)
                .saturating_mul(image.width as usize)
                .saturating_add(region.x as usize))
            .saturating_mul(4);
            let row_len = region.width as usize * 4;
            image.rgba[dst_off..dst_off + row_len]
                .copy_from_slice(&data[src_off..src_off + row_len]);
        }
        if image.format == ImageFormat::Rgbx8888 {
            force_opaque_alpha(image.rgba.as_mut_slice());
        }
        image.copy_cpu_to_dma();
        image.rebuild_mask_dma();
        image.gpu_dirty = false;
        Ok(())
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        let buffer = self.buffer_mut(id).ok_or(Error::NotFound)?;
        if buffer.memory != MemoryType::HostVisible {
            return Err(Error::Unsupported);
        }
        let offset = usize::try_from(offset).map_err(|_| Error::Invalid)?;
        let end = offset.saturating_add(data.len());
        if end > buffer.bytes.len() {
            return Err(Error::Invalid);
        }
        buffer.bytes[offset..end].copy_from_slice(data);
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        let submit_start_ms = embassy_time::Instant::now().as_millis();
        static SUBMIT_ENTRY_LOGS: core::sync::atomic::AtomicU32 =
            core::sync::atomic::AtomicU32::new(0);
        let entry_n = SUBMIT_ENTRY_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if entry_n < 16 {
            crate::log!(
                "intel/gfx-backend: submit entry n={} cmds={} extent={}x{}\n",
                entry_n + 1,
                cmds.commands.len(),
                self.swapchain_desc.extent.width,
                self.swapchain_desc.extent.height
            );
        }
        if let Err(err) = self.ensure_screen_rgba() {
            if entry_n < 16 {
                crate::log!(
                    "intel/gfx-backend: submit ensure_screen_rgba failed err={:?} extent={}x{}\n",
                    err,
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height
                );
            }
            return Err(err);
        }

        let mut target = RenderTarget::Screen;
        let mut scissor: Option<ScissorRect> = None;
        let mut blend = BlendDesc::disabled();
        let mut sampler = SamplerDesc::default_2d();
        let mut pipeline = PipelineKind::Rgb;
        let mut bound_buffer = BufferId::invalid();
        let mut bound_offset = 0u64;
        let mut bound_image = ImageId::invalid();
        let mut unsupported = 0u32;
        let mut present_cmds = 0u32;

        let trace_commands = entry_n < 4;
        for (cmd_idx, cmd) in cmds.commands.iter().enumerate() {
            if trace_commands {
                crate::log!(
                    "intel/gfx-backend: cmd-enter submit={} idx={} op={} target={} unsupported={}\n",
                    entry_n + 1,
                    cmd_idx,
                    command_label(cmd),
                    render_target_label(target),
                    unsupported
                );
            }
            match *cmd {
                Command::ClearColor { rgb } => {
                    let fast_ok = match target {
                        RenderTarget::Screen => {
                            let screen_surface_gpu =
                                crate::intel::primary_present_surface_gpu_addr()
                                    .unwrap_or(0x0200_0000);
                            crate::intel::rcs_clear_rgba_surface(
                                self.screen_rgba.as_slice(),
                                self.swapchain_desc.extent.width,
                                self.swapchain_desc.extent.height,
                                screen_surface_gpu,
                                rgb,
                            )
                            .then_some((
                                self.swapchain_desc.extent.width,
                                self.swapchain_desc.extent.height,
                                screen_surface_gpu,
                            ))
                        }
                        RenderTarget::Image(id) if ENABLE_IMAGE_GPGPU_CLEAR => {
                            self.image(id).and_then(|image| {
                                let surface = image.gpgpu_surface()?;
                                let spans = crate::intel::gpgpu::fill_rect_worklist_rgba8(
                                    surface,
                                    crate::intel::gpgpu::GpgpuRect::new(
                                        0,
                                        0,
                                        image.width,
                                        image.height,
                                    ),
                                    rgb_to_kernel_rgba(rgb, 0xFF),
                                );
                                (spans > 0).then_some((image.width, image.height, image.gpu_addr))
                            })
                        }
                        RenderTarget::Image(_) => None,
                    };
                    if let Some((target_w, target_h, target_gpu_addr)) = fast_ok {
                        match target {
                            RenderTarget::Screen => self.screen_rgba_gpu_dirty = true,
                            RenderTarget::Image(id) => {
                                if let Some(image) = self.image_mut(id) {
                                    image.gpu_dirty = true;
                                }
                            }
                        }
                        if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                            crate::log!(
                                "intel/gfx-backend: clear mode=rcs-store size={}x{} rgb=0x{:06X} gpu=0x{:X}\n",
                                target_w,
                                target_h,
                                rgb & 0x00FF_FFFF,
                                target_gpu_addr
                            );
                        }
                    } else {
                        if matches!(target, RenderTarget::Screen) {
                            self.sync_screen_rgba_from_gpu();
                        }
                        self.with_target_mut(target, |rgba, width, height| {
                            clear_rgba_buffer(rgba, width, height, rgb);
                        })?;
                    }
                }
                Command::ClearColorRgba { rgba: clear } => {
                    let fast_ok = match target {
                        RenderTarget::Screen => None,
                        RenderTarget::Image(id) if ENABLE_IMAGE_GPGPU_CLEAR => {
                            self.image(id).and_then(|image| {
                                let surface = image.gpgpu_surface()?;
                                let spans = crate::intel::gpgpu::fill_rect_worklist_rgba8(
                                    surface,
                                    crate::intel::gpgpu::GpgpuRect::new(
                                        0,
                                        0,
                                        image.width,
                                        image.height,
                                    ),
                                    rgba_to_kernel_rgba(clear.r, clear.g, clear.b, clear.a),
                                );
                                (spans > 0).then_some((image.width, image.height, image.gpu_addr))
                            })
                        }
                        RenderTarget::Image(_) => None,
                    };
                    if let Some((target_w, target_h, target_gpu_addr)) = fast_ok {
                        if let RenderTarget::Image(id) = target
                            && let Some(image) = self.image_mut(id)
                        {
                            image.gpu_dirty = true;
                        }
                        if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                            crate::log!(
                                "intel/gfx-backend: clear-rgba mode=gpgpu-fill size={}x{} rgba=[{},{},{},{}] gpu=0x{:X}\n",
                                target_w,
                                target_h,
                                clear.r,
                                clear.g,
                                clear.b,
                                clear.a,
                                target_gpu_addr
                            );
                        }
                    } else {
                        if matches!(target, RenderTarget::Screen) {
                            self.sync_screen_rgba_from_gpu();
                        }
                        self.with_target_mut(target, |rgba, width, height| {
                            for px in rgba
                                .chunks_exact_mut(4)
                                .take((width as usize).saturating_mul(height as usize))
                            {
                                px[0] = clear.r;
                                px[1] = clear.g;
                                px[2] = clear.b;
                                px[3] = clear.a;
                            }
                        })?;
                    }
                }
                Command::ClearRect {
                    rgb,
                    x,
                    y,
                    width,
                    height,
                } => {
                    let target_dims = match target {
                        RenderTarget::Screen => Some((
                            self.swapchain_desc.extent.width,
                            self.swapchain_desc.extent.height,
                            0,
                        )),
                        RenderTarget::Image(id) => self
                            .image(id)
                            .map(|image| (image.width, image.height, image.gpu_addr)),
                    };
                    let mut fast_ok = None;
                    if let Some((target_w, target_h, target_gpu_addr)) = target_dims {
                        let mut x0 = x.min(target_w);
                        let mut y0 = y.min(target_h);
                        let mut x1 = x.saturating_add(width).min(target_w);
                        let mut y1 = y.saturating_add(height).min(target_h);
                        if let Some(scissor) = scissor {
                            x0 = x0.max(scissor.x.min(target_w));
                            y0 = y0.max(scissor.y.min(target_h));
                            x1 = x1.min(scissor.x.saturating_add(scissor.width).min(target_w));
                            y1 = y1.min(scissor.y.saturating_add(scissor.height).min(target_h));
                        }
                        if x0 < x1 && y0 < y1 {
                            if ENABLE_IMAGE_GPGPU_CLEAR
                                && let RenderTarget::Image(id) = target
                                && let Some(image) = self.image(id)
                                && let Some(surface) = image.gpgpu_surface()
                            {
                                let spans = crate::intel::gpgpu::fill_rect_worklist_rgba8(
                                    surface,
                                    crate::intel::gpgpu::GpgpuRect::new(
                                        x0 as i32,
                                        y0 as i32,
                                        x1 - x0,
                                        y1 - y0,
                                    ),
                                    rgb_to_kernel_rgba(rgb, 0xFF),
                                );
                                if spans > 0 {
                                    fast_ok =
                                        Some((target_w, target_h, target_gpu_addr, x0, y0, x1, y1));
                                }
                            }
                        }
                    }
                    if let Some((target_w, target_h, target_gpu_addr, x0, y0, x1, y1)) = fast_ok {
                        if let RenderTarget::Image(id) = target
                            && let Some(image) = self.image_mut(id)
                        {
                            image.gpu_dirty = true;
                        }
                        if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                            crate::log!(
                                "intel/gfx-backend: clear-rect mode=gpgpu-fill size={}x{} rect={}x{}+{},{} rgb=0x{:06X} gpu=0x{:X}\n",
                                target_w,
                                target_h,
                                x1 - x0,
                                y1 - y0,
                                x0,
                                y0,
                                rgb & 0x00FF_FFFF,
                                target_gpu_addr
                            );
                        }
                    } else {
                        if matches!(target, RenderTarget::Screen) {
                            self.sync_screen_rgba_from_gpu();
                        }
                        self.with_target_mut(target, |rgba, target_w, target_h| {
                            let mut x0 = x.min(target_w);
                            let mut y0 = y.min(target_h);
                            let mut x1 = x.saturating_add(width).min(target_w);
                            let mut y1 = y.saturating_add(height).min(target_h);
                            if let Some(scissor) = scissor {
                                x0 = x0.max(scissor.x.min(target_w));
                                y0 = y0.max(scissor.y.min(target_h));
                                x1 = x1.min(scissor.x.saturating_add(scissor.width).min(target_w));
                                y1 = y1.min(scissor.y.saturating_add(scissor.height).min(target_h));
                            }
                            if x0 < x1 && y0 < y1 {
                                clear_rgba_rect(
                                    rgba,
                                    target_w,
                                    target_h,
                                    x0,
                                    y0,
                                    x1 - x0,
                                    y1 - y0,
                                    rgb,
                                );
                            }
                        })?;
                    }
                }
                Command::BindPipeline(id) => {
                    if let Some(entry) = self.pipeline(id) {
                        let _layout = entry.desc.vertex_layout;
                        pipeline = entry.kind;
                    } else {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
                Command::BindVertexBuffer { buffer, offset } => {
                    bound_buffer = buffer;
                    bound_offset = offset;
                }
                Command::BindImage(image) => {
                    bound_image = image;
                }
                Command::SetRenderTarget(render_target) => {
                    target = match render_target {
                        Some(image) => {
                            if self.image(image).is_some() {
                                RenderTarget::Image(image)
                            } else {
                                unsupported = unsupported.saturating_add(1);
                                RenderTarget::Screen
                            }
                        }
                        None => RenderTarget::Screen,
                    };
                }
                Command::SetSampler(next) => sampler = next,
                Command::SetBlend(next) => blend = next,
                Command::SetViewport(_viewport) => {}
                Command::SetScissor(next) => scissor = next,
                Command::Draw {
                    vertex_count,
                    first_vertex,
                } => {
                    if bound_buffer.is_valid() {
                        let draw_res = if matches!(
                            pipeline,
                            PipelineKind::Mandelbrot
                                | PipelineKind::Julia
                                | PipelineKind::BurningShip
                        ) {
                            self.draw_mandelbrot(
                                target,
                                bound_buffer,
                                bound_offset,
                                vertex_count,
                                first_vertex,
                                scissor,
                                blend,
                                pipeline,
                            )
                        } else if bound_image.is_valid() || pipeline != PipelineKind::Rgb {
                            self.draw_tex(
                                target,
                                bound_buffer,
                                bound_offset,
                                vertex_count,
                                first_vertex,
                                bound_image,
                                pipeline,
                                sampler,
                                scissor,
                                blend,
                            )
                        } else {
                            self.draw_rgb(
                                target,
                                bound_buffer,
                                bound_offset,
                                vertex_count,
                                first_vertex,
                                scissor,
                                blend,
                            )
                        };
                        if draw_res.is_err() {
                            unsupported = unsupported.saturating_add(1);
                        }
                    } else {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
                Command::Present => {
                    present_cmds = present_cmds.saturating_add(1);
                    let present_res = match target {
                        RenderTarget::Screen => self.present_screen(),
                        RenderTarget::Image(id) => self.present_image_target(id),
                    };
                    if present_res.is_err() {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
            }
            if trace_commands {
                crate::log!(
                    "intel/gfx-backend: cmd-exit submit={} idx={} op={} target={} unsupported={}\n",
                    entry_n + 1,
                    cmd_idx,
                    command_label(cmd),
                    render_target_label(target),
                    unsupported
                );
            }
        }

        self.submit_seq = self.submit_seq.wrapping_add(1);
        let elapsed_ms = embassy_time::Instant::now()
            .as_millis()
            .saturating_sub(submit_start_ms);
        if self.submit_seq <= 8
            || self.submit_seq.is_multiple_of(120)
            || unsupported != 0
            || present_cmds != 0
            || elapsed_ms >= 20
        {
            crate::log!(
                "intel/gfx-backend: submit seq={} cmds={} present_cmds={} unsupported={} target={} elapsed_ms={}\n",
                self.submit_seq,
                cmds.commands.len(),
                present_cmds,
                unsupported,
                match target {
                    RenderTarget::Screen => "screen",
                    RenderTarget::Image(_) => "image",
                },
                elapsed_ms
            );
        }

        let fence = FenceId::from_raw(self.next_fence_raw);
        self.next_fence_raw = self.next_fence_raw.wrapping_add(1).max(1);
        Ok(fence)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        fence.is_valid()
    }

    fn device_idle(&mut self) {}
}

impl GfxPresent for IntelGfxBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        if desc.extent.width == 0 || desc.extent.height == 0 {
            return Err(Error::Invalid);
        }
        self.swapchain_desc = desc;
        self.ensure_screen_rgba()
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain_desc
    }

    fn display_refresh_millihz(&mut self) -> Option<u32> {
        Some(60_000)
    }
}

#[inline]
fn rgba_len(width: u32, height: u32) -> Option<usize> {
    (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)
}

#[inline]
fn pack_xrgb(r: u8, g: u8, b: u8) -> u32 {
    ((r as u32) << 16) | ((g as u32) << 8) | (b as u32)
}

#[inline]
fn sample_rgba_word(rgba: &[u8], width: u32, x: u32, y: u32) -> u32 {
    if width == 0 {
        return 0;
    }
    let Some(idx) = (y as usize)
        .checked_mul(width as usize)
        .and_then(|row| row.checked_add(x as usize))
        .and_then(|pixel| pixel.checked_mul(4))
    else {
        return 0;
    };
    if idx + 4 > rgba.len() {
        return 0;
    }
    ((rgba[idx] as u32) << 24)
        | ((rgba[idx + 1] as u32) << 16)
        | ((rgba[idx + 2] as u32) << 8)
        | rgba[idx + 3] as u32
}

#[derive(Clone, Copy)]
struct RgbaAlphaSummary {
    min: u8,
    max: u8,
    zero: usize,
    mid: usize,
    opaque: usize,
    first_nonzero_valid: bool,
    first_nonzero_x: u32,
    first_nonzero_y: u32,
    first_nonzero: u32,
    first_opaque_valid: bool,
    first_opaque_x: u32,
    first_opaque_y: u32,
    first_opaque: u32,
    tl: u32,
    center: u32,
    br: u32,
    q1: u32,
    q3: u32,
}

fn rgba_alpha_summary(
    rgba: &[u8],
    width: u32,
    height: u32,
    pitch_bytes: usize,
) -> RgbaAlphaSummary {
    let tl = sample_rgba_word_pitched(rgba, pitch_bytes, 0, 0);
    let center = sample_rgba_word_pitched(rgba, pitch_bytes, width / 2, height / 2);
    let br = sample_rgba_word_pitched(
        rgba,
        pitch_bytes,
        width.saturating_sub(1),
        height.saturating_sub(1),
    );
    let q1 = sample_rgba_word_pitched(rgba, pitch_bytes, width / 4, height / 4);
    let q3 = sample_rgba_word_pitched(
        rgba,
        pitch_bytes,
        width.saturating_mul(3) / 4,
        height.saturating_mul(3) / 4,
    );

    let mut out = RgbaAlphaSummary {
        min: 0,
        max: 0,
        zero: 0,
        mid: 0,
        opaque: 0,
        first_nonzero_valid: false,
        first_nonzero_x: 0,
        first_nonzero_y: 0,
        first_nonzero: 0,
        first_opaque_valid: false,
        first_opaque_x: 0,
        first_opaque_y: 0,
        first_opaque: 0,
        tl,
        center,
        br,
        q1,
        q3,
    };
    if width == 0 || height == 0 {
        return out;
    }
    let row_bytes = width as usize * 4;
    if pitch_bytes < row_bytes {
        return out;
    }

    let mut saw_pixel = false;
    let mut min_a = u8::MAX;
    let mut max_a = 0u8;
    for y in 0..height as usize {
        let row_off = y.saturating_mul(pitch_bytes);
        let Some(row) = rgba.get(row_off..row_off.saturating_add(row_bytes)) else {
            break;
        };
        for x in 0..width as usize {
            let off = x.saturating_mul(4);
            let a = row[off + 3];
            saw_pixel = true;
            min_a = min_a.min(a);
            max_a = max_a.max(a);
            if a == 0 {
                out.zero = out.zero.saturating_add(1);
            } else if a == 0xFF {
                out.opaque = out.opaque.saturating_add(1);
                if !out.first_opaque_valid {
                    out.first_opaque_valid = true;
                    out.first_opaque_x = x as u32;
                    out.first_opaque_y = y as u32;
                    out.first_opaque = ((row[off] as u32) << 24)
                        | ((row[off + 1] as u32) << 16)
                        | ((row[off + 2] as u32) << 8)
                        | row[off + 3] as u32;
                }
            } else {
                out.mid = out.mid.saturating_add(1);
            }
            if a != 0 && !out.first_nonzero_valid {
                out.first_nonzero_valid = true;
                out.first_nonzero_x = x as u32;
                out.first_nonzero_y = y as u32;
                out.first_nonzero = ((row[off] as u32) << 24)
                    | ((row[off + 1] as u32) << 16)
                    | ((row[off + 2] as u32) << 8)
                    | row[off + 3] as u32;
            }
        }
    }
    if saw_pixel {
        out.min = min_a;
        out.max = max_a;
    }
    out
}

#[inline]
fn sample_rgba_word_pitched(rgba: &[u8], pitch_bytes: usize, x: u32, y: u32) -> u32 {
    let Some(idx) = (y as usize)
        .checked_mul(pitch_bytes)
        .and_then(|row| row.checked_add((x as usize).saturating_mul(4)))
    else {
        return 0;
    };
    if idx + 4 > rgba.len() {
        return 0;
    }
    ((rgba[idx] as u32) << 24)
        | ((rgba[idx + 1] as u32) << 16)
        | ((rgba[idx + 2] as u32) << 8)
        | rgba[idx + 3] as u32
}

#[inline]
fn clear_rgba_buffer(rgba: &mut [u8], width: u32, height: u32, rgb: u32) {
    clear_rgba_rect(rgba, width, height, 0, 0, width, height, rgb);
}

fn clear_rgba_rect(
    rgba: &mut [u8],
    target_w: u32,
    target_h: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    rgb: u32,
) {
    let x0 = x.min(target_w) as usize;
    let y0 = y.min(target_h) as usize;
    let x1 = x.saturating_add(width).min(target_w) as usize;
    let y1 = y.saturating_add(height).min(target_h) as usize;
    if x0 >= x1 || y0 >= y1 {
        return;
    }

    let color = [
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
        0xFF,
    ];
    for row in y0..y1 {
        let row_off = row.saturating_mul(target_w as usize).saturating_mul(4);
        for col in x0..x1 {
            let idx = row_off + col.saturating_mul(4);
            if idx + 4 > rgba.len() {
                return;
            }
            rgba[idx..idx + 4].copy_from_slice(&color);
        }
    }
}

#[inline]
fn force_opaque_alpha(rgba: &mut [u8]) {
    let mut idx = 3usize;
    while idx < rgba.len() {
        rgba[idx] = 0xFF;
        idx = idx.saturating_add(4);
    }
}

fn command_label(cmd: &Command) -> &'static str {
    match *cmd {
        Command::ClearColor { .. } => "ClearColor",
        Command::ClearColorRgba { .. } => "ClearColorRgba",
        Command::ClearRect { .. } => "ClearRect",
        Command::BindPipeline(_) => "BindPipeline",
        Command::BindVertexBuffer { .. } => "BindVertexBuffer",
        Command::BindImage(_) => "BindImage",
        Command::SetRenderTarget(_) => "SetRenderTarget",
        Command::SetSampler(_) => "SetSampler",
        Command::SetBlend(_) => "SetBlend",
        Command::SetViewport(_) => "SetViewport",
        Command::SetScissor(_) => "SetScissor",
        Command::Draw { .. } => "Draw",
        Command::Present => "Present",
    }
}

fn render_target_label(target: RenderTarget) -> &'static str {
    match target {
        RenderTarget::Screen => "screen",
        RenderTarget::Image(_) => "image",
    }
}

#[inline]
fn rgba_to_kernel_rgba(r: u8, g: u8, b: u8, a: u8) -> u32 {
    ((a as u32) << 24) | ((b as u32) << 16) | ((g as u32) << 8) | (r as u32)
}

#[inline]
fn rgb_to_kernel_rgba(rgb: u32, alpha: u8) -> u32 {
    rgba_to_kernel_rgba(
        ((rgb >> 16) & 0xFF) as u8,
        ((rgb >> 8) & 0xFF) as u8,
        (rgb & 0xFF) as u8,
        alpha,
    )
}

#[inline]
fn align_up_u64(value: u64, align: u64) -> u64 {
    if align <= 1 {
        value
    } else {
        let rem = value % align;
        if rem == 0 {
            value
        } else {
            value.saturating_add(align - rem)
        }
    }
}

#[inline]
fn align_up_usize(value: usize, align: usize) -> Option<usize> {
    if align <= 1 {
        Some(value)
    } else {
        let rem = value % align;
        if rem == 0 {
            Some(value)
        } else {
            value.checked_add(align - rem)
        }
    }
}

#[inline]
fn clamp01(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
    }
}

#[inline]
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

#[inline]
fn ndc_to_target_x(x: f32, width: u32) -> f32 {
    ((x + 1.0) * 0.5) * width as f32
}

#[inline]
fn ndc_to_target_y(y: f32, height: u32) -> f32 {
    ((1.0 - y) * 0.5) * height as f32
}

fn decode_rgb_fill_quad_shape(
    bytes: &[u8],
    off: usize,
    dst_width: u32,
    dst_height: u32,
) -> Option<RgbFillQuad> {
    let v0 = read_rgb_vertex_f32_bytes(bytes, off)?;
    let v1 = read_rgb_vertex_f32_bytes(bytes, off + trueos_gfx_core::RGB_VERTEX_SIZE)?;
    let v2 = read_rgb_vertex_f32_bytes(bytes, off + 2 * trueos_gfx_core::RGB_VERTEX_SIZE)?;
    let v3 = read_rgb_vertex_f32_bytes(bytes, off + 3 * trueos_gfx_core::RGB_VERTEX_SIZE)?;
    let v4 = read_rgb_vertex_f32_bytes(bytes, off + 4 * trueos_gfx_core::RGB_VERTEX_SIZE)?;
    let v5 = read_rgb_vertex_f32_bytes(bytes, off + 5 * trueos_gfx_core::RGB_VERTEX_SIZE)?;
    let verts = [v0, v1, v2, v3, v4, v5];
    let color_rgba = constant_rgb_vertex_color_rgba(&verts)?;

    let left = ndc_to_target_x(v0.x, dst_width);
    let top = ndc_to_target_y(v0.y, dst_height);
    let right = ndc_to_target_x(v1.x, dst_width);
    let bottom = ndc_to_target_y(v2.y, dst_height);
    if !(left.is_finite()
        && right.is_finite()
        && top.is_finite()
        && bottom.is_finite()
        && left < right
        && top < bottom)
    {
        return None;
    }

    let expected_xy = [
        (left, top),
        (right, top),
        (right, bottom),
        (left, top),
        (right, bottom),
        (left, bottom),
    ];
    for (vertex, (expected_x, expected_y)) in verts.iter().zip(expected_xy) {
        let px = ndc_to_target_x(vertex.x, dst_width);
        let py = ndc_to_target_y(vertex.y, dst_height);
        if !nearly_eq_px(px, expected_x) || !nearly_eq_px(py, expected_y) {
            return None;
        }
    }

    let dst_x = round_i32_if_near(left)?;
    let dst_y = round_i32_if_near(top)?;
    let width = round_u32_if_near(right - left)?;
    let height = round_u32_if_near(bottom - top)?;
    if width == 0 || height == 0 {
        return None;
    }

    Some(RgbFillQuad {
        dst_x,
        dst_y,
        width,
        height,
        color_rgba,
    })
}

fn clip_rgb_fill_quad_to_scissor(
    quad: RgbFillQuad,
    scissor: Option<ScissorRect>,
) -> Option<RgbFillQuad> {
    let Some(scissor) = scissor else {
        return Some(quad);
    };
    if scissor.width == 0 || scissor.height == 0 {
        return None;
    }

    let dst_x0 = quad.dst_x as i64;
    let dst_y0 = quad.dst_y as i64;
    let dst_x1 = dst_x0.saturating_add(quad.width as i64);
    let dst_y1 = dst_y0.saturating_add(quad.height as i64);
    let clip_x0 = scissor.x as i64;
    let clip_y0 = scissor.y as i64;
    let clip_x1 = scissor.x.saturating_add(scissor.width) as i64;
    let clip_y1 = scissor.y.saturating_add(scissor.height) as i64;

    let out_x0 = dst_x0.max(clip_x0);
    let out_y0 = dst_y0.max(clip_y0);
    let out_x1 = dst_x1.min(clip_x1);
    let out_y1 = dst_y1.min(clip_y1);
    if out_x0 >= out_x1 || out_y0 >= out_y1 {
        return None;
    }

    Some(RgbFillQuad {
        dst_x: i32::try_from(out_x0).ok()?,
        dst_y: i32::try_from(out_y0).ok()?,
        width: u32::try_from(out_x1.saturating_sub(out_x0)).ok()?,
        height: u32::try_from(out_y1.saturating_sub(out_y0)).ok()?,
        color_rgba: quad.color_rgba,
    })
}

fn decode_copy_tex_quad(
    bytes: &[u8],
    off: usize,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<TextureCopyQuad> {
    let (quad, verts) =
        decode_tex_quad_shape(bytes, off, src_width, src_height, dst_width, dst_height)?;
    if !verts.iter().all(tex_vertex_is_untinted_white) {
        return None;
    }
    Some(quad)
}

fn decode_alpha_tex_quad(
    bytes: &[u8],
    off: usize,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<TextureCopyQuad> {
    let (quad, verts) =
        decode_tex_quad_shape(bytes, off, src_width, src_height, dst_width, dst_height)?;
    if !verts.iter().all(tex_vertex_is_white_rgb) {
        return None;
    }
    Some(quad)
}

fn decode_mask_tex_quad(
    bytes: &[u8],
    off: usize,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<(TextureCopyQuad, u32)> {
    let (quad, verts) =
        decode_tex_quad_shape(bytes, off, src_width, src_height, dst_width, dst_height)?;
    let color_rgba = constant_tex_vertex_color_rgba(&verts)?;
    Some((quad, color_rgba))
}

fn clip_texture_copy_quad_to_scissor(
    quad: TextureCopyQuad,
    scissor: Option<ScissorRect>,
) -> Option<TextureCopyQuad> {
    let Some(scissor) = scissor else {
        return Some(quad);
    };
    if scissor.width == 0 || scissor.height == 0 {
        return None;
    }

    let dst_x0 = quad.dst_x as i64;
    let dst_y0 = quad.dst_y as i64;
    let dst_x1 = dst_x0.saturating_add(quad.width as i64);
    let dst_y1 = dst_y0.saturating_add(quad.height as i64);
    let clip_x0 = scissor.x as i64;
    let clip_y0 = scissor.y as i64;
    let clip_x1 = scissor.x.saturating_add(scissor.width) as i64;
    let clip_y1 = scissor.y.saturating_add(scissor.height) as i64;

    let out_x0 = dst_x0.max(clip_x0);
    let out_y0 = dst_y0.max(clip_y0);
    let out_x1 = dst_x1.min(clip_x1);
    let out_y1 = dst_y1.min(clip_y1);
    if out_x0 >= out_x1 || out_y0 >= out_y1 {
        return None;
    }

    let trim_left = u32::try_from(out_x0.saturating_sub(dst_x0)).ok()?;
    let trim_top = u32::try_from(out_y0.saturating_sub(dst_y0)).ok()?;
    Some(TextureCopyQuad {
        src_x: quad.src_x.saturating_add(trim_left),
        src_y: quad.src_y.saturating_add(trim_top),
        dst_x: i32::try_from(out_x0).ok()?,
        dst_y: i32::try_from(out_y0).ok()?,
        width: u32::try_from(out_x1.saturating_sub(out_x0)).ok()?,
        height: u32::try_from(out_y1.saturating_sub(out_y0)).ok()?,
    })
}

fn clip_texture_scale_quad_to_scissor(
    quad: TextureScaleQuad,
    scissor: Option<ScissorRect>,
) -> Option<TextureScaleQuad> {
    let Some(scissor) = scissor else {
        return Some(quad);
    };
    if scissor.width == 0 || scissor.height == 0 || quad.dst_w == 0 || quad.dst_h == 0 {
        return None;
    }

    let dst_x0 = quad.dst_x as i64;
    let dst_y0 = quad.dst_y as i64;
    let dst_x1 = dst_x0.saturating_add(quad.dst_w as i64);
    let dst_y1 = dst_y0.saturating_add(quad.dst_h as i64);
    let clip_x0 = scissor.x as i64;
    let clip_y0 = scissor.y as i64;
    let clip_x1 = scissor.x.saturating_add(scissor.width) as i64;
    let clip_y1 = scissor.y.saturating_add(scissor.height) as i64;

    let out_x0 = dst_x0.max(clip_x0);
    let out_y0 = dst_y0.max(clip_y0);
    let out_x1 = dst_x1.min(clip_x1);
    let out_y1 = dst_y1.min(clip_y1);
    if out_x0 >= out_x1 || out_y0 >= out_y1 {
        return None;
    }

    let trim_left = u32::try_from(out_x0.saturating_sub(dst_x0)).ok()?;
    let trim_top = u32::try_from(out_y0.saturating_sub(dst_y0)).ok()?;
    let trim_right = u32::try_from(dst_x1.saturating_sub(out_x1)).ok()?;
    let trim_bottom = u32::try_from(dst_y1.saturating_sub(out_y1)).ok()?;
    let src_x0 = (trim_left as u64)
        .saturating_mul(quad.src_w as u64)
        .checked_div(quad.dst_w as u64)
        .and_then(|v| u32::try_from(v).ok())?;
    let src_y0 = (trim_top as u64)
        .saturating_mul(quad.src_h as u64)
        .checked_div(quad.dst_h as u64)
        .and_then(|v| u32::try_from(v).ok())?;
    let src_x1_trim = (trim_right as u64)
        .saturating_mul(quad.src_w as u64)
        .checked_div(quad.dst_w as u64)
        .and_then(|v| u32::try_from(v).ok())?;
    let src_y1_trim = (trim_bottom as u64)
        .saturating_mul(quad.src_h as u64)
        .checked_div(quad.dst_h as u64)
        .and_then(|v| u32::try_from(v).ok())?;
    let src_w = quad
        .src_w
        .saturating_sub(src_x0)
        .saturating_sub(src_x1_trim);
    let src_h = quad
        .src_h
        .saturating_sub(src_y0)
        .saturating_sub(src_y1_trim);
    if src_w == 0 || src_h == 0 {
        return None;
    }

    Some(TextureScaleQuad {
        src_x: quad.src_x.saturating_add(src_x0),
        src_y: quad.src_y.saturating_add(src_y0),
        src_w,
        src_h,
        dst_x: i32::try_from(out_x0).ok()?,
        dst_y: i32::try_from(out_y0).ok()?,
        dst_w: u32::try_from(out_x1.saturating_sub(out_x0)).ok()?,
        dst_h: u32::try_from(out_y1.saturating_sub(out_y0)).ok()?,
    })
}

fn decode_tex_quad_shape(
    bytes: &[u8],
    off: usize,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<(TextureCopyQuad, [TexVertexF32; 6])> {
    let v0 = read_tex_vertex_f32_bytes(bytes, off)?;
    let v1 = read_tex_vertex_f32_bytes(bytes, off + trueos_gfx_core::TEX_VERTEX_SIZE)?;
    let v2 = read_tex_vertex_f32_bytes(bytes, off + 2 * trueos_gfx_core::TEX_VERTEX_SIZE)?;
    let v3 = read_tex_vertex_f32_bytes(bytes, off + 3 * trueos_gfx_core::TEX_VERTEX_SIZE)?;
    let v4 = read_tex_vertex_f32_bytes(bytes, off + 4 * trueos_gfx_core::TEX_VERTEX_SIZE)?;
    let v5 = read_tex_vertex_f32_bytes(bytes, off + 5 * trueos_gfx_core::TEX_VERTEX_SIZE)?;
    let verts = [v0, v1, v2, v3, v4, v5];

    let left = ndc_to_target_x(v0.x, dst_width);
    let right = ndc_to_target_x(v1.x, dst_width);
    let bottom = ndc_to_target_y(v0.y, dst_height);
    let top = ndc_to_target_y(v2.y, dst_height);
    if !(left.is_finite()
        && right.is_finite()
        && top.is_finite()
        && bottom.is_finite()
        && left < right
        && top < bottom)
    {
        return None;
    }

    let expected_xy = [
        (left, bottom),
        (right, bottom),
        (right, top),
        (left, bottom),
        (right, top),
        (left, top),
    ];
    for (vertex, (expected_x, expected_y)) in verts.iter().zip(expected_xy) {
        let px = ndc_to_target_x(vertex.x, dst_width);
        let py = ndc_to_target_y(vertex.y, dst_height);
        if !nearly_eq_px(px, expected_x) || !nearly_eq_px(py, expected_y) {
            return None;
        }
    }

    let u0 = v0.u;
    let u1 = v1.u;
    let tex_top = v2.v;
    let tex_bottom = v0.v;
    if !(u0.is_finite()
        && u1.is_finite()
        && tex_top.is_finite()
        && tex_bottom.is_finite()
        && u0 >= 0.0
        && tex_top >= 0.0
        && u1 <= 1.0
        && tex_bottom <= 1.0
        && u0 < u1
        && tex_top < tex_bottom)
    {
        return None;
    }
    let expected_uv = [
        (u0, tex_bottom),
        (u1, tex_bottom),
        (u1, tex_top),
        (u0, tex_bottom),
        (u1, tex_top),
        (u0, tex_top),
    ];
    for (vertex, (expected_u, expected_v)) in verts.iter().zip(expected_uv) {
        if !nearly_eq_uv(vertex.u, expected_u) || !nearly_eq_uv(vertex.v, expected_v) {
            return None;
        }
    }

    let dst_x = round_i32_if_near(left)?;
    let dst_y = round_i32_if_near(top)?;
    let dst_w = round_u32_if_near(right - left)?;
    let dst_h = round_u32_if_near(bottom - top)?;
    let src_x = round_u32_if_near(u0 * src_width as f32)?;
    let src_y = round_u32_if_near(tex_top * src_height as f32)?;
    let src_w = round_u32_if_near((u1 - u0) * src_width as f32)?;
    let src_h = round_u32_if_near((tex_bottom - tex_top) * src_height as f32)?;
    if dst_w == 0
        || dst_h == 0
        || dst_w != src_w
        || dst_h != src_h
        || src_x.saturating_add(src_w) > src_width
        || src_y.saturating_add(src_h) > src_height
    {
        return None;
    }

    Some((
        TextureCopyQuad {
            src_x,
            src_y,
            dst_x,
            dst_y,
            width: dst_w,
            height: dst_h,
        },
        verts,
    ))
}

fn decode_tex_quad_bounds(
    bytes: &[u8],
    off: usize,
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> Option<TextureScaleQuad> {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_u = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;

    for lane in 0..6usize {
        let vertex =
            read_tex_vertex_f32_bytes(bytes, off + lane * trueos_gfx_core::TEX_VERTEX_SIZE)?;
        let px = ndc_to_target_x(vertex.x, dst_width);
        let py = ndc_to_target_y(vertex.y, dst_height);
        if !(px.is_finite() && py.is_finite() && vertex.u.is_finite() && vertex.v.is_finite()) {
            return None;
        }
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
        min_u = min_u.min(vertex.u);
        max_u = max_u.max(vertex.u);
        min_v = min_v.min(vertex.v);
        max_v = max_v.max(vertex.v);
    }
    if !(min_x < max_x
        && min_y < max_y
        && min_u >= 0.0
        && min_v >= 0.0
        && max_u <= 1.0
        && max_v <= 1.0
        && min_u < max_u
        && min_v < max_v)
    {
        return None;
    }

    let dst_x = round_i32_if_near(min_x)?;
    let dst_y = round_i32_if_near(min_y)?;
    let dst_w = round_u32_if_near(max_x - min_x)?;
    let dst_h = round_u32_if_near(max_y - min_y)?;
    let src_x = round_u32_if_near(min_u * src_width as f32)?;
    let src_y = round_u32_if_near(min_v * src_height as f32)?;
    let src_w = round_u32_if_near((max_u - min_u) * src_width as f32)?;
    let src_h = round_u32_if_near((max_v - min_v) * src_height as f32)?;
    if dst_w == 0
        || dst_h == 0
        || src_w == 0
        || src_h == 0
        || src_x.saturating_add(src_w) > src_width
        || src_y.saturating_add(src_h) > src_height
    {
        return None;
    }

    Some(TextureScaleQuad {
        src_x,
        src_y,
        src_w,
        src_h,
        dst_x,
        dst_y,
        dst_w,
        dst_h,
    })
}

fn tex_quad_is_full_target_untinted(bytes: &[u8], dst_width: u32, dst_height: u32) -> bool {
    if bytes.len() < 6 * trueos_gfx_core::TEX_VERTEX_SIZE {
        return false;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_u = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;

    for lane in 0..6usize {
        let Some(vertex) =
            read_tex_vertex_f32_bytes(bytes, lane * trueos_gfx_core::TEX_VERTEX_SIZE)
        else {
            return false;
        };
        if !tex_vertex_is_untinted_white(&vertex) {
            return false;
        }
        let px = ndc_to_target_x(vertex.x, dst_width);
        let py = ndc_to_target_y(vertex.y, dst_height);
        if !(px.is_finite() && py.is_finite() && vertex.u.is_finite() && vertex.v.is_finite()) {
            return false;
        }
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
        min_u = min_u.min(vertex.u);
        max_u = max_u.max(vertex.u);
        min_v = min_v.min(vertex.v);
        max_v = max_v.max(vertex.v);
    }

    nearly_eq_px(min_x, 0.0)
        && nearly_eq_px(max_x, dst_width as f32)
        && nearly_eq_px(min_y, 0.0)
        && nearly_eq_px(max_y, dst_height as f32)
        && nearly_eq_uv(min_u, 0.0)
        && nearly_eq_uv(max_u, 1.0)
        && nearly_eq_uv(min_v, 0.0)
        && nearly_eq_uv(max_v, 1.0)
}

fn tex_quad_is_full_target_white_rgb(bytes: &[u8], dst_width: u32, dst_height: u32) -> bool {
    if bytes.len() < 6 * trueos_gfx_core::TEX_VERTEX_SIZE {
        return false;
    }

    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    let mut min_u = f32::INFINITY;
    let mut max_u = f32::NEG_INFINITY;
    let mut min_v = f32::INFINITY;
    let mut max_v = f32::NEG_INFINITY;

    for lane in 0..6usize {
        let Some(vertex) =
            read_tex_vertex_f32_bytes(bytes, lane * trueos_gfx_core::TEX_VERTEX_SIZE)
        else {
            return false;
        };
        if !tex_vertex_is_white_rgb(&vertex) {
            return false;
        }
        let px = ndc_to_target_x(vertex.x, dst_width);
        let py = ndc_to_target_y(vertex.y, dst_height);
        if !(px.is_finite() && py.is_finite() && vertex.u.is_finite() && vertex.v.is_finite()) {
            return false;
        }
        min_x = min_x.min(px);
        max_x = max_x.max(px);
        min_y = min_y.min(py);
        max_y = max_y.max(py);
        min_u = min_u.min(vertex.u);
        max_u = max_u.max(vertex.u);
        min_v = min_v.min(vertex.v);
        max_v = max_v.max(vertex.v);
    }

    nearly_eq_px(min_x, 0.0)
        && nearly_eq_px(max_x, dst_width as f32)
        && nearly_eq_px(min_y, 0.0)
        && nearly_eq_px(max_y, dst_height as f32)
        && nearly_eq_uv(min_u, 0.0)
        && nearly_eq_uv(max_u, 1.0)
        && nearly_eq_uv(min_v, 0.0)
        && nearly_eq_uv(max_v, 1.0)
}

#[inline]
fn sampler_filter_is_texel_center_equivalent(sampler: SamplerDesc) -> bool {
    matches!(sampler.min_filter, SamplerFilter::Nearest | SamplerFilter::Linear)
        && matches!(sampler.mag_filter, SamplerFilter::Nearest | SamplerFilter::Linear)
}

#[inline]
fn tex_vertex_is_untinted_white(vertex: &TexVertexF32) -> bool {
    nearly_eq_unit(vertex.r)
        && nearly_eq_unit(vertex.g)
        && nearly_eq_unit(vertex.b)
        && nearly_eq_unit(vertex.a)
}

#[inline]
fn tex_vertex_is_white_rgb(vertex: &TexVertexF32) -> bool {
    nearly_eq_unit(vertex.r) && nearly_eq_unit(vertex.g) && nearly_eq_unit(vertex.b)
}

fn constant_tex_vertex_color_rgba(verts: &[TexVertexF32; 6]) -> Option<u32> {
    let first = verts[0];
    if !tex_vertex_color_is_finite(first) {
        return None;
    }
    for vertex in verts.iter().skip(1) {
        if !tex_vertex_color_is_finite(*vertex)
            || !nearly_eq_color(vertex.r, first.r)
            || !nearly_eq_color(vertex.g, first.g)
            || !nearly_eq_color(vertex.b, first.b)
            || !nearly_eq_color(vertex.a, first.a)
        {
            return None;
        }
    }
    Some(rgba_to_kernel_rgba(
        unit_float_to_u8(first.r),
        unit_float_to_u8(first.g),
        unit_float_to_u8(first.b),
        unit_float_to_u8(first.a),
    ))
}

fn constant_rgb_vertex_color_rgba(verts: &[trueos_gfx_core::RgbVertexF32; 6]) -> Option<u32> {
    let first = verts[0];
    if !rgb_vertex_color_is_finite(first) {
        return None;
    }
    for vertex in verts.iter().skip(1) {
        if !rgb_vertex_color_is_finite(*vertex)
            || !nearly_eq_color(vertex.r, first.r)
            || !nearly_eq_color(vertex.g, first.g)
            || !nearly_eq_color(vertex.b, first.b)
            || !nearly_eq_color(vertex.a, first.a)
        {
            return None;
        }
    }
    Some(rgba_to_kernel_rgba(
        unit_float_to_u8(first.r),
        unit_float_to_u8(first.g),
        unit_float_to_u8(first.b),
        unit_float_to_u8(first.a),
    ))
}

#[inline]
fn tex_vertex_color_is_finite(vertex: TexVertexF32) -> bool {
    vertex.r.is_finite() && vertex.g.is_finite() && vertex.b.is_finite() && vertex.a.is_finite()
}

#[inline]
fn rgb_vertex_color_is_finite(vertex: trueos_gfx_core::RgbVertexF32) -> bool {
    vertex.r.is_finite() && vertex.g.is_finite() && vertex.b.is_finite() && vertex.a.is_finite()
}

#[inline]
fn nearly_eq_color(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= (1.0 / 255.0)
}

#[inline]
fn unit_float_to_u8(value: f32) -> u8 {
    libm::roundf(clamp01(value) * 255.0) as u8
}

#[inline]
fn nearly_eq_unit(value: f32) -> bool {
    value.is_finite() && (value - 1.0).abs() <= (1.0 / 255.0)
}

#[inline]
fn nearly_eq_px(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 0.05
}

#[inline]
fn nearly_eq_uv(a: f32, b: f32) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() <= 0.000_01
}

#[inline]
fn round_i32_if_near(value: f32) -> Option<i32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = libm::roundf(value);
    ((value - rounded).abs() <= 0.05 && rounded >= i32::MIN as f32 && rounded <= i32::MAX as f32)
        .then_some(rounded as i32)
}

#[inline]
fn round_u32_if_near(value: f32) -> Option<u32> {
    if !value.is_finite() {
        return None;
    }
    let rounded = libm::roundf(value);
    ((value - rounded).abs() <= 0.05 && rounded >= 0.0 && rounded <= u32::MAX as f32)
        .then_some(rounded as u32)
}

#[inline]
fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f32, py: f32) -> f32 {
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

#[inline]
fn pixel_to_rgba(dst: &[u8]) -> [f32; 4] {
    [
        dst[0] as f32 / 255.0,
        dst[1] as f32 / 255.0,
        dst[2] as f32 / 255.0,
        dst[3] as f32 / 255.0,
    ]
}

#[inline]
fn write_rgba_pixel(dst: &mut [u8], src: [f32; 4]) {
    dst[0] = (clamp01(src[0]) * 255.0 + 0.5) as u8;
    dst[1] = (clamp01(src[1]) * 255.0 + 0.5) as u8;
    dst[2] = (clamp01(src[2]) * 255.0 + 0.5) as u8;
    dst[3] = (clamp01(src[3]) * 255.0 + 0.5) as u8;
}

#[inline]
fn blend_factor_rgba(factor: BlendFactor, src: [f32; 4], dst: [f32; 4]) -> [f32; 4] {
    match factor {
        BlendFactor::Zero => [0.0, 0.0, 0.0, 0.0],
        BlendFactor::One => [1.0, 1.0, 1.0, 1.0],
        BlendFactor::SrcAlpha => [src[3], src[3], src[3], src[3]],
        BlendFactor::OneMinusSrcAlpha => {
            let v = 1.0 - src[3];
            [v, v, v, v]
        }
        BlendFactor::DstColor => dst,
        BlendFactor::OneMinusDstColor => [1.0 - dst[0], 1.0 - dst[1], 1.0 - dst[2], 1.0 - dst[3]],
        BlendFactor::OneMinusSrcColor => [1.0 - src[0], 1.0 - src[1], 1.0 - src[2], 1.0 - src[3]],
    }
}

#[inline]
fn blend_pixel(dst: &mut [u8], src: [f32; 4], blend: BlendDesc) {
    if !blend.enabled {
        write_rgba_pixel(dst, src);
        return;
    }

    let dst_rgba = pixel_to_rgba(dst);
    let src_factor = blend_factor_rgba(blend.src, src, dst_rgba);
    let dst_factor = blend_factor_rgba(blend.dst, src, dst_rgba);
    let out = [
        src[0] * src_factor[0] + dst_rgba[0] * dst_factor[0],
        src[1] * src_factor[1] + dst_rgba[1] * dst_factor[1],
        src[2] * src_factor[2] + dst_rgba[2] * dst_factor[2],
        src[3] * src_factor[3] + dst_rgba[3] * dst_factor[3],
    ];
    write_rgba_pixel(dst, out);
}

#[inline]
fn wrap_tex_coord(coord: f32, wrap: SamplerWrap) -> f32 {
    match wrap {
        SamplerWrap::ClampToEdge => clamp01(coord),
        SamplerWrap::Repeat => {
            let wrapped = coord - floorf(coord);
            if wrapped < 0.0 {
                wrapped + 1.0
            } else {
                wrapped
            }
        }
    }
}

#[inline]
fn sample_texel_clamped(rgba: &[u8], width: u32, height: u32, x: i32, y: i32) -> [f32; 4] {
    if width == 0 || height == 0 {
        return [0.0, 0.0, 0.0, 0.0];
    }
    let xi = x.clamp(0, width.saturating_sub(1) as i32) as usize;
    let yi = y.clamp(0, height.saturating_sub(1) as i32) as usize;
    let idx = yi
        .saturating_mul(width as usize)
        .saturating_add(xi)
        .saturating_mul(4);
    if idx + 4 > rgba.len() {
        return [0.0, 0.0, 0.0, 0.0];
    }
    pixel_to_rgba(&rgba[idx..idx + 4])
}

fn sample_texture_rgba(texture: &ImageEntry, sampler: SamplerDesc, u: f32, v: f32) -> [f32; 4] {
    let u = wrap_tex_coord(u, sampler.wrap_s);
    let v = wrap_tex_coord(v, sampler.wrap_t);
    let width_f = texture.width as f32;
    let height_f = texture.height as f32;

    if sampler.min_filter == SamplerFilter::Nearest && sampler.mag_filter == SamplerFilter::Nearest
    {
        let x = floorf((u * width_f).min(width_f - 1.0)) as i32;
        let y = floorf((v * height_f).min(height_f - 1.0)) as i32;
        return sample_texel_clamped(&texture.rgba, texture.width, texture.height, x, y);
    }

    let fx = u * width_f - 0.5;
    let fy = v * height_f - 0.5;
    let x0 = floorf(fx) as i32;
    let y0 = floorf(fy) as i32;
    let x1 = x0 + 1;
    let y1 = y0 + 1;
    let tx = fx - x0 as f32;
    let ty = fy - y0 as f32;
    let c00 = sample_texel_clamped(&texture.rgba, texture.width, texture.height, x0, y0);
    let c10 = sample_texel_clamped(&texture.rgba, texture.width, texture.height, x1, y0);
    let c01 = sample_texel_clamped(&texture.rgba, texture.width, texture.height, x0, y1);
    let c11 = sample_texel_clamped(&texture.rgba, texture.width, texture.height, x1, y1);
    let mut out = [0.0; 4];
    for i in 0..4 {
        let top = lerp(c00[i], c10[i], tx);
        let bottom = lerp(c01[i], c11[i], tx);
        out[i] = lerp(top, bottom, ty);
    }
    out
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SampleKind {
    Mask,
    Rgba,
}

fn draw_rgb_triangle_rgba(
    target: &mut [u8],
    width: u32,
    height: u32,
    scissor: Option<ScissorRect>,
    blend: BlendDesc,
    v0: trueos_gfx_core::RgbVertexF32,
    v1: trueos_gfx_core::RgbVertexF32,
    v2: trueos_gfx_core::RgbVertexF32,
) {
    if width == 0 || height == 0 {
        return;
    }

    let p0 = (ndc_to_target_x(v0.x, width), ndc_to_target_y(v0.y, height));
    let p1 = (ndc_to_target_x(v1.x, width), ndc_to_target_y(v1.y, height));
    let p2 = (ndc_to_target_x(v2.x, width), ndc_to_target_y(v2.y, height));
    let area = edge_fn(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
    if area.abs() <= 1e-6 {
        return;
    }

    let mut min_x = floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
    let mut max_x = ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
    let mut min_y = floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
    let mut max_y = ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;
    if let Some(scissor) = scissor {
        min_x = min_x.max(scissor.x.min(width) as i32);
        max_x = max_x.min(scissor.x.saturating_add(scissor.width).min(width) as i32);
        min_y = min_y.max(scissor.y.min(height) as i32);
        max_y = max_y.min(scissor.y.saturating_add(scissor.height).min(height) as i32);
    }
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, px, py);
            let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, px, py);
            let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, px, py);
            if (area > 0.0 && (w0 < 0.0 || w1 < 0.0 || w2 < 0.0))
                || (area < 0.0 && (w0 > 0.0 || w1 > 0.0 || w2 > 0.0))
            {
                continue;
            }

            let inv_area = 1.0 / area;
            let b0 = w0 * inv_area;
            let b1 = w1 * inv_area;
            let b2 = w2 * inv_area;
            let src = [
                v0.r * b0 + v1.r * b1 + v2.r * b2,
                v0.g * b0 + v1.g * b1 + v2.g * b2,
                v0.b * b0 + v1.b * b1 + v2.b * b2,
                v0.a * b0 + v1.a * b1 + v2.a * b2,
            ];
            let idx = (y as usize)
                .saturating_mul(width as usize)
                .saturating_add(x as usize)
                .saturating_mul(4);
            if idx + 4 <= target.len() {
                blend_pixel(&mut target[idx..idx + 4], src, blend);
            }
        }
    }
}

fn draw_tex_triangle_rgba(
    target: &mut [u8],
    width: u32,
    height: u32,
    scissor: Option<ScissorRect>,
    blend: BlendDesc,
    sampler: SamplerDesc,
    sample_kind: SampleKind,
    texture: &ImageEntry,
    v0: trueos_gfx_core::TexVertexF32,
    v1: trueos_gfx_core::TexVertexF32,
    v2: trueos_gfx_core::TexVertexF32,
) {
    if width == 0 || height == 0 {
        return;
    }

    let p0 = (ndc_to_target_x(v0.x, width), ndc_to_target_y(v0.y, height));
    let p1 = (ndc_to_target_x(v1.x, width), ndc_to_target_y(v1.y, height));
    let p2 = (ndc_to_target_x(v2.x, width), ndc_to_target_y(v2.y, height));
    let area = edge_fn(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
    if area.abs() <= 1e-6 {
        return;
    }

    let mut min_x = floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
    let mut max_x = ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
    let mut min_y = floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
    let mut max_y = ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;
    if let Some(scissor) = scissor {
        min_x = min_x.max(scissor.x.min(width) as i32);
        max_x = max_x.min(scissor.x.saturating_add(scissor.width).min(width) as i32);
        min_y = min_y.max(scissor.y.min(height) as i32);
        max_y = max_y.min(scissor.y.saturating_add(scissor.height).min(height) as i32);
    }
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, px, py);
            let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, px, py);
            let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, px, py);
            if (area > 0.0 && (w0 < 0.0 || w1 < 0.0 || w2 < 0.0))
                || (area < 0.0 && (w0 > 0.0 || w1 > 0.0 || w2 > 0.0))
            {
                continue;
            }

            let inv_area = 1.0 / area;
            let b0 = w0 * inv_area;
            let b1 = w1 * inv_area;
            let b2 = w2 * inv_area;
            let u = v0.u * b0 + v1.u * b1 + v2.u * b2;
            let v = v0.v * b0 + v1.v * b1 + v2.v * b2;
            let vert = [
                v0.r * b0 + v1.r * b1 + v2.r * b2,
                v0.g * b0 + v1.g * b1 + v2.g * b2,
                v0.b * b0 + v1.b * b1 + v2.b * b2,
                v0.a * b0 + v1.a * b1 + v2.a * b2,
            ];
            let tex = sample_texture_rgba(texture, sampler, u, v);
            let mask = if tex[3] < 1.0 { tex[3] } else { tex[0] };
            let src = match sample_kind {
                SampleKind::Mask => [
                    vert[0] * mask,
                    vert[1] * mask,
                    vert[2] * mask,
                    vert[3] * mask,
                ],
                SampleKind::Rgba => [
                    tex[0] * vert[0],
                    tex[1] * vert[1],
                    tex[2] * vert[2],
                    tex[3] * vert[3],
                ],
            };
            let idx = (y as usize)
                .saturating_mul(width as usize)
                .saturating_add(x as usize)
                .saturating_mul(4);
            if idx + 4 <= target.len() {
                blend_pixel(&mut target[idx..idx + 4], src, blend);
            }
        }
    }
}

fn draw_mandelbrot_triangle_rgba(
    target: &mut [u8],
    width: u32,
    height: u32,
    scissor: Option<ScissorRect>,
    blend: BlendDesc,
    pipeline_kind: PipelineKind,
    v0: trueos_gfx_core::TexVertexF32,
    v1: trueos_gfx_core::TexVertexF32,
    v2: trueos_gfx_core::TexVertexF32,
) {
    // SIMD16 pixel dispatch: four 2x2 subspans packed into one 16-bit execution mask.
    const LANE_X: [i32; 16] = [0, 1, 0, 1, 2, 3, 2, 3, 0, 1, 0, 1, 2, 3, 2, 3];
    const LANE_Y: [i32; 16] = [0, 0, 1, 1, 0, 0, 1, 1, 2, 2, 3, 3, 2, 2, 3, 3];

    if width == 0 || height == 0 {
        return;
    }

    let p0 = (ndc_to_target_x(v0.x, width), ndc_to_target_y(v0.y, height));
    let p1 = (ndc_to_target_x(v1.x, width), ndc_to_target_y(v1.y, height));
    let p2 = (ndc_to_target_x(v2.x, width), ndc_to_target_y(v2.y, height));
    let area = edge_fn(p0.0, p0.1, p1.0, p1.1, p2.0, p2.1);
    if area.abs() <= 1e-6 {
        return;
    }

    let mut min_x = floorf(p0.0.min(p1.0).min(p2.0)).max(0.0) as i32;
    let mut max_x = ceilf(p0.0.max(p1.0).max(p2.0)).min(width as f32) as i32;
    let mut min_y = floorf(p0.1.min(p1.1).min(p2.1)).max(0.0) as i32;
    let mut max_y = ceilf(p0.1.max(p1.1).max(p2.1)).min(height as f32) as i32;
    if let Some(scissor) = scissor {
        min_x = min_x.max(scissor.x.min(width) as i32);
        max_x = max_x.min(scissor.x.saturating_add(scissor.width).min(width) as i32);
        min_y = min_y.max(scissor.y.min(height) as i32);
        max_y = max_y.min(scissor.y.saturating_add(scissor.height).min(height) as i32);
    }
    if min_x >= max_x || min_y >= max_y {
        return;
    }

    let inv_area = 1.0 / area;
    let tile_min_x = min_x & !3;
    let tile_min_y = min_y & !3;
    let mut tile_y = tile_min_y;
    while tile_y < max_y {
        let mut tile_x = tile_min_x;
        while tile_x < max_x {
            let mut dispatch_mask = 0u16;
            let mut us = [0.0f32; 16];
            let mut vs = [0.0f32; 16];

            for lane in 0..16 {
                let x = tile_x + LANE_X[lane];
                let y = tile_y + LANE_Y[lane];
                if x < min_x || x >= max_x || y < min_y || y >= max_y {
                    continue;
                }

                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, px, py);
                let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, px, py);
                let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, px, py);
                if (area > 0.0 && (w0 < 0.0 || w1 < 0.0 || w2 < 0.0))
                    || (area < 0.0 && (w0 > 0.0 || w1 > 0.0 || w2 > 0.0))
                {
                    continue;
                }

                let b0 = w0 * inv_area;
                let b1 = w1 * inv_area;
                let b2 = w2 * inv_area;
                us[lane] = v0.u * b0 + v1.u * b1 + v2.u * b2;
                vs[lane] = v0.v * b0 + v1.v * b1 + v2.v * b2;
                dispatch_mask |= 1u16 << lane;
            }

            if dispatch_mask != 0 {
                let colors = match pipeline_kind {
                    PipelineKind::Julia => crate::gfx::mandelbrot::shade_julia_uv_simd16(
                        us,
                        vs,
                        dispatch_mask,
                        crate::gfx::mandelbrot::JULIA_ITERATIONS,
                    ),
                    PipelineKind::BurningShip => {
                        crate::gfx::mandelbrot::shade_burning_ship_uv_simd16(
                            us,
                            vs,
                            dispatch_mask,
                            crate::gfx::mandelbrot::BURNING_SHIP_ITERATIONS,
                        )
                    }
                    _ => crate::gfx::mandelbrot::shade_uv_simd16(
                        us,
                        vs,
                        dispatch_mask,
                        crate::gfx::mandelbrot::MANDELBROT_ITERATIONS,
                    ),
                };
                for lane in 0..16 {
                    if (dispatch_mask & (1u16 << lane)) == 0 {
                        continue;
                    }
                    let x = (tile_x + LANE_X[lane]) as usize;
                    let y = (tile_y + LANE_Y[lane]) as usize;
                    let idx = y
                        .saturating_mul(width as usize)
                        .saturating_add(x)
                        .saturating_mul(4);
                    if idx + 4 <= target.len() {
                        blend_pixel(&mut target[idx..idx + 4], colors[lane], blend);
                    }
                }
            }

            tile_x += 4;
        }
        tile_y += 4;
    }
}
