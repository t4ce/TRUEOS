use spin::Mutex;

use crate::hda::{self, PcmStreamHandle};

static ALSA_HDA_STREAM: Mutex<Option<PcmStreamHandle>> = Mutex::new(None);

const EBADF: i32 = 9;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const EIO: i32 = 5;
const ENODEV: i32 = 19;

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
    if !hda::is_initialized() {
        hda::init().map_err(|_| ENODEV)?;
    }

    let mut stream = ALSA_HDA_STREAM.lock();
    if stream.is_none() {
        *stream = Some(hda::open_pcm_stream().map_err(|_| ENODEV)?);
    }
    Ok(1)
}

fn close(handle: usize) {
    if handle != 1 {
        return;
    }

    if let Some(mut stream) = ALSA_HDA_STREAM.lock().take() {
        stream.stop_reset();
    }
}

fn start(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle == 1 {
        Ok(())
    } else {
        Err(EBADF)
    }
}

fn drop_stream(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle != 1 {
        return Err(EBADF);
    }

    let mut stream = ALSA_HDA_STREAM.lock();
    let Some(stream) = stream.as_mut() else {
        return Err(ENODEV);
    };
    stream.stop_reset();
    Ok(())
}

fn drain(handle: usize) -> alsa::trueos::BackendResult<()> {
    if handle == 1 {
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
    if handle != 1 {
        return Err(EBADF);
    }
    if samples.is_null() && len != 0 {
        return Err(EFAULT);
    }
    if len % hda::PCM_CHANNELS != 0 {
        return Err(EINVAL);
    }

    let samples = if len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(samples, len) }
    };

    let mut stream = ALSA_HDA_STREAM.lock();
    let Some(stream) = stream.as_mut() else {
        return Err(ENODEV);
    };
    stream.push_samples(samples).map_err(|_| EIO)?;
    Ok(len / hda::PCM_CHANNELS)
}

fn writable_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != 1 {
        return Err(EBADF);
    }

    let stream = ALSA_HDA_STREAM.lock();
    let Some(stream) = stream.as_ref() else {
        return Err(ENODEV);
    };
    let samples = stream.writable_samples(hda::PCM_CHANNELS).ok_or(EIO)?;
    Ok(samples / hda::PCM_CHANNELS)
}

fn queued_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != 1 {
        return Err(EBADF);
    }

    let stream = ALSA_HDA_STREAM.lock();
    let Some(stream) = stream.as_ref() else {
        return Err(ENODEV);
    };
    let samples = stream.queued_samples().ok_or(EIO)?;
    Ok(samples / hda::PCM_CHANNELS)
}

fn buffer_frames(handle: usize) -> alsa::trueos::BackendResult<usize> {
    if handle != 1 {
        return Err(EBADF);
    }

    let stream = ALSA_HDA_STREAM.lock();
    let Some(stream) = stream.as_ref() else {
        return Err(ENODEV);
    };
    Ok(stream.info().buffer_frames)
}

fn state(handle: usize) -> alsa::pcm::State {
    if handle != 1 {
        return alsa::pcm::State::Disconnected;
    }

    let stream = ALSA_HDA_STREAM.lock();
    match stream.as_ref() {
        Some(stream) if stream.is_started() => alsa::pcm::State::Running,
        Some(_) => alsa::pcm::State::Prepared,
        None => alsa::pcm::State::Disconnected,
    }
}
