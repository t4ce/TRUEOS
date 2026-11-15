//! Single completed-clip handoff. No stream handles, mixer or producer endpoint.
use super::{SINK, sink};

fn submit(bytes: &[u8]) -> i32 {
    // The clip must outlive the caller and its VM heap/teardown.
    crate::allocators::with_host_alloc_domain_strong(|| {
        SINK.lock().submit(bytes).map_or_else(|error| error, |_| 0)
    })
}

/// VMCALL input is restricted to the calling VM's heap; TTSTT hands off its Vec.
/// This avoids large shared-page transfers while the paused caller keeps the
/// complete input allocation alive until we have copied it.
pub(crate) fn submit_guest(vm_id: u8, address: u64, bytes: u64) -> i32 {
    let (Ok(address), Ok(bytes)) = (usize::try_from(address), usize::try_from(bytes)) else {
        return -22;
    };
    if let Err(error) = sink::validate_size(bytes) {
        return error;
    }
    let Some((start, end)) = crate::allocators::hv_guest_heap_bounds(vm_id) else {
        return -14;
    };
    let Some(limit) = address.checked_add(bytes) else {
        return -14;
    };
    if address < start || limit > end {
        return -14;
    }
    let pcm = unsafe { core::slice::from_raw_parts(address as *const u8, bytes) };
    submit(pcm)
}

/// Submit one complete stereo s16le/48 kHz clip, at most 120 seconds. Returns 0
/// after a host-owned copy, or successful discard if another clip is playing.
/// Negative values reject invalid size (-22), oversize (-27), invalid memory
/// (-14), allocation failure (-12), or transport failure (-5).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_http_audio_clip_submit_v1(
    pcm_ptr: *const u8,
    pcm_bytes: usize,
) -> i32 {
    if let Err(error) = sink::validate_size(pcm_bytes) {
        return error;
    }
    if pcm_ptr.is_null() {
        return -14;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, value) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_HTTP_AUDIO_CLIP_SUBMIT_V1,
            pcm_ptr as u64,
            pcm_bytes as u64,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            value as i64 as i32
        } else {
            -5
        };
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        return submit_guest(vm_id, pcm_ptr as u64, pcm_bytes as u64);
    }
    let Some(pcm) = crate::native_abi::abi_read_bytes(pcm_ptr, pcm_bytes) else {
        return -14;
    };
    submit(pcm)
}
