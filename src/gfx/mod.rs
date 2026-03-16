pub mod backends;
pub mod cursor;
#[cfg(feature = "gfx_intel")]
pub mod intel;
pub mod jpeg_codec;
pub mod loadscreen;
pub mod lyon;
pub mod png_codec;
pub mod screenshot;
pub mod svg;
pub mod text;
#[cfg(feature = "gfx_virgl")]
pub mod virtio_gpu_3d;

use alloc::vec;
use spin::{Mutex, Once};

use core::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use embassy_time_driver::{TICK_HZ, now};
use trueos_gfx_core::GfxContext;

pub(crate) use screenshot::publish_virgl_image_buffer;

static SYSTEM: Once<Mutex<System>> = Once::new();
static CPU_BACKBUFFER: Once<Mutex<Option<CpuBackbuffer>>> = Once::new();
static CABI_FRAME_LOCK: Mutex<()> = Mutex::new(());
static BACKEND_EPOCH: AtomicU64 = AtomicU64::new(1);
static PRESENT_OWNER: AtomicU8 = AtomicU8::new(0);
static SYSTEM_LOCK_OWNER: AtomicU32 = AtomicU32::new(SystemLockOwner::Unknown as u32);
static SYSTEM_LOCK_OWNER_CPU: AtomicU32 = AtomicU32::new(u32::MAX);
static SYSTEM_LOCK_OWNER_SINCE: AtomicU64 = AtomicU64::new(0);

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
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
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
        }
    }
}

struct CpuBackbuffer {
    width: usize,
    height: usize,
    pixels: vec::Vec<u32>,
}

fn alloc_cpu_backbuffer(
    framebuffers: Option<&'static ::limine::response::FramebufferResponse>,
) -> Option<CpuBackbuffer> {
    let fb = framebuffers.and_then(|resp| resp.framebuffers().next())?;
    let width = fb.width() as usize;
    let height = fb.height() as usize;
    if width == 0 || height == 0 {
        return None;
    }
    let len = width.saturating_mul(height);
    Some(CpuBackbuffer {
        width,
        height,
        pixels: vec![0u32; len],
    })
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
    let _ = CPU_BACKBUFFER.call_once(|| Mutex::new(alloc_cpu_backbuffer(framebuffers)));
    let _ = SYSTEM.call_once(|| {
        // if we use this qemu will do whatever it wants. that hurts particularly much
        // because a seemingly harmless init is a contract here:
        // that takes our eyeballs
        let backend = backends::Backend::init_auto(framebuffers);
        let backend_name = match &backend {
            #[cfg(feature = "gfx_virgl")]
            backends::Backend::Virgl(_) => "virgl",
            #[cfg(feature = "gfx_intel")]
            backends::Backend::Intel(_) => "intel",
            backends::Backend::None(_) => "none",
        };
        crate::log!("gfx: backend={}\n", backend_name);
        if !matches!(&backend, backends::Backend::None(_)) {
            crate::v::readiness::set(crate::v::readiness::GFX_BACKEND_READY);
        }
        Mutex::new(System::new(backend, framebuffers))
    });
}

pub fn with_cpu_backbuffer_mut<R>(f: impl FnOnce(&mut [u32], usize, usize) -> R) -> Option<R> {
    let bb = CPU_BACKBUFFER.get()?;
    let mut guard = bb.lock();
    let buf = guard.as_mut()?;
    Some(f(buf.pixels.as_mut_slice(), buf.width, buf.height))
}

pub fn with_cabi_frame_lock<R>(f: impl FnOnce() -> R) -> R {
    let _guard = CABI_FRAME_LOCK.lock();
    f()
}

pub fn cpu_backbuffer_dimensions() -> Option<(usize, usize)> {
    let bb = CPU_BACKBUFFER.get()?;
    let guard = bb.lock();
    let buf = guard.as_ref()?;
    Some((buf.width, buf.height))
}

pub fn with_system<R>(f: impl FnOnce(&mut System) -> R) -> Option<R> {
    with_system_tag(SystemLockOwner::Unknown, f)
}

pub fn with_system_tag<R>(owner: SystemLockOwner, f: impl FnOnce(&mut System) -> R) -> Option<R> {
    let sys = SYSTEM.get()?;
    let waiter_cpu = crate::percpu::this_cpu().cpu_index() as u32;

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
            let holder_name = match holder {
                x if x == SystemLockOwner::DrawRgbTriangles as u32 => {
                    SystemLockOwner::DrawRgbTriangles.as_str()
                }
                x if x == SystemLockOwner::UploadTexture as u32 => {
                    SystemLockOwner::UploadTexture.as_str()
                }
                x if x == SystemLockOwner::EndFrame as u32 => SystemLockOwner::EndFrame.as_str(),
                x if x == SystemLockOwner::CursorQueryViewport as u32 => {
                    SystemLockOwner::CursorQueryViewport.as_str()
                }
                x if x == SystemLockOwner::CursorEndFrame as u32 => {
                    SystemLockOwner::CursorEndFrame.as_str()
                }
                _ => SystemLockOwner::Unknown.as_str(),
            };
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
                    let holder_name = match holder {
                        x if x == SystemLockOwner::DrawRgbTriangles as u32 => {
                            SystemLockOwner::DrawRgbTriangles.as_str()
                        }
                        x if x == SystemLockOwner::UploadTexture as u32 => {
                            SystemLockOwner::UploadTexture.as_str()
                        }
                        x if x == SystemLockOwner::EndFrame as u32 => {
                            SystemLockOwner::EndFrame.as_str()
                        }
                        x if x == SystemLockOwner::CursorQueryViewport as u32 => {
                            SystemLockOwner::CursorQueryViewport.as_str()
                        }
                        x if x == SystemLockOwner::CursorEndFrame as u32 => {
                            SystemLockOwner::CursorEndFrame.as_str()
                        }
                        _ => SystemLockOwner::Unknown.as_str(),
                    };
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

#[inline]
pub fn cursor_overlay_tick() -> i32 {
    cursor::cursor_overlay_tick()
}

pub fn with_context<R>(f: impl FnOnce(&mut dyn GfxContext) -> R) -> Option<R> {
    with_context_tag(SystemLockOwner::Unknown, f)
}

pub fn with_context_tag<R>(
    owner: SystemLockOwner,
    f: impl FnOnce(&mut dyn GfxContext) -> R,
) -> Option<R> {
    with_system_tag(owner, |sys| f(sys.context_mut()))
}

pub fn with_framebuffers<R>(
    f: impl FnOnce(Option<&'static ::limine::response::FramebufferResponse>) -> R,
) -> Option<R> {
    with_system(|sys| f(sys.framebuffers))
}

#[cfg(feature = "gfx_virgl")]
pub fn is_virgl_active() -> bool {
    with_system(|sys| matches!(sys.backend, backends::Backend::Virgl(_))).unwrap_or(false)
}

/// Returns whether a virgl-capable virtio-gpu device is currently visible.
///
/// This keeps virgl probing behind the `gfx` API so non-gfx modules do not
/// reach into backend implementation modules directly.
pub fn is_virgl_present_cached() -> bool {
    #[cfg(feature = "gfx_virgl")]
    {
        return virtio_gpu_3d::is_present_cached();
    }

    #[cfg(not(feature = "gfx_virgl"))]
    {
        false
    }
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
        crate::v::readiness::set(crate::v::readiness::GFX_BACKEND_READY);
        crate::log!("gfx: switch_to_virgl: ok epoch={}\n", backend_epoch());
        true
    })
    .unwrap_or(false)
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BackendKind {
    #[cfg(feature = "gfx_virgl")]
    Virgl,
    #[cfg(feature = "gfx_intel")]
    Intel,
    None,
}

pub fn backend_kind() -> Option<BackendKind> {
    with_system(|sys| match &sys.backend {
        #[cfg(feature = "gfx_virgl")]
        backends::Backend::Virgl(_) => BackendKind::Virgl,
        #[cfg(feature = "gfx_intel")]
        backends::Backend::Intel(_) => BackendKind::Intel,
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
        #[cfg(feature = "gfx_intel")]
        BackendKind::Intel => BackendKind::Intel,
        #[cfg(feature = "gfx_virgl")]
        BackendKind::Virgl => BackendKind::Virgl,
        BackendKind::None => {
            #[cfg(feature = "gfx_virgl")]
            if switch_to_virgl() {
                return BackendKind::Virgl;
            }

            BackendKind::None
        }
    }
}
