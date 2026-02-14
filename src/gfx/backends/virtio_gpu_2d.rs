use alloc::vec::Vec;

use libm::{ceilf, floorf};

use trueos_gfx_core::{
    BufferDesc, BufferId, BufferUsage, ColorFormat, Command, CommandBuffer, DeviceCaps, Error,
    Extent2D, FenceId, GfxDevice, GfxPresent, ImageFormat, MapMode, MappedRange, MemoryType,
    PipelineDesc, PipelineId, Result, ShaderDesc, ShaderId, SwapchainDesc, VertexLayout, Viewport,
};

use crate::pci::virtio_gpu::{Rect, VirtioGpu2d};

struct Buffer {
    desc: BufferDesc,
    bytes: Vec<u8>,
    mapped: bool,
}

struct Pipeline {
    desc: PipelineDesc,
}

pub struct VirtioGpu2dBackend {
    gpu: VirtioGpu2d,
    swapchain: SwapchainDesc,

    buffers: Vec<Option<Buffer>>,
    pipelines: Vec<Option<Pipeline>>,

    state: DrawState,
    dirty: Option<Rect>,

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

impl VirtioGpu2dBackend {
    pub fn init_first() -> Option<Self> {
        let mut gpu = VirtioGpu2d::init_first()?;
        let (w, h) = gpu.extent();
        if w == 0 || h == 0 {
            return None;
        }

        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D { width: w, height: h },
        };

        let viewport = Viewport {
            x: 0,
            y: 0,
            width: w as i32,
            height: h as i32,
        };

        // Clear once so the scanout resource isn't uninitialized.
        let full = Rect { x: 0, y: 0, width: w, height: h };
        Self::clear_rect(Self::backbuffer_mut(&mut gpu), swapchain.extent, full, 0x00_08_18_30);
        let _ = gpu.transfer_and_flush(full);

        Some(Self {
            gpu,
            swapchain,
            buffers: Vec::new(),
            pipelines: Vec::new(),
            state: DrawState {
                pipeline: PipelineId::invalid(),
                vertex: BufferBinding { id: BufferId::invalid(), offset: 0 },
                viewport,
            },
            dirty: None,
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

    fn backbuffer_mut(gpu: &mut VirtioGpu2d) -> &mut [u32] {
        let ptr = gpu.backing_ptr_u32();
        let len = gpu.backing_len_u32();
        if ptr.is_null() || len == 0 {
            return &mut [];
        }
        unsafe { core::slice::from_raw_parts_mut(ptr, len) }
    }

    fn get_buffer_mut(&mut self, id: BufferId) -> Result<&mut Buffer> {
        let idx = id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
        self.buffers
            .get_mut(idx)
            .and_then(|b| b.as_mut())
            .ok_or(Error::NotFound)
    }

    fn get_pipeline_layout(&self, id: PipelineId) -> Result<VertexLayout> {
        let idx = id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
        let p = self
            .pipelines
            .get(idx)
            .and_then(|p| p.as_ref())
            .ok_or(Error::NotFound)?;
        Ok(p.desc.vertex_layout)
    }

    fn clear_rect(backbuffer: &mut [u32], extent: Extent2D, rect: Rect, rgb: u32) {
        let w = extent.width as usize;
        let h = extent.height as usize;
        if w == 0 || h == 0 {
            return;
        }

        let x0 = (rect.x as usize).min(w);
        let y0 = (rect.y as usize).min(h);
        let x1 = x0.saturating_add(rect.width as usize).min(w);
        let y1 = y0.saturating_add(rect.height as usize).min(h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }

        let color = rgb & 0x00FF_FFFF;
        for yy in y0..y1 {
            let row = yy.saturating_mul(w);
            for xx in x0..x1 {
                let idx = row.saturating_add(xx);
                if idx < backbuffer.len() {
                    backbuffer[idx] = color;
                }
            }
        }
    }

    fn ndc_to_screen(x_ndc: f32, y_ndc: f32, vp: Viewport) -> (f32, f32) {
        let vw = vp.width.max(1) as f32;
        let vh = vp.height.max(1) as f32;

        let sx = (x_ndc * 0.5 + 0.5) * vw + (vp.x as f32);
        let sy = (-y_ndc * 0.5 + 0.5) * vh + (vp.y as f32);
        (sx, sy)
    }

    fn draw_triangles_pos_color_rgbu8(
        backbuffer: &mut [u32],
        extent: Extent2D,
        viewport: Viewport,
        vbuf: &[u8],
        layout: VertexLayout,
        first_vertex: u32,
        vertex_count: u32,
    ) {
        if extent.width == 0 || extent.height == 0 {
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

        let w = extent.width as i32;
        let h = extent.height as i32;
        let vp = viewport;
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

                let (sx, sy) = Self::ndc_to_screen(x, y, vp);
                px[i] = sx;
                py[i] = sy;

                cr[i] = vbuf[col_off + 0] as f32;
                cg[i] = vbuf[col_off + 1] as f32;
                cb[i] = vbuf[col_off + 2] as f32;
            }

            let min_x = floorf(px.iter().copied().fold(f32::INFINITY, f32::min)) as i32;
            let max_x = ceilf(px.iter().copied().fold(f32::NEG_INFINITY, f32::max)) as i32;
            let min_y = floorf(py.iter().copied().fold(f32::INFINITY, f32::min)) as i32;
            let max_y = ceilf(py.iter().copied().fold(f32::NEG_INFINITY, f32::max)) as i32;

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

            let sw = extent.width as usize;
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
                    if idx < backbuffer.len() {
                        backbuffer[idx] = (r << 16) | (g << 8) | b;
                    }
                }
            }
        }
    }

    fn process(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::ClearColor { rgb } => {
                let full = Rect {
                    x: 0,
                    y: 0,
                    width: self.swapchain.extent.width,
                    height: self.swapchain.extent.height,
                };
                Self::clear_rect(Self::backbuffer_mut(&mut self.gpu), self.swapchain.extent, full, rgb);
                self.dirty = Some(full);
                Ok(())
            }
            Command::ClearRect { rgb, x, y, width, height } => {
                let rect = Rect { x, y, width, height };
                Self::clear_rect(Self::backbuffer_mut(&mut self.gpu), self.swapchain.extent, rect, rgb);
                self.dirty = Some(rect);
                Ok(())
            }
            Command::BindPipeline(pid) => {
                if !pid.is_valid() {
                    return Err(Error::Invalid);
                }
                let _ = self.get_pipeline_layout(pid)?;
                self.state.pipeline = pid;
                Ok(())
            }
            Command::BindVertexBuffer { buffer, offset } => {
                if !buffer.is_valid() {
                    return Err(Error::Invalid);
                }
                let idx = buffer.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
                let _ = self
                    .buffers
                    .get(idx)
                    .and_then(|b| b.as_ref())
                    .ok_or(Error::NotFound)?;
                self.state.vertex = BufferBinding { id: buffer, offset };
                Ok(())
            }
            Command::SetViewport(vp) => {
                self.state.viewport = vp;
                Ok(())
            }
            Command::Draw { vertex_count, first_vertex } => {
                let pipeline_id = self.state.pipeline;
                if !pipeline_id.is_valid() {
                    return Err(Error::Invalid);
                }
                let layout = self.get_pipeline_layout(pipeline_id)?;

                let vb = self.state.vertex;
                if !vb.id.is_valid() {
                    return Err(Error::Invalid);
                }

                let bidx = vb.id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
                let buffer = self
                    .buffers
                    .get(bidx)
                    .and_then(|b| b.as_ref())
                    .ok_or(Error::NotFound)?;
                if buffer.desc.usage != BufferUsage::Vertex {
                    return Err(Error::Invalid);
                }

                let start = vb.offset as usize;
                if start > buffer.bytes.len() {
                    return Err(Error::Invalid);
                }
                let vbuf = &buffer.bytes[start..];

                Self::draw_triangles_pos_color_rgbu8(
                    Self::backbuffer_mut(&mut self.gpu),
                    self.swapchain.extent,
                    self.state.viewport,
                    vbuf,
                    layout,
                    first_vertex,
                    vertex_count,
                );

                // Track viewport as dirty region (best-effort).
                if self.state.viewport.width > 0 && self.state.viewport.height > 0 {
                    self.dirty = Some(Rect {
                        x: self.state.viewport.x.max(0) as u32,
                        y: self.state.viewport.y.max(0) as u32,
                        width: self.state.viewport.width as u32,
                        height: self.state.viewport.height as u32,
                    });
                }

                Ok(())
            }
            Command::Present => {
                let rect = self.dirty.take().unwrap_or(Rect {
                    x: 0,
                    y: 0,
                    width: self.swapchain.extent.width,
                    height: self.swapchain.extent.height,
                });
                if !self.gpu.transfer_and_flush(rect) {
                    return Err(Error::Unsupported);
                }
                Ok(())
            }
        }
    }
}

impl GfxDevice for VirtioGpu2dBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            supports_rgbx8888: true,
            supports_host_visible_buffers: true,
        }
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

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId> {
        Err(Error::Unsupported)
    }

    fn destroy_shader(&mut self, _id: ShaderId) {}

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

impl GfxPresent for VirtioGpu2dBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        // For now: scanout size is fixed by the device bootstrap.
        if desc.format != self.swapchain.format {
            return Err(Error::Unsupported);
        }
        if desc.extent != self.swapchain.extent {
            return Err(Error::Unsupported);
        }
        Ok(())
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain
    }
}
