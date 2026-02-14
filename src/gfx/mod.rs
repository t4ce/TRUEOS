pub mod backends;
pub mod demo;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_gpu_3d;

use spin::{Once, Mutex};

use trueos_gfx_core::GfxContext;

static SYSTEM: Once<Mutex<System>> = Once::new();

pub struct System {
    backend: backends::Backend,
}

impl System {
    fn new(backend: backends::Backend) -> Self {
        Self { backend }
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        self.backend.context_mut()
    }
}

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let _ = SYSTEM.call_once(|| {
        let backend = backends::Backend::init_auto(framebuffers);
        Mutex::new(System::new(backend))
    });
}

pub fn with_system<R>(f: impl FnOnce(&mut System) -> R) -> Option<R> {
    let sys = SYSTEM.get()?;
    let mut guard = sys.lock();
    Some(f(&mut *guard))
}

pub fn with_context<R>(f: impl FnOnce(&mut dyn GfxContext) -> R) -> Option<R> {
    with_system(|sys| f(sys.context_mut()))
}
