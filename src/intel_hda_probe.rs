use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

const HDA_PROBE_WAV_LOOP_ENABLED: bool = false;
const PROBE_PATTERN_NAME: &str = "arp";
const PIANO_PROBE_PATTERN_NAME: &str = "piano-held";
const PROBE_PATTERN_LOOPS: u32 = 1;
const PROBE_RETRY_DELAY_MS: u64 = 1_000;
const PROBE_LOOP_DELAY_MS: u64 = 250;
const PIANO_NOTE_POLL_DELAY_MS: u64 = 25;
const PIANO_CHORD_RENDER_MS: u32 = 50;
const BASSLINE_IDLE_POLL_DELAY_MS: u64 = 25;
const HDA_WAV_LOOP_RETRY_DELAY_MS: u64 = 1_000;
const HDA_WAV_LOOP_IDLE_MS: u64 = 500;
const HDA_WAV_FETCH_TIMEOUT_MS: u32 = 60_000;
const HDA_WAV_FETCH_MAX_BYTES: usize = 32 * 1024 * 1024;
const HDA_WAV_SAMPLE_RATE_HZ: u64 = 48_000;
const HDA_WAV_CHANNELS: usize = 2;
const HDA_WAV_TRACE_MARKER: &str = "hda-wav-trace-v3";

fn piano_claimed() -> bool {
    crate::usb2::midi::piano_connected()
}

fn play_default_probe_pattern() -> Result<&'static str, &'static str> {
    crate::aud::pattern_play(PROBE_PATTERN_NAME, PROBE_PATTERN_LOOPS)?;
    Ok(PROBE_PATTERN_NAME)
}

fn play_piano_probe_held(
    snapshot: &crate::usb2::midi::PianoHeldSnapshot,
    log_event: bool,
) -> Result<&'static str, &'static str> {
    if log_event {
        crate::log_trace!(
            "intel/hda-probe: piano held seq={} notes={} first={} vel={}\n",
            snapshot.seq,
            snapshot.len,
            snapshot.notes.first().copied().unwrap_or(0),
            snapshot.velocities.first().copied().unwrap_or(0),
        );
    }
    crate::aud::play_piano_held_probe(
        &snapshot.notes,
        &snapshot.velocities,
        snapshot.len,
        PIANO_CHORD_RENDER_MS,
    )?;
    Ok(PIANO_PROBE_PATTERN_NAME)
}

fn uptime_ms() -> u64 {
    let ticks = embassy_time_driver::now() as u128;
    let hz = embassy_time_driver::TICK_HZ as u128;
    if hz == 0 {
        0
    } else {
        ((ticks * 1000u128) / hz) as u64
    }
}

fn next_grid_ms(now_ms: u64, quantum_ms: u64) -> u64 {
    if quantum_ms == 0 {
        return now_ms;
    }
    let rem = now_ms % quantum_ms;
    if rem == 0 {
        now_ms
    } else {
        now_ms + (quantum_ms - rem)
    }
}

fn next_running_beat_ms(now_ms: u64, start_ms: u64, beat_ms: u64) -> u64 {
    if beat_ms == 0 || now_ms <= start_ms {
        return now_ms;
    }
    let elapsed = now_ms - start_ms;
    let rem = elapsed % beat_ms;
    if rem == 0 {
        now_ms
    } else {
        now_ms + (beat_ms - rem)
    }
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
                && usize::from(channels) == HDA_WAV_CHANNELS
                && u64::from(sample_rate) == HDA_WAV_SAMPLE_RATE_HZ
                && bits_per_sample == 16
                && block_align == 4;
        } else if chunk_id == b"data" {
            data_range = Some((chunk_data_off, chunk_len));
        }

        let padded_len = (chunk_len + 1) & !1;
        off = chunk_data_off.checked_add(padded_len)?;
    }

    if fmt_ok { data_range } else { None }
}

fn decode_wav_pcm_s16_stereo_48k(bytes: &[u8]) -> Result<Vec<i16>, &'static str> {
    let (data_off, data_len) =
        wav_pcm_s16_stereo_48k_data_range(bytes).ok_or("unsupported wav format")?;
    if data_len == 0 || data_len % 4 != 0 {
        return Err("invalid wav data length");
    }

    let data = bytes
        .get(data_off..data_off + data_len)
        .ok_or("invalid wav data range")?;
    let mut samples = Vec::with_capacity(data_len / 2);
    for pair in data.chunks_exact(2) {
        samples.push(i16::from_le_bytes([pair[0], pair[1]]));
    }
    Ok(samples)
}

fn hda_duration_ms_for_samples(sample_count: usize) -> u32 {
    let frames = (sample_count / HDA_WAV_CHANNELS) as u64;
    let ms = ((frames * 1_000) + HDA_WAV_SAMPLE_RATE_HZ - 1) / HDA_WAV_SAMPLE_RATE_HZ;
    ms.clamp(1, u64::from(u32::MAX)) as u32
}

async fn load_hda_wav_loop_samples() -> Result<Vec<i16>, &'static str> {
    let url = crate::allports::local_assets::AUDIO_DEMO_URL;
    crate::log_trace!(
        "intel/hda-probe: wav fetch submit trace={} url={}\n",
        HDA_WAV_TRACE_MARKER,
        url
    );
    let body = crate::t::run_on_shared_tokio(move || async move {
        crate::log_trace!(
            "intel/hda-probe: wav fetch job enter trace={} url={}\n",
            HDA_WAV_TRACE_MARKER,
            url
        );
        crate::t::net::http::fetch_http_body_hyper(
            url,
            HDA_WAV_FETCH_TIMEOUT_MS,
            HDA_WAV_FETCH_MAX_BYTES,
        )
        .await
    })
    .await
    .map_err(|_| "shared tokio unavailable")?
    .map_err(|_| "http fetch failed")?;
    crate::log_trace!(
        "intel/hda-probe: wav fetch body trace={} url={} bytes={}\n",
        HDA_WAV_TRACE_MARKER,
        url,
        body.len()
    );
    let samples = decode_wav_pcm_s16_stereo_48k(body.as_slice())?;
    crate::log_trace!(
        "intel/hda-probe: wav decode ok trace={} samples={} frames={}\n",
        HDA_WAV_TRACE_MARKER,
        samples.len(),
        samples.len() / HDA_WAV_CHANNELS
    );
    Ok(samples)
}

async fn hda_wav_loop_probe_task() {
    let url = crate::allports::local_assets::AUDIO_DEMO_URL;
    crate::log_trace!(
        "intel/hda-probe: wav loop mode trace={} url={}\n",
        HDA_WAV_TRACE_MARKER,
        url
    );

    loop {
        if !crate::hda::is_initialized() {
            match crate::hda::init() {
                Ok(()) => crate::log_trace!("intel/hda-probe: hda initialized for wav loop\n"),
                Err(err) => {
                    crate::log_trace!("intel/hda-probe: hda init err for wav loop err={}\n", err);
                    Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
                    continue;
                }
            }
        }

        crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;
        crate::log_trace!(
            "intel/hda-probe: wav loop net ready trace={}\n",
            HDA_WAV_TRACE_MARKER
        );
        while !crate::t::shared_tokio_runtime_ready() {
            crate::log_trace!(
                "intel/hda-probe: wav loop waiting for shared tokio runtime trace={}\n",
                HDA_WAV_TRACE_MARKER
            );
            Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
        }
        crate::log_trace!(
            "intel/hda-probe: wav loop shared tokio ready trace={}\n",
            HDA_WAV_TRACE_MARKER
        );

        let samples = match load_hda_wav_loop_samples().await {
            Ok(samples) => samples,
            Err(err) => {
                crate::log_trace!("intel/hda-probe: wav fetch/decode err url={} err={}\n", url, err);
                Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
                continue;
            }
        };

        let Some((_buf, dma_capacity_samples)) = crate::hda::get_dma_buffer_info() else {
            crate::log_trace!("intel/hda-probe: wav loop no hda dma buffer\n");
            Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
            continue;
        };
        let chunk_samples = dma_capacity_samples & !(HDA_WAV_CHANNELS - 1);
        if chunk_samples == 0 {
            crate::log_trace!(
                "intel/hda-probe: wav loop invalid dma capacity samples={}\n",
                dma_capacity_samples
            );
            Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
            continue;
        }

        crate::log_trace!(
            "intel/hda-probe: wav loop loaded trace={} url={} samples={} frames={} dma_samples={}\n",
            HDA_WAV_TRACE_MARKER,
            url,
            samples.len(),
            samples.len() / HDA_WAV_CHANNELS,
            chunk_samples
        );

        if samples.len() <= chunk_samples {
            match crate::hda::start_looped_playback(samples.as_slice()) {
                Ok(()) => loop {
                    crate::hda::clear_stream_status();
                    let _ = crate::hda::ensure_running();
                    Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_IDLE_MS)).await;
                },
                Err(err) => {
                    crate::log_trace!("intel/hda-probe: wav loop start err={}\n", err);
                    Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS)).await;
                    continue;
                }
            }
        }

        loop {
            let mut off = 0usize;
            while off < samples.len() {
                let end = (off + chunk_samples).min(samples.len()) & !(HDA_WAV_CHANNELS - 1);
                if end <= off {
                    break;
                }
                let chunk = &samples[off..end];
                let duration_ms = hda_duration_ms_for_samples(chunk.len());
                if off == 0 {
                    crate::log_trace!(
                        "intel/hda-probe: wav chunk play begin trace={} len={} duration_ms={}\n",
                        HDA_WAV_TRACE_MARKER,
                        chunk.len(),
                        duration_ms
                    );
                }
                match crate::hda::write_samples_and_play(chunk, duration_ms) {
                    Ok(()) => {}
                    Err(err) => {
                        crate::log_trace!(
                            "intel/hda-probe: wav chunk err off={} len={} err={}\n",
                            off,
                            chunk.len(),
                            err
                        );
                        Timer::after(EmbassyDuration::from_millis(HDA_WAV_LOOP_RETRY_DELAY_MS))
                            .await;
                        break;
                    }
                }
                off = end;
            }
        }
    }
}

#[embassy_executor::task]
pub async fn task() {
    crate::log_trace!(
        "intel/hda-probe: task start wav_loop={} pattern={} piano={} loops={}\n",
        HDA_PROBE_WAV_LOOP_ENABLED,
        PROBE_PATTERN_NAME,
        PIANO_PROBE_PATTERN_NAME,
        PROBE_PATTERN_LOOPS,
    );

    if HDA_PROBE_WAV_LOOP_ENABLED {
        hda_wav_loop_probe_task().await;
        return;
    }

    crate::log_trace!(
        "intel/hda-probe: task start pattern={} loops={}\n",
        PROBE_PATTERN_NAME,
        PROBE_PATTERN_LOOPS,
    );

    let mut last_piano_seq: Option<u16> = None;
    let mut last_bassline_toggle_seq = crate::aud::bassline_toggle_seq();
    let mut bassline_active = false;
    let mut bassline_started_ms = 0u64;
    let mut bassline_beat_ms = 60000u64 / 116;
    let mut pending_bassline_target: Option<bool> = None;
    let mut pending_bassline_due_ms = 0u64;

    loop {
        let toggle_seq = crate::aud::bassline_toggle_seq();
        let toggle_delta = toggle_seq.wrapping_sub(last_bassline_toggle_seq);
        if toggle_delta != 0 {
            last_bassline_toggle_seq = toggle_seq;
            if (toggle_delta & 1) != 0 {
                let now = uptime_ms();
                let target = pending_bassline_target
                    .map(|pending| !pending)
                    .unwrap_or(!bassline_active);
                if target == bassline_active {
                    pending_bassline_target = None;
                    crate::log_trace!(
                        "intel/hda-probe: bassline toggle canceled active={}\n",
                        bassline_active
                    );
                } else {
                    pending_bassline_due_ms = if bassline_active {
                        next_running_beat_ms(now, bassline_started_ms, bassline_beat_ms)
                    } else {
                        next_grid_ms(now, bassline_beat_ms)
                    };
                    pending_bassline_target = Some(target);
                    crate::log_trace!(
                        "intel/hda-probe: bassline toggle armed target={} due_ms={} now_ms={}\n",
                        target,
                        pending_bassline_due_ms,
                        now
                    );
                }
            }
        }

        if let Some(target) = pending_bassline_target {
            let now = uptime_ms();
            if now >= pending_bassline_due_ms {
                pending_bassline_target = None;
                if target {
                    match crate::aud::render_retro_bassline() {
                        Ok((samples, bpm, step_ms)) => {
                            bassline_beat_ms = (60000u64 / u64::from(bpm)).max(1);
                            match crate::hda::start_looped_playback(samples.as_slice()) {
                                Ok(()) => {
                                    bassline_active = true;
                                    bassline_started_ms = now;
                                    crate::log_trace!(
                                        "intel/hda-probe: bassline on bpm={} beat_ms={} step_ms={} samples={}\n",
                                        bpm,
                                        bassline_beat_ms,
                                        step_ms,
                                        samples.len()
                                    );
                                }
                                Err(err) => {
                                    crate::log_trace!("intel/hda-probe: bassline start err={}\n", err);
                                }
                            }
                        }
                        Err(err) => {
                            crate::log_trace!("intel/hda-probe: bassline render err={}\n", err);
                        }
                    }
                } else {
                    match crate::aud::stop() {
                        Ok(()) => {
                            bassline_active = false;
                            bassline_started_ms = 0;
                            crate::log_trace!("intel/hda-probe: bassline off\n");
                        }
                        Err(err) => {
                            crate::log_trace!("intel/hda-probe: bassline stop err={}\n", err);
                        }
                    }
                }
            }
        }

        if bassline_active || pending_bassline_target.is_some() {
            Timer::after(EmbassyDuration::from_millis(BASSLINE_IDLE_POLL_DELAY_MS)).await;
            continue;
        }

        if piano_claimed() {
            match crate::usb2::midi::piano_held_snapshot() {
                Some(snapshot) if snapshot.len > 0 || last_piano_seq != Some(snapshot.seq) => {
                    let log_event = last_piano_seq != Some(snapshot.seq);
                    last_piano_seq = Some(snapshot.seq);
                    match play_piano_probe_held(&snapshot, log_event) {
                        Ok(pattern_name) => {
                            if log_event {
                                crate::log_trace!(
                                    "intel/hda-probe: pattern ok name={} notes={}\n",
                                    pattern_name,
                                    snapshot.len,
                                );
                            }
                            Timer::after(EmbassyDuration::from_millis(PIANO_NOTE_POLL_DELAY_MS))
                                .await;
                        }
                        Err(err) => {
                            crate::log_trace!(
                                "intel/hda-probe: pattern err piano_claimed={} err={}\n",
                                true,
                                err,
                            );
                            Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
                        }
                    }
                }
                Some(_) | None => {
                    Timer::after(EmbassyDuration::from_millis(PIANO_NOTE_POLL_DELAY_MS)).await;
                }
            }
        } else {
            last_piano_seq = None;
            match play_default_probe_pattern() {
                Ok(pattern_name) => {
                    crate::log_trace!(
                        "intel/hda-probe: pattern ok name={} loops={}\n",
                        pattern_name,
                        PROBE_PATTERN_LOOPS,
                    );
                    Timer::after(EmbassyDuration::from_millis(PROBE_LOOP_DELAY_MS)).await;
                }
                Err(err) => {
                    crate::log_trace!(
                        "intel/hda-probe: pattern err piano_claimed={} err={}\n",
                        false,
                        err,
                    );
                    Timer::after(EmbassyDuration::from_millis(PROBE_RETRY_DELAY_MS)).await;
                }
            }
        }
    }
}
