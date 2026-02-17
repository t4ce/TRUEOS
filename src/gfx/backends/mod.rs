use trueos_gfx_core::{
    BufferDesc, BufferId, CommandBuffer, DeviceCaps, Error, Extent2D, FenceId, GfxContext,
    GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, PipelineDesc, PipelineId, Result,
    ShaderDesc, ShaderId, SwapchainDesc,
};

#[cfg(feature = "gfx_virgl")]
use crate::gfx::virtio_gpu_3d;

pub enum Backend {

    #[cfg(feature = "gfx_virgl")]
    Virgl(virtio_gpu_3d::VirglGfxBackend),

    None(NullBackend),
}

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

    fn create_image(&mut self, _desc: ImageDesc) -> Result<ImageId> {
        Err(Error::Unsupported)
    }

    fn destroy_image(&mut self, _id: ImageId) {}

    fn write_image(&mut self, _id: ImageId, _data: &[u8]) -> Result<()> {
        Err(Error::Unsupported)
    }

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

fn ensure_pci_enumerated_if_empty() {
    let mut count: usize = 0;
    crate::pci::with_devices(|list| {
        count = list.len();
    });
    if count == 0 {
        crate::log!("gfx: pci device list empty; enumerating before backend init\n");
        crate::pci::enumerate_impl();
    }
}

impl Backend {
    pub fn init_auto(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Self {
        #[cfg(feature = "gfx_virgl")]
        {
            if let Some(v) = Self::init_virgl(framebuffers) {
                crate::log!("gfx: using virgl backend (auto)\n");
                return v;
            }
            crate::log!("gfx: virgl auto init failed\n");
        }


        crate::log!("gfx: no accelerated backend available; gfx backend inactive\n");
        Backend::None(NullBackend)
    }

    #[cfg(feature = "gfx_virgl")]
    pub fn init_virgl(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        ensure_pci_enumerated_if_empty();
        virtio_gpu_3d::VirglGfxBackend::init(framebuffers).map(|b| Backend::Virgl(b))
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        match self {
            #[cfg(feature = "gfx_virgl")]
            Backend::Virgl(b) => b,

            Backend::None(b) => b,
        }
    }
}
