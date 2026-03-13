use crate::gfx::backends::intel_cmd::{IntelCmd, RingMmio};
use alloc::vec;
use alloc::vec::Vec;

use trueos_gfx_core::{
    BufferDesc, BufferId, Command, CommandBuffer, DeviceCaps, Error, Extent2D, FenceId, GfxDevice,
    GfxPresent, ImageDesc, ImageFormat, ImageId, MapMode, MappedRange, PipelineDesc, PipelineId,
    Result, ShaderDesc, ShaderId, SwapchainDesc,
};

pub struct IntelGfxBackend {
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    swapchain: SwapchainDesc,
    fence_seq: u64,
    buffers: Vec<Option<SwBuffer>>,
    pipelines: Vec<Option<PipelineDesc>>,
    images: Vec<Option<SwImage>>,
    cmd: Option<IntelCmd>,
}

struct SwBuffer {
    data: Vec<u8>,
    _desc: BufferDesc,
}

struct SwImage {
    width: u32,
    height: u32,
    format: ImageFormat,
    data: Vec<u8>,
}

struct FramebufferTarget {
    width: usize,
    height: usize,
}

impl IntelGfxBackend {
    pub fn init(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        if !crate::gfx::intel::has_claimed_device() {
            return None;
        }

        let (w, h) = first_fb_dimensions(framebuffers)?;
        let swapchain = SwapchainDesc {
            format: ImageFormat::Rgbx8888,
            extent: Extent2D {
                width: w,
                height: h,
            },
        };

        crate::log!("gfx: using intel backend (no software raster path)\n");

        let cmd = crate::gfx::intel::first_claimed_device().and_then(|info| {
            let mmio = RingMmio {
                base: info.mmio_base,
                len: info.mmio_len,
            };
            let mut eng = IntelCmd::new(
                mmio,
                info.cmd_scratch_phys,
                info.cmd_scratch_virt,
                info.cmd_scratch_len,
            )?;
            if eng.init_ring().is_err() {
                return None;
            }
            Some(eng)
        });
        if cmd.is_none() {
            crate::log!("gfx-intel: command streamer init unavailable\n");
        }

        Some(Self {
            framebuffers,
            swapchain,
            fence_seq: 1,
            buffers: Vec::new(),
            pipelines: Vec::new(),
            images: Vec::new(),
            cmd,
        })
    }

    fn slot_to_id(slot: usize) -> u32 {
        (slot as u32).saturating_add(1)
    }

    fn id_to_slot(raw: u32) -> Option<usize> {
        if raw == 0 {
            return None;
        }
        Some((raw - 1) as usize)
    }

    fn alloc_slot<T>(list: &mut Vec<Option<T>>, value: T) -> u32 {
        if let Some((idx, _)) = list.iter().enumerate().find(|(_, e)| e.is_none()) {
            list[idx] = Some(value);
            return Self::slot_to_id(idx);
        }
        let idx = list.len();
        list.push(Some(value));
        Self::slot_to_id(idx)
    }

    fn buffer_mut(&mut self, id: BufferId) -> Option<&mut SwBuffer> {
        let slot = Self::id_to_slot(id.raw())?;
        self.buffers.get_mut(slot)?.as_mut()
    }

    fn buffer_ref(&self, id: BufferId) -> Option<&SwBuffer> {
        let slot = Self::id_to_slot(id.raw())?;
        self.buffers.get(slot)?.as_ref()
    }

    fn pipeline_ref(&self, id: PipelineId) -> Option<&PipelineDesc> {
        let slot = Self::id_to_slot(id.raw())?;
        self.pipelines.get(slot)?.as_ref()
    }

    fn image_ref(&self, id: ImageId) -> Option<&SwImage> {
        let slot = Self::id_to_slot(id.raw())?;
        self.images.get(slot)?.as_ref()
    }

    fn current_fb(&self) -> Option<FramebufferTarget> {
        let fb = self
            .framebuffers
            .and_then(|r| r.framebuffers().next())
            .or_else(|| {
                crate::limine::framebuffer_response().and_then(|r| r.framebuffers().next())
            })?;

        Some(FramebufferTarget {
            width: fb.width() as usize,
            height: fb.height() as usize,
        })
    }
}

impl GfxDevice for IntelGfxBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps::minimal_software()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        let len = usize::try_from(desc.size).map_err(|_| Error::OutOfMemory)?;
        let data = vec![0u8; len];
        let raw = Self::alloc_slot(&mut self.buffers, SwBuffer { data, _desc: desc });
        Ok(BufferId::from_raw(raw))
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.buffers.get_mut(slot) {
            *entry = None;
        }
    }

    fn create_shader(&mut self, _desc: ShaderDesc<'_>) -> Result<ShaderId> {
        Err(Error::Unsupported)
    }

    fn destroy_shader(&mut self, _id: ShaderId) {}

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        let raw = Self::alloc_slot(&mut self.pipelines, desc);
        Ok(PipelineId::from_raw(raw))
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.pipelines.get_mut(slot) {
            *entry = None;
        }
    }

    fn create_image(&mut self, desc: ImageDesc) -> Result<ImageId> {
        let bytes = (desc.width as usize)
            .saturating_mul(desc.height as usize)
            .saturating_mul(4);
        let data = vec![0u8; bytes];
        let raw = Self::alloc_slot(
            &mut self.images,
            SwImage {
                width: desc.width,
                height: desc.height,
                format: desc.format,
                data,
            },
        );
        Ok(ImageId::from_raw(raw))
    }

    fn destroy_image(&mut self, id: ImageId) {
        let Some(slot) = Self::id_to_slot(id.raw()) else {
            return;
        };
        if let Some(entry) = self.images.get_mut(slot) {
            *entry = None;
        }
    }

    fn write_image(&mut self, id: ImageId, data: &[u8]) -> Result<()> {
        let slot = Self::id_to_slot(id.raw()).ok_or(Error::NotFound)?;
        let Some(img) = self.images.get_mut(slot).and_then(|x| x.as_mut()) else {
            return Err(Error::NotFound);
        };

        let expected = (img.width as usize)
            .saturating_mul(img.height as usize)
            .saturating_mul(4);
        if data.len() < expected {
            return Err(Error::Invalid);
        }

        if img.format != ImageFormat::Rgba8888 && img.format != ImageFormat::Rgbx8888 {
            return Err(Error::Unsupported);
        }

        img.data[..expected].copy_from_slice(&data[..expected]);
        Ok(())
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        let offset = usize::try_from(offset).map_err(|_| Error::Invalid)?;
        let Some(buf) = self.buffer_mut(id) else {
            return Err(Error::NotFound);
        };
        if offset.saturating_add(data.len()) > buf.data.len() {
            return Err(Error::Invalid);
        }
        buf.data[offset..offset + data.len()].copy_from_slice(data);
        Ok(())
    }

    fn map_buffer(&mut self, id: BufferId, mode: MapMode) -> Result<MappedRange> {
        let _ = mode;
        let Some(buf) = self.buffer_mut(id) else {
            return Err(Error::NotFound);
        };
        Ok(MappedRange {
            ptr: buf.data.as_mut_ptr(),
            len: buf.data.len(),
        })
    }

    fn unmap_buffer(&mut self, _id: BufferId) -> Result<()> {
        Ok(())
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        let Some(fb) = self.current_fb() else {
            return Err(Error::Unsupported);
        };
        if fb.width == 0 || fb.height == 0 {
            return Err(Error::Unsupported);
        }

        for cmd in cmds.commands {
            match *cmd {
                Command::ClearColor { rgb: _ } => {}
                Command::ClearRect {
                    rgb: _,
                    x: _,
                    y: _,
                    width: _,
                    height: _,
                } => {}
                Command::BindPipeline(id) => {
                    if self.pipeline_ref(id).is_none() {
                        return Err(Error::NotFound);
                    }
                }
                Command::BindVertexBuffer { buffer, offset } => {
                    if self.buffer_ref(buffer).is_none() {
                        return Err(Error::NotFound);
                    }
                    let _ = usize::try_from(offset).map_err(|_| Error::Invalid)?;
                }
                Command::BindImage(id) => {
                    if self.image_ref(id).is_none() {
                        return Err(Error::NotFound);
                    }
                }
                Command::SetSampler(_s) => {}
                Command::SetBlend(_b) => {}
                Command::SetViewport(_vp) => {}
                Command::SetScissor(_scissor) => {}
                Command::Draw {
                    vertex_count: _,
                    first_vertex: _,
                } => {}
                Command::Present => {}
            }
        }

        let Some(engine) = self.cmd.as_mut() else {
            return Err(Error::Unsupported);
        };
        engine.begin_batch();
        // Command-streamer proof path: emit work + flush + end + kick every submit.
        // This is the minimum non-stub execution path before full 3D packet programming.
        engine.emit_noop()?;
        engine.emit_cache_flush()?;
        engine.emit_batch_end()?;
        engine.submit_batch()?;

        let id = FenceId::from_raw(self.fence_seq);
        self.fence_seq = self.fence_seq.wrapping_add(1).max(1);
        Ok(id)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        fence.is_valid()
    }

    fn device_idle(&mut self) {}
}

impl GfxPresent for IntelGfxBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        self.swapchain = desc;
        Ok(())
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.swapchain
    }
}

fn first_fb_dimensions(
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
) -> Option<(u32, u32)> {
    let fb = framebuffers
        .and_then(|r| r.framebuffers().next())
        .or_else(|| crate::limine::framebuffer_response().and_then(|r| r.framebuffers().next()))?;
    Some((fb.width() as u32, fb.height() as u32))
}
