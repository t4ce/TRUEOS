extern crate alloc;

use alloc::vec::Vec;
use trueos_gfx_core::{
    BlendDesc, BlendFactor, BufferDesc, BufferId, BufferUsage, ColorFormat, Command, CommandBuffer,
    DeviceCaps, Error, Extent2D, FenceId, GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId,
    ImageRegion, MapMode, MappedRange, MemoryType, PipelineDesc, PipelineId, Result, RgbVertex,
    Rgba8, SamplerDesc, SamplerFilter, SamplerWrap, ScissorRect, ShaderDesc, ShaderId,
    SwapchainDesc, TexCoordFormat, TexVertex, VertexLayout, push_rgb_vertex_bytes,
    push_tex_vertex_bytes,
};

const TEX_PIPELINE_FS_MASK_TAG_RAW: u32 = 0x4D41_534B;
const TEX_PIPELINE_FS_RGBA_TAG_RAW: u32 = 0x5247_4241;
const TEX_PIPELINE_FS_PARTICLE_TAG_RAW: u32 = 0x5052_5443;
const BEGIN_FLAG_PRESERVE_CONTENTS: u32 = 1;
const BEGIN_FLAG_ALLOW_SCREEN_PRESENT: u32 = 2;
const END_FLAG_ALLOW_SCREEN_PRESENT: u32 = 1;
const END_FLAG_PRESERVE_CONTENTS: u32 = 2;
const MAX_BACKEND_IMAGE_DIM: u32 = 8192;
const MAX_BACKEND_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const MAX_RDP_DRAW_BYTES: usize = 64 * 1024;

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
    rgba: Vec<u8>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum RenderTarget {
    Screen,
    Image(ImageId),
}

#[derive(Clone, Copy)]
struct FrameState {
    seq: u32,
    target: RenderTarget,
    rgb_draws: u32,
    tex_draws: u32,
    draw_bytes: usize,
    preserve_contents: bool,
}

pub struct RdpGfxBackend {
    swapchain_desc: SwapchainDesc,
    buffers: Vec<Option<BufferEntry>>,
    images: Vec<Option<ImageEntry>>,
    pipelines: Vec<Option<PipelineEntry>>,
    next_shader_raw: u32,
    next_fence_raw: u64,
    next_frame_seq: u32,
    submit_seq: u32,
    frame: Option<FrameState>,
    target: RenderTarget,
    scissor: Option<ScissorRect>,
    blend: BlendDesc,
    sampler: SamplerDesc,
}

impl RdpGfxBackend {
    pub fn init(framebuffers: Option<&'static crate::limine::FramebufferResponse>) -> Self {
        let (width, height) = crate::intel::active_scanout_dimensions()
            .or_else(|| {
                framebuffers
                    .and_then(|resp| resp.framebuffers().first().copied())
                    .map(|fb| (fb.width as u32, fb.height as u32))
            })
            .unwrap_or((1280, 800));
        Self::new(width, height)
    }

    pub fn new(width: u32, height: u32) -> Self {
        Self {
            swapchain_desc: SwapchainDesc {
                format: ImageFormat::Rgbx8888,
                extent: Extent2D {
                    width: width.max(1),
                    height: height.max(1),
                },
            },
            buffers: Vec::new(),
            images: Vec::new(),
            pipelines: Vec::new(),
            next_shader_raw: 1,
            next_fence_raw: 1,
            next_frame_seq: 1,
            submit_seq: 0,
            frame: None,
            target: RenderTarget::Screen,
            scissor: None,
            blend: BlendDesc::disabled(),
            sampler: SamplerDesc::default_2d(),
        }
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

    fn begin_frame(&mut self, clear_rgb: u32, preserve_contents: bool) -> u32 {
        if let Some(frame) = self.frame {
            return frame.seq;
        }

        let seq = self.next_frame_seq;
        self.next_frame_seq = self.next_frame_seq.wrapping_add(1).max(1);
        let mut flags = 0u32;
        if preserve_contents {
            flags |= BEGIN_FLAG_PRESERVE_CONTENTS;
        }
        if matches!(self.target, RenderTarget::Screen) {
            flags |= BEGIN_FLAG_ALLOW_SCREEN_PRESENT;
        }
        crate::r::rdp::publish_begin_frame(seq, flags, clear_rgb & 0x00FF_FFFF);
        self.frame = Some(FrameState {
            seq,
            target: self.target,
            rgb_draws: 0,
            tex_draws: 0,
            draw_bytes: 0,
            preserve_contents,
        });
        self.publish_frame_state(seq);
        seq
    }

    fn publish_frame_state(&self, seq: u32) {
        match self.target {
            RenderTarget::Screen => crate::r::rdp::publish_clear_render_target(seq),
            RenderTarget::Image(id) => crate::r::rdp::publish_set_render_target(seq, id.raw()),
        }
        if let Some(scissor) = self.scissor {
            crate::r::rdp::publish_set_scissor(
                seq,
                scissor.x,
                scissor.y,
                scissor.width,
                scissor.height,
            );
        } else {
            crate::r::rdp::publish_clear_scissor(seq);
        }
        publish_blend(seq, self.blend);
        publish_sampler(seq, self.sampler);
    }

    fn note_target(&mut self, target: RenderTarget) {
        self.target = target;
        if let Some(frame) = self.frame.as_mut() {
            frame.target = target;
            match target {
                RenderTarget::Screen => crate::r::rdp::publish_clear_render_target(frame.seq),
                RenderTarget::Image(id) => {
                    crate::r::rdp::publish_set_render_target(frame.seq, id.raw())
                }
            }
        }
    }

    fn clear_rect(&mut self, rgb: u32, x: u32, y: u32, width: u32, height: u32) {
        let seq = self.begin_frame(0, true);
        crate::r::rdp::publish_clear_rect(seq, rgb, x, y, width, height);
    }

    fn end_frame(&mut self) {
        let Some(frame) = self.frame.take() else {
            return;
        };
        let mut flags = 0u32;
        if matches!(frame.target, RenderTarget::Screen) {
            flags |= END_FLAG_ALLOW_SCREEN_PRESENT;
        }
        if frame.preserve_contents {
            flags |= END_FLAG_PRESERVE_CONTENTS;
        }
        crate::r::rdp::publish_end_frame(
            frame.seq,
            flags,
            frame.rgb_draws,
            frame.tex_draws,
            frame.draw_bytes.min(u32::MAX as usize) as u32,
        );
    }

    fn publish_image(&self, id: ImageId, region: Option<ImageRegion>, data: &[u8]) {
        let Some(image) = self.image(id) else {
            return;
        };
        let expected = match region {
            Some(region) => rgba_len(region.width, region.height),
            None => rgba_len(image.width, image.height),
        }
        .unwrap_or(0);
        if expected == 0 || data.len() < expected {
            return;
        }
        crate::r::rdp::publish_texture_rgba(
            id.raw(),
            image.width,
            image.height,
            1,
            region.map(|r| (r.x, r.y, r.width, r.height)),
            &data[..expected],
        );
    }

    fn draw_rgb(
        &mut self,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        layout: VertexLayout,
    ) -> Result<()> {
        let verts =
            self.pack_rgb_vertices(buffer, byte_offset, vertex_count, first_vertex, layout)?;
        if verts.is_empty() {
            return Ok(());
        }
        let seq = self.begin_frame(0, true);
        publish_blend(seq, self.blend);
        self.publish_draw_chunks_rgb(seq, verts.as_slice());
        Ok(())
    }

    fn draw_tex(
        &mut self,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        source: ImageId,
        pipeline_id: PipelineId,
        entry: &PipelineEntry,
    ) -> Result<()> {
        if !matches!(
            entry.kind,
            PipelineKind::Mandelbrot | PipelineKind::Julia | PipelineKind::BurningShip
        ) && self.image(source).is_none()
        {
            return Err(Error::NotFound);
        }
        let verts = self.pack_tex_vertices(
            buffer,
            byte_offset,
            vertex_count,
            first_vertex,
            entry.desc.vertex_layout,
        )?;
        if verts.is_empty() {
            return Ok(());
        }
        let seq = self.begin_frame(0, true);
        publish_blend(seq, self.blend);
        publish_sampler(seq, self.sampler);
        if matches!(
            entry.kind,
            PipelineKind::Mandelbrot | PipelineKind::Julia | PipelineKind::BurningShip
        ) {
            self.publish_draw_chunks_pipeline(seq, pipeline_id.raw(), verts.as_slice());
        } else {
            self.publish_draw_chunks_tex(seq, source.raw(), entry.kind, verts.as_slice());
        }
        Ok(())
    }

    fn publish_draw_chunks_rgb(&mut self, seq: u32, verts: &[u8]) {
        let mut off = 0usize;
        while off < verts.len() {
            let rem = verts.len() - off;
            let chunk = core::cmp::min(MAX_RDP_DRAW_BYTES, rem);
            let tri_size = 3 * trueos_gfx_core::RGB_VERTEX_SIZE;
            let chunk = chunk - (chunk % tri_size);
            if chunk == 0 {
                break;
            }
            let vcount = (chunk / trueos_gfx_core::RGB_VERTEX_SIZE) as u32;
            crate::r::rdp::publish_draw_rgb_triangles(seq, vcount, &verts[off..off + chunk]);
            if let Some(frame) = self.frame.as_mut() {
                frame.rgb_draws = frame.rgb_draws.saturating_add(1);
                frame.draw_bytes = frame.draw_bytes.saturating_add(chunk);
            }
            off += chunk;
        }
    }

    fn publish_draw_chunks_tex(&mut self, seq: u32, tex_id: u32, kind: PipelineKind, verts: &[u8]) {
        let sampler_flags = sampler_flags(self.sampler);
        let sample_kind = match kind {
            PipelineKind::TexMask => 0,
            PipelineKind::TexRgba | PipelineKind::TexParticle => 1,
            PipelineKind::Rgb
            | PipelineKind::Mandelbrot
            | PipelineKind::Julia
            | PipelineKind::BurningShip => 0,
        };
        let mut off = 0usize;
        while off < verts.len() {
            let rem = verts.len() - off;
            let chunk = core::cmp::min(MAX_RDP_DRAW_BYTES, rem);
            let tri_size = 3 * trueos_gfx_core::TEX_VERTEX_SIZE;
            let chunk = chunk - (chunk % tri_size);
            if chunk == 0 {
                break;
            }
            let vcount = (chunk / trueos_gfx_core::TEX_VERTEX_SIZE) as u32;
            crate::r::rdp::publish_draw_tex_triangles(
                seq,
                tex_id,
                vcount,
                sampler_flags,
                sample_kind,
                &verts[off..off + chunk],
            );
            if let Some(frame) = self.frame.as_mut() {
                frame.tex_draws = frame.tex_draws.saturating_add(1);
                frame.draw_bytes = frame.draw_bytes.saturating_add(chunk);
            }
            off += chunk;
        }
    }

    fn publish_draw_chunks_pipeline(&mut self, seq: u32, pipeline_id: u32, verts: &[u8]) {
        self.publish_shader_pipeline(pipeline_id);
        let mut off = 0usize;
        while off < verts.len() {
            let rem = verts.len() - off;
            let chunk = core::cmp::min(MAX_RDP_DRAW_BYTES, rem);
            let tri_size = 3 * trueos_gfx_core::TEX_VERTEX_SIZE;
            let chunk = chunk - (chunk % tri_size);
            if chunk == 0 {
                break;
            }
            let vcount = (chunk / trueos_gfx_core::TEX_VERTEX_SIZE) as u32;
            crate::r::rdp::publish_draw_pipeline_triangles(
                seq,
                pipeline_id,
                vcount,
                &verts[off..off + chunk],
            );
            if let Some(frame) = self.frame.as_mut() {
                frame.tex_draws = frame.tex_draws.saturating_add(1);
                frame.draw_bytes = frame.draw_bytes.saturating_add(chunk);
            }
            off += chunk;
        }
    }

    fn publish_shader_pipeline(&self, pipeline_id: u32) {
        const SHADER_STAGE_FRAGMENT: u32 = 1;
        const SHADER_FORMAT_WGSL: u32 = 1;
        const COLOR_FORMAT_RGBA_U8: u32 = 1;
        const TEXCOORD_FORMAT_UV_F32: u32 = 1;
        let Some(entry) = pipeline_id
            .checked_sub(1)
            .and_then(|idx| self.pipelines.get(idx as usize))
            .and_then(|entry| entry.as_ref())
        else {
            return;
        };
        let (fs_shader_id, source) = match entry.kind {
            PipelineKind::Mandelbrot => (
                crate::gfx::mandelbrot::MANDELBROT_PIPELINE_FS_TAG_RAW,
                crate::gfx::mandelbrot::MANDELBROT_WGSL_FRAGMENT.as_bytes(),
            ),
            PipelineKind::Julia => (
                crate::gfx::mandelbrot::JULIA_PIPELINE_FS_TAG_RAW,
                crate::gfx::mandelbrot::JULIA_WGSL_FRAGMENT.as_bytes(),
            ),
            PipelineKind::BurningShip => (
                crate::gfx::mandelbrot::BURNING_SHIP_PIPELINE_FS_TAG_RAW,
                crate::gfx::mandelbrot::BURNING_SHIP_WGSL_FRAGMENT.as_bytes(),
            ),
            _ => return,
        };
        crate::r::rdp::publish_shader_create(
            fs_shader_id,
            SHADER_STAGE_FRAGMENT,
            SHADER_FORMAT_WGSL,
            0,
            source,
        );
        crate::r::rdp::publish_pipeline_create(
            pipeline_id,
            20,
            0,
            16,
            COLOR_FORMAT_RGBA_U8,
            8,
            TEXCOORD_FORMAT_UV_F32,
            0,
            fs_shader_id,
        );
    }

    fn pack_rgb_vertices(
        &self,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        layout: VertexLayout,
    ) -> Result<Vec<u8>> {
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        if buffer.usage != BufferUsage::Vertex {
            return Err(Error::Invalid);
        }
        let stride = usize::from(layout.stride);
        if stride == 0 {
            return Err(Error::Invalid);
        }
        let start = vertex_start(byte_offset, first_vertex, stride)?;
        let cap = (vertex_count as usize)
            .checked_mul(trueos_gfx_core::RGB_VERTEX_SIZE)
            .ok_or(Error::Invalid)?;
        let mut out = Vec::with_capacity(cap);
        for i in 0..vertex_count as usize {
            let base = start.saturating_add(i.saturating_mul(stride));
            let x =
                read_f32(buffer.bytes.as_slice(), base.saturating_add(layout.pos_offset as usize))
                    .ok_or(Error::Invalid)?;
            let y = read_f32(
                buffer.bytes.as_slice(),
                base.saturating_add(layout.pos_offset as usize)
                    .saturating_add(4),
            )
            .ok_or(Error::Invalid)?;
            let color = read_color(buffer.bytes.as_slice(), base, layout).ok_or(Error::Invalid)?;
            push_rgb_vertex_bytes(&mut out, RgbVertex { x, y, color });
        }
        Ok(out)
    }

    fn pack_tex_vertices(
        &self,
        buffer: BufferId,
        byte_offset: u64,
        vertex_count: u32,
        first_vertex: u32,
        layout: VertexLayout,
    ) -> Result<Vec<u8>> {
        let buffer = self.buffer(buffer).ok_or(Error::NotFound)?;
        if buffer.usage != BufferUsage::Vertex {
            return Err(Error::Invalid);
        }
        if layout.texcoord_format != TexCoordFormat::UvF32 {
            return Err(Error::Invalid);
        }
        let stride = usize::from(layout.stride);
        if stride == 0 {
            return Err(Error::Invalid);
        }
        let start = vertex_start(byte_offset, first_vertex, stride)?;
        let cap = (vertex_count as usize)
            .checked_mul(trueos_gfx_core::TEX_VERTEX_SIZE)
            .ok_or(Error::Invalid)?;
        let mut out = Vec::with_capacity(cap);
        for i in 0..vertex_count as usize {
            let base = start.saturating_add(i.saturating_mul(stride));
            let pos = base.saturating_add(layout.pos_offset as usize);
            let tex = base.saturating_add(layout.texcoord_offset as usize);
            let x = read_f32(buffer.bytes.as_slice(), pos).ok_or(Error::Invalid)?;
            let y =
                read_f32(buffer.bytes.as_slice(), pos.saturating_add(4)).ok_or(Error::Invalid)?;
            let u = read_f32(buffer.bytes.as_slice(), tex).ok_or(Error::Invalid)?;
            let v =
                read_f32(buffer.bytes.as_slice(), tex.saturating_add(4)).ok_or(Error::Invalid)?;
            let color = read_color(buffer.bytes.as_slice(), base, layout).ok_or(Error::Invalid)?;
            push_tex_vertex_bytes(&mut out, TexVertex { x, y, u, v, color });
        }
        Ok(out)
    }
}

impl GfxDevice for RdpGfxBackend {
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
        if desc.width == 0
            || desc.height == 0
            || desc.width > MAX_BACKEND_IMAGE_DIM
            || desc.height > MAX_BACKEND_IMAGE_DIM
            || len > MAX_BACKEND_IMAGE_BYTES
        {
            return Err(Error::Invalid);
        }
        let id = Self::alloc_slot(
            &mut self.images,
            ImageEntry {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                rgba: alloc::vec![0; len],
            },
        );
        let image_id = ImageId::from_raw(id);
        if let Some(image) = self.image(image_id) {
            self.publish_image(image_id, None, image.rgba.as_slice());
        }
        Ok(image_id)
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
        let rgba = image.rgba.clone();
        self.publish_image(id, None, rgba.as_slice());
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
        let mut upload = Vec::with_capacity(need);
        for row in 0..region.height as usize {
            let dst_off = ((region.y as usize + row)
                .saturating_mul(image.width as usize)
                .saturating_add(region.x as usize))
            .saturating_mul(4);
            let row_len = region.width as usize * 4;
            upload.extend_from_slice(&image.rgba[dst_off..dst_off + row_len]);
        }
        self.publish_image(id, Some(region), upload.as_slice());
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

    fn map_buffer(&mut self, id: BufferId, _mode: MapMode) -> Result<MappedRange> {
        let buffer = self.buffer_mut(id).ok_or(Error::NotFound)?;
        if buffer.memory != MemoryType::HostVisible {
            return Err(Error::Unsupported);
        }
        Ok(MappedRange {
            ptr: buffer.bytes.as_mut_ptr(),
            len: buffer.bytes.len(),
        })
    }

    fn unmap_buffer(&mut self, _id: BufferId) -> Result<()> {
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        let mut pipeline = PipelineId::invalid();
        let mut bound_buffer = BufferId::invalid();
        let mut bound_offset = 0u64;
        let mut bound_image = ImageId::invalid();
        let mut unsupported = 0u32;

        for cmd in cmds.commands {
            match *cmd {
                Command::ClearColor { rgb } => {
                    if matches!(self.target, RenderTarget::Screen) {
                        self.begin_frame(rgb, false);
                    } else {
                        let (width, height) = match self.target {
                            RenderTarget::Screen => (
                                self.swapchain_desc.extent.width,
                                self.swapchain_desc.extent.height,
                            ),
                            RenderTarget::Image(id) => self
                                .image(id)
                                .map(|image| (image.width, image.height))
                                .unwrap_or((0, 0)),
                        };
                        self.clear_rect(rgb, 0, 0, width, height);
                    }
                }
                Command::ClearRect {
                    rgb,
                    x,
                    y,
                    width,
                    height,
                } => self.clear_rect(rgb, x, y, width, height),
                Command::BindPipeline(id) => {
                    if self.pipeline(id).is_some() {
                        pipeline = id;
                    } else {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
                Command::BindVertexBuffer { buffer, offset } => {
                    bound_buffer = buffer;
                    bound_offset = offset;
                }
                Command::BindImage(image) => bound_image = image,
                Command::SetRenderTarget(render_target) => match render_target {
                    Some(image) if self.image(image).is_some() => {
                        self.note_target(RenderTarget::Image(image))
                    }
                    Some(_) => unsupported = unsupported.saturating_add(1),
                    None => self.note_target(RenderTarget::Screen),
                },
                Command::SetSampler(next) => {
                    self.sampler = next;
                    if let Some(frame) = self.frame {
                        publish_sampler(frame.seq, next);
                    }
                }
                Command::SetBlend(next) => {
                    self.blend = next;
                    if let Some(frame) = self.frame {
                        publish_blend(frame.seq, next);
                    }
                }
                Command::SetViewport(_viewport) => {}
                Command::SetScissor(next) => {
                    self.scissor = next;
                    if let Some(frame) = self.frame {
                        if let Some(scissor) = next {
                            crate::r::rdp::publish_set_scissor(
                                frame.seq,
                                scissor.x,
                                scissor.y,
                                scissor.width,
                                scissor.height,
                            );
                        } else {
                            crate::r::rdp::publish_clear_scissor(frame.seq);
                        }
                    }
                }
                Command::Draw {
                    vertex_count,
                    first_vertex,
                } => {
                    let draw_res = self
                        .pipeline(pipeline)
                        .cloned()
                        .ok_or(Error::NotFound)
                        .and_then(|entry| {
                            if entry.kind == PipelineKind::Rgb {
                                self.draw_rgb(
                                    bound_buffer,
                                    bound_offset,
                                    vertex_count,
                                    first_vertex,
                                    entry.desc.vertex_layout,
                                )
                            } else {
                                self.draw_tex(
                                    bound_buffer,
                                    bound_offset,
                                    vertex_count,
                                    first_vertex,
                                    bound_image,
                                    pipeline,
                                    &entry,
                                )
                            }
                        });
                    if draw_res.is_err() {
                        unsupported = unsupported.saturating_add(1);
                    }
                }
                Command::Present => self.end_frame(),
            }
        }

        self.submit_seq = self.submit_seq.wrapping_add(1);
        if unsupported != 0 {
            crate::log!(
                "rdp/gfx-backend: submit seq={} cmds={} unsupported={}\n",
                self.submit_seq,
                cmds.commands.len(),
                unsupported
            );
        }

        let fence = FenceId::from_raw(self.next_fence_raw);
        self.next_fence_raw = self.next_fence_raw.wrapping_add(1).max(1);
        Ok(fence)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        fence.is_valid()
    }

    fn device_idle(&mut self) {
        self.end_frame();
    }
}

impl GfxPresent for RdpGfxBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        if desc.extent.width == 0 || desc.extent.height == 0 {
            return Err(Error::Invalid);
        }
        self.swapchain_desc = desc;
        Ok(())
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
fn read_f32(bytes: &[u8], off: usize) -> Option<f32> {
    Some(f32::from_le_bytes([
        *bytes.get(off)?,
        *bytes.get(off + 1)?,
        *bytes.get(off + 2)?,
        *bytes.get(off + 3)?,
    ]))
}

#[inline]
fn vertex_start(byte_offset: u64, first_vertex: u32, stride: usize) -> Result<usize> {
    usize::try_from(byte_offset)
        .map_err(|_| Error::Invalid)?
        .checked_add(
            (first_vertex as usize)
                .checked_mul(stride)
                .ok_or(Error::Invalid)?,
        )
        .ok_or(Error::Invalid)
}

#[inline]
fn read_color(bytes: &[u8], base: usize, layout: VertexLayout) -> Option<Rgba8> {
    let off = base.checked_add(layout.color_offset as usize)?;
    match layout.color_format {
        ColorFormat::RgbU8 => {
            Some(Rgba8::new(*bytes.get(off)?, *bytes.get(off + 1)?, *bytes.get(off + 2)?, 255))
        }
        ColorFormat::RgbaU8 => Some(Rgba8::new(
            *bytes.get(off)?,
            *bytes.get(off + 1)?,
            *bytes.get(off + 2)?,
            *bytes.get(off + 3)?,
        )),
    }
}

#[inline]
fn force_opaque_alpha(rgba: &mut [u8]) {
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
}

#[inline]
fn sampler_flags(sampler: SamplerDesc) -> u32 {
    ((sampler.wrap_s as u32) & 0xFF)
        | (((sampler.wrap_t as u32) & 0xFF) << 8)
        | (((sampler.min_filter as u32) & 0xFF) << 16)
        | (((sampler.mag_filter as u32) & 0xFF) << 24)
}

#[inline]
fn publish_sampler(seq: u32, sampler: SamplerDesc) {
    crate::r::rdp::publish_set_sampler(
        seq,
        sampler_wrap_raw(sampler.wrap_s),
        sampler_wrap_raw(sampler.wrap_t),
        sampler_filter_raw(sampler.min_filter),
        sampler_filter_raw(sampler.mag_filter),
    );
}

#[inline]
fn sampler_wrap_raw(wrap: SamplerWrap) -> u32 {
    match wrap {
        SamplerWrap::ClampToEdge => 0,
        SamplerWrap::Repeat => 1,
    }
}

#[inline]
fn sampler_filter_raw(filter: SamplerFilter) -> u32 {
    match filter {
        SamplerFilter::Nearest => 0,
        SamplerFilter::Linear => 1,
    }
}

#[inline]
fn publish_blend(seq: u32, blend: BlendDesc) {
    crate::r::rdp::publish_set_blend(
        seq,
        blend.enabled as u32,
        blend_factor_raw(blend.src),
        blend_factor_raw(blend.dst),
        blend_factor_raw(blend.src),
        blend_factor_raw(blend.dst),
    );
}

#[inline]
fn blend_factor_raw(factor: BlendFactor) -> u32 {
    match factor {
        BlendFactor::Zero => 0,
        BlendFactor::One => 1,
        BlendFactor::OneMinusSrcColor => 0x0301,
        BlendFactor::SrcAlpha => 0x0302,
        BlendFactor::OneMinusSrcAlpha => 0x0303,
        BlendFactor::DstColor => 0x0306,
        BlendFactor::OneMinusDstColor => 0x0307,
    }
}
