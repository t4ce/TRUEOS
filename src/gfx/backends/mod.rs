mod limine_fb;
#[cfg(feature = "gfx_virtio_gpu")]
mod virtio_gpu_2d;

use trueos_gfx_core::{GfxContext, GfxDevice, GfxPresent};

pub enum Backend {
    LimineFb(limine_fb::LimineFbBackend),
    #[cfg(feature = "gfx_virtio_gpu")]
    VirtioGpu2d(virtio_gpu_2d::VirtioGpu2dBackend),
    None(limine_fb::NullBackend),
}

impl Backend {
    pub fn init_auto(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Self {
        #[cfg(feature = "gfx_virtio_gpu")]
        {
            if let Some(b) = virtio_gpu_2d::VirtioGpu2dBackend::init_first() {
                crate::log!("gfx: using virtio-gpu 2D backend\n");
                return Backend::VirtioGpu2d(b);
            }

            crate::log!("gfx: virtio-gpu 2D backend unavailable; falling back\n");
        }

        Self::init_limine_fb(framebuffers)
    }

    pub fn init_limine_fb(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) -> Self {
        limine_fb::LimineFbBackend::from_limine(framebuffers)
            .map(Backend::LimineFb)
            .unwrap_or_else(|| {
                crate::log!("gfx: limine framebuffer unavailable; no gfx backend active\n");
                Backend::None(limine_fb::NullBackend)
            })
    }

    pub fn device_mut(&mut self) -> &mut dyn GfxDevice {
        match self {
            Backend::LimineFb(b) => b,
            #[cfg(feature = "gfx_virtio_gpu")]
            Backend::VirtioGpu2d(b) => b,
            Backend::None(b) => b,
        }
    }

    pub fn present_mut(&mut self) -> &mut dyn GfxPresent {
        match self {
            Backend::LimineFb(b) => b,
            #[cfg(feature = "gfx_virtio_gpu")]
            Backend::VirtioGpu2d(b) => b,
            Backend::None(b) => b,
        }
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        match self {
            Backend::LimineFb(b) => b,
            #[cfg(feature = "gfx_virtio_gpu")]
            Backend::VirtioGpu2d(b) => b,
            Backend::None(b) => b,
        }
    }
}
