pub mod backends;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_gpu_3d;

use spin::{Mutex, Once};

use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, Ordering};
use embassy_time_driver::{now, TICK_HZ};
use trueos_gfx_core::GfxContext;

static SYSTEM: Once<Mutex<System>> = Once::new();
static BACKEND_EPOCH: AtomicU64 = AtomicU64::new(1);
static DISPLAY_SOURCE: AtomicU8 = AtomicU8::new(0);

// Frame completion register.
//
// The command stream owner (e.g. QJS WebGL shim) knows when a logical frame is complete.
// Multiple producers can OR-in their "done" bit; once all required bits are present, the
// frame boundary can be consumed and the register resets for the next frame.
static FRAME_DONE_REQUIRED: AtomicU32 = AtomicU32::new(1);
static FRAME_DONE_BITS: AtomicU32 = AtomicU32::new(0);
static FRAME_DONE_SEQ: AtomicU32 = AtomicU32::new(0);

#[inline]
pub fn frame_done_set_required(mask: u32) {
    // Require at least one bit by default to avoid accidental always-ready if a caller passes 0.
    let req = if mask == 0 { 1 } else { mask };
    FRAME_DONE_REQUIRED.store(req, Ordering::Release);
}

#[inline]
pub fn frame_done_signal(bits: u32) {
    if bits != 0 {
        let _ = FRAME_DONE_BITS.fetch_or(bits, Ordering::AcqRel);
    }
}

#[inline]
pub fn frame_done_is_ready() -> bool {
    let req = FRAME_DONE_REQUIRED.load(Ordering::Acquire);
    let done = FRAME_DONE_BITS.load(Ordering::Acquire);
    (done & req) == req
}

#[inline]
pub fn frame_done_consume_if_ready() -> Option<u32> {
    if !frame_done_is_ready() {
        return None;
    }

    // Consume the boundary: clear done bits and bump sequence.
    FRAME_DONE_BITS.store(0, Ordering::Release);
    let seq = FRAME_DONE_SEQ
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    Some(seq)
}

#[no_mangle]
pub extern "C" fn trueos_cabi_gfx_frame_done_set_required(mask: u32) {
    frame_done_set_required(mask);
}

#[no_mangle]
pub extern "C" fn trueos_cabi_gfx_frame_done_signal(bits: u32) {
    frame_done_signal(bits);
}

#[no_mangle]
pub extern "C" fn trueos_cabi_gfx_frame_done_is_ready() -> u32 {
    if frame_done_is_ready() {
        1
    } else {
        0
    }
}

/// Returns a monotonically increasing sequence when a ready frame boundary is consumed, or 0.
#[no_mangle]
pub extern "C" fn trueos_cabi_gfx_frame_done_consume_if_ready() -> u32 {
    frame_done_consume_if_ready().unwrap_or(0)
}

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
        Self {
            backend,
            framebuffers,
        }
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

pub fn with_framebuffers<R>(
    f: impl FnOnce(Option<&'static ::limine::response::FramebufferResponse>) -> R,
) -> Option<R> {
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

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BackendKind {
    #[cfg(feature = "gfx_intel")]
    Virgl,
    None,
}

pub fn backend_kind() -> Option<BackendKind> {
    with_system(|sys| match &sys.backend {
        #[cfg(feature = "gfx_virgl")]
        backends::Backend::Virgl(_) => BackendKind::Virgl,
        backends::Backend::None(_) => BackendKind::None,
    })
}

/// Toggle the gfx backend.
///
/// A/B cycle between accelerated backends when available.
pub fn toggle_backend() -> BackendKind {
    let Some(kind) = backend_kind() else {
        return BackendKind::None;
    };

    match kind {
        BackendKind::Virgl => BackendKind::Virgl,
        BackendKind::None => {
            if switch_to_virgl() {
                return BackendKind::Virgl;
            }

            BackendKind::None
        }
    }
}
