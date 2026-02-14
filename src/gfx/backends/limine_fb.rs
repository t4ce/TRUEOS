use alloc::vec::Vec;

use trueos_gfx_core::{
    BufferDesc, BufferId, BufferUsage, ColorFormat, Command, CommandBuffer, DeviceCaps, Error,
    Extent2D, FenceId, GfxDevice, GfxPresent, ImageFormat, MapMode, MappedRange, MemoryType,
    PipelineDesc, PipelineId, Result, ShaderDesc, ShaderFormat, ShaderId, SwapchainDesc, VertexLayout,
    Viewport,
};

pub struct NullBackend;

impl GfxDevice for NullBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            supports_rgbx8888: false,
            supports_host_visible_buffers: false,
        }
    }

    fn create_buffer(&mut self, _desc: BufferDesc) -> Result<BufferId> {
        Err(Error::Unsupported)
    }

    fn destroy_buffer(&mut self, _id: BufferId) {}

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId> {
        Err(Error::Unsupported)
    }

    fn destroy_shader(&mut self, _id: ShaderId) {}

    fn create_pipeline(&mut self, _desc: PipelineDesc) -> Result<PipelineId> {
        Err(Error::Unsupported)
    }

    fn destroy_pipeline(&mut self, _id: PipelineId) {}

    fn write_buffer(&mut self, _id: BufferId, _offset: u64, _data: &[u8]) -> Result<()> {
        Err(Error::Unsupported)
    }

    fn submit(&mut self, _cmds: CommandBuffer<'_>) -> Result<FenceId> {
        Err(Error::Unsupported)
    }

    fn poll(&mut self, _fence: FenceId) -> bool {
        false
    }

    fn device_idle(&mut self) {}
}

impl GfxPresent for NullBackend {
    fn configure_swapchain(&mut self, _desc: SwapchainDesc) -> Result<()> {
        Err(Error::Unsupported)
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D {
                width: 0,
                height: 0,
            },
        }
    }
}

#[derive(Clone, Copy)]
struct FramebufferSurface {
    addr: *mut u8,
    pitch: usize,
    bytes_per_pixel: usize,
    width: usize,
    height: usize,
}

unsafe impl Send for FramebufferSurface {}
unsafe impl Sync for FramebufferSurface {}

impl FramebufferSurface {
    fn from_limine(fb: ::limine::framebuffer::Framebuffer<'static>) -> Option<Self> {
        use ::limine::framebuffer::MemoryModel;

        if fb.memory_model() != MemoryModel::RGB {
            return None;
        }
        let bpp = fb.bpp();
        if bpp != 32 {
            return None;
        }
        Some(Self {
            addr: fb.addr(),
            pitch: fb.pitch() as usize,
            bytes_per_pixel: (bpp / 8) as usize,
            width: fb.width() as usize,
            height: fb.height() as usize,
        })
    }

    fn write_pixel(&self, x: usize, y: usize, color: u32) {
        let offset = y
            .saturating_mul(self.pitch)
            .saturating_add(x.saturating_mul(self.bytes_per_pixel));
        unsafe {
            core::ptr::write_volatile(self.addr.add(offset) as *mut u32, color);
        }
    }

    fn present_from_rgb32(&self, src: &[u32]) {
        if self.width == 0 || self.height == 0 {
            return;
        }
        let expected = self.width.saturating_mul(self.height);
        if src.len() < expected {
            return;
        }

        for y in 0..self.height {
            let row_ptr = unsafe { self.addr.add(y.saturating_mul(self.pitch)) as *mut u32 };
            let src_row = &src[y.saturating_mul(self.width)..][..self.width];
            for x in 0..self.width {
                unsafe { row_ptr.add(x).write_volatile(src_row[x]) };
            }
        }
    }
}

struct Buffer {
    desc: BufferDesc,
    bytes: Vec<u8>,
    mapped: bool,
}

struct Shader {
    _stage: trueos_gfx_core::ShaderStage,
    _format: ShaderFormat,
    _bytes: Vec<u8>,
}

struct Pipeline {
    desc: PipelineDesc,
}

pub struct LimineFbBackend {
    fb: FramebufferSurface,
    swapchain: SwapchainDesc,
    backbuffer: Vec<u32>,

    buffers: Vec<Option<Buffer>>,
    shaders: Vec<Option<Shader>>,
    pipelines: Vec<Option<Pipeline>>,

    state: DrawState,

    next_fence: u64,
    completed_fence: u64,
}

#[derive(Clone, Copy, Debug)]
struct DrawState {
    pipeline: PipelineId,
    vertex: BufferBinding,
    viewport: Viewport,
}

#[derive(Clone, Copy, Debug)]
struct BufferBinding {
    id: BufferId,
    offset: u64,
}

impl LimineFbBackend {
    pub fn from_limine(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        let fb = framebuffers
            .and_then(|resp| resp.framebuffers().next())
            .and_then(FramebufferSurface::from_limine)?;

        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D {
                width: fb.width as u32,
                height: fb.height as u32,
            },
        };

        let expected = fb.width.saturating_mul(fb.height);
        let mut backbuffer = Vec::new();
        backbuffer.resize(expected, 0);

        let viewport = Viewport {
            x: 0,
            y: 0,
            width: fb.width as i32,
            height: fb.height as i32,
        };

        Some(Self {
            fb,
            swapchain,
            backbuffer,
            buffers: Vec::new(),
            shaders: Vec::new(),
            pipelines: Vec::new(),
            state: DrawState {
                pipeline: PipelineId::invalid(),
                vertex: BufferBinding {
                    id: BufferId::invalid(),
                    offset: 0,
                },
                viewport,
            },
            next_fence: 1,
            completed_fence: 0,
        })
    }

    fn alloc_slot<T>(list: &mut Vec<Option<T>>, value: T) -> u32 {
        for (idx, slot) in list.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(value);
                return (idx as u32) + 1;
            }
        }
        list.push(Some(value));
        list.len() as u32
    }

    fn get_buffer(&self, id: BufferId) -> Result<&Buffer> {
        let idx = id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
        self.buffers
            .get(idx)
            .and_then(|b| b.as_ref())
            .ok_or(Error::NotFound)
    }

    fn get_buffer_mut(&mut self, id: BufferId) -> Result<&mut Buffer> {
        let idx = id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
        self.buffers
            .get_mut(idx)
            .and_then(|b| b.as_mut())
            .ok_or(Error::NotFound)
    }

    fn get_pipeline(&self, id: PipelineId) -> Result<&Pipeline> {
        let idx = id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
        self.pipelines
            .get(idx)
            .and_then(|p| p.as_ref())
            .ok_or(Error::NotFound)
    }

    fn clear_backbuffer(&mut self, rgb: u32) {
        for px in self.backbuffer.iter_mut() {
            *px = rgb & 0x00FF_FFFF;
        }
    }

    fn ndc_to_screen(&self, x_ndc: f32, y_ndc: f32, vp: Viewport) -> (f32, f32) {
        let vw = vp.width.max(1) as f32;
        let vh = vp.height.max(1) as f32;

        let sx = (x_ndc * 0.5 + 0.5) * vw + (vp.x as f32);
        let sy = (-y_ndc * 0.5 + 0.5) * vh + (vp.y as f32);
        (sx, sy)
    }

    fn draw_triangles_pos_color_rgbu8(
        &mut self,
        vbuf: &[u8],
        layout: VertexLayout,
        first_vertex: u32,
        vertex_count: u32,
    ) {
        if self.swapchain.extent.width == 0 || self.swapchain.extent.height == 0 {
            return;
        }
        if layout.color_format != ColorFormat::RgbU8 {
            return;
        }

        let stride = layout.stride as usize;
        if stride == 0 {
            return;
        }

        let tri_count = (vertex_count / 3) as usize;
        if tri_count == 0 {
            return;
        }

        let w = self.swapchain.extent.width as i32;
        let h = self.swapchain.extent.height as i32;
        let vp = self.state.viewport;
        if vp.width <= 0 || vp.height <= 0 {
            return;
        }

        for tri_i in 0..tri_count {
            let base = first_vertex as usize + tri_i * 3;

            let mut px = [0.0f32; 3];
            let mut py = [0.0f32; 3];
            let mut cr = [0.0f32; 3];
            let mut cg = [0.0f32; 3];
            let mut cb = [0.0f32; 3];

            for i in 0..3 {
                let vi = base + i;
                let off = vi.saturating_mul(stride);
                if off.saturating_add(stride) > vbuf.len() {
                    return;
                }

                let pos_off = off.saturating_add(layout.pos_offset as usize);
                let col_off = off.saturating_add(layout.color_offset as usize);

                if pos_off.saturating_add(8) > vbuf.len() || col_off.saturating_add(3) > vbuf.len() {
                    return;
                }

                let x = f32::from_le_bytes([
                    vbuf[pos_off + 0],
                    vbuf[pos_off + 1],
                    vbuf[pos_off + 2],
                    vbuf[pos_off + 3],
                ]);
                let y = f32::from_le_bytes([
                    vbuf[pos_off + 4],
                    vbuf[pos_off + 5],
                    vbuf[pos_off + 6],
                    vbuf[pos_off + 7],
                ]);

                let (sx, sy) = self.ndc_to_screen(x, y, vp);
                px[i] = sx;
                py[i] = sy;

                cr[i] = vbuf[col_off + 0] as f32;
                cg[i] = vbuf[col_off + 1] as f32;
                cb[i] = vbuf[col_off + 2] as f32;
            }

            let min_x = px
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min)
                .floor() as i32;
            let max_x = px
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32;
            let min_y = py
                .iter()
                .copied()
                .fold(f32::INFINITY, f32::min)
                .floor() as i32;
            let max_y = py
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max)
                .ceil() as i32;

            let x0 = min_x.max(vp.x).max(0);
            let x1 = max_x.min(vp.x.saturating_add(vp.width)).min(w - 1);
            let y0 = min_y.max(vp.y).max(0);
            let y1 = max_y.min(vp.y.saturating_add(vp.height)).min(h - 1);

            if x1 < x0 || y1 < y0 {
                continue;
            }

            let ax = px[0];
            let ay = py[0];
            let bx = px[1];
            let by = py[1];
            let cx = px[2];
            let cy = py[2];

            let area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax);
            if area.abs() <= f32::EPSILON {
                continue;
            }
            let inv_area = 1.0 / area;

            let sw = self.swapchain.extent.width as usize;
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let fx = x as f32 + 0.5;
                    let fy = y as f32 + 0.5;

                    let w0 = ((bx - fx) * (cy - fy) - (by - fy) * (cx - fx)) * inv_area;
                    let w1 = ((cx - fx) * (ay - fy) - (cy - fy) * (ax - fx)) * inv_area;
                    let w2 = 1.0 - w0 - w1;

                    if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                        continue;
                    }

                    let r = (cr[0] * w0 + cr[1] * w1 + cr[2] * w2).clamp(0.0, 255.0) as u32;
                    let g = (cg[0] * w0 + cg[1] * w1 + cg[2] * w2).clamp(0.0, 255.0) as u32;
                    let b = (cb[0] * w0 + cb[1] * w1 + cb[2] * w2).clamp(0.0, 255.0) as u32;

                    let idx = (y as usize).saturating_mul(sw).saturating_add(x as usize);
                    if idx < self.backbuffer.len() {
                        self.backbuffer[idx] = (r << 16) | (g << 8) | b;
                    }
                }
            }
        }
    }

    fn process(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::ClearColor { rgb } => {
                self.clear_backbuffer(rgb);
                Ok(())
            }
            Command::BindPipeline(pid) => {
                if !pid.is_valid() {
                    return Err(Error::Invalid);
                }
                let _ = self.get_pipeline(pid)?;
                self.state.pipeline = pid;
                Ok(())
            }
            Command::BindVertexBuffer { buffer, offset } => {
                if !buffer.is_valid() {
                    return Err(Error::Invalid);
                }
                let _ = self.get_buffer(buffer)?;
                self.state.vertex = BufferBinding { id: buffer, offset };
                Ok(())
            }
            Command::SetViewport(vp) => {
                self.state.viewport = vp;
                Ok(())
            }
            Command::Draw {
                vertex_count,
                first_vertex,
            } => {
                let pipeline_id = self.state.pipeline;
                if !pipeline_id.is_valid() {
                    return Err(Error::Invalid);
                }
                let pipeline = self.get_pipeline(pipeline_id)?;

                let vb = self.state.vertex;
                if !vb.id.is_valid() {
                    return Err(Error::Invalid);
                }
                let buffer = self.get_buffer(vb.id)?;
                if buffer.desc.usage != BufferUsage::Vertex {
                    return Err(Error::Invalid);
                }

                let start = vb.offset as usize;
                if start > buffer.bytes.len() {
                    return Err(Error::Invalid);
                }
                let vbuf = &buffer.bytes[start..];

                self.draw_triangles_pos_color_rgbu8(
                    vbuf,
                    pipeline.desc.vertex_layout,
                    first_vertex,
                    vertex_count,
                );
                Ok(())
            }
            Command::Present => {
                self.fb.present_from_rgb32(&self.backbuffer);
                Ok(())
            }
        }
    }
}

impl GfxDevice for LimineFbBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps::minimal_software()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        if desc.size == 0 {
            return Err(Error::Invalid);
        }
        if desc.memory != MemoryType::HostVisible {
            return Err(Error::Unsupported);
        }
        let mut bytes = Vec::new();
        let size = desc.size.min(usize::MAX as u64) as usize;
        bytes.resize(size, 0);

        let slot = Self::alloc_slot(
            &mut self.buffers,
            Buffer {
                desc,
                bytes,
                mapped: false,
            },
        );
        Ok(BufferId::from_raw(slot))
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        if !id.is_valid() {
            return;
        }
        let Some(idx) = id.raw().checked_sub(1).map(|v| v as usize) else {
            return;
        };
        if let Some(slot) = self.buffers.get_mut(idx) {
            *slot = None;
        }
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(desc.bytes);

        let slot = Self::alloc_slot(
            &mut self.shaders,
            Shader {
                _stage: desc.stage,
                _format: desc.format,
                _bytes: bytes,
            },
        );
        Ok(ShaderId::from_raw(slot))
    }

    fn destroy_shader(&mut self, id: ShaderId) {
        if !id.is_valid() {
            return;
        }
        let Some(idx) = id.raw().checked_sub(1).map(|v| v as usize) else {
            return;
        };
        if let Some(slot) = self.shaders.get_mut(idx) {
            *slot = None;
        }
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        if desc.vertex_layout.stride == 0 {
            return Err(Error::Invalid);
        }
        let slot = Self::alloc_slot(&mut self.pipelines, Pipeline { desc });
        Ok(PipelineId::from_raw(slot))
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        if !id.is_valid() {
            return;
        }
        let Some(idx) = id.raw().checked_sub(1).map(|v| v as usize) else {
            return;
        };
        if let Some(slot) = self.pipelines.get_mut(idx) {
            *slot = None;
        }
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        let buf = self.get_buffer_mut(id)?;
        let off = offset as usize;
        let end = off.saturating_add(data.len());
        if end > buf.bytes.len() {
            return Err(Error::Invalid);
        }
        buf.bytes[off..end].copy_from_slice(data);
        Ok(())
    }

    fn map_buffer(&mut self, id: BufferId, _mode: MapMode) -> Result<MappedRange> {
        let buf = self.get_buffer_mut(id)?;
        if buf.mapped {
            return Err(Error::Invalid);
        }
        buf.mapped = true;
        Ok(MappedRange {
            ptr: buf.bytes.as_mut_ptr(),
            len: buf.bytes.len(),
        })
    }

    fn unmap_buffer(&mut self, id: BufferId) -> Result<()> {
        let buf = self.get_buffer_mut(id)?;
        if !buf.mapped {
            return Err(Error::Invalid);
        }
        buf.mapped = false;
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        for &cmd in cmds.commands {
            self.process(cmd)?;
        }
        let fence = FenceId::from_raw(self.next_fence);
        self.next_fence = self.next_fence.wrapping_add(1).max(1);
        self.completed_fence = fence.raw();
        Ok(fence)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        if !fence.is_valid() {
            return true;
        }
        fence.raw() <= self.completed_fence
    }

    fn device_idle(&mut self) {
        self.completed_fence = self.next_fence.saturating_sub(1);
    }
}

impl GfxPresent for LimineFbBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        if desc.format != ImageFormat::Rgbx8888 {
            return Err(Error::Unsupported);
        }
        if desc.extent.width == 0 || desc.extent.height == 0 {
            return Err(Error::Invalid);
        }

        let expected = (desc.extent.width as usize).saturating_mul(desc.extent.height as usize);
        self.backbuffer.resize(expected, 0);
        self.swapchain = desc;
        self.state.viewport = Viewport {
            x: 0,
            y: 0,
            width: desc.extent.width as i32,
            height: desc.extent.height as i32,
        };
        Ok(())
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain
    }
}
