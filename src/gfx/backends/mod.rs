mod limine_fb;
use trueos_gfx_core::GfxContext;

#[cfg(feature = "gfx_intel")]
mod intel_gpu;

#[cfg(feature = "gfx_virgl")]
use crate::gfx::virtio_gpu_3d;

#[cfg(feature = "gfx_virgl")]
mod virtio_sw;

pub enum Backend {
    LimineFb(limine_fb::LimineFbBackend),

    #[cfg(feature = "gfx_intel")]
    Intel(intel_gpu::IntelGpuBackend),

    #[cfg(feature = "gfx_virgl")]
    Virgl(virtio_gpu_3d::VirglGfxBackend),

    #[cfg(feature = "gfx_virgl")]
    VirtioSw(virtio_sw::VirtioSwBackend),

    None(limine_fb::NullBackend),
}

impl Backend {
    pub fn init_auto(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Self {
        // Default is intentionally conservative: use the known-good Limine framebuffer backend.
        // GPU backends (virtio-gpu/virgl, Xe, ...) are opt-in and switched explicitly.
        Self::init_limine_fb(framebuffers)
    }

    #[cfg(feature = "gfx_virgl")]
    pub fn init_virgl(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        virtio_gpu_3d::VirglGfxBackend::init(framebuffers).map(|b| Backend::Virgl(b))
    }

    #[cfg(feature = "gfx_virgl")]
    pub fn init_virtio_sw() -> Option<Self> {
        virtio_sw::VirtioSwBackend::init().map(Backend::VirtioSw)
    }

    #[cfg(feature = "gfx_intel")]
    pub fn init_intel(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        intel_gpu::IntelGpuBackend::init(framebuffers).map(Backend::Intel)
    }

    pub fn init_limine_fb(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) -> Self {
        limine_fb::LimineFbBackend::from_limine(framebuffers)
            .map(|b| {
                crate::log!("gfx: using limine framebuffer backend\n");
                Backend::LimineFb(b)
            })
            .unwrap_or_else(|| {
                crate::log!("gfx: limine framebuffer unavailable; no gfx backend active\n");
                Backend::None(limine_fb::NullBackend)
            })
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        match self {
            Backend::LimineFb(b) => b,

            #[cfg(feature = "gfx_intel")]
            Backend::Intel(b) => b,

            #[cfg(feature = "gfx_virgl")]
            Backend::Virgl(b) => b,

            #[cfg(feature = "gfx_virgl")]
            Backend::VirtioSw(b) => b,

            Backend::None(b) => b,
        }
    }
}
