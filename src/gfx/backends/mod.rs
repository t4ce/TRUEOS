mod limine_fb;
use trueos_gfx_core::GfxContext;

pub enum Backend {
    LimineFb(limine_fb::LimineFbBackend),
    None(limine_fb::NullBackend),
}

impl Backend {
    pub fn init_auto(
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Self {
        Self::init_limine_fb(framebuffers)
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
            Backend::None(b) => b,
        }
    }
}
