extern crate alloc;

use alloc::vec::Vec;
use libm::{ceilf, floorf};
use trueos_gfx_core::{
    BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, Command, CommandBuffer, DeviceCaps,
    Error, FenceId, GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, ImageRegion,
    MemoryType, PipelineDesc, PipelineId, Result, SamplerDesc, SamplerFilter, SamplerWrap,
    ScissorRect, ShaderDesc, ShaderId, SwapchainDesc, TexCoordFormat, read_rgb_vertex_f32_bytes,
    read_tex_vertex_f32_bytes,
};

const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;
const TEX_PIPELINE_FS_PARTICLE_TAG_RAW: u32 = 0x5052_5443;
const RCS_PRESENT_RETRY_COOLDOWN_PRESENTS: u32 = 600;
const IMAGE_GPU_VA_BASE: u64 = 0x0400_0000;
const IMAGE_GPU_VA_ALIGN: u64 = 0x0040_0000;
const MAX_BACKEND_IMAGE_DIM: u32 = 8192;
const MAX_BACKEND_IMAGE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
struct BufferEntry {
    bytes: Vec<u8>,
    usage: BufferUsage,
    memory: MemoryType,
}

#[derive(Clone)]
struct ImageEntry {
    width: u32,
    height: u32,
    format: ImageFormat,
    gpu_addr: u64,
    gpu_dirty: bool,
    rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PipelineKind {
    Rgb,
    TexMask,
    TexRgba,
    TexParticle,
    Mandelbrot,
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

pub struct IntelGfxBackend {
    swapchain_desc: SwapchainDesc,
    framebuffer_ptr: *mut u8,
    framebuffer_pitch: usize,
    framebuffer_width: u32,
    framebuffer_height: u32,
    screen_rgba: Vec<u8>,
    screen_rgba_gpu_dirty: bool,
    screen_scanout_rgba: Vec<u8>,
    screen_scanout_gpu_dirty: bool,
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
    pub fn init(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        use ::limine::framebuffer::MemoryModel;

        if !crate::intel::has_claimed_device() {
            return None;
        }

        let fb = framebuffers?.framebuffers().next()?;
        if fb.memory_model() != MemoryModel::RGB || fb.bpp() != 32 || fb.addr().is_null() {
            return None;
        }

        let width = fb.width() as u32;
        let height = fb.height() as u32;
        let pitch = fb.pitch() as usize;
        if width == 0 || height == 0 || pitch < width as usize * 4 {
            return None;
        }

        let swapchain_desc = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: trueos_gfx_core::Extent2D { width, height },
        };
        let screen_len = rgba_len(width, height)?;
        Some(Self {
            swapchain_desc,
            framebuffer_ptr: fb.addr() as *mut u8,
            framebuffer_pitch: pitch,
            framebuffer_width: width,
            framebuffer_height: height,
            screen_rgba: alloc::vec![0; screen_len],
            screen_rgba_gpu_dirty: false,
            screen_scanout_rgba: alloc::vec![0; screen_len],
            screen_scanout_gpu_dirty: false,
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
            self.screen_scanout_rgba.resize(len, 0);
            self.screen_scanout_gpu_dirty = false;
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

    fn rotate_screen_present_buffers(&mut self) {
        core::mem::swap(&mut self.screen_rgba, &mut self.screen_scanout_rgba);
        core::mem::swap(&mut self.screen_rgba_gpu_dirty, &mut self.screen_scanout_gpu_dirty);
        if self.screen_rgba.len() == self.screen_scanout_rgba.len() {
            self.screen_rgba
                .copy_from_slice(self.screen_scanout_rgba.as_slice());
        }
        self.screen_rgba_gpu_dirty = false;
        self.screen_scanout_gpu_dirty = false;
    }

    fn sync_image_rgba_from_gpu(&mut self, id: ImageId) {
        let Some(image) = self.image_mut(id) else {
            return;
        };
        if !image.gpu_dirty || image.rgba.is_empty() {
            return;
        }
        crate::intel::dma_cache_flush_range(image.rgba.as_ptr(), image.rgba.len());
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

    fn image_mut(&mut self, id: ImageId) -> Option<&mut ImageEntry> {
        let idx = id.raw().checked_sub(1)? as usize;
        self.images.get_mut(idx)?.as_mut()
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
                let image = self.image_mut(id).ok_or(Error::NotFound)?;
                Ok(f(image.rgba.as_mut_slice(), image.width, image.height))
            }
        }
    }

    fn present_screen_cpu_fallback(&mut self) -> Result<()> {
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
        self.ensure_screen_rgba()?;
        self.sync_screen_rgba_from_gpu();
        let copy_w = self.swapchain_desc.extent.width.min(self.framebuffer_width) as usize;
        let copy_h = self
            .swapchain_desc
            .extent
            .height
            .min(self.framebuffer_height) as usize;
        self.present_seq = self.present_seq.wrapping_add(1);

        if let Some(surface_gpu_addr) = crate::intel::primary_present_surface_gpu_addr() {
            crate::intel::dma_cache_flush_range(self.screen_rgba.as_ptr(), self.screen_rgba.len());
            let mapped = crate::intel::ggtt_map_screen_rgba_surface(
                self.screen_rgba.as_slice(),
                self.swapchain_desc.extent.width,
                self.swapchain_desc.extent.height,
                surface_gpu_addr,
            );
            if mapped
                && crate::intel::plane_rebind_present_surface(
                    surface_gpu_addr,
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    self.swapchain_desc.extent.width.saturating_mul(4),
                )
            {
                self.rotate_screen_present_buffers();
                if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
                    crate::log!(
                        "intel/gfx-backend: present seq={} mode=plane-rebind-backbuffer size={}x{} gpu=0x{:X}\n",
                        self.present_seq,
                        copy_w,
                        copy_h,
                        surface_gpu_addr
                    );
                }
                return Ok(());
            }
        }

        let guc_ready = crate::intel::guc_ready();
        let allow_rcs_retry = guc_ready && self.present_seq >= self.rcs_retry_after_present_seq;
        if allow_rcs_retry {
            if crate::intel::rcs_present_rgba_frame(self.screen_rgba.as_slice(), copy_w, copy_h) {
                self.rcs_retry_after_present_seq = 0;
                self.rcs_present_failures = 0;
                if self.present_seq <= 8 || self.present_seq.is_multiple_of(120) {
                    crate::log!(
                        "intel/gfx-backend: present seq={} mode=rcs-execlist-store size={}x{}\n",
                        self.present_seq,
                        copy_w,
                        copy_h
                    );
                }
                return Ok(());
            }

            self.rcs_present_failures = self.rcs_present_failures.saturating_add(1);
            self.rcs_retry_after_present_seq = self
                .present_seq
                .saturating_add(RCS_PRESENT_RETRY_COOLDOWN_PRESENTS);
            crate::log!(
                "intel/gfx-backend: present seq={} rcs-present-failed failures={} cooldown_until_seq={} size={}x{}\n",
                self.present_seq,
                self.rcs_present_failures,
                self.rcs_retry_after_present_seq,
                copy_w,
                copy_h
            );
        }

        if !guc_ready && (self.present_seq <= 8 || self.present_seq.is_multiple_of(120)) {
            let guc_status = crate::intel::warm_state()
                .map(crate::intel::guc_status)
                .unwrap_or(0);
            crate::log!(
                "intel/gfx-backend: present seq={} waiting-for-guc status=0x{:08X} size={}x{}\n",
                self.present_seq,
                guc_status,
                copy_w,
                copy_h
            );
        }

        let fallback = self.present_screen_cpu_fallback();
        if fallback.is_ok() && (self.present_seq <= 8 || self.present_seq.is_multiple_of(120)) {
            crate::log!(
                "intel/gfx-backend: present seq={} fallback=cpu-scanout guc_ready={} rcs_retry_ready={} size={}x{}\n",
                self.present_seq,
                guc_ready as u8,
                allow_rcs_retry as u8,
                copy_w,
                copy_h
            );
        }
        fallback
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
                        crate::intel::dma_cache_flush_range(image.rgba.as_ptr(), image.rgba.len());
                        force_opaque_alpha(image.rgba.as_mut_slice());
                        image.gpu_dirty = false;
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
        if matches!(target, RenderTarget::Screen) {
            self.sync_screen_rgba_from_gpu();
        }
        self.sync_image_rgba_from_gpu(source);
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        let source = self.image(source).ok_or(Error::NotFound)?.clone();
        let start = byte_offset as usize + first_vertex as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        let need = vertex_count as usize * trueos_gfx_core::TEX_VERTEX_SIZE;
        if start > buffer.bytes.len() || start.saturating_add(need) > buffer.bytes.len() {
            return Err(Error::Invalid);
        }
        let verts = buffer.bytes[start..start + need].to_vec();
        let sample_kind = match pipeline_kind {
            PipelineKind::TexMask => SampleKind::Mask,
            PipelineKind::TexRgba | PipelineKind::TexParticle => SampleKind::Rgba,
            PipelineKind::Mandelbrot => return Err(Error::Unsupported),
            PipelineKind::Rgb => return Err(Error::Invalid),
        };
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
            if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) {
                crate::log!(
                    "intel/gfx-backend: draw-tex mode=rcs-store triangles={} size={}x{} gpu=0x{:X}\n",
                    verts.len() / (3 * trueos_gfx_core::TEX_VERTEX_SIZE),
                    self.swapchain_desc.extent.width,
                    self.swapchain_desc.extent.height,
                    screen_surface_gpu
                );
            }
            return Ok(());
        }
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
        self.next_image_gpu_addr = align_up_u64(
            gpu_addr
                .saturating_add(alloc_span)
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
        self.ensure_screen_rgba()?;

        let mut target = RenderTarget::Screen;
        let mut scissor: Option<ScissorRect> = None;
        let mut blend = BlendDesc::disabled();
        let mut sampler = SamplerDesc::default_2d();
        let mut pipeline = PipelineKind::Rgb;
        let mut bound_buffer = BufferId::invalid();
        let mut bound_offset = 0u64;
        let mut bound_image = ImageId::invalid();
        let mut unsupported = 0u32;

        for cmd in cmds.commands {
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
                        RenderTarget::Image(id) => self.image(id).and_then(|image| {
                            crate::intel::rcs_clear_rgba_surface(
                                image.rgba.as_slice(),
                                image.width,
                                image.height,
                                image.gpu_addr,
                                rgb,
                            )
                            .then_some((
                                image.width,
                                image.height,
                                image.gpu_addr,
                            ))
                        }),
                    };
                    if let Some((target_w, target_h, target_gpu_addr)) = fast_ok {
                        match target {
                            RenderTarget::Screen => self.screen_rgba_gpu_dirty = true,
                            RenderTarget::Image(id) => {
                                if let Some(image) = self.image_mut(id) {
                                    crate::intel::dma_cache_flush_range(
                                        image.rgba.as_ptr(),
                                        image.rgba.len(),
                                    );
                                    force_opaque_alpha(image.rgba.as_mut_slice());
                                    image.gpu_dirty = false;
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
                Command::ClearRect {
                    rgb,
                    x,
                    y,
                    width,
                    height,
                } => {
                    if matches!(target, RenderTarget::Screen) {
                        self.sync_screen_rgba_from_gpu();
                    }
                    self.with_target_mut(target, |rgba, target_w, target_h| {
                        clear_rgba_rect(rgba, target_w, target_h, x, y, width, height, rgb);
                    })?;
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
                        let draw_res = if bound_image.is_valid() || pipeline != PipelineKind::Rgb {
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
                    if !matches!(target, RenderTarget::Screen) || self.present_screen().is_err() {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
            }
        }

        self.submit_seq = self.submit_seq.wrapping_add(1);
        if self.submit_seq <= 8 || self.submit_seq.is_multiple_of(120) || unsupported != 0 {
            crate::log!(
                "intel/gfx-backend: submit seq={} cmds={} unsupported={} target={}\n",
                self.submit_seq,
                cmds.commands.len(),
                unsupported,
                match target {
                    RenderTarget::Screen => "screen",
                    RenderTarget::Image(_) => "image",
                }
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
    let max_x = texture.width.saturating_sub(1) as f32;
    let max_y = texture.height.saturating_sub(1) as f32;

    if sampler.min_filter == SamplerFilter::Nearest && sampler.mag_filter == SamplerFilter::Nearest
    {
        let x = floorf(u * max_x + 0.5) as i32;
        let y = floorf(v * max_y + 0.5) as i32;
        return sample_texel_clamped(&texture.rgba, texture.width, texture.height, x, y);
    }

    let fx = u * max_x;
    let fy = v * max_y;
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

#[derive(Clone, Copy)]
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
            let mask = if tex[3] > 0.0 { tex[3] } else { tex[0] };
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
