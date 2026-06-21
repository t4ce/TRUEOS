use alloc::format;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use spin::Mutex;

use crate::shell2::{MatrixTarget, print_matrix_target_line, set_matrix_target_active};

const AUD_PATH: &str = "aud.m4a";
const READ_RETRY_ATTEMPTS: usize = 80;
const READ_RETRY_DELAY_MS: u64 = 25;

static AUD_JOB: Mutex<Option<AudJob>> = Mutex::new(None);
static AUD_JOB_RUNNING: AtomicBool = AtomicBool::new(false);

struct AudJob {
    target: MatrixTarget,
}

struct DecodedAudio {
    samples: Vec<i16>,
    frames: usize,
}

enum AudReadError {
    NoRoot,
    NotFound,
    Device(crate::disc::block::Error),
}

enum AudDecodeError {
    M4a(crate::aud::m4a::M4aDecodeError),
    UnsupportedM4a,
    UnsupportedWav,
    InvalidWavDataLength,
    InvalidWavDataRange,
    UnknownContainer,
}

impl AudReadError {
    fn should_retry(&self) -> bool {
        matches!(
            self,
            Self::NoRoot
                | Self::Device(crate::disc::block::Error::NotReady)
                | Self::Device(crate::disc::block::Error::Timeout)
                | Self::Device(crate::disc::block::Error::Io)
        )
    }
}

pub fn submit_default(target: MatrixTarget) -> Result<(), &'static str> {
    let mut job = AUD_JOB.lock();
    if job.is_some() || AUD_JOB_RUNNING.load(Ordering::Acquire) {
        return Err("busy");
    }
    *job = Some(AudJob { target });
    Ok(())
}

fn take_job() -> Option<AudJob> {
    AUD_JOB.lock().take()
}

fn le_u16(bytes: &[u8], off: usize) -> Option<u16> {
    let b = bytes.get(off..off + 2)?;
    Some(u16::from_le_bytes([b[0], b[1]]))
}

fn le_u32(bytes: &[u8], off: usize) -> Option<u32> {
    let b = bytes.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn wav_pcm_s16_stereo_48k_data_range(bytes: &[u8]) -> Option<(usize, usize)> {
    if bytes.len() < 44 || bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }

    let mut fmt_ok = false;
    let mut data_range = None;
    let mut off = 12usize;

    while off + 8 <= bytes.len() {
        let chunk_id = bytes.get(off..off + 4)?;
        let chunk_len = usize::try_from(le_u32(bytes, off + 4)?).ok()?;
        let chunk_data_off = off + 8;
        let chunk_end = chunk_data_off.checked_add(chunk_len)?;
        if chunk_end > bytes.len() {
            return None;
        }

        if chunk_id == b"fmt " {
            let format_tag = le_u16(bytes, chunk_data_off)?;
            let channels = le_u16(bytes, chunk_data_off + 2)?;
            let sample_rate = le_u32(bytes, chunk_data_off + 4)?;
            let block_align = le_u16(bytes, chunk_data_off + 12)?;
            let bits_per_sample = le_u16(bytes, chunk_data_off + 14)?;
            fmt_ok = format_tag == 1
                && usize::from(channels) == crate::hda::PCM_CHANNELS
                && sample_rate == crate::hda::PCM_SAMPLE_RATE_HZ
                && usize::from(bits_per_sample) == crate::hda::PCM_SAMPLE_BITS
                && usize::from(block_align) == crate::hda::PCM_FRAME_BYTES;
        } else if chunk_id == b"data" {
            data_range = Some((chunk_data_off, chunk_len));
        }

        let padded_len = (chunk_len + 1) & !1;
        off = chunk_data_off.checked_add(padded_len)?;
    }

    if fmt_ok { data_range } else { None }
}

fn decode_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Result<DecodedAudio, AudDecodeError> {
    let (data_off, data_len) =
        wav_pcm_s16_stereo_48k_data_range(bytes).ok_or(AudDecodeError::UnsupportedWav)?;
    if data_len == 0 || data_len % crate::hda::PCM_FRAME_BYTES != 0 {
        return Err(AudDecodeError::InvalidWavDataLength);
    }

    let data = bytes
        .get(data_off..data_off + data_len)
        .ok_or(AudDecodeError::InvalidWavDataRange)?;
    let mut samples = Vec::with_capacity(data_len / crate::hda::PCM_SAMPLE_BYTES);
    for pair in data.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }

    let frames = samples.len() / crate::hda::PCM_CHANNELS;
    Ok(DecodedAudio { samples, frames })
}

fn decode_audio_to_pcm(bytes: &[u8]) -> Result<DecodedAudio, AudDecodeError> {
    if bytes.get(0..4) == Some(b"RIFF") {
        return decode_wav_pcm_s16_stereo_48k(bytes);
    }
    if bytes.get(4..8) == Some(b"ftyp") {
        let decoded = crate::aud::m4a::decode_m4a_to_pcm_48k_stereo_s16(bytes)
            .map_err(AudDecodeError::M4a)?;
        return Ok(DecodedAudio {
            samples: decoded.samples,
            frames: decoded.frames,
        });
    }
    Err(AudDecodeError::UnknownContainer)
}

fn duration_ms_for_frames(frames: usize) -> u32 {
    let ms = (((frames as u128) * 1_000) + u128::from(crate::hda::PCM_SAMPLE_RATE_HZ) - 1)
        / u128::from(crate::hda::PCM_SAMPLE_RATE_HZ);
    ms.clamp(1, u128::from(u32::MAX)) as u32
}

async fn read_aud_file_once() -> Result<Vec<u8>, AudReadError> {
    let disk = crate::r::fs::trueosfs::primary_root_handle().ok_or(AudReadError::NoRoot)?;
    crate::r::fs::trueosfs::file_out_async(disk, AUD_PATH)
        .await
        .map_err(AudReadError::Device)?
        .ok_or(AudReadError::NotFound)
}

async fn read_aud_file() -> Result<Vec<u8>, AudReadError> {
    let mut last = None;
    for attempt in 0..READ_RETRY_ATTEMPTS {
        match read_aud_file_once().await {
            Ok(bytes) => return Ok(bytes),
            Err(err) if err.should_retry() && attempt + 1 < READ_RETRY_ATTEMPTS => {
                last = Some(err);
                Timer::after(EmbassyDuration::from_millis(READ_RETRY_DELAY_MS)).await;
            }
            Err(err) => return Err(err),
        }
    }
    Err(last.unwrap_or(AudReadError::NoRoot))
}

fn print_read_error(target: &MatrixTarget, err: AudReadError) {
    match err {
        AudReadError::NoRoot => print_matrix_target_line(target, "aud: TRUEOSFS root not ready"),
        AudReadError::NotFound => print_matrix_target_line(
            target,
            format!("aud: {AUD_PATH} not found in TRUEOSFS root").as_str(),
        ),
        AudReadError::Device(err) => print_matrix_target_line(
            target,
            format!("aud: read {AUD_PATH} failed: Device({err:?})").as_str(),
        ),
    }
}

fn print_decode_error(target: &MatrixTarget, bytes_len: usize, err: AudDecodeError) {
    match err {
        AudDecodeError::M4a(err) => match err {
            crate::aud::m4a::M4aDecodeError::DecoderMissing {
                container,
                source_sample_rate,
                source_channels,
                packet_count,
                asc_len,
                object_type_indication,
            } => {
                print_matrix_target_line(
                    target,
                    format!(
                        "aud: m4a/aac demuxed bytes={} brand={} oti={:?} rate={:?} channels={:?} packets={} asc={} target=s16le/stereo/48k ap1=1 decoder=missing",
                        bytes_len,
                        container.major_brand,
                        object_type_indication,
                        source_sample_rate,
                        source_channels,
                        packet_count,
                        asc_len,
                    )
                    .as_str(),
                );
            }
            _ => {
                print_matrix_target_line(
                    target,
                    format!("aud: m4a decode failed code={}", err.code()).as_str(),
                );
            }
        },
        AudDecodeError::UnsupportedM4a => {
            print_matrix_target_line(
                target,
                format!(
                    "aud: m4a/aac detected bytes={} target=s16le/stereo/48k ap1=1 decoder=missing",
                    bytes_len
                )
                .as_str(),
            );
        }
        AudDecodeError::UnsupportedWav => {
            print_matrix_target_line(target, "aud: unsupported wav format");
        }
        AudDecodeError::InvalidWavDataLength => {
            print_matrix_target_line(target, "aud: invalid wav data length");
        }
        AudDecodeError::InvalidWavDataRange => {
            print_matrix_target_line(target, "aud: invalid wav data range");
        }
        AudDecodeError::UnknownContainer => {
            print_matrix_target_line(target, "aud: unsupported audio container");
        }
    }
}

fn queue_decoded_via_pcm_lane(decoded: DecodedAudio) -> Result<usize, &'static str> {
    crate::aud::pcm_lane::submit_i16_stereo_48k(AUD_PATH, decoded.samples)
}

async fn handle_job(job: AudJob) {
    let stop_generation = crate::aud::pcm_lane::stop_generation();
    print_matrix_target_line(&job.target, format!("aud: ap1 loading {}", AUD_PATH).as_str());

    let bytes = match read_aud_file().await {
        Ok(bytes) => bytes,
        Err(err) => {
            print_read_error(&job.target, err);
            set_matrix_target_active(&job.target, false);
            return;
        }
    };

    let decoded = match decode_audio_to_pcm(bytes.as_slice()) {
        Ok(decoded) => decoded,
        Err(err) => {
            print_decode_error(&job.target, bytes.len(), err);
            set_matrix_target_active(&job.target, false);
            return;
        }
    };

    let duration_ms = duration_ms_for_frames(decoded.frames);
    print_matrix_target_line(
        &job.target,
        format!(
            "aud: playing {} frames={} duration={}ms rate={} channels={}",
            AUD_PATH,
            decoded.frames,
            duration_ms,
            crate::hda::PCM_SAMPLE_RATE_HZ,
            crate::hda::PCM_CHANNELS
        )
        .as_str(),
    );

    if crate::aud::pcm_lane::stop_generation() != stop_generation {
        print_matrix_target_line(&job.target, "aud: decode discarded after stop");
        set_matrix_target_active(&job.target, false);
        return;
    }

    match queue_decoded_via_pcm_lane(decoded) {
        Ok(frames) => print_matrix_target_line(
            &job.target,
            format!("aud: queued pcm-lane frames={frames} sinks=hda,http82").as_str(),
        ),
        Err(err) => print_matrix_target_line(
            &job.target,
            format!("aud: pcm-lane queue failed: {err}").as_str(),
        ),
    }

    set_matrix_target_active(&job.target, false);
}

#[embassy_executor::task(pool_size = 1)]
async fn aud_decode_job_task(job: AudJob) {
    handle_job(job).await;
    AUD_JOB_RUNNING.store(false, Ordering::Release);
}

#[embassy_executor::task(pool_size = 1)]
pub async fn aud_file_service_task() {
    let slot = crate::percpu::current_slot();
    crate::log!("aud-file-service: task start slot={} policy=ap1-ui-service\n", slot);
    let spawner: Spawner = unsafe { Spawner::for_current_executor().await };

    loop {
        if let Some(job) = take_job() {
            AUD_JOB_RUNNING.store(true, Ordering::Release);
            let target = job.target.clone();
            match aud_decode_job_task(job) {
                Ok(token) => {
                    spawner.spawn(token);
                }
                Err(_) => {
                    AUD_JOB_RUNNING.store(false, Ordering::Release);
                    set_matrix_target_active(&target, false);
                    print_matrix_target_line(&target, "aud: decode worker unavailable");
                }
            }
        } else {
            Timer::after(EmbassyDuration::from_millis(10)).await;
        }
    }
}
