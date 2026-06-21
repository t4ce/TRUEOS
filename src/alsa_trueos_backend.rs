use spin::Mutex;

use crate::hda;

const EBADF: i32 = 9;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const EIO: i32 = 5;
const ENODEV: i32 = 19;

const ALSA_TRUEOS_HANDLE: usize = 1;
const ALSA_TRUEOS_BUFFER_FRAMES: usize = hda::PCM_SAMPLE_RATE_HZ as usize * 30;

static ALSA_PCM_STATE: Mutex<AlsaPcmState> = Mutex::new(AlsaPcmState::new());

struct AlsaPcmState {
    open: bool,
    running: bool,
}

impl AlsaPcmState {
    const fn new() -> Self {
        Self {
            open: false,
            running: false,
        }
    }
}

pub fn install() {
    alsa::trueos::install_pcm_backend(&PCM_BACKEND);
}

static PCM_BACKEND: alsa::trueos::PcmBackend = alsa::trueos::PcmBackend {
    open_playback,
    close,
    start,
    drop_stream,
    drain,
    write_i16_interleaved,
    writable_frames,
    queued_frames,
    buffer_frames,
    state,
};

fn open_playback() -> alsa::trueos::BackendResult<usize> {
    let mut state = ALSA_PCM_STATE.lock();
    if state.open {
        return Err(EBADF);
    }

    state.open = true;
    state.running = false;
    crate::log!("alsa-trueos: pcm open playback backend=trueos-pcm-lane\n");
    Ok(ALSA_TRUEOS_HANDLE)
}

fn close(handle: usize) {
    if handle != ALSA_TRUEOS_HANDLE {
        return;
    }

    let mut state = ALSA_PCM_STATE.lock();
    state.open = false;
    state.running = false;
}

fn start(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }

    let mut state = ALSA_PCM_STATE.lock();
    if !state.open {
        return Err(ENODEV);
    }

    state.running = true;
    Ok(())
}

fn drop_stream(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }

    let mut state = ALSA_PCM_STATE.lock();
    if !state.open {
        return Err(ENODEV);
    }

    state.running = false;
    Ok(())
}

fn drain(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle == ALSA_TRUEOS_HANDLE {
        Ok(())
    } else {
        Err(EBADF)
    }
}

fn write_i16_interleaved(
    handle: usize,
    samples: *const i16,
    len: usize,
) -> alsa::trueos::BackendResult<usize> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }
    if samples.is_null() && len != 0 {
        return Err(EFAULT);
    }
    if len % hda::PCM_CHANNELS != 0 {
        return Err(EINVAL);
    }
    if !ALSA_PCM_STATE.lock().open {
        return Err(ENODEV);
    }

    let samples = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(samples, len) }
    };
    let frames = samples.len() / hda::PCM_CHANNELS;
    if frames == 0 {
        return Ok(0);
    }

    crate::aud::pcm_lane::submit_i16_stereo_48k("alsa-trueos-pcm", alloc::vec::Vec::from(samples))
        .map_err(|_| EIO)?;
    ALSA_PCM_STATE.lock().running = true;
    crate::log!(
        "alsa-trueos: write_i16_interleaved samples={} frames={} route=trueos-pcm-lane\n",
        samples.len(),
        frames
    );
    Ok(frames)
}

fn writable_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }
    if !ALSA_PCM_STATE.lock().open {
        return Err(ENODEV);
    }

    Ok(ALSA_TRUEOS_BUFFER_FRAMES)
}

fn queued_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }
    if !ALSA_PCM_STATE.lock().open {
        return Err(ENODEV);
    }

    Ok(0)
}

fn buffer_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != ALSA_TRUEOS_HANDLE {
        return Err(EBADF);
    }
    if !ALSA_PCM_STATE.lock().open {
        return Err(ENODEV);
    }

    Ok(ALSA_TRUEOS_BUFFER_FRAMES)
}

fn state(handle: usize) -> alsa::pcm::State {
    if handle != ALSA_TRUEOS_HANDLE {
        return alsa::pcm::State::Disconnected;
    }

    let state = ALSA_PCM_STATE.lock();
    match (state.open, state.running) {
        (true, true) => alsa::pcm::State::Running,
        (true, false) => alsa::pcm::State::Prepared,
        (false, _) => alsa::pcm::State::Disconnected,
    }
}
