mod limine_fb;
use trueos_gfx_core::GfxContext;

#[cfg(feature = "gfx_intel")]
mod intel_gpu;

#[cfg(feature = "gfx_virgl")]
use crate::gfx::virtio_gpu_3d;

pub enum Backend {
    LimineFb(limine_fb::LimineFbBackend),

    #[cfg(feature = "gfx_intel")]
    Intel(intel_gpu::IntelGpuBackend),

    #[cfg(feature = "gfx_virgl")]
    Virgl(virtio_gpu_3d::VirglGfxBackend),

    None(limine_fb::NullBackend),
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
            crate::log!("gfx: virgl auto init failed; fallback to limine framebuffer\n");
        }
        Self::init_limine_fb(framebuffers)
    }

    #[cfg(feature = "gfx_virgl")]
    pub fn init_virgl(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        ensure_pci_enumerated_if_empty();
        virtio_gpu_3d::VirglGfxBackend::init(framebuffers).map(|b| Backend::Virgl(b))
    }

    #[cfg(feature = "gfx_intel")]
    pub fn init_intel(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Option<Self> {
        ensure_pci_enumerated_if_empty();
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

            Backend::None(b) => b,
        }
    }
}
