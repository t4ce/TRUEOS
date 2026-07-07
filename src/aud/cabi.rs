use alloc::vec::Vec;
use spin::Mutex;

use crate::hda;

const TRUEOS_AUDIO_HANDLE: u32 = 1;
const TRUEOS_AUDIO_BUFFER_FRAMES: usize = hda::PCM_SAMPLE_RATE_HZ as usize * 30;

const TRUEOS_AUDIO_FORMAT_S16LE: u32 = 1;

const EIO: i32 = 5;
const EBADF: i32 = 9;
const EBUSY: i32 = 16;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENODEV: i32 = 19;

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
        return Err(EINVAL);
    }
    if samples.is_empty() {
        return Ok(0);
    }

    crate::aud::pcm_lane::submit_i16_stereo_48k(label, Vec::from(samples)).map_err(|err| {
        match err {
            crate::aud::pcm_lane::PcmLaneError::QueueFull => EBUSY,
            crate::aud::pcm_lane::PcmLaneError::BadShape => EINVAL,
            crate::aud::pcm_lane::PcmLaneError::EmptyBuffer => EIO,
        }
    })
}

fn guest_audio_write_samples(samples: &[i16]) -> Result<usize, i32> {
    if samples.len() % hda::PCM_CHANNELS != 0 {
        return Err(EINVAL);
    }
    if samples.is_empty() {
        return Ok(0);
    }

    let bytes = unsafe {
        core::slice::from_raw_parts(samples.as_ptr().cast::<u8>(), core::mem::size_of_val(samples))
    };
    let mut written_frames = 0usize;
    let mut offset = 0usize;
    let max_payload = trueos_vm::vmcall::PAYLOAD_CAP & !1;
    while offset < bytes.len() {
        let end = core::cmp::min(offset.saturating_add(max_payload), bytes.len()) & !1;
        if end <= offset {
            return Err(EIO);
        }
        let chunk = &bytes[offset..end];
        let (status, rc) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_AUDIO_WRITE_I16_STEREO_48K,
            0,
            0,
            chunk,
            &mut [],
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return Err(EIO);
        }
        let frames = (rc as i64) as isize;
        if frames < 0 {
            let err = (-frames) as i32;
            if err == EBUSY && written_frames != 0 {
                return Ok(written_frames);
            }
            return Err(err);
        }
        if frames == 0 {
            break;
        }
        written_frames = written_frames.saturating_add(frames as usize);
        offset = end;
    }
    Ok(written_frames)
}

fn guest_audio_stop() {
    let _ = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_AUDIO_STOP, 0, 0);
}

fn guest_audio_pending_frames() -> usize {
    let (status, frames) =
        trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_AUDIO_PENDING_FRAMES, 0, 0);
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
        return -EFAULT;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        if let Err(err) = ensure_supported_shape(format, channels, rate_hz) {
            return -err;
        }
        let mut state = AUDIO_CABI_STATE.lock();
        if state.open {
            return -EBUSY;
        }
        guest_audio_stop();
        state.open = true;
        state.running = false;
        unsafe {
            out_handle.write(TRUEOS_AUDIO_HANDLE);
        }
        return 0;
    }
    if let Err(err) = ensure_supported(format, channels, rate_hz) {
        return -err;
    }

    let mut state = AUDIO_CABI_STATE.lock();
    if state.open {
        return -EBUSY;
    }
    crate::aud::pcm_lane::request_stop();
    state.open = true;
    state.running = false;
    crate::aud::pcm_lane::set_paused(false);

    unsafe {
        out_handle.write(TRUEOS_AUDIO_HANDLE);
    }
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
        return 0;
    }
    crate::aud::pcm_lane::set_paused(false);
    state.running = true;
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
        return 0;
    }
    crate::aud::pcm_lane::request_stop();
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
        return 0;
    }
    crate::aud::pcm_lane::set_paused(paused != 0);
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
        return percent.min(u16::MAX as u32).min(100) as i32;
    }
    crate::aud::pcm_lane::set_volume_percent(percent.min(u16::MAX as u32) as u16) as i32
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
        return 100;
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

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_audio_monitor_start_cursor(preroll_samples: usize) -> u64 {
    crate::tst::esynth::live_pcm_stream_start_cursor(preroll_samples).unwrap_or(u64::MAX)
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
    let Some(next) = crate::tst::esynth::live_pcm_read_since(cursor, &mut samples, out_cap) else {
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
