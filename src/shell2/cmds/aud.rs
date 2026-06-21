use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Instant, Timer};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const AUD_WAV_PATH: &str = "aud.wav";
const STREAM_GUARD_SAMPLES: usize =
    (crate::hda::PCM_SAMPLE_RATE_HZ as usize / 200) * crate::hda::PCM_CHANNELS;
const STREAM_CHUNK_MS: usize = 20;
const STREAM_CHUNK_SAMPLES: usize = ((crate::hda::PCM_SAMPLE_RATE_HZ as usize * STREAM_CHUNK_MS)
    / 1_000)
    * crate::hda::PCM_CHANNELS;
const STREAM_DRAIN_TIMEOUT_MS: u64 = 10_000;

struct DecodedWav {
    samples: Vec<i16>,
    frames: usize,
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

fn decode_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Result<DecodedWav, &'static str> {
    let (data_off, data_len) =
        wav_pcm_s16_stereo_48k_data_range(bytes).ok_or("unsupported wav format")?;
    if data_len == 0 || data_len % crate::hda::PCM_FRAME_BYTES != 0 {
        return Err("invalid wav data length");
    }

    let data = bytes
        .get(data_off..data_off + data_len)
        .ok_or("invalid wav data range")?;
    let mut samples = Vec::with_capacity(data_len / crate::hda::PCM_SAMPLE_BYTES);
    for pair in data.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }

    let frames = samples.len() / crate::hda::PCM_CHANNELS;
    Ok(DecodedWav { samples, frames })
}

fn duration_ms_for_frames(frames: usize) -> u32 {
    let ms = (((frames as u128) * 1_000) + u128::from(crate::hda::PCM_SAMPLE_RATE_HZ) - 1)
        / u128::from(crate::hda::PCM_SAMPLE_RATE_HZ);
    ms.clamp(1, u128::from(u32::MAX)) as u32
}

fn read_probe_wav() -> Result<DecodedWav, String> {
    let bytes = crate::io::kfs::read_file(AUD_WAV_PATH)
        .map_err(|err| format!("read {AUD_WAV_PATH} failed: {:?}", err))?;
    decode_wav_pcm_s16_stereo_48k(bytes.as_slice()).map_err(String::from)
}

async fn stream_decoded_wav(decoded: &DecodedWav) -> Result<(), &'static str> {
    let mut stream = crate::hda::open_pcm_stream()?;
    let mut cursor = 0usize;

    while cursor < decoded.samples.len() {
        let writable = stream.writable_samples(STREAM_GUARD_SAMPLES).unwrap_or(0);
        let chunk_samples = decoded.samples.len().saturating_sub(cursor);
        let push_samples =
            chunk_samples.min(writable).min(STREAM_CHUNK_SAMPLES) & !(crate::hda::PCM_CHANNELS - 1);

        if push_samples == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
            continue;
        }

        let end = cursor + push_samples;
        stream.push_samples(&decoded.samples[cursor..end])?;
        cursor = end;
        Timer::after(EmbassyDuration::from_millis(1)).await;
    }

    let drain_deadline = Instant::now() + EmbassyDuration::from_millis(STREAM_DRAIN_TIMEOUT_MS);
    loop {
        let queued = stream.queued_samples().unwrap_or(0);
        if queued <= crate::hda::PCM_CHANNELS {
            break;
        }
        if Instant::now() >= drain_deadline {
            stream.stop_reset();
            return Err("HDA drain timeout");
        }
        Timer::after(EmbassyDuration::from_millis(5)).await;
    }

    stream.stop_reset();
    Ok(())
}

#[embassy_executor::task(pool_size = 1)]
async fn aud_command_task(target: MatrixTarget) {
    Timer::after(EmbassyDuration::from_millis(1)).await;

    match read_probe_wav() {
        Ok(decoded) => {
            let duration_ms = duration_ms_for_frames(decoded.frames);
            print_matrix_target_line(
                &target,
                format!(
                    "aud: playing {} frames={} duration={}ms rate={} channels={}",
                    AUD_WAV_PATH,
                    decoded.frames,
                    duration_ms,
                    crate::hda::PCM_SAMPLE_RATE_HZ,
                    crate::hda::PCM_CHANNELS
                )
                .as_str(),
            );

            match stream_decoded_wav(&decoded).await {
                Ok(()) => print_matrix_target_line(&target, "aud: done"),
                Err(err) => print_matrix_target_line(
                    &target,
                    format!("aud: playback failed: {err}").as_str(),
                ),
            }
        }
        Err(err) => print_matrix_target_line(&target, format!("aud: {err}").as_str()),
    }

    set_matrix_target_active(&target, false);
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "aud: usage `aud`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    print_matrix_target_line(&target, format!("aud: loading {}", AUD_WAV_PATH).as_str());
    set_matrix_target_active(&target, true);

    match aud_command_task(target.clone()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "aud: spawn failed");
        }
    }

    ParseOutcome::Handled
}
