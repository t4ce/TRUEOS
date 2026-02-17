pub mod backends;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_gpu_3d;

use spin::{Once, Mutex};

use core::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use trueos_gfx_core::GfxContext;
use embassy_time_driver::{now, TICK_HZ};

static SYSTEM: Once<Mutex<System>> = Once::new();
static BACKEND_EPOCH: AtomicU64 = AtomicU64::new(1);
static DISPLAY_SOURCE: AtomicU8 = AtomicU8::new(0);

#[inline]
pub fn backend_epoch() -> u64 {
    BACKEND_EPOCH.load(Ordering::Relaxed)
}

#[inline]
fn bump_backend_epoch() {
    let _ = BACKEND_EPOCH.fetch_add(1, Ordering::Relaxed);
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum DisplaySource {
    Gfx = 0,
    Vga = 1,
}

#[inline]
pub fn display_source() -> DisplaySource {
    match DISPLAY_SOURCE.load(Ordering::Relaxed) {
        1 => DisplaySource::Vga,
        _ => DisplaySource::Gfx,
    }
}

#[inline]
pub fn set_display_source(src: DisplaySource) {
    DISPLAY_SOURCE.store(src as u8, Ordering::Relaxed);
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

    // If gfx was previously initialized without virgl, retry virgl on explicit init calls.
    // Virgl backend init programs scanout via VIRTIO_GPU_CMD_SET_SCANOUT.
    #[cfg(feature = "gfx_virgl")]
    {
        match backend_kind() {
            Some(BackendKind::LimineFb) | Some(BackendKind::None) => {
                let _ = switch_to_virgl();
            }
            _ => {}
        }
    }
}

pub fn with_system<R>(f: impl FnOnce(&mut System) -> R) -> Option<R> {
    let sys = SYSTEM.get()?;

    // `spin::Mutex::lock()` is an unbounded spin. Backend switches can be invoked from the shell;
    // if any code path accidentally re-enters gfx while holding this lock, it can look like a
    // hard BSP freeze. Prefer a bounded wait with a loud log.
    let mut guard = match sys.try_lock() {
        Some(g) => g,
        None => {
            crate::log!("gfx: waiting for SYSTEM lock...\n");

            let timeout_ms: u64 = 2000;
            let hz = TICK_HZ as u64;
            let ticks = if hz == 0 {
                0
            } else {
                ((timeout_ms.saturating_mul(hz) + 999) / 1000).max(1)
            };
            let deadline = now().saturating_add(ticks);

            loop {
                if let Some(g) = sys.try_lock() {
                    break g;
                }
                if ticks != 0 && now() >= deadline {
                    crate::log!("gfx: SYSTEM lock timeout (possible re-entrancy/deadlock)\n");
                    return None;
                }
                crate::wait::spin_step();
            }
        }
    };

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
    crate::log!("gfx: switch_to_virgl: begin\n");

    // Perform backend init outside SYSTEM lock.
    let fbs = with_framebuffers(|f| f).flatten();
    let Some(b) = backends::Backend::init_virgl(fbs) else {
        crate::log!("gfx: switch_to_virgl: init_virgl failed\n");
        return false;
    };

    with_system(|sys| {
        sys.backend = b;
        bump_backend_epoch();
        crate::log!("gfx: switch_to_virgl: ok epoch={}\n", backend_epoch());
        true
    })
    .unwrap_or(false)
}

#[cfg(not(feature = "gfx_virgl"))]
pub fn switch_to_virgl() -> bool {
    false
}

fn set_limine_fb_backend() -> bool {
    crate::log!("gfx: set_limine_fb_backend: begin\n");
    // Snapshot framebuffers without holding SYSTEM across backend init.
    let fbs = with_framebuffers(|f| f).flatten();
    let b = backends::Backend::init_limine_fb(fbs);

    with_system(|sys| {
        sys.backend = b;
        bump_backend_epoch();
        crate::log!("gfx: set_limine_fb_backend: ok epoch={}\n", backend_epoch());
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
    None,
}

pub fn backend_kind() -> Option<BackendKind> {
    with_system(|sys| match &sys.backend {
        backends::Backend::LimineFb(_) => BackendKind::LimineFb,
        #[cfg(feature = "gfx_intel")]
        backends::Backend::Intel(_) => BackendKind::Intel,
        #[cfg(feature = "gfx_virgl")]
        backends::Backend::Virgl(_) => BackendKind::Virgl,
        backends::Backend::None(_) => BackendKind::None,
    })
}

/// Toggle the gfx backend.
///
/// A/B swap cycle:
/// - Virgl (gfx) <-> LimineFb (direct)
///
/// Notes:
/// - `LimineFb` here uses direct framebuffer backend selection.
pub fn toggle_backend() -> BackendKind {
    let Some(kind) = backend_kind() else {
        return BackendKind::None;
    };

    match kind {
        #[cfg(feature = "gfx_intel")]
        BackendKind::Intel => {
            // Keep toggle behavior simple: Intel is not part of the LimineFB<->virgl toggle.
            // If we're on Intel, toggle returns to the known-good LimineFB.
            let _ = set_limine_fb_backend();
            BackendKind::LimineFb
        }
        BackendKind::Virgl => {
            let _ = set_limine_fb_backend();
            BackendKind::LimineFb
        }
        BackendKind::LimineFb => {
            if switch_to_virgl() {
                return BackendKind::Virgl;
            }
            BackendKind::LimineFb
        }
        BackendKind::None => {
            if switch_to_virgl() {
                return BackendKind::Virgl;
            }

            let _ = set_limine_fb_backend();
            BackendKind::LimineFb
        }
    }
}
