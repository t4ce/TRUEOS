extern crate alloc;

use alloc::boxed::Box;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active, switch_matrix_target_slot,
};
use crate::r::ttstt_service::{ServiceState, SpeechRequestError, SttRequest, TtsAudio, TtsRequest};
use crate::shell2::shell2_cmd::ParseOutcome;

const DEFAULT_VOICE: &str = "af_heart";
const DEFAULT_SPEED: f32 = 1.0;
const TTS_TEXT_MAX_BYTES: usize = 8 * 1024;
const TTS_PLAYBACK_CHUNK_FRAMES: usize = 12_000;
const STT_AUDIO_MAX_BYTES: u64 = 64 * 1024 * 1024;
const STT_AUDIO_MAX_SECONDS: usize = 5 * 60;
const STT_SAMPLE_RATE_HZ: u32 = 16_000;
const STT_CONVERT_YIELD_FRAMES: usize = 8 * 1024;

pub(crate) fn try_parse_tts(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let input = rest.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("help") {
        tts_usage(io);
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("status") {
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("stop") {
        let generation = crate::aud::pcm_lane::request_stop();
        print_shell_line(io, alloc::format!("tts: playback stop generation={generation}").as_str());
        return ParseOutcome::Handled;
    }

    let (text, voice, speed) = match parse_tts_request(input) {
        Ok(request) => request,
        Err(reason) => {
            print_shell_line(io, alloc::format!("tts: rejected reason={reason}").as_str());
            tts_usage(io);
            return ParseOutcome::Handled;
        }
    };
    if crate::r::ttstt_service::speech_backend_name().is_none() {
        print_shell_line(
            io,
            "tts: unavailable reason=native-kokoro-backend-unregistered; models/pool status follows",
        );
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::status().state != ServiceState::Ready {
        print_shell_line(io, "tts: unavailable reason=model-service-not-ready");
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if !crate::r::ttstt_service::speech_backend_ready() {
        print_shell_line(io, "tts: unavailable reason=native-kokoro-backend-warming");
        print_status(io, "tts");
        return ParseOutcome::Handled;
    }
    if !crate::r::readiness::is_set(crate::r::readiness::INTEL_HDA_READY) {
        print_shell_line(io, "tts: unavailable reason=intel-hda-not-ready");
        return ParseOutcome::Handled;
    }

    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, "tts");
    set_matrix_target_active(&target, true);
    let completion_target = target.clone();
    let complete = Box::new(move |result: Result<TtsAudio, &'static str>| match result {
        Ok(audio) => start_tts_playback(completion_target, audio),
        Err(reason) => {
            print_matrix_target_line(
                &completion_target,
                alloc::format!("tts: inference failed reason={reason}").as_str(),
            );
            set_matrix_target_active(&completion_target, false);
        }
    });
    let request = TtsRequest {
        text,
        voice: voice.clone(),
        speed,
        complete,
    };
    match crate::r::ttstt_service::submit_tts(request) {
        Ok(id) => print_matrix_target_line(
            &target,
            alloc::format!("tts: queued id={id} voice={voice} speed={speed:.2}").as_str(),
        ),
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("tts: submit failed reason={}", request_error(error)).as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }

    ParseOutcome::Handled
}

pub(crate) fn try_parse_stt(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let input = rest.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("help") {
        stt_usage(io);
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("status") {
        print_status(io, "stt");
        print_capture_status(io);
        return ParseOutcome::Handled;
    }
    if input
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("record"))
    {
        print_capture_status(io);
        print_shell_line(
            io,
            "stt: record is experimental and disabled until HDA input BDL/stream routing is implemented",
        );
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::speech_backend_name().is_none() {
        print_shell_line(
            io,
            "stt: unavailable reason=native-whisper-backend-unregistered; no audio file was read",
        );
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }
    if crate::r::ttstt_service::status().state != ServiceState::Ready {
        print_shell_line(
            io,
            "stt: unavailable reason=model-service-not-ready; no audio file was read",
        );
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }
    if !crate::r::ttstt_service::speech_backend_ready() {
        print_shell_line(
            io,
            "stt: unavailable reason=native-whisper-backend-warming; no audio file was read",
        );
        print_status(io, "stt");
        return ParseOutcome::Handled;
    }

    let command = match parse_stt_file_request(input) {
        Ok(command) => command,
        Err(reason) => {
            print_shell_line(io, alloc::format!("stt: rejected reason={reason}").as_str());
            stt_usage(io);
            return ParseOutcome::Handled;
        }
    };
    let active_target = matrix_target_for_backend(io);
    let target = switch_matrix_target_slot(&active_target, "stt");
    set_matrix_target_active(&target, true);
    match stt_file_task(target.clone(), command) {
        Ok(token) => {
            spawner.spawn(token);
            print_matrix_target_line(&target, "stt: audio load queued on BSP");
        }
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("stt: task spawn failed err={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }
    ParseOutcome::Handled
}

fn print_status(io: &'static dyn ShellBackend2, command: &str) {
    let status = crate::r::ttstt_service::status();
    let state = match status.state {
        ServiceState::WaitingForModels => "waiting-models",
        ServiceState::LoadingModels => "loading-models",
        ServiceState::ModelsResident => "models-resident",
        ServiceState::Ready => "ready",
    };
    let backend = crate::r::ttstt_service::speech_backend_name().unwrap_or("unregistered");
    let backend_state = if backend == "unregistered" {
        "unregistered"
    } else if crate::r::ttstt_service::speech_backend_ready() {
        "ready"
    } else {
        "warming"
    };
    print_shell_line(
        io,
        alloc::format!(
            "{command}: state={state} backend={backend} backend_state={backend_state} resident_bytes={} workers={} outstanding={} policy=AP2+-prefer-pcore",
            status.resident_bytes,
            status.workers,
            status.outstanding_jobs
        )
        .as_str(),
    );
}

fn print_capture_status(io: &'static dyn ShellBackend2) {
    match crate::hda::pcm_capture_capabilities() {
        Some(caps) => print_shell_line(
            io,
            alloc::format!(
                "stt: hda_capture input_streams={} adc_widgets={} mic_pins={} line_input_pins={} dma_configured={}",
                caps.input_streams,
                caps.adc_widgets,
                caps.microphone_pins,
                caps.line_input_pins,
                caps.dma_configured as u8
            )
            .as_str(),
        ),
        None => print_shell_line(io, "stt: hda_capture unavailable reason=hda-not-initialized"),
    }
}

fn parse_tts_request(input: &str) -> Result<(String, String, f32), &'static str> {
    let mut remaining = input.trim_start();
    let mut voice = DEFAULT_VOICE.to_string();
    let mut speed = DEFAULT_SPEED;
    loop {
        let token_end = remaining
            .char_indices()
            .find_map(|(index, ch)| ch.is_whitespace().then_some(index))
            .unwrap_or(remaining.len());
        let token = &remaining[..token_end];
        if let Some(value) = token.strip_prefix("voice=") {
            if value.is_empty() {
                return Err("empty-voice");
            }
            voice = value.to_string();
        } else if let Some(value) = token.strip_prefix("speed=") {
            speed = value.parse::<f32>().map_err(|_| "invalid-speed")?;
            if !speed.is_finite() || !(0.25..=4.0).contains(&speed) {
                return Err("speed-out-of-range-0.25-to-4.0");
            }
        } else {
            break;
        }
        remaining = remaining[token_end..].trim_start();
        if remaining.is_empty() {
            return Err("empty-text");
        }
    }

    let text = if remaining.starts_with('"') {
        parse_quoted_text(remaining)?
    } else {
        remaining.to_string()
    };
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("empty-text");
    }
    if text.len() > TTS_TEXT_MAX_BYTES {
        return Err("text-too-large");
    }
    Ok((text, voice, speed))
}

fn parse_quoted_text(input: &str) -> Result<String, &'static str> {
    let quoted = input
        .strip_prefix('"')
        .ok_or("text-must-start-with-quote")?;
    let mut text = String::new();
    let mut escaped = false;
    for (offset, ch) in quoted.char_indices() {
        if escaped {
            match ch {
                '"' | '\\' => text.push(ch),
                'n' => text.push('\n'),
                'r' => text.push('\r'),
                't' => text.push('\t'),
                _ => return Err("unsupported-escape"),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                let tail = &quoted[offset + ch.len_utf8()..];
                if !tail.trim().is_empty() {
                    return Err("unexpected-text-after-closing-quote");
                }
                return Ok(text);
            }
            _ => text.push(ch),
        }
    }
    Err("missing-closing-quote")
}

struct SttFileCommand {
    path: String,
    language: Option<String>,
    translate: bool,
}

fn parse_stt_file_request(input: &str) -> Result<SttFileCommand, &'static str> {
    let mut args = input.split_whitespace();
    let first = args.next().ok_or("missing-path")?;
    let path = if first.eq_ignore_ascii_case("file") {
        args.next().ok_or("missing-path")?
    } else {
        first
    };
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Err("missing-path");
    }
    let mut language = Some(String::from("en"));
    let mut translate = false;
    for arg in args {
        if let Some(value) = arg.strip_prefix("language=") {
            if value.eq_ignore_ascii_case("auto") {
                language = None;
            } else if value.is_empty() {
                return Err("empty-language");
            } else {
                language = Some(value.to_string());
            }
        } else if arg.eq_ignore_ascii_case("translate") {
            translate = true;
        } else {
            return Err("unknown-option");
        }
    }
    Ok(SttFileCommand {
        path: path.to_string(),
        language,
        translate,
    })
}

#[embassy_executor::task(pool_size = 2)]
async fn stt_file_task(target: MatrixTarget, command: SttFileCommand) {
    let result = load_wav_mono_16k(command.path.as_str()).await;
    let pcm = match result {
        Ok(pcm) => pcm,
        Err(reason) => {
            print_matrix_target_line(&target, alloc::format!("stt: {reason}").as_str());
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let samples = pcm.len();
    let completion_target = target.clone();
    let complete = Box::new(move |result: Result<String, &'static str>| {
        match result {
            Ok(text) => {
                let text = text.replace(['\r', '\n'], " ");
                print_matrix_target_line(
                    &completion_target,
                    alloc::format!("stt: text={text}").as_str(),
                );
            }
            Err(reason) => print_matrix_target_line(
                &completion_target,
                alloc::format!("stt: inference failed reason={reason}").as_str(),
            ),
        }
        set_matrix_target_active(&completion_target, false);
    });
    match crate::r::ttstt_service::submit_stt(SttRequest {
        pcm_f32_mono_16k: pcm,
        language: command.language,
        translate: command.translate,
        complete,
    }) {
        Ok(id) => print_matrix_target_line(
            &target,
            alloc::format!(
                "stt: queued id={id} path=trueosfs:/{} samples={} rate=16000 channels=1",
                command.path,
                samples
            )
            .as_str(),
        ),
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("stt: submit failed reason={}", request_error(error)).as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }
}

async fn load_wav_mono_16k(path: &str) -> Result<Vec<f32>, String> {
    let disk = crate::r::fs::trueosfs::primary_root_handle()
        .ok_or_else(|| String::from("no TRUEOSFS root mounted"))?;
    let info = crate::r::fs::trueosfs::file_info_async(disk, path)
        .await
        .map_err(|error| alloc::format!("file info failed path=trueosfs:/{path} err={error:?}"))?
        .ok_or_else(|| alloc::format!("file missing path=trueosfs:/{path}"))?;
    if info.data_len > STT_AUDIO_MAX_BYTES {
        return Err(alloc::format!(
            "audio too large bytes={} cap={STT_AUDIO_MAX_BYTES}",
            info.data_len
        ));
    }
    let bytes = crate::r::fs::trueosfs::file_out_async(disk, path)
        .await
        .map_err(|error| alloc::format!("file read failed path=trueosfs:/{path} err={error:?}"))?
        .ok_or_else(|| alloc::format!("file missing path=trueosfs:/{path}"))?;
    if bytes.len() as u64 > STT_AUDIO_MAX_BYTES {
        return Err(alloc::format!(
            "audio changed while loading bytes={} cap={STT_AUDIO_MAX_BYTES}",
            bytes.len()
        ));
    }
    let wav = crate::hda::parse_wav(bytes.as_slice()).map_err(String::from)?;
    if wav.bits_per_sample != 16 {
        return Err(String::from("WAV must contain signed 16-bit PCM"));
    }
    if wav.channels == 0 || wav.channels > 2 || wav.sample_rate == 0 {
        return Err(String::from("WAV must be mono/stereo with a nonzero sample rate"));
    }
    let channels = wav.channels as usize;
    let pcm = &bytes[wav.data_offset..wav.data_offset + wav.data_size];
    let source_frames = pcm.len() / (2 * channels);
    let max_source_frames = (wav.sample_rate as usize).saturating_mul(STT_AUDIO_MAX_SECONDS);
    if source_frames > max_source_frames {
        return Err(alloc::format!("audio duration exceeds {} seconds", STT_AUDIO_MAX_SECONDS));
    }
    let output_frames = (source_frames as u64)
        .saturating_mul(STT_SAMPLE_RATE_HZ as u64)
        .div_ceil(wav.sample_rate as u64) as usize;
    let mut out = Vec::new();
    out.try_reserve_exact(output_frames)
        .map_err(|_| String::from("cannot reserve STT PCM buffer"))?;

    for output_frame in 0..output_frames {
        let source_frame = (output_frame as u64)
            .saturating_mul(wav.sample_rate as u64)
            .checked_div(STT_SAMPLE_RATE_HZ as u64)
            .unwrap_or(0) as usize;
        if source_frame >= source_frames {
            break;
        }
        let sample_base = source_frame * channels;
        let left_offset = sample_base * 2;
        let left = i16::from_le_bytes([pcm[left_offset], pcm[left_offset + 1]]) as i32;
        let mono = if channels == 2 {
            let right_offset = left_offset + 2;
            let right = i16::from_le_bytes([pcm[right_offset], pcm[right_offset + 1]]) as i32;
            (left + right) / 2
        } else {
            left
        };
        out.push(mono as f32 / 32768.0);
        if output_frame != 0 && output_frame % STT_CONVERT_YIELD_FRAMES == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }
    if out.is_empty() {
        return Err(String::from("WAV contains no PCM frames"));
    }
    Ok(out)
}

fn start_tts_playback(target: MatrixTarget, audio: TtsAudio) {
    if audio.samples_i16_stereo_48k.is_empty()
        || !audio.samples_i16_stereo_48k.len().is_multiple_of(2)
    {
        print_matrix_target_line(&target, "tts: backend returned malformed PCM");
        set_matrix_target_active(&target, false);
        return;
    }
    let Some(ap1) = crate::workers::ap1_ui_core_spawner() else {
        print_matrix_target_line(&target, "tts: AP1 audio/service core is unavailable");
        set_matrix_target_active(&target, false);
        return;
    };
    let stop_generation = crate::aud::pcm_lane::stop_generation();
    match tts_playback_task(target.clone(), audio.samples_i16_stereo_48k, stop_generation) {
        Ok(token) => ap1.spawn(token),
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("tts: playback task spawn failed err={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
        }
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn tts_playback_task(target: MatrixTarget, samples: Vec<i16>, stop_generation: u32) {
    let total_frames = samples.len() / 2;
    let mut frame_offset = 0usize;
    while frame_offset < total_frames {
        if crate::aud::pcm_lane::stop_generation() != stop_generation {
            print_matrix_target_line(&target, "tts: playback stopped");
            set_matrix_target_active(&target, false);
            return;
        }
        let frames = (total_frames - frame_offset).min(TTS_PLAYBACK_CHUNK_FRAMES);
        while crate::aud::pcm_lane::pending_frames().saturating_add(frames)
            > crate::hda::PCM_SAMPLE_RATE_HZ as usize
        {
            if crate::aud::pcm_lane::stop_generation() != stop_generation {
                print_matrix_target_line(&target, "tts: playback stopped");
                set_matrix_target_active(&target, false);
                return;
            }
            Timer::after(EmbassyDuration::from_millis(5)).await;
        }
        let sample_start = frame_offset * 2;
        let sample_end = sample_start + frames * 2;
        match crate::aud::pcm_lane::submit_i16_stereo_48k(
            "shell2-ttstt",
            samples[sample_start..sample_end].to_vec(),
        ) {
            Ok(_) => frame_offset += frames,
            Err(crate::aud::pcm_lane::PcmLaneError::QueueFull) => {
                Timer::after(EmbassyDuration::from_millis(5)).await;
            }
            Err(error) => {
                print_matrix_target_line(
                    &target,
                    alloc::format!("tts: playback failed err={error:?}").as_str(),
                );
                set_matrix_target_active(&target, false);
                return;
            }
        }
    }
    print_matrix_target_line(
        &target,
        alloc::format!("tts: audio queued frames={total_frames} rate=48000 channels=2").as_str(),
    );
    set_matrix_target_active(&target, false);
}

fn request_error(error: SpeechRequestError) -> String {
    match error {
        SpeechRequestError::BackendUnavailable => String::from("backend-unavailable"),
        SpeechRequestError::BackendWarming => String::from("backend-warming"),
        SpeechRequestError::InvalidRequest(reason) => alloc::format!("invalid-{reason}"),
        SpeechRequestError::BackendRejected(reason) => alloc::format!("backend-{reason}"),
        SpeechRequestError::Service(error) => alloc::format!("service-{error:?}"),
    }
}

fn tts_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "tts: usage `tts [voice=NAME] [speed=0.25..4.0] <text|\"quoted text\">` | `tts status` | `tts stop`",
    );
}

fn stt_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "stt: usage `stt [file] <audio.wav> [language=CODE|auto] [translate]` | `stt status` | `stt record` (experimental)",
    );
}
