use alloc::vec::Vec;

use libm::{ceilf, floorf};

use trueos_gfx_core::{
    BufferDesc, BufferId, BufferUsage, ColorFormat, Command, CommandBuffer, DeviceCaps, Error,
    Extent2D, FenceId, GfxDevice, GfxPresent, ImageFormat, MapMode, MappedRange, MemoryType,
    PipelineDesc, PipelineId, Result, ShaderDesc, ShaderFormat, ShaderId, SwapchainDesc,
    VertexLayout, Viewport,
};

use crate::gfx::virtio_gpu_3d::{
    gpu_get_display_info, gpu_resource_attach_backing, gpu_resource_create_2d, gpu_resource_flush,
    gpu_set_scanout, gpu_transfer_to_host_2d,
};

use core::sync::atomic::{AtomicU32, Ordering};

static NEXT_RES_ID: AtomicU32 = AtomicU32::new(0x9000);

// Must match the format used by the virtio-gpu backend and QEMU scanout.
// virgl uses B8G8R8X8 for scanout compatibility.
const FORMAT_B8G8R8X8_UNORM: u32 = 2;

fn alloc_res_id() -> u32 {
    // Avoid 0 and keep a high range away from virgl's small ids.
    let id = NEXT_RES_ID.fetch_add(1, Ordering::Relaxed);
    if id == 0 {
        NEXT_RES_ID.store(0x9000, Ordering::Relaxed);
        0x9000
    } else {
        id
    }
}

struct DmaRegion {
    phys: u64,
    virt: *mut u8,
    len: usize,
}

unsafe impl Send for DmaRegion {}

impl DmaRegion {
    fn alloc(size: usize, align: usize) -> Option<Self> {
        let (phys, virt) = crate::pci::dma::alloc(size, align)?;
        Some(Self { phys, virt, len: size })
    }

    fn phys(&self) -> u64 {
        self.phys
    }

    fn virt(&self) -> *mut u8 {
        self.virt
    }

    fn len(&self) -> usize {
        self.len
    }
}

impl Drop for DmaRegion {
    fn drop(&mut self) {
        if self.len == 0 || self.virt.is_null() {
            return;
        }
        crate::pci::dma::dealloc(self.virt, self.len);
        self.len = 0;
        self.virt = core::ptr::null_mut();
        self.phys = 0;
    }
}

struct VirtioScanout {
    width: u32,
    height: u32,
    scanout_res: u32,
    backing: DmaRegion,
}

impl VirtioScanout {
    fn init() -> Option<Self> {
        let (scanout_id, width, height) = gpu_get_display_info(2000)?;

        crate::log!(
            "virtio-scanout: display_info scanout={} {}x{}\n",
            scanout_id,
            width,
            height
        );

        let scanout_res = alloc_res_id();
        crate::log!(
            "virtio-scanout: resource_create_2d begin res={} fmt={} {}x{}\n",
            scanout_res,
            FORMAT_B8G8R8X8_UNORM,
            width,
            height
        );
        let ok_create = gpu_resource_create_2d(scanout_res, FORMAT_B8G8R8X8_UNORM, width, height, 2000);
        crate::log!(
            "virtio-scanout: resource_create_2d end ok={}\n",
            ok_create as u8
        );
        if !ok_create {
            crate::log!("virtio-scanout: resource_create_2d failed\n");
            return None;
        }

        let bytes = (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
            .max(4096);
        let backing = DmaRegion::alloc(bytes, 4096)?;
        unsafe { core::ptr::write_bytes(backing.virt(), 0, backing.len()) };

        crate::log!(
            "virtio-scanout: attach_backing begin res={} phys=0x{:X} len={}\n",
            scanout_res,
            backing.phys(),
            bytes
        );
        let ok_attach = gpu_resource_attach_backing(scanout_res, backing.phys(), bytes as u32, 2000);
        crate::log!("virtio-scanout: attach_backing end ok={}\n", ok_attach as u8);
        if !ok_attach {
            crate::log!("virtio-scanout: attach_backing failed\n");
            return None;
        }

        crate::log!(
            "virtio-scanout: set_scanout begin scanout={} res={} {}x{}\n",
            scanout_id,
            scanout_res,
            width,
            height
        );
        let ok_scanout = gpu_set_scanout(scanout_id, scanout_res, width, height, 2000);
        crate::log!("virtio-scanout: set_scanout end ok={}\n", ok_scanout as u8);
        if !ok_scanout {
            crate::log!("virtio-scanout: set_scanout failed\n");
            return None;
        }

        crate::log!(
            "virtio-scanout: ready res={} bytes={} backing_phys=0x{:X}\n",
            scanout_res,
            bytes,
            backing.phys()
        );

        Some(Self {
            width,
            height,
            scanout_res,
            backing,
        })
    }

    fn present_rgbx8888(&mut self, src: &[u32], extent: Extent2D) {
        let w = extent.width.min(self.width) as usize;
        let h = extent.height.min(self.height) as usize;
        if w == 0 || h == 0 {
            return;
        }
        let expected = w.saturating_mul(h);
        if src.len() < expected {
            return;
        }

        let dst = self.backing.virt() as *mut u32;
        if dst.is_null() {
            return;
        }

        for y in 0..h {
            let row = unsafe { dst.add(y.saturating_mul(self.width as usize)) };
            let src_row = &src[y.saturating_mul(w)..][..w];
            unsafe {
                core::ptr::copy_nonoverlapping(src_row.as_ptr(), row, w);
            }
        }

        // Keep global lock hold times short (avoid holding it across multiple ctrlq submissions).
        let ok_tth = gpu_transfer_to_host_2d(self.scanout_res, self.width, self.height, 1000);
        let ok_flush = gpu_resource_flush(self.scanout_res, self.width, self.height, 1000);
        if !ok_tth || !ok_flush {
            crate::log!(
                "virtio-scanout: present transfer_to_host={} flush={}\n",
                ok_tth as u8,
                ok_flush as u8
            );
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

pub struct VirtioSwBackend {
    scanout: VirtioScanout,

    swapchain: SwapchainDesc,
    backbuffer: Vec<u32>,

    buffers: Vec<Option<Buffer>>,
    shaders: Vec<Option<Shader>>,
    pipelines: Vec<Option<Pipeline>>,

    state: DrawState,

    next_fence: u64,
    completed_fence: u64,
}

impl VirtioSwBackend {
    pub fn init() -> Option<Self> {
        let scanout = VirtioScanout::init()?;

        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D {
                width: scanout.width,
                height: scanout.height,
            },
        };

        let expected = (scanout.width as usize).saturating_mul(scanout.height as usize);
        let mut backbuffer = Vec::new();
        backbuffer.resize(expected, 0);

        let viewport = Viewport {
            x: 0,
            y: 0,
            width: scanout.width as i32,
            height: scanout.height as i32,
        };

        Some(Self {
            scanout,
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
        for (i, slot) in list.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(value);
                return (i + 1) as u32;
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

    fn clear_rect(
        backbuffer: &mut [u32],
        extent: Extent2D,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgb: u32,
    ) {
        let w = extent.width as i32;
        let h = extent.height as i32;
        if w <= 0 || h <= 0 {
            return;
        }

        let x0 = x.min(extent.width) as i32;
        let y0 = y.min(extent.height) as i32;
        let x1 = x0.saturating_add(width as i32).min(w);
        let y1 = y0.saturating_add(height as i32).min(h);
        if x1 <= x0 || y1 <= y0 {
            return;
        }

        let sw = extent.width as usize;
        let color = rgb & 0x00FF_FFFF;
        for yy in y0..y1 {
            let row = (yy as usize).saturating_mul(sw);
            for xx in x0..x1 {
                let idx = row.saturating_add(xx as usize);
                if idx < backbuffer.len() {
                    backbuffer[idx] = color;
                }
            }
        }
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

        let sw = extent.width as usize;

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

                if pos_off.saturating_add(8) > vbuf.len()
                    || col_off.saturating_add(3) > vbuf.len()
                {
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

                let vw = vp.width.max(1) as f32;
                let vh = vp.height.max(1) as f32;
                let sx = (x * 0.5 + 0.5) * vw + (vp.x as f32);
                let sy = (-y * 0.5 + 0.5) * vh + (vp.y as f32);
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
            if !(area.abs() > 0.00001) {
                continue;
            }
            let inv_area = 1.0 / area;

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

                    let r = (cr[0] * w0 + cr[1] * w1 + cr[2] * w2)
                        .clamp(0.0, 255.0) as u32;
                    let g = (cg[0] * w0 + cg[1] * w1 + cg[2] * w2)
                        .clamp(0.0, 255.0) as u32;
                    let b = (cb[0] * w0 + cb[1] * w1 + cb[2] * w2)
                        .clamp(0.0, 255.0) as u32;

                    let idx = (y as usize)
                        .saturating_mul(sw)
                        .saturating_add(x as usize);
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
                self.clear_backbuffer(rgb);
                Ok(())
            }
            Command::ClearRect {
                rgb,
                x,
                y,
                width,
                height,
            } => {
                Self::clear_rect(
                    &mut self.backbuffer,
                    self.swapchain.extent,
                    x,
                    y,
                    width,
                    height,
                    rgb,
                );
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
                let pidx = pipeline_id.raw().checked_sub(1).ok_or(Error::Invalid)? as usize;
                let layout = self
                    .pipelines
                    .get(pidx)
                    .and_then(|p| p.as_ref())
                    .ok_or(Error::NotFound)?
                    .desc
                    .vertex_layout;

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
                    &mut self.backbuffer,
                    self.swapchain.extent,
                    self.state.viewport,
                    vbuf,
                    layout,
                    first_vertex,
                    vertex_count,
                );
                Ok(())
            }
            Command::Present => {
                self.scanout
                    .present_rgbx8888(&self.backbuffer, self.swapchain.extent);
                Ok(())
            }
        }
    }
}

impl GfxDevice for VirtioSwBackend {
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
        let start = offset.min(buf.bytes.len() as u64) as usize;
        let end = start.saturating_add(data.len());
        if end > buf.bytes.len() {
            return Err(Error::Invalid);
        }
        buf.bytes[start..end].copy_from_slice(data);
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
        fence.raw() <= self.completed_fence
    }

    fn device_idle(&mut self) {}
}

impl GfxPresent for VirtioSwBackend {
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
