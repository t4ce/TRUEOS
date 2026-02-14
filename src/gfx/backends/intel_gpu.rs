use trueos_gfx_core::{
    BufferDesc, BufferId, CommandBuffer, DeviceCaps, FenceId, GfxDevice, GfxPresent, MapMode,
    MappedRange, PipelineDesc, PipelineId, Result, ShaderDesc, ShaderId, SwapchainDesc,
};

use super::limine_fb::LimineFbBackend;

const INTEL_VENDOR_ID: u16 = 0x8086;
// Alder Lake iGPU (as reported by your lspci snippet).
const INTEL_ALDERLAKE_IGPU_DEVICE_ID: u16 = 0x4680;

/// Intel GPU backend wiring stub.
///
/// This intentionally does **not** implement real Intel GPU programming yet.
/// It exists to:
/// - provide a stable backend slot (`Backend::IntelGpu`)
/// - exercise the backend switching and gfx-core plumbing
/// - keep Intel-specific probe/init logic out of higher layers
///
/// For now it delegates rendering to the Limine framebuffer backend while logging
/// the presence/BDF of a matching Intel display device.
pub struct IntelGpuBackend {
    _bdf: (u8, u8, u8),
    _device_id: u16,
    inner: LimineFbBackend,
}

impl IntelGpuBackend {
    pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) -> Option<Self> {
        let dev = find_intel_display_device()?;
        crate::log!(
            "gfx/intel_gpu: probed intel display dev bdf={:02X}:{:02X}.{} vid=0x{:04X} did=0x{:04X} class=0x{:02X}/0x{:02X} pi=0x{:02X}\n",
            dev.bus,
            dev.slot,
            dev.function,
            dev.vendor,
            dev.device,
            dev.class,
            dev.subclass,
            dev.prog_if
        );

        let inner = LimineFbBackend::from_limine(framebuffers)?;
        Some(Self {
            _bdf: (dev.bus, dev.slot, dev.function),
            _device_id: dev.device,
            inner,
        })
    }
}

fn find_intel_display_device() -> Option<crate::pci::PciDevice> {
    let mut found: Option<crate::pci::PciDevice> = None;
    crate::pci::with_devices(|list| {
        for dev in list {
            if dev.vendor != INTEL_VENDOR_ID {
                continue;
            }
            // Prefer the explicit ADL iGPU id when present.
            if dev.device == INTEL_ALDERLAKE_IGPU_DEVICE_ID {
                found = Some(*dev);
                break;
            }
            // Fallback: any display controller.
            if dev.class == 0x03 {
                found = Some(*dev);
                break;
            }
        }
    });
    found
}

impl GfxDevice for IntelGpuBackend {
    fn caps(&self) -> DeviceCaps {
        self.inner.caps()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        self.inner.create_buffer(desc)
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        self.inner.destroy_buffer(id)
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId> {
        self.inner.create_shader(desc)
    }

    fn destroy_shader(&mut self, id: ShaderId) {
        self.inner.destroy_shader(id)
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        self.inner.create_pipeline(desc)
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        self.inner.destroy_pipeline(id)
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        self.inner.write_buffer(id, offset, data)
    }

    fn map_buffer(&mut self, id: BufferId, mode: MapMode) -> Result<MappedRange> {
        self.inner.map_buffer(id, mode)
    }

    fn unmap_buffer(&mut self, id: BufferId) -> Result<()> {
        self.inner.unmap_buffer(id)
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        self.inner.submit(cmds)
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        self.inner.poll(fence)
    }

    fn device_idle(&mut self) {
        self.inner.device_idle()
    }
}

impl GfxPresent for IntelGpuBackend {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        self.inner.configure_swapchain(desc)
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.inner.swapchain_desc()
    }
}
