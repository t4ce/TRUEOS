pub mod backends;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_gpu_3d;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_limine;

use spin::{Once, Mutex};

use core::sync::atomic::{AtomicU64, Ordering};
use trueos_gfx_core::GfxContext;

static SYSTEM: Once<Mutex<System>> = Once::new();
static BACKEND_EPOCH: AtomicU64 = AtomicU64::new(1);

#[inline]
pub fn backend_epoch() -> u64 {
    BACKEND_EPOCH.load(Ordering::Relaxed)
}

#[inline]
fn bump_backend_epoch() {
    let _ = BACKEND_EPOCH.fetch_add(1, Ordering::Relaxed);
}

pub struct System {
    backend: backends::Backend,
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
}

impl System {
    fn new(
        backend: backends::Backend,
        framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
    ) -> Self {
        Self { backend, framebuffers }
    }

    pub fn context_mut(&mut self) -> &mut dyn GfxContext {
        self.backend.context_mut()
    }
}

pub fn init(framebuffers: Option<&'static ::limine::response::FramebufferResponse>) {
    let _ = SYSTEM.call_once(|| {
        let backend = backends::Backend::init_auto(framebuffers);
        Mutex::new(System::new(backend, framebuffers))
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

pub fn with_framebuffers<R>(f: impl FnOnce(Option<&'static ::limine::response::FramebufferResponse>) -> R) -> Option<R> {
    with_system(|sys| f(sys.framebuffers))
}

#[cfg(feature = "gfx_virgl")]
#[allow(dead_code)]
pub fn switch_to_virgl() -> bool {
    crate::log!("gfx: switch_to_virgl: disabled (A/B swap mode)\n");
    false
}

#[cfg(not(feature = "gfx_virgl"))]
pub fn switch_to_virgl() -> bool {
    false
}

#[cfg(feature = "gfx_virgl")]
pub fn switch_to_virtio_sw() -> bool {
    with_system(|sys| {
        crate::log!("gfx: switch_to_virtio_sw: begin\n");
        let Some(b) = backends::Backend::init_virtio_sw() else {
            crate::log!("gfx: switch_to_virtio_sw: init_virtio_sw failed\n");
            return false;
        };
        sys.backend = b;
        bump_backend_epoch();
        crate::log!("gfx: switch_to_virtio_sw: ok epoch={}\n", backend_epoch());
        true
    })
    .unwrap_or(false)
}

#[cfg(not(feature = "gfx_virgl"))]
pub fn switch_to_virtio_sw() -> bool {
    false
}

pub fn switch_to_limine_fb() -> bool {
    with_system(|sys| {
        crate::log!("gfx: switch_to_limine_fb: begin\n");
        sys.backend = backends::Backend::init_limine_fb(sys.framebuffers);
        bump_backend_epoch();
        crate::log!("gfx: switch_to_limine_fb: ok epoch={}\n", backend_epoch());
        true
    })
    .unwrap_or(false)
}

#[cfg(feature = "gfx_intel")]
#[allow(dead_code)]
pub fn switch_to_intel() -> bool {
    with_system(|sys| {
        let Some(b) = backends::Backend::init_intel(sys.framebuffers) else {
            return false;
        };
        sys.backend = b;
        bump_backend_epoch();
        true
    })
    .unwrap_or(false)
}

#[cfg(not(feature = "gfx_intel"))]
pub fn switch_to_intel() -> bool {
    false
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BackendKind {
    LimineFb,
    #[cfg(feature = "gfx_intel")]
    Intel,
    Virgl,
    VirtioSw,
    None,
}

pub fn backend_kind() -> Option<BackendKind> {
    with_system(|sys| match &sys.backend {
        backends::Backend::LimineFb(_) => BackendKind::LimineFb,
        #[cfg(feature = "gfx_intel")]
        backends::Backend::Intel(_) => BackendKind::Intel,
        #[cfg(feature = "gfx_virgl")]
        backends::Backend::Virgl(_) => BackendKind::Virgl,
        #[cfg(feature = "gfx_virgl")]
        backends::Backend::VirtioSw(_) => BackendKind::VirtioSw,
        backends::Backend::None(_) => BackendKind::None,
    })
}

/// Toggle the gfx backend.
///
/// A/B swap cycle:
/// - VirtioSw (gfx) <-> LimineFb (visible via virtio_limine mirror)
///
/// Notes:
/// - `LimineFb` here is intended to be made visible via `virtio_limine` (virtio scanout backed
///   by the Limine framebuffer + periodic transfer/flush), not by "making VGA the display".
pub fn toggle_backend() -> BackendKind {
    let Some(kind) = backend_kind() else {
        return BackendKind::None;
    };

    match kind {
        #[cfg(feature = "gfx_intel")]
        BackendKind::Intel => {
            // Keep toggle behavior simple: Intel is not part of the LimineFB<->virgl toggle.
            // If we're on Intel, toggle returns to the known-good LimineFB.
            let _ = switch_to_limine_fb();
            BackendKind::LimineFb
        }
        BackendKind::Virgl => {
            // Virgl is disabled in A/B swap mode, but handle the state anyway.
            // Prefer moving into the virtio-owned software scanout.
            if switch_to_virtio_sw() {
                return BackendKind::VirtioSw;
            }
            let _ = switch_to_limine_fb();
            BackendKind::LimineFb
        }
        BackendKind::VirtioSw => {
            let _ = switch_to_limine_fb();
            BackendKind::LimineFb
        }
        BackendKind::LimineFb => {
            if switch_to_virtio_sw() {
                return BackendKind::VirtioSw;
            }
            BackendKind::LimineFb
        }
        BackendKind::None => {
            if switch_to_virtio_sw() {
                return BackendKind::VirtioSw;
            }

            let _ = switch_to_limine_fb();
            BackendKind::LimineFb
        }
    }
}
