pub mod backends;
pub mod demo;

use spin::{Once, Mutex};

use trueos_gfx_core::{GfxDevice, GfxPresent};

static SYSTEM: Once<Mutex<System>> = Once::new();

pub struct System {
    backend: backends::Backend,
}

impl System {
    fn new(backend: backends::Backend) -> Self {
        Self { backend }
    }

    pub fn device_mut(&mut self) -> &mut dyn GfxDevice {
        self.backend.device_mut()
    }

    pub fn present_mut(&mut self) -> &mut dyn GfxPresent {
        self.backend.present_mut()
    }
}

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let _ = SYSTEM.call_once(|| {
        let backend = backends::Backend::init_limine_fb(framebuffers);
        Mutex::new(System::new(backend))
    });
}

pub fn with_system<R>(f: impl FnOnce(&mut System) -> R) -> Option<R> {
    let sys = SYSTEM.get()?;
    let mut guard = sys.lock();
    Some(f(&mut *guard))
}

pub fn with_device<R>(f: impl FnOnce(&mut dyn GfxDevice, &mut dyn GfxPresent) -> R) -> Option<R> {
    with_system(|sys| {
        let dev = sys.device_mut();
        let pres = sys.present_mut();
        f(dev, pres)
    })
}
