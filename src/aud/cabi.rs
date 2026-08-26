use alloc::vec::Vec;
use spin::Mutex;

use crate::hda;

const TRUEOS_AUDIO_HANDLE: u32 = 1;
const TRUEOS_AUDIO_BUFFER_FRAMES: usize = hda::PCM_SAMPLE_RATE_HZ as usize * 30;

const TRUEOS_AUDIO_FORMAT_S16LE: u32 = 1;
const TRUEOS_AUDIO_FRAME_BYTES: usize = core::mem::size_of::<i16>() * hda::PCM_CHANNELS;

const EIO: i32 = 5;
const EBADF: i32 = 9;
const EBUSY: i32 = 16;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENODEV: i32 = 19;
const ENOSPC: i32 = 28;

const STATE_CLOSED: i32 = 0;
const STATE_PREPARED: i32 = 1;
const STATE_RUNNING: i32 = 2;
const STATE_DISCONNECTED: i32 = 3;

static AUDIO_CABI_STATE: Mutex<AudioCabiState> = Mutex::new(AudioCabiState::new());

struct AudioCabiState {
    open: bool,
    running: bool,
}

impl AudioCabiState {
    const fn new() -> Self {
        Self {
            open: false,
            running: false,
        }
    }
}

fn valid_handle(handle: u32) -> bool {
    handle == TRUEOS_AUDIO_HANDLE
}

fn ensure_supported(format: u32, channels: u32, rate_hz: u32) -> Result<(), i32> {
    if format != TRUEOS_AUDIO_FORMAT_S16LE {
        return Err(EINVAL);
    }
    if channels != hda::PCM_CHANNELS as u32 {
        return Err(EINVAL);
    }
    if rate_hz != hda::PCM_SAMPLE_RATE_HZ {
        return Err(EINVAL);
    }
    if !hda::is_initialized() {
        return Err(ENODEV);
    }
    Ok(())
}

fn ensure_supported_shape(format: u32, channels: u32, rate_hz: u32) -> Result<(), i32> {
    if format != TRUEOS_AUDIO_FORMAT_S16LE {
        return Err(EINVAL);
    }
    if channels != hda::PCM_CHANNELS as u32 {
        return Err(EINVAL);
    }
    if rate_hz != hda::PCM_SAMPLE_RATE_HZ {
        return Err(EINVAL);
    }
    Ok(())
}

fn write_samples(label: &'static str, samples: &[i16]) -> Result<usize, i32> {
    if samples.len() % hda::PCM_CHANNELS != 0 {
        crate::log_warn!(
            target: "audio";
            "audio-cabi: host-write bad-shape label={} samples={} channels={}\n",
            label,
            samples.len(),
            hda::PCM_CHANNELS
        );
        return Err(EINVAL);
    }
    if samples.is_empty() {
        crate::log_trace!(
            target: "audio";
            "audio-cabi: host-write empty label={}\n",
            label
        );
        return Ok(0);
    }

    crate::aud::pcm_lane::submit_i16_stereo_48k(label, Vec::from(samples)).map_err(
        |err| match err {
            crate::aud::pcm_lane::PcmLaneError::QueueFull => EBUSY,
            crate::aud::pcm_lane::PcmLaneError::BadShape => EINVAL,
            crate::aud::pcm_lane::PcmLaneError::EmptyBuffer => EIO,
        },
    )
}

fn guest_audio_write_samples(samples: &[i16]) -> Result<usize, i32> {
    if samples.len() % hda::PCM_CHANNELS != 0 {
        crate::log_warn!(
            target: "audio";
            "audio-cabi: guest-write bad-shape samples={} channels={}\n",
            samples.len(),
            hda::PCM_CHANNELS
        );
        return Err(EINVAL);
    }
    if samples.is_empty() {
        crate::log_trace!(target: "audio"; "audio-cabi: guest-write empty\n");
        return Ok(0);
    }

    let bytes = unsafe {
        core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), core::mem::size_of_val(samples))
    };
    crate::log_trace!(
        target: "audio";
        "audio-cabi: guest-write begin samples={} frames={} bytes={} frame_bytes={} max_payload={}\n",
        samples.len(),
        samples.len() / hda::PCM_CHANNELS,
        bytes.len(),
        TRUEOS_AUDIO_FRAME_BYTES,
        trueos_vm::vmcall::PAYLOAD_CAP & !(TRUEOS_AUDIO_FRAME_BYTES - 1)
    );
    crate::audio_probe!(
        "audio-cabi: guest-write begin samples={} frames={} bytes={} frame_bytes={} max_payload={}\n",
        samples.len(),
        samples.len() / hda::PCM_CHANNELS,
        bytes.len(),
        TRUEOS_AUDIO_FRAME_BYTES,
        trueos_vm::vmcall::PAYLOAD_CAP & !(TRUEOS_AUDIO_FRAME_BYTES - 1)
    );
    let mut written_frames = 0usize;
    let mut offset = 0usize;
    let max_payload = trueos_vm::vmcall::PAYLOAD_CAP & !(TRUEOS_AUDIO_FRAME_BYTES - 1);
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(max_payload), bytes.len())
            & !(TRUEOS_AUDIO_FRAME_BYTES - 1);
        if end <= offset {
            crate::log_error!(
                target: "audio";
                "audio-cabi: guest-write chunk-align-failed offset={} bytes={} max_payload={}\n",
                offset,
                bytes.len(),
                max_payload
            );
            return Err(EIO);
        }
        let chunk = &bytes[offset..end];
        crate::log_trace!(
            target: "audio";
            "audio-cabi: guest-write vmcall offset={} chunk_bytes={} chunk_frames={}\n",
            offset,
            chunk.len(),
            chunk.len() / TRUEOS_AUDIO_FRAME_BYTES
        );
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_AUDIO_WRITE_I16_STEREO_48K,
            0,
            0,
            chunk,
            &mut [],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            crate::log_error!(
                target: "audio";
                "audio-cabi: guest-write vmcall-status status={} offset={} chunk_bytes={}\n",
                status,
                offset,
                chunk.len()
            );
            return Err(EIO);
        }
        let frames = (rc as i64) as isize;
        if frames < 0 {
            let err = (-frames) as i32;
            if err == EBUSY && written_frames != 0 {
                crate::log_warn!(
                    target: "audio";
                    "audio-cabi: guest-write queue-busy partial_frames={} offset={} chunk_bytes={}\n",
                    written_frames,
                    offset,
                    chunk.len()
                );
                return Ok(written_frames);
            }
            crate::log_warn!(
                target: "audio";
                "audio-cabi: guest-write failed err={} offset={} chunk_bytes={} partial_frames={}\n",
                err,
                offset,
                chunk.len(),
                written_frames
            );
            crate::audio_probe!(
                "audio-cabi: guest-write failed err={} offset={} chunk_bytes={} partial_frames={}\n",
                err,
                offset,
                chunk.len(),
                written_frames
            );
            return Err(err);
        }
        if frames == 0 {
            crate::log_warn!(
                target: "audio";
                "audio-cabi: guest-write zero-progress offset={} chunk_bytes={} partial_frames={}\n",
                offset,
                chunk.len(),
                written_frames
            );
            break;
        }
        written_frames = written_frames.saturating_add(frames as usize);
        offset = end;
    }
    crate::log_trace!(
        target: "audio";
        "audio-cabi: guest-write done requested_frames={} written_frames={} bytes={}\n",
        samples.len() / hda::PCM_CHANNELS,
        written_frames,
        bytes.len()
    );
    crate::audio_probe!(
        "audio-cabi: guest-write done requested_frames={} written_frames={} bytes={}\n",
        samples.len() / hda::PCM_CHANNELS,
        written_frames,
        bytes.len()
    );
    Ok(written_frames)
}

fn guest_audio_stop() {
    crate::log_info!(target: "audio"; "audio-cabi: guest-stop vmcall\n");
    crate::audio_probe!("audio-cabi: guest-stop vmcall\n");
    let _ = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_AUDIO_STOP, 0, 0);
}

fn guest_audio_pending_frames() -> usize {
    let (status, frames) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_AUDIO_PENDING_FRAMES, 0, 0);
    crate::log_trace!(
        target: "audio";
        "audio-cabi: guest-pending status={} frames={}\n",
        status,
        frames
    );
    if status == trueos_vm::vmcall::STATUS_OK {
        frames as usize
    } else {
        0
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_open_playback(
    format: u32,
    channels: u32,
    rate_hz: u32,
    out_handle: *mut u32,
) -> i32 {
    if out_handle.is_null() {
        crate::log_warn!(target: "audio"; "audio-cabi: open failed err=EFAULT null-handle\n");
        return -EFAULT;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if let Err(err) = ensure_supported_shape(format, channels, rate_hz) {
            crate::log_warn!(
                target: "audio";
                "audio-cabi: guest-open unsupported format={} channels={} rate={} err={}\n",
                format,
                channels,
                rate_hz,
                err
            );
            return -err;
        }
        let mut state = AUDIO_CABI_STATE.lock();
        if state.open {
            crate::log_warn!(target: "audio"; "audio-cabi: guest-open busy\n");
            return -EBUSY;
        }
        guest_audio_stop();
        state.open = true;
        state.running = false;
        unsafe {
            out_handle.write(TRUEOS_AUDIO_HANDLE);
        }
        crate::log_info!(
            target: "audio";
            "audio-cabi: guest-open ok handle={} format=s16le/stereo/48k buffer_frames={}\n",
            TRUEOS_AUDIO_HANDLE,
            TRUEOS_AUDIO_BUFFER_FRAMES
        );
        crate::audio_probe!(
            "audio-cabi: guest-open ok handle={} format=s16le/stereo/48k buffer_frames={}\n",
            TRUEOS_AUDIO_HANDLE,
            TRUEOS_AUDIO_BUFFER_FRAMES
        );
        return 0;
    }
    if let Err(err) = ensure_supported(format, channels, rate_hz) {
        crate::log_warn!(
            target: "audio";
            "audio-cabi: host-open unsupported format={} channels={} rate={} err={}\n",
            format,
            channels,
            rate_hz,
            err
        );
        return -err;
    }

    let mut state = AUDIO_CABI_STATE.lock();
    if state.open {
        crate::log_warn!(target: "audio"; "audio-cabi: host-open busy\n");
        return -EBUSY;
    }
    crate::aud::pcm_lane::request_stop();
    state.open = true;
    state.running = false;
    crate::aud::pcm_lane::set_paused(false);

    unsafe {
        out_handle.write(TRUEOS_AUDIO_HANDLE);
    }
    crate::log_info!(
        target: "audio";
        "audio-cabi: host-open ok handle={} format=s16le/stereo/48k buffer_frames={}\n",
        TRUEOS_AUDIO_HANDLE,
        TRUEOS_AUDIO_BUFFER_FRAMES
    );
    crate::audio_probe!(
        "audio-cabi: host-open ok handle={} format=s16le/stereo/48k buffer_frames={}\n",
        TRUEOS_AUDIO_HANDLE,
        TRUEOS_AUDIO_BUFFER_FRAMES
    );
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_close(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_audio_stop();
    }
    let mut state = AUDIO_CABI_STATE.lock();
    state.open = false;
    state.running = false;
    crate::log_info!(target: "audio"; "audio-cabi: close handle={}\n", handle);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_start(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }

    let mut state = AUDIO_CABI_STATE.lock();
    if !state.open {
        return -ENODEV;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        state.running = true;
        crate::log_info!(target: "audio"; "audio-cabi: guest-start handle={}\n", handle);
        crate::audio_probe!("audio-cabi: guest-start handle={}\n", handle);
        return 0;
    }
    crate::aud::pcm_lane::set_paused(false);
    state.running = true;
    crate::log_info!(target: "audio"; "audio-cabi: host-start handle={}\n", handle);
    crate::audio_probe!("audio-cabi: host-start handle={}\n", handle);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_drop(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }

    let mut state = AUDIO_CABI_STATE.lock();
    if !state.open {
        return -ENODEV;
    }
    state.running = false;
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_audio_stop();
        crate::log_info!(target: "audio"; "audio-cabi: guest-drop handle={}\n", handle);
        return 0;
    }
    crate::aud::pcm_lane::request_stop();
    crate::log_info!(target: "audio"; "audio-cabi: host-drop handle={}\n", handle);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_set_paused(handle: u32, paused: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        crate::log_info!(
            target: "audio";
            "audio-cabi: guest-set-paused handle={} paused={} noop=host-owned\n",
            handle,
            paused
        );
        return 0;
    }
    crate::aud::pcm_lane::set_paused(paused != 0);
    crate::log_info!(
        target: "audio";
        "audio-cabi: host-set-paused handle={} paused={}\n",
        handle,
        paused
    );
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_paused(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return 0;
    }
    i32::from(crate::aud::pcm_lane::paused())
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_set_volume_percent(handle: u32, percent: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let percent = percent.min(100);
        crate::log_info!(
            target: "audio";
            "audio-cabi: guest-set-volume handle={} percent={}\n",
            handle,
            percent
        );
        let (status, applied) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_AUDIO_SET_VOLUME_PERCENT,
            percent as u64,
            0,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return -EIO;
        }
        return applied.min(100) as i32;
    }
    let applied = crate::aud::pcm_lane::set_volume_percent(percent.min(u16::MAX as u32) as u16);
    crate::log_info!(
        target: "audio";
        "audio-cabi: host-set-volume handle={} percent={} applied={}\n",
        handle,
        percent,
        applied
    );
    applied as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_volume_percent(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }

    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, percent) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_AUDIO_VOLUME_PERCENT, 0, 0);
        if status != trueos_vm::vmcall::STATUS_OK {
            return -EIO;
        }
        return percent.min(100) as i32;
    }
    crate::aud::pcm_lane::volume_percent() as i32
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_drain(handle: u32, _timeout_ms: u64) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return 0;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_write_i16_interleaved(
    handle: u32,
    samples_ptr: *const i16,
    sample_count: usize,
) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if samples_ptr.is_null() && sample_count != 0 {
        return -(EFAULT as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }

    let samples = if sample_count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(samples_ptr, sample_count) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return match guest_audio_write_samples(samples) {
            Ok(frames) => {
                AUDIO_CABI_STATE.lock().running = true;
                frames as isize
            }
            Err(err) => -(err as isize),
        };
    }
    match write_samples("blueprint-audio-pcm", samples) {
        Ok(frames) => {
            AUDIO_CABI_STATE.lock().running = true;
            frames as isize
        }
        Err(err) => -(err as isize),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_write_i16_stereo_48k(
    samples_ptr: *const i16,
    sample_count: usize,
) -> isize {
    if samples_ptr.is_null() && sample_count != 0 {
        return -(EFAULT as isize);
    }
    if !hda::is_initialized() {
        return -(ENODEV as isize);
    }

    let samples = if sample_count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(samples_ptr, sample_count) }
    };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return match guest_audio_write_samples(samples) {
            Ok(frames) => frames as isize,
            Err(err) => -(err as isize),
        };
    }
    if !hda::is_initialized() {
        return -(ENODEV as isize);
    }
    match write_samples("blueprint-audio-direct", samples) {
        Ok(frames) => frames as isize,
        Err(err) => -(err as isize),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_queued_frames(handle: u32) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_audio_pending_frames() as isize;
    }
    crate::aud::pcm_lane::pending_frames() as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_buffer_frames(handle: u32) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }
    TRUEOS_AUDIO_BUFFER_FRAMES as isize
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_state(handle: u32) -> i32 {
    if !valid_handle(handle) {
        return STATE_DISCONNECTED;
    }

    let state = AUDIO_CABI_STATE.lock();
    match (state.open, state.running) {
        (true, true) => STATE_RUNNING,
        (true, false) => STATE_PREPARED,
        (false, _) => STATE_CLOSED,
    }
}

/// Copy a read-only snapshot of the host HDA playback endpoint.
///
/// This is intentionally independent of opening a stream: callers can see
/// that audio is unavailable before trying to open it. In Blueprint guests
/// the snapshot comes from the host through a dedicated vmcall, never from
/// guest-local audio state. An unavailable endpoint is a successful all-zero
/// snapshot with `ready == 0`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_endpoint_caps_v1(
    out: *mut crate::hda::HdaEndpointCapabilitiesV1,
    out_size: usize,
) -> i32 {
    if out.is_null() {
        return -EFAULT;
    }
    if out_size != core::mem::size_of::<crate::hda::HdaEndpointCapabilitiesV1>() {
        return -EINVAL;
    }
    let caps = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut wire = [0u8; core::mem::size_of::<crate::hda::HdaEndpointCapabilitiesV1>()];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_AUDIO_ENDPOINT_CAPS_V1,
            0,
            0,
            &[],
            &mut wire,
        );
        if status != trueos_vm::vmcall::STATUS_OK || rc != 0 {
            return -EIO;
        }
        unsafe {
            core::ptr::read_unaligned(
                wire.as_ptr()
                    .cast::<crate::hda::HdaEndpointCapabilitiesV1>(),
            )
        }
    } else {
        crate::hda::endpoint_capabilities_v1()
    };
    unsafe { core::ptr::write_unaligned(out, caps) };
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_monitor_start_cursor(preroll_samples: usize) -> u64 {
    crate::aud::esynth::live_pcm_stream_start_cursor(preroll_samples).unwrap_or(u64::MAX)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_monitor_read_i16_since(
    cursor: u64,
    out_ptr: *mut i16,
    out_cap: usize,
    out_next_cursor: *mut u64,
) -> isize {
    if out_ptr.is_null() && out_cap != 0 {
        return -(EFAULT as isize);
    }
    if out_next_cursor.is_null() {
        return -(EFAULT as isize);
    }

    let mut samples = Vec::with_capacity(out_cap);
    let Some(next) = crate::aud::esynth::live_pcm_read_since(cursor, &mut samples, out_cap) else {
        return -(ENODEV as isize);
    };

    let out = if out_cap == 0 {
        &mut []
    } else {
        unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) }
    };
    let count = samples.len().min(out.len());
    out[..count].copy_from_slice(&samples[..count]);
    unsafe {
        out_next_cursor.write(next);
    }
    count as isize
}

fn native_error_code(error: crate::aud::native_engine::Error) -> i32 {
    match error {
        crate::aud::native_engine::Error::Invalid => EINVAL,
        crate::aud::native_engine::Error::MissingSample => ENODEV,
        crate::aud::native_engine::Error::NoSpace => ENOSPC,
    }
}

fn render_native_host_v1(
    header: &crate::aud::native_engine::NativeBlockHeaderV1,
    commands: &[crate::aud::native_engine::NativeRenderCommandV1],
) -> isize {
    let pcm = match crate::aud::native_engine::render_block_v1(header, commands) {
        Ok(pcm) => pcm,
        Err(error) => return -(native_error_code(error) as isize),
    };
    match crate::aud::pcm_lane::submit_i16_stereo_48k("blueprint-audio-native-v1", pcm) {
        Ok(frames) => frames as isize,
        Err(crate::aud::pcm_lane::PcmLaneError::QueueFull) => -(EBUSY as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::BadShape) => -(EINVAL as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::EmptyBuffer) => -(EIO as isize),
    }
}

fn guest_native_render_v1(
    header: &crate::aud::native_engine::NativeBlockHeaderV1,
    commands: &[crate::aud::native_engine::NativeRenderCommandV1],
) -> isize {
    let header_bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(header).cast::<u8>(),
            core::mem::size_of_val(header),
        )
    };
    let command_bytes = unsafe {
        core::slice::from_raw_parts(
            commands.as_ptr().cast::<u8>(),
            core::mem::size_of_val(commands),
        )
    };
    let payload_len = header_bytes.len().saturating_add(command_bytes.len());
    if payload_len > trueos_vm::vmcall::PAYLOAD_CAP {
        return -(EINVAL as isize);
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(header_bytes);
    payload.extend_from_slice(command_bytes);
    let (status, rc) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_AUDIO_NATIVE_RENDER_V1,
        commands.len() as u64,
        0,
        &payload,
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -(EIO as isize);
    }
    (rc as i64) as isize
}

fn render_native_host_v2(
    header: &crate::aud::native_engine::NativeBlockHeaderV2,
    commands: &[crate::aud::native_engine::NativeRenderCommandV2],
) -> isize {
    let pcm = match crate::aud::native_engine::render_block_v2(header, commands) {
        Ok(pcm) => pcm,
        Err(error) => return -(native_error_code(error) as isize),
    };
    match crate::aud::pcm_lane::submit_i16_stereo_48k("blueprint-audio-native-v2", pcm) {
        Ok(frames) => frames as isize,
        Err(crate::aud::pcm_lane::PcmLaneError::QueueFull) => -(EBUSY as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::BadShape) => -(EINVAL as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::EmptyBuffer) => -(EIO as isize),
    }
}

fn guest_native_render_v2(
    header: &crate::aud::native_engine::NativeBlockHeaderV2,
    commands: &[crate::aud::native_engine::NativeRenderCommandV2],
) -> isize {
    let header_bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(header).cast::<u8>(),
            core::mem::size_of_val(header),
        )
    };
    let command_bytes = unsafe {
        core::slice::from_raw_parts(
            commands.as_ptr().cast::<u8>(),
            core::mem::size_of_val(commands),
        )
    };
    let payload_len = header_bytes.len().saturating_add(command_bytes.len());
    if payload_len > trueos_vm::vmcall::PAYLOAD_CAP {
        return -(EINVAL as isize);
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(header_bytes);
    payload.extend_from_slice(command_bytes);
    let (status, rc) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_AUDIO_NATIVE_RENDER_V2,
        commands.len() as u64,
        0,
        &payload,
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -(EIO as isize);
    }
    (rc as i64) as isize
}

fn render_native_host_v3(
    header: &crate::aud::native_engine::NativeBlockHeaderV3,
    commands: &[crate::aud::native_engine::NativeRenderCommandV3],
) -> isize {
    let pcm = match crate::aud::native_engine::render_block_v3(header, commands) {
        Ok(pcm) => pcm,
        Err(error) => return -(native_error_code(error) as isize),
    };
    match crate::aud::pcm_lane::submit_i16_stereo_48k("blueprint-audio-native-v3", pcm) {
        Ok(frames) => frames as isize,
        Err(crate::aud::pcm_lane::PcmLaneError::QueueFull) => -(EBUSY as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::BadShape) => -(EINVAL as isize),
        Err(crate::aud::pcm_lane::PcmLaneError::EmptyBuffer) => -(EIO as isize),
    }
}

fn guest_native_render_v3(
    header: &crate::aud::native_engine::NativeBlockHeaderV3,
    commands: &[crate::aud::native_engine::NativeRenderCommandV3],
) -> isize {
    let header_bytes = unsafe {
        core::slice::from_raw_parts(
            core::ptr::from_ref(header).cast::<u8>(),
            core::mem::size_of_val(header),
        )
    };
    let command_bytes = unsafe {
        core::slice::from_raw_parts(
            commands.as_ptr().cast::<u8>(),
            core::mem::size_of_val(commands),
        )
    };
    let payload_len = header_bytes.len().saturating_add(command_bytes.len());
    if payload_len > trueos_vm::vmcall::PAYLOAD_CAP {
        return -(EINVAL as isize);
    }
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(header_bytes);
    payload.extend_from_slice(command_bytes);
    let (status, rc) = trueos_vm::vmcall::call_with_payload(
        trueos_vm::vmcall::OP_BP_AUDIO_NATIVE_RENDER_V3,
        commands.len() as u64,
        0,
        &payload,
        &mut [],
    );
    if status != trueos_vm::vmcall::STATUS_OK {
        return -(EIO as isize);
    }
    (rc as i64) as isize
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_native_render_v1(
    handle: u32,
    header: *const crate::aud::native_engine::NativeBlockHeaderV1,
    commands: *const crate::aud::native_engine::NativeRenderCommandV1,
    count: usize,
) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }
    if header.is_null() || (commands.is_null() && count != 0) {
        return -(EFAULT as isize);
    }
    if count > crate::aud::native_engine::MAX_COMMANDS {
        return -(EINVAL as isize);
    }

    let header = unsafe { &*header };
    let commands = if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(commands, count) }
    };
    let frames = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_native_render_v1(header, commands)
    } else {
        render_native_host_v1(header, commands)
    };
    if frames >= 0 {
        AUDIO_CABI_STATE.lock().running = true;
    }
    frames
}

/// Submit fixed-width V2 commands. V1 remains an independent compatibility
/// path, so old Blueprint guests cannot accidentally opt into envelope fields.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_native_render_v2(
    handle: u32,
    header: *const crate::aud::native_engine::NativeBlockHeaderV2,
    commands: *const crate::aud::native_engine::NativeRenderCommandV2,
    count: usize,
) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }
    if header.is_null() || (commands.is_null() && count != 0) {
        return -(EFAULT as isize);
    }
    if count > crate::aud::native_engine::MAX_COMMANDS {
        return -(EINVAL as isize);
    }
    let header = unsafe { &*header };
    let commands = if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(commands, count) }
    };
    let frames = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_native_render_v2(header, commands)
    } else {
        render_native_host_v2(header, commands)
    };
    if frames >= 0 {
        AUDIO_CABI_STATE.lock().running = true;
    }
    frames
}

/// Submit additive V3 commands. V1 and V2 remain independently callable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_native_render_v3(
    handle: u32,
    header: *const crate::aud::native_engine::NativeBlockHeaderV3,
    commands: *const crate::aud::native_engine::NativeRenderCommandV3,
    count: usize,
) -> isize {
    if !valid_handle(handle) {
        return -(EBADF as isize);
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -(ENODEV as isize);
    }
    if header.is_null() || (commands.is_null() && count != 0) {
        return -(EFAULT as isize);
    }
    if count > crate::aud::native_engine::MAX_COMMANDS {
        return -(EINVAL as isize);
    }
    let header = unsafe { &*header };
    let commands = if count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(commands, count) }
    };
    let frames = if crate::hv::current_hull_guest_context_vm_id().is_some() {
        guest_native_render_v3(header, commands)
    } else {
        render_native_host_v3(header, commands)
    };
    if frames >= 0 {
        AUDIO_CABI_STATE.lock().running = true;
    }
    frames
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_audio_native_sample_register_v1(
    handle: u32,
    sample_id: u64,
    channels: u32,
    rate_hz: u32,
    samples: *const i16,
    sample_count: usize,
) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }
    if samples.is_null() && sample_count != 0 {
        return -EFAULT;
    }
    let Ok(channels) = u16::try_from(channels) else {
        return -EINVAL;
    };
    let samples = if sample_count == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(samples, sample_count) }
    };
    match crate::aud::native_engine::register_sample_v1(sample_id, channels, rate_hz, samples) {
        Ok(()) => 0,
        Err(error) => -native_error_code(error),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_native_sample_remove_v1(handle: u32, sample_id: u64) -> i32 {
    if !valid_handle(handle) {
        return -EBADF;
    }
    if !AUDIO_CABI_STATE.lock().open {
        return -ENODEV;
    }
    if sample_id == 0 {
        return -EINVAL;
    }
    if crate::aud::native_engine::remove_sample_v1(sample_id) {
        0
    } else {
        -ENODEV
    }
}
