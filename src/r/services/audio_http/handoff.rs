//! Host-owned complete clips and bounded progressive PCM handoff.
use super::{SINK, sink};

fn submit(bytes: &[u8]) -> i32 {
    // The clip must outlive the caller and its VM heap/teardown.
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut sink = SINK.lock();
        let _ = sink.expire(super::monotonic_millis());
        sink.submit(bytes).map_or_else(|error| error, |_| 0)
    })
}

/// VMCALL input is restricted to PCM in the calling VM's heap.
/// This avoids large shared-page transfers while the paused caller keeps the
/// complete input allocation alive until we have copied it.
pub(crate) fn submit_guest(vm_id: u8, address: u64, bytes: u64) -> i32 {
    let (Ok(address), Ok(bytes)) = (usize::try_from(address), usize::try_from(bytes)) else {
        return -22;
    };
    if let Err(error) = sink::validate_size(bytes) {
        return error;
    }
    let Some(pcm) = guest_pcm(vm_id, address, bytes) else {
        return -14;
    };
    submit(pcm)
}

fn guest_pcm<'a>(vm_id: u8, address: usize, bytes: usize) -> Option<&'a [u8]> {
    let (start, end) = crate::allocators::hv_guest_heap_bounds(vm_id)?;
    let limit = address.checked_add(bytes)?;
    if address == 0 || address < start || limit > end {
        return None;
    }
    Some(unsafe { core::slice::from_raw_parts(address as *const u8, bytes) })
}

/// Submit one complete stereo s16le/48 kHz clip, at most 120 seconds. Returns 0
/// after a host-owned copy, or successful discard if both producer slots are occupied.
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

fn validate_stream_request(action: u64, stream: u64, bytes: usize) -> Result<(), i32> {
    match action {
        0 if stream == 0 && bytes == 0 => Ok(()),
        1 if (1..=i64::MAX as u64).contains(&stream) => sink::validate_append_size(bytes),
        2 | 3 if (1..=i64::MAX as u64).contains(&stream) && bytes == 0 => Ok(()),
        _ => Err(-22),
    }
}

fn stream_for(owner: sink::Owner, action: u64, stream: u64, pcm: &[u8]) -> i64 {
    if let Err(error) = validate_stream_request(action, stream, pcm.len()) {
        return i64::from(error);
    }
    crate::allocators::with_host_alloc_domain_strong(|| {
        let mut sink = SINK.lock();
        let now_ms = super::monotonic_millis();
        let _ = sink.expire(now_ms);
        let result = match action {
            0 => sink.begin(owner, now_ms).map(|id| id as i64),
            1 => sink.append(owner, stream, pcm, now_ms).map(|()| 0),
            2 => sink.finish(owner, stream).map(|()| 0),
            3 => sink.abort(owner, stream).map(|()| 0),
            _ => unreachable!("validated audio stream action"),
        };
        result.unwrap_or_else(i64::from)
    })
}

pub(crate) fn stream_guest(vm_id: u8, action: u64, stream: u64, pcm: &[u8]) -> i64 {
    let Some(generation) = crate::hv::vm_run_generation(vm_id) else {
        return -14;
    };
    stream_for(sink::Owner::Vm { id: vm_id, generation }, action, stream, pcm)
}

pub(crate) fn release_vm(id: u8, generation: u64) {
    crate::allocators::with_host_alloc_domain_strong(|| {
        SINK.lock().release(sink::Owner::Vm { id, generation });
    });
}

/// Begin (0), append (1), finish (2), or abort (3) one stereo s16le/48 kHz
/// stream. Begin returns a positive token or -16 if both slots are occupied.
/// Append accepts complete frames, at most 64 KiB per call. The other actions
/// take an empty payload. Finished PCM remains available as a retained clip.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_http_audio_stream_v1(
    action: u32,
    stream: u64,
    pcm_ptr: *const u8,
    pcm_bytes: usize,
) -> i64 {
    let action = u64::from(action);
    if let Err(error) = validate_stream_request(action, stream, pcm_bytes) {
        return i64::from(error);
    }
    if let Some(vm_id) = crate::hv::current_hull_guest_context_vm_id() {
        let pcm = if pcm_bytes == 0 {
            &[]
        } else if let Some(pcm) = guest_pcm(vm_id, pcm_ptr as usize, pcm_bytes) {
            pcm
        } else {
            return -14;
        };
        let (status, value) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_HTTP_AUDIO_STREAM_V1,
            action,
            stream,
            pcm,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK { value as i64 } else { -5 };
    }
    if let Some(vm_id) = crate::hv::current_guest_execution_context_vm_id() {
        let pcm = if pcm_bytes == 0 {
            &[]
        } else if let Some(pcm) = guest_pcm(vm_id, pcm_ptr as usize, pcm_bytes) {
            pcm
        } else {
            return -14;
        };
        return stream_guest(vm_id, action, stream, pcm);
    }
    let Some(pcm) = crate::native_abi::abi_read_bytes(pcm_ptr, pcm_bytes) else {
        return -14;
    };
    stream_for(sink::Owner::Host, action, stream, pcm)
}
