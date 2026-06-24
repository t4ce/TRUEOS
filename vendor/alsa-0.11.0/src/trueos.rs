//! TRUEOS-native backend hook for the ALSA-shaped Rust facade.
//!
//! This module deliberately avoids `alsa-sys` and `libasound`. The kernel
//! installs a Rust backend that is expected to route PCM playback to TRUEOS'
//! audio core.

use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use crate::pcm::State;

pub type BackendResult<T> = core::result::Result<T, i32>;

#[derive(Clone, Copy, Debug)]
pub struct PcmBackend {
    pub open_playback: fn() -> BackendResult<usize>,
    pub close: fn(usize),
    pub start: fn(usize) -> BackendResult<()>,
    pub drop_stream: fn(usize) -> BackendResult<()>,
    pub drain: fn(usize) -> BackendResult<()>,
    pub write_i16_interleaved: fn(usize, *const i16, usize) -> BackendResult<usize>,
    pub writable_frames: fn(usize) -> BackendResult<usize>,
    pub queued_frames: fn(usize) -> BackendResult<usize>,
    pub buffer_frames: fn(usize) -> BackendResult<usize>,
    pub state: fn(usize) -> State,
}

static PCM_BACKEND: AtomicPtr<PcmBackend> = AtomicPtr::new(ptr::null_mut());

pub fn install_pcm_backend(backend: &'static PcmBackend) {
    PCM_BACKEND.store(backend as *const PcmBackend as *mut PcmBackend, Ordering::Release);
}

pub(crate) fn pcm_backend() -> Option<&'static PcmBackend> {
    let ptr = PCM_BACKEND.load(Ordering::Acquire);
    if ptr.is_null() {
        None
    } else {
        Some(unsafe { &*ptr })
    }
}
