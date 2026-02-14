mod limine_fb;

use trueos_gfx_core::{GfxDevice, GfxPresent};

pub enum Backend {
    LimineFb(limine_fb::LimineFbBackend),
    None(limine_fb::NullBackend),
}

impl Backend {
    pub fn init_limine_fb(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) -> Self {
        limine_fb::LimineFbBackend::from_limine(framebuffers)
            .map(Backend::LimineFb)
            .unwrap_or(Backend::None(limine_fb::NullBackend))
    }

    pub fn device_mut(&mut self) -> &mut dyn GfxDevice {
        match self {
            Backend::LimineFb(b) => b,
            Backend::None(b) => b,
        }
    }

    pub fn present_mut(&mut self) -> &mut dyn GfxPresent {
        match self {
            Backend::LimineFb(b) => b,
            Backend::None(b) => b,
        }
    }
}
