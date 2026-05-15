pub mod althlasfont;
pub mod backends;
pub mod jpeg_codec;
pub mod lyon;
pub mod mandelbrot;
pub mod png_codec;
mod png_decode_pool;
pub mod screenshot;
pub mod svg;
pub mod virtio_gpu_3d;

use spin::{Mutex, Once};

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use embassy_time_driver::{TICK_HZ, now};
use trueos_gfx_core::GfxContext;

pub(crate) use screenshot::{publish_screenshot_rgba_buffer, screenshot_capture_armed};

static SYSTEM: Once<Mutex<System>> = Once::new();
static CABI_FRAME_LOCK: AtomicBool = AtomicBool::new(false);
static BACKEND_EPOCH: AtomicU64 = AtomicU64::new(1);
static SYSTEM_LOCK_OWNER: AtomicU32 = AtomicU32::new(SystemLockOwner::Unknown as u32);
static SYSTEM_LOCK_OWNER_CPU: AtomicU32 = AtomicU32::new(u32::MAX);
static SYSTEM_LOCK_OWNER_SINCE: AtomicU64 = AtomicU64::new(0);
static BACKEND_READY_PUBLISHED: AtomicBool = AtomicBool::new(false);
static BACKEND_KIND_ATOMIC: AtomicU8 = AtomicU8::new(0);

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

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_frame_done_set_required(mask: u32) {
    frame_done_set_required(mask);
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_frame_done_signal(bits: u32) {
    frame_done_signal(bits);
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_frame_done_is_ready() -> u32 {
    if frame_done_is_ready() { 1 } else { 0 }
}

/// Returns a monotonically increasing sequence when a ready frame boundary is consumed, or 0.
#[unsafe(no_mangle)]
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

pub struct System {
    backend: backends::Backend,
    framebuffers: Option<&'static crate::limine::FramebufferResponse>,
}

fn finalize_backend_init() {
    if BACKEND_READY_PUBLISHED.swap(true, Ordering::AcqRel) {
        return;
    }
    crate::r::readiness::set(crate::r::readiness::GFX_BACKEND_READY);
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum SystemLockOwner {
    Unknown = 0,
    DrawRgbTriangles = 1,
    UploadTexture = 2,
    EndFrame = 3,
    CursorQueryViewport = 4,
    CursorEndFrame = 5,
    DrawMandelbrot = 6,
    FinalizeBackendInit = 7,
    WithFramebuffers = 8,
    IsVirglActive = 9,
    IsIntelActive = 10,
    SwitchToVirgl = 11,
    BackendKind = 12,
}

impl SystemLockOwner {
    #[inline]
    fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::DrawRgbTriangles => "draw_rgb_triangles",
            Self::UploadTexture => "upload_texture",
            Self::EndFrame => "end_frame",
            Self::CursorQueryViewport => "cursor_query_viewport",
            Self::CursorEndFrame => "cursor_end_frame",
            Self::DrawMandelbrot => "draw_mandelbrot",
            Self::FinalizeBackendInit => "finalize_backend_init",
            Self::WithFramebuffers => "with_framebuffers",
            Self::IsVirglActive => "is_virgl_active",
            Self::IsIntelActive => "is_intel_active",
            Self::SwitchToVirgl => "switch_to_virgl",
            Self::BackendKind => "backend_kind",
        }
    }

    #[inline]
    fn from_raw(raw: u32) -> Self {
        match raw {
            x if x == Self::DrawRgbTriangles as u32 => Self::DrawRgbTriangles,
            x if x == Self::UploadTexture as u32 => Self::UploadTexture,
            x if x == Self::EndFrame as u32 => Self::EndFrame,
            x if x == Self::CursorQueryViewport as u32 => Self::CursorQueryViewport,
            x if x == Self::CursorEndFrame as u32 => Self::CursorEndFrame,
            x if x == Self::DrawMandelbrot as u32 => Self::DrawMandelbrot,
            x if x == Self::FinalizeBackendInit as u32 => Self::FinalizeBackendInit,
            x if x == Self::WithFramebuffers as u32 => Self::WithFramebuffers,
            x if x == Self::IsVirglActive as u32 => Self::IsVirglActive,
            x if x == Self::IsIntelActive as u32 => Self::IsIntelActive,
            x if x == Self::SwitchToVirgl as u32 => Self::SwitchToVirgl,
            x if x == Self::BackendKind as u32 => Self::BackendKind,
            _ => Self::Unknown,
        }
    }
}

impl System {
    fn new(
        backend: backends::Backend,
        framebuffers: Option<&'static crate::limine::FramebufferResponse>,
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

#[inline]
fn backend_kind_raw(backend: &backends::Backend) -> u8 {
    match backend {
        backends::Backend::Virgl(_) => 1,
        backends::Backend::None(_) => 2,
    }
}

#[inline]
fn publish_backend_kind(backend: &backends::Backend) {
    BACKEND_KIND_ATOMIC.store(backend_kind_raw(backend), Ordering::Release);
}

#[inline]
fn backend_kind_cached() -> Option<BackendKind> {
    match BACKEND_KIND_ATOMIC.load(Ordering::Acquire) {
        1 => Some(BackendKind::Virgl),
        2 => Some(BackendKind::None),
        _ => None,
    }
}

pub fn init(framebuffers: Option<&'static crate::limine::FramebufferResponse>) {
    let _ = SYSTEM.call_once(|| {
        // if we use this qemu will do whatever it wants. that hurts particularly much
        // because a seemingly harmless init is a contract here:
        // that takes our eyeballs
        let backend = backends::Backend::init_auto(framebuffers);
        let backend_name = match &backend {
            backends::Backend::Virgl(_) => "virgl",
            backends::Backend::None(_) => "none",
        };
        crate::log_info!(target: "gfx"; "gfx: backend={}\n", backend_name);
        publish_backend_kind(&backend);
        if !matches!(backend, backends::Backend::None(_)) {
            BACKEND_READY_PUBLISHED.store(true, Ordering::Release);
            crate::r::readiness::set(crate::r::readiness::GFX_BACKEND_READY);
        }
        Mutex::new(System::new(backend, framebuffers))
    });
}

pub fn with_cabi_frame_lock<R>(f: impl FnOnce() -> R) -> R {
    cabi_frame_lock_begin();
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            cabi_frame_lock_end();
        }
    }
    let _guard = Guard;
    f()
}

#[inline]
pub fn cabi_frame_lock_begin() {
    while CABI_FRAME_LOCK
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        core::hint::spin_loop();
    }
}

#[inline]
pub fn cabi_frame_lock_end() {
    CABI_FRAME_LOCK.store(false, Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_frame_lock_begin() {
    cabi_frame_lock_begin();
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_gfx_frame_lock_end() {
    cabi_frame_lock_end();
}

#[inline]
fn system_lock_requester_id() -> u32 {
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return 0x8000_0000 | vm_id as u32;
    }
    crate::percpu::this_cpu().cpu_index() as u32
}

pub fn with_system_tag<R>(owner: SystemLockOwner, f: impl FnOnce(&mut System) -> R) -> Option<R> {
    let sys = SYSTEM.get()?;
    let waiter_cpu = system_lock_requester_id();

    // `spin::Mutex::lock()` is an unbounded spin. Backend switches can be invoked from the shell;
    // if any code path accidentally re-enters gfx while holding this lock, it can look like a
    // hard BSP freeze. Prefer a bounded wait with a loud log.
    let mut guard = match sys.try_lock() {
        Some(g) => g,
        None => {
            let holder = SYSTEM_LOCK_OWNER.load(Ordering::Acquire);
            let holder_cpu = SYSTEM_LOCK_OWNER_CPU.load(Ordering::Acquire);
            let holder_since = SYSTEM_LOCK_OWNER_SINCE.load(Ordering::Acquire);
            let held_ticks = now().saturating_sub(holder_since);
            let holder_name = SystemLockOwner::from_raw(holder).as_str();
            crate::log!(
                "gfx: waiting for SYSTEM lock requester={} cpu={} holder={} holder_cpu={} held_ticks={}\n",
                owner.as_str(),
                waiter_cpu,
                holder_name,
                holder_cpu,
                held_ticks
            );

            let timeout_ms: u64 = 2000;
            let hz = TICK_HZ;
            let ticks = if hz == 0 {
                0
            } else {
                timeout_ms.saturating_mul(hz).div_ceil(1000).max(1)
            };
            let deadline = now().saturating_add(ticks);

            loop {
                if let Some(g) = sys.try_lock() {
                    break g;
                }
                if ticks != 0 && now() >= deadline {
                    let holder = SYSTEM_LOCK_OWNER.load(Ordering::Acquire);
                    let holder_cpu = SYSTEM_LOCK_OWNER_CPU.load(Ordering::Acquire);
                    let holder_since = SYSTEM_LOCK_OWNER_SINCE.load(Ordering::Acquire);
                    let held_ticks = now().saturating_sub(holder_since);
                    let holder_name = SystemLockOwner::from_raw(holder).as_str();
                    crate::log!(
                        "gfx: SYSTEM lock timeout requester={} cpu={} holder={} holder_cpu={} held_ticks={} (possible re-entrancy/deadlock)\n",
                        owner.as_str(),
                        waiter_cpu,
                        holder_name,
                        holder_cpu,
                        held_ticks
                    );
                    return None;
                }
                crate::wait::spin_step();
            }
        }
    };

    SYSTEM_LOCK_OWNER.store(owner as u32, Ordering::Release);
    SYSTEM_LOCK_OWNER_CPU.store(waiter_cpu, Ordering::Release);
    SYSTEM_LOCK_OWNER_SINCE.store(now(), Ordering::Release);

    let ret = f(&mut guard);

    SYSTEM_LOCK_OWNER.store(SystemLockOwner::Unknown as u32, Ordering::Release);
    SYSTEM_LOCK_OWNER_CPU.store(u32::MAX, Ordering::Release);
    SYSTEM_LOCK_OWNER_SINCE.store(0, Ordering::Release);

    Some(ret)
}

pub fn with_context_tag<R>(
    owner: SystemLockOwner,
    f: impl FnOnce(&mut dyn GfxContext) -> R,
) -> Option<R> {
    with_system_tag(owner, |sys| f(sys.context_mut()))
}

pub fn with_framebuffers<R>(
    f: impl FnOnce(Option<&'static crate::limine::FramebufferResponse>) -> R,
) -> Option<R> {
    with_system_tag(SystemLockOwner::WithFramebuffers, |sys| f(sys.framebuffers))
}

pub fn is_virgl_active() -> bool {
    matches!(backend_kind_cached(), Some(BackendKind::Virgl))
}

pub fn is_intel_active() -> bool {
    false
}

/// Returns whether a virgl-capable virtio-gpu device is currently visible.
///
/// This keeps virgl probing behind the `gfx` API so non-gfx modules do not
/// reach into backend implementation modules directly.
pub fn is_virgl_present_cached() -> bool {
    virtio_gpu_3d::is_present_cached()
}

#[allow(dead_code)]
pub fn switch_to_virgl() -> bool {
    crate::log!("gfx: switch_to_virgl: begin\n");

    // Perform backend init outside SYSTEM lock.
    let fbs = with_framebuffers(|f| f).flatten();
    let Some(b) = backends::Backend::init_virgl(fbs) else {
        crate::log!("gfx: switch_to_virgl: init_virgl failed\n");
        return false;
    };

    let switched = with_system_tag(SystemLockOwner::SwitchToVirgl, |sys| {
        sys.backend = b;
        publish_backend_kind(&sys.backend);
        bump_backend_epoch();
        crate::log!("gfx: switch_to_virgl: ok epoch={}\n", backend_epoch());
        true
    })
    .unwrap_or(false);

    if switched {
        finalize_backend_init();
    }

    switched
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BackendKind {
    Virgl,
    None,
}
