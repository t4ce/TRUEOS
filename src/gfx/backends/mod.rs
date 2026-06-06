use trueos_gfx_core::{
    BufferDesc, BufferId, CommandBuffer, DeviceCaps, Error, Extent2D, FenceId, GfxContext,
    GfxDevice, GfxPresent, ImageDesc, ImageFormat, ImageId, ImageRegion, PipelineDesc, PipelineId,
    Result, ShaderDesc, ShaderId, SwapchainDesc,
};

pub mod intel;
#[cfg(feature = "trueos_rdp")]
pub mod rdp;

use crate::gfx::virtio_gpu_3d;

pub enum Backend {
    #[cfg(not(feature = "trueos_rdp"))]
    Intel(intel::IntelGfxBackend),
    #[cfg(not(feature = "trueos_rdp"))]
    Virgl(virtio_gpu_3d::VirglGfxBackend),
    #[cfg(feature = "trueos_rdp")]
    IntelRdp(IntelRdpBackend),
    #[cfg(feature = "trueos_rdp")]
    VirglRdp(VirglRdpBackend),
    #[cfg(feature = "trueos_rdp")]
    Rdp(rdp::RdpGfxBackend),
    None(NullBackend),
}

#[cfg(feature = "trueos_rdp")]
pub type IntelRdpBackend = RdpMirrorBackend<intel::IntelGfxBackend>;

#[cfg(feature = "trueos_rdp")]
pub type VirglRdpBackend = RdpMirrorBackend<virtio_gpu_3d::VirglGfxBackend>;

#[cfg(feature = "trueos_rdp")]
pub struct RdpMirrorBackend<T> {
    primary: T,
    rdp: rdp::RdpGfxBackend,
}

#[cfg(feature = "trueos_rdp")]
impl<T> RdpMirrorBackend<T> {
    fn new(primary: T, rdp: rdp::RdpGfxBackend) -> Self {
        Self { primary, rdp }
    }
}

pub struct NullBackend;

impl GfxDevice for NullBackend {
    fn caps(&self) -> DeviceCaps {
        DeviceCaps {
            supports_rgbx8888: false,
            supports_host_visible_buffers: false,
            supports_scissor: false,
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

    fn write_image_region(
        &mut self,
        _id: ImageId,
        _region: ImageRegion,
        _data: &[u8],
    ) -> Result<()> {
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
    pub fn init_auto(framebuffers: Option<&'static crate::limine::FramebufferResponse>) -> Self {
        ensure_pci_enumerated_if_empty();

        #[cfg(feature = "trueos_rdp")]
        {
            if let Some(intel) = intel::IntelGfxBackend::init(framebuffers) {
                crate::log_info!(target: "gfx"; "gfx: using intel+rdp backend (auto)\n");
                crate::log!("gfx: using intel+rdp backend (auto)\n");
                return Backend::IntelRdp(RdpMirrorBackend::new(
                    intel,
                    rdp::RdpGfxBackend::init(framebuffers),
                ));
            }
            if let Some(virgl) = virtio_gpu_3d::VirglGfxBackend::init(framebuffers) {
                crate::log_info!(target: "gfx"; "gfx: using virgl+rdp backend (auto)\n");
                return Backend::VirglRdp(RdpMirrorBackend::new(
                    virgl,
                    rdp::RdpGfxBackend::init(framebuffers),
                ));
            }
            crate::log_info!(target: "gfx"; "gfx: virgl auto init failed; using rdp backend\n");
            return Backend::Rdp(rdp::RdpGfxBackend::init(framebuffers));
        }

        #[cfg(not(feature = "trueos_rdp"))]
        {
            if let Some(intel) = intel::IntelGfxBackend::init(framebuffers) {
                crate::log_info!(target: "gfx"; "gfx: using intel backend (auto)\n");
                return Backend::Intel(intel);
            }
            if let Some(v) = Self::init_virgl(framebuffers) {
                crate::log_info!(target: "gfx"; "gfx: using virgl backend (auto)\n");
                return v;
            }
            crate::log_info!(target: "gfx"; "gfx: virgl auto init failed\n");

            crate::log_info!(target: "gfx"; "gfx: no accelerated backend available; gfx backend inactive\n");
            Backend::None(NullBackend)
        }
    }

    pub fn init_virgl(
        framebuffers: Option<&'static crate::limine::FramebufferResponse>,
    ) -> Option<Self> {
        ensure_pci_enumerated_if_empty();
        let virgl = virtio_gpu_3d::VirglGfxBackend::init(framebuffers)?;
        #[cfg(feature = "trueos_rdp")]
        {
            Some(Backend::VirglRdp(RdpMirrorBackend::new(
                virgl,
                rdp::RdpGfxBackend::init(framebuffers),
            )))
        }
        #[cfg(not(feature = "trueos_rdp"))]
        {
            Some(Backend::Virgl(virgl))
        }
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        match self {
            #[cfg(not(feature = "trueos_rdp"))]
            Backend::Intel(b) => b,
            #[cfg(not(feature = "trueos_rdp"))]
            Backend::Virgl(b) => b,
            #[cfg(feature = "trueos_rdp")]
            Backend::IntelRdp(b) => b,
            #[cfg(feature = "trueos_rdp")]
            Backend::VirglRdp(b) => b,
            #[cfg(feature = "trueos_rdp")]
            Backend::Rdp(b) => b,
            Backend::None(b) => b,
        }
    }

    pub(crate) fn intel_image_gpgpu_surface(
        &self,
        id: ImageId,
    ) -> Option<crate::intel::gpgpu::GpgpuRgba8Surface> {
        match self {
            #[cfg(not(feature = "trueos_rdp"))]
            Backend::Intel(b) => b.image_gpgpu_surface(id),
            #[cfg(feature = "trueos_rdp")]
            Backend::IntelRdp(b) => b.primary.image_gpgpu_surface(id),
            _ => None,
        }
    }

    pub(crate) fn intel_image_gpgpu_mask_surface(
        &self,
        id: ImageId,
    ) -> Option<crate::intel::gpgpu::GpgpuMask8Surface> {
        match self {
            #[cfg(not(feature = "trueos_rdp"))]
            Backend::Intel(b) => b.image_gpgpu_mask_surface(id),
            #[cfg(feature = "trueos_rdp")]
            Backend::IntelRdp(b) => b.primary.image_gpgpu_mask_surface(id),
            _ => None,
        }
    }
}

#[cfg(feature = "trueos_rdp")]
impl<T: GfxDevice> GfxDevice for RdpMirrorBackend<T> {
    fn caps(&self) -> DeviceCaps {
        self.primary.caps()
    }

    fn create_buffer(&mut self, desc: BufferDesc) -> Result<BufferId> {
        let id = self.primary.create_buffer(desc)?;
        match self.rdp.create_buffer(desc) {
            Ok(mirror_id) if mirror_id == id => Ok(id),
            Ok(mirror_id) => {
                self.primary.destroy_buffer(id);
                self.rdp.destroy_buffer(mirror_id);
                Err(Error::Invalid)
            }
            Err(err) => {
                self.primary.destroy_buffer(id);
                Err(err)
            }
        }
    }

    fn destroy_buffer(&mut self, id: BufferId) {
        self.primary.destroy_buffer(id);
        self.rdp.destroy_buffer(id);
    }

    fn create_shader(&mut self, desc: ShaderDesc<'_>) -> Result<ShaderId> {
        let id = self.primary.create_shader(desc)?;
        let _ = self.rdp.create_shader(desc);
        Ok(id)
    }

    fn destroy_shader(&mut self, id: ShaderId) {
        self.primary.destroy_shader(id);
        self.rdp.destroy_shader(id);
    }

    fn create_pipeline(&mut self, desc: PipelineDesc) -> Result<PipelineId> {
        let id = self.primary.create_pipeline(desc)?;
        match self.rdp.create_pipeline(desc) {
            Ok(mirror_id) if mirror_id == id => Ok(id),
            Ok(mirror_id) => {
                self.primary.destroy_pipeline(id);
                self.rdp.destroy_pipeline(mirror_id);
                Err(Error::Invalid)
            }
            Err(err) => {
                self.primary.destroy_pipeline(id);
                Err(err)
            }
        }
    }

    fn destroy_pipeline(&mut self, id: PipelineId) {
        self.primary.destroy_pipeline(id);
        self.rdp.destroy_pipeline(id);
    }

    fn create_image(&mut self, desc: ImageDesc) -> Result<ImageId> {
        let id = self.primary.create_image(desc)?;
        match self.rdp.create_image(desc) {
            Ok(mirror_id) if mirror_id == id => Ok(id),
            Ok(mirror_id) => {
                self.primary.destroy_image(id);
                self.rdp.destroy_image(mirror_id);
                Err(Error::Invalid)
            }
            Err(err) => {
                self.primary.destroy_image(id);
                Err(err)
            }
        }
    }

    fn destroy_image(&mut self, id: ImageId) {
        self.primary.destroy_image(id);
        self.rdp.destroy_image(id);
    }

    fn write_image(&mut self, id: ImageId, data: &[u8]) -> Result<()> {
        self.primary.write_image(id, data)?;
        self.rdp.write_image(id, data)
    }

    fn write_image_region(&mut self, id: ImageId, region: ImageRegion, data: &[u8]) -> Result<()> {
        self.primary.write_image_region(id, region, data)?;
        self.rdp.write_image_region(id, region, data)
    }

    fn write_buffer(&mut self, id: BufferId, offset: u64, data: &[u8]) -> Result<()> {
        self.primary.write_buffer(id, offset, data)?;
        self.rdp.write_buffer(id, offset, data)
    }

    fn submit(&mut self, cmds: CommandBuffer<'_>) -> Result<FenceId> {
        static SUBMIT_LOGS: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
        let n = SUBMIT_LOGS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if n < 16 {
            crate::log!(
                "gfx/rdp-mirror: submit n={} cmds={} primary_then_rdp=1 rdp_fallback=1\n",
                n + 1,
                cmds.commands.len()
            );
        }
        let primary_res = self.primary.submit(cmds);
        let rdp_res = self.rdp.submit(cmds);
        match (primary_res, rdp_res) {
            (Ok(fence), Ok(_)) => Ok(fence),
            (Ok(fence), Err(err)) => {
                if n < 16 {
                    crate::log!("gfx/rdp-mirror: rdp submit failed err={:?}\n", err);
                }
                Ok(fence)
            }
            (Err(primary_err), Ok(fence)) => {
                if n < 16 {
                    crate::log!(
                        "gfx/rdp-mirror: primary submit failed err={:?}; rdp submit ok\n",
                        primary_err
                    );
                }
                Ok(fence)
            }
            (Err(primary_err), Err(rdp_err)) => {
                if n < 16 {
                    crate::log!(
                        "gfx/rdp-mirror: submit failed primary_err={:?} rdp_err={:?}\n",
                        primary_err,
                        rdp_err
                    );
                }
                Err(primary_err)
            }
        }
    }

    fn poll(&mut self, fence: FenceId) -> bool {
        self.primary.poll(fence)
    }

    fn device_idle(&mut self) {
        self.primary.device_idle();
        self.rdp.device_idle();
    }
}

#[cfg(feature = "trueos_rdp")]
impl<T: GfxPresent> GfxPresent for RdpMirrorBackend<T> {
    fn configure_swapchain(&mut self, desc: SwapchainDesc) -> Result<()> {
        self.primary.configure_swapchain(desc)?;
        self.rdp.configure_swapchain(desc)
    }

    fn swapchain_desc(&self) -> SwapchainDesc {
        self.primary.swapchain_desc()
    }

    fn display_refresh_millihz(&mut self) -> Option<u32> {
        self.primary.display_refresh_millihz()
    }
}
