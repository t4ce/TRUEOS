use std::io;
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail, ensure};
use tts_rs::SynthesisEngine;
use tts_rs::engines::kokoro::{
    KokoroBackend, KokoroEngine, KokoroInferenceParams, KokoroModelParams, SAMPLE_RATE,
};
use trueos_ttstt_protocol::{
    Done, ErrorMessage, Frame, Hello, HelloOk, Kind, PROTOCOL_VERSION, SttOpen, SttText, TtsDone,
    TtsSpeak, read_frame, write_frame,
};

use crate::cli::{ServeArgs, TtsBackend};
use crate::listen::{LiveParams, LiveSession, TranscriptChunk};
use crate::playback::PlaybackStream;
use crate::whisper::Engine as WhisperEngine;
use crate::{is_broken_pipe, status, validate_kokoro_model_dir};

const TTS_QUEUE_DEPTH: usize = 8;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const CLIENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

type Ready = Result<Duration, String>;

#[derive(Clone)]
struct Workers {
    stt: SyncSender<SttJob>,
    tts: SyncSender<TtsJob>,
    stt_active: Arc<AtomicBool>,
}

struct SttJob {
    stream: TcpStream,
    request_id: u32,
    request: SttOpen,
    _active: ActiveStt,
}

struct ActiveStt(Arc<AtomicBool>);

impl Drop for ActiveStt {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

struct TtsJob {
    stream: TcpStream,
    request_id: u32,
    request: TtsSpeak,
}

struct TtsWorkerConfig {
    model_dir: PathBuf,
    backend: TtsBackend,
    threads: usize,
    optimized_model_cache: Option<PathBuf>,
    espeak_bin: Option<PathBuf>,
    espeak_data: Option<PathBuf>,
}

pub(crate) fn run(
    args: ServeArgs,
    quiet: bool,
    default_stt_model: PathBuf,
    default_tts_model_dir: PathBuf,
) -> Result<()> {
    let stt_model = args.stt_model.unwrap_or(default_stt_model);
    let tts_model_dir = args.tts_model_dir.unwrap_or(default_tts_model_dir);
    ensure!(
        stt_model.is_file(),
        "Whisper model {} does not exist; download it as described in README.md, pass --stt-model FILE, or set TTSTT_STT_MODEL",
        stt_model.display()
    );
    ensure!(
        stt_model.to_str().is_some(),
        "Whisper model path must be valid UTF-8 (upstream whisper.cpp limitation)"
    );
    validate_kokoro_model_dir(&tts_model_dir)?;

    let listener = TcpListener::bind(args.bind)
        .with_context(|| format!("failed to bind trueos-ttstt service to {}", args.bind))?;
    if !args.bind.ip().is_loopback() {
        status(
            quiet,
            format!(
                "WARNING: {} is not loopback; trueos-ttstt protocol v1 has no authentication or encryption.",
                args.bind
            ),
        );
    }

    status(
        quiet,
        format!(
            "Loading persistent Whisper model from {} ...",
            stt_model.display()
        ),
    );
    status(
        quiet,
        format!(
            "Loading persistent Kokoro model from {} ...",
            tts_model_dir.display()
        ),
    );

    let (stt, stt_ready) = start_stt_worker(stt_model, quiet)?;
    let (tts, tts_ready) = start_tts_worker(
        TtsWorkerConfig {
            model_dir: tts_model_dir,
            backend: args.tts_backend,
            threads: args.tts_threads,
            optimized_model_cache: args.tts_optimized_model_cache,
            espeak_bin: args.espeak_bin,
            espeak_data: args.espeak_data,
        },
        quiet,
    )?;

    let stt_elapsed = wait_until_ready("Whisper", stt_ready)?;
    let tts_elapsed = wait_until_ready("Kokoro", tts_ready)?;
    status(
        quiet,
        format!("Persistent models ready (Whisper {stt_elapsed:.2?}, Kokoro {tts_elapsed:.2?})."),
    );
    status(
        quiet,
        format!(
            "trueos-ttstt protocol v{PROTOCOL_VERSION} listening on {}; press Ctrl+C to stop.",
            args.bind
        ),
    );

    let workers = Workers {
        stt,
        tts,
        stt_active: Arc::new(AtomicBool::new(false)),
    };
    accept_connections(listener, workers, quiet)
}

fn start_stt_worker(model: PathBuf, quiet: bool) -> Result<(SyncSender<SttJob>, Receiver<Ready>)> {
    let (jobs, receiver) = sync_channel::<SttJob>(1);
    let (ready_sender, ready) = sync_channel(1);
    thread::Builder::new()
        .name("trueos-ttstt-stt-worker".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let mut engine = match WhisperEngine::load(&model) {
                Ok(engine) => engine,
                Err(error) => {
                    let _ = ready_sender.send(Err(format!("{error:#}")));
                    return;
                }
            };
            if ready_sender.send(Ok(started.elapsed())).is_err() {
                return;
            }

            while let Ok(mut job) = receiver.recv() {
                let result =
                    serve_stt_session(&mut engine, &mut job.stream, job.request_id, job.request);
                if let Err(error) = result {
                    let _ = write_error(
                        &mut job.stream,
                        job.request_id,
                        "stt_failed",
                        &format!("{error:#}"),
                        true,
                    );
                    if !is_broken_pipe(&error) {
                        status(quiet, format!("STT request failed: {error:#}"));
                    }
                }
            }
        })
        .context("failed to start persistent Whisper worker")?;
    Ok((jobs, ready))
}

fn start_tts_worker(
    config: TtsWorkerConfig,
    quiet: bool,
) -> Result<(SyncSender<TtsJob>, Receiver<Ready>)> {
    let (jobs, receiver) = sync_channel::<TtsJob>(TTS_QUEUE_DEPTH);
    let (ready_sender, ready) = sync_channel(1);
    thread::Builder::new()
        .name("trueos-ttstt-tts-worker".to_owned())
        .spawn(move || {
            let started = Instant::now();
            let mut engine = KokoroEngine::with_espeak(config.espeak_bin, config.espeak_data);
            if let Err(error) = engine.load_model_with_params(
                &config.model_dir,
                KokoroModelParams {
                    backend: match config.backend {
                        TtsBackend::Ort => KokoroBackend::OnnxRuntime,
                        TtsBackend::Rten => KokoroBackend::Rten,
                    },
                    num_threads: Some(config.threads),
                    optimized_model_cache_path: config.optimized_model_cache,
                },
            ) {
                let _ = ready_sender.send(Err(format!(
                    "failed to load Kokoro model from {}: {error}",
                    config.model_dir.display()
                )));
                return;
            }
            if ready_sender.send(Ok(started.elapsed())).is_err() {
                return;
            }

            while let Ok(mut job) = receiver.recv() {
                let result = serve_tts_request(
                    &mut engine,
                    &config.model_dir,
                    &mut job.stream,
                    job.request_id,
                    job.request,
                );
                if let Err(error) = result {
                    let _ = write_error(
                        &mut job.stream,
                        job.request_id,
                        "tts_failed",
                        &format!("{error:#}"),
                        true,
                    );
                    if !is_broken_pipe(&error) {
                        status(quiet, format!("TTS request failed: {error:#}"));
                    }
                }
            }
        })
        .context("failed to start persistent Kokoro worker")?;
    Ok((jobs, ready))
}

fn wait_until_ready(name: &str, ready: Receiver<Ready>) -> Result<Duration> {
    match ready.recv() {
        Ok(Ok(elapsed)) => Ok(elapsed),
        Ok(Err(message)) => bail!("{name} worker could not start: {message}"),
        Err(_) => bail!("{name} worker stopped before reporting readiness"),
    }
}

fn accept_connections(listener: TcpListener, workers: Workers, quiet: bool) -> Result<()> {
    loop {
        let (stream, peer) = match listener.accept() {
            Ok(connection) => connection,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(error).context("failed to accept a trueos-ttstt connection");
            }
        };
        let connection_workers = workers.clone();
        if let Err(error) = thread::Builder::new()
            .name("trueos-ttstt-connection".to_owned())
            .spawn(move || {
                if let Err(error) = handle_connection(stream, connection_workers) {
                    status(quiet, format!("Connection from {peer} failed: {error:#}"));
                }
            })
        {
            status(
                quiet,
                format!("Could not start handler for connection from {peer}: {error}"),
            );
        }
    }
}

fn handle_connection(mut stream: TcpStream, workers: Workers) -> Result<()> {
    stream
        .set_nodelay(true)
        .context("failed to enable TCP_NODELAY")?;
    stream
        .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
        .context("failed to set handshake timeout")?;
    stream
        .set_write_timeout(Some(CLIENT_WRITE_TIMEOUT))
        .context("failed to set client write timeout")?;

    let Some(hello_frame) = read_frame(&mut stream).context("failed to read protocol hello")?
    else {
        return Ok(());
    };
    if hello_frame.kind != Kind::Hello || hello_frame.request_id != 0 {
        write_error(
            &mut stream,
            hello_frame.request_id,
            "hello_required",
            "the first frame must be HELLO with request id 0",
            true,
        )?;
        return Ok(());
    }
    let hello: Hello = match hello_frame.decode_json() {
        Ok(hello) => hello,
        Err(error) => {
            write_error(&mut stream, 0, "invalid_hello", &error.to_string(), true)?;
            return Ok(());
        }
    };
    if hello.client.trim().is_empty() {
        write_error(
            &mut stream,
            0,
            "invalid_hello",
            "HELLO client name cannot be empty",
            true,
        )?;
        return Ok(());
    }
    write_json(
        &mut stream,
        Kind::HelloOk,
        0,
        &HelloOk {
            server: format!("trueos-ttstt/{}", env!("CARGO_PKG_VERSION")),
            protocol: PROTOCOL_VERSION,
        },
    )?;

    let operation = loop {
        let Some(frame) = read_frame(&mut stream).context("failed to read operation frame")? else {
            return Ok(());
        };
        if frame.kind == Kind::Ping {
            ensure!(frame.payload.is_empty(), "PING payload must be empty");
            write_frame(&mut stream, &Frame::empty(Kind::Pong, frame.request_id))?;
            continue;
        }
        break frame;
    };

    if operation.request_id == 0 {
        write_error(
            &mut stream,
            0,
            "invalid_request_id",
            "operation request ids must be nonzero",
            true,
        )?;
        return Ok(());
    }
    stream
        .set_read_timeout(None)
        .context("failed to clear handshake timeout")?;

    match operation.kind {
        Kind::SttOpen => dispatch_stt(stream, operation, workers),
        Kind::TtsSpeak => dispatch_tts(stream, operation, workers),
        // Reserved in protocol v1. The TTS worker deliberately owns playback
        // synchronously and does not claim cancellation support.
        Kind::TtsCancel => {
            write_error(
                &mut stream,
                operation.request_id,
                "tts_cancel_unsupported",
                "TTS cancellation is reserved but not implemented in protocol v1",
                true,
            )?;
            Ok(())
        }
        _ => {
            write_error(
                &mut stream,
                operation.request_id,
                "invalid_operation",
                "expected STT_OPEN or TTS_SPEAK after HELLO",
                true,
            )?;
            Ok(())
        }
    }
}

fn dispatch_stt(mut stream: TcpStream, frame: Frame, workers: Workers) -> Result<()> {
    let request: SttOpen = match frame.decode_json() {
        Ok(request) => request,
        Err(error) => {
            write_error(
                &mut stream,
                frame.request_id,
                "invalid_stt_request",
                &error.to_string(),
                true,
            )?;
            return Ok(());
        }
    };
    if let Err(error) = validate_stt_request(&request) {
        write_error(
            &mut stream,
            frame.request_id,
            "invalid_stt_request",
            &format!("{error:#}"),
            true,
        )?;
        return Ok(());
    }

    if workers
        .stt_active
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        write_error(
            &mut stream,
            frame.request_id,
            "stt_busy",
            "another continuous STT session already owns the Whisper worker",
            true,
        )?;
        return Ok(());
    }

    let job = SttJob {
        stream,
        request_id: frame.request_id,
        request,
        _active: ActiveStt(Arc::clone(&workers.stt_active)),
    };
    match workers.stt.try_send(job) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(mut job)) => {
            write_error(
                &mut job.stream,
                job.request_id,
                "stt_busy",
                "Whisper worker is busy",
                true,
            )?;
            Ok(())
        }
        Err(TrySendError::Disconnected(mut job)) => {
            write_error(
                &mut job.stream,
                job.request_id,
                "stt_unavailable",
                "Whisper worker is not running",
                true,
            )?;
            Ok(())
        }
    }
}

fn dispatch_tts(mut stream: TcpStream, frame: Frame, workers: Workers) -> Result<()> {
    let request: TtsSpeak = match frame.decode_json() {
        Ok(request) => request,
        Err(error) => {
            write_error(
                &mut stream,
                frame.request_id,
                "invalid_tts_request",
                &error.to_string(),
                true,
            )?;
            return Ok(());
        }
    };
    if let Err(error) = validate_tts_request(&request) {
        write_error(
            &mut stream,
            frame.request_id,
            "invalid_tts_request",
            &format!("{error:#}"),
            true,
        )?;
        return Ok(());
    }

    let job = TtsJob {
        stream,
        request_id: frame.request_id,
        request,
    };
    match workers.tts.try_send(job) {
        Ok(()) => Ok(()),
        Err(TrySendError::Full(mut job)) => {
            write_error(
                &mut job.stream,
                job.request_id,
                "tts_busy",
                "Kokoro request queue is full",
                true,
            )?;
            Ok(())
        }
        Err(TrySendError::Disconnected(mut job)) => {
            write_error(
                &mut job.stream,
                job.request_id,
                "tts_unavailable",
                "Kokoro worker is not running",
                true,
            )?;
            Ok(())
        }
    }
}

fn validate_stt_request(request: &SttOpen) -> Result<()> {
    ensure!(
        request.detect_language || !request.language.trim().is_empty(),
        "language cannot be empty"
    );
    ensure!(
        request.vad_threshold.is_finite() && (0.0..=1.0).contains(&request.vad_threshold),
        "VAD threshold must be between 0 and 1"
    );
    ensure!(
        request.no_speech_threshold.is_finite()
            && (0.0..=1.0).contains(&request.no_speech_threshold),
        "no-speech threshold must be between 0 and 1"
    );
    Ok(())
}

fn validate_tts_request(request: &TtsSpeak) -> Result<()> {
    ensure!(!request.text.trim().is_empty(), "text cannot be empty");
    ensure!(!request.voice.trim().is_empty(), "voice cannot be empty");
    ensure!(
        request.speed.is_finite() && (0.5..=2.0).contains(&request.speed),
        "speed must be between 0.5 and 2"
    );
    Ok(())
}

fn serve_stt_session(
    engine: &mut WhisperEngine,
    stream: &mut TcpStream,
    request_id: u32,
    request: SttOpen,
) -> Result<()> {
    let language = (!request.detect_language).then_some(request.language);
    engine.validate(language.as_deref(), request.translate)?;
    let mut session = LiveSession::new(LiveParams {
        language,
        translate: request.translate,
        prompt: request.prompt,
        vad_threshold: request.vad_threshold,
        no_speech_threshold: request.no_speech_threshold,
    });
    write_frame(stream, &Frame::empty(Kind::SttReady, request_id))?;

    loop {
        let Some(frame) = read_frame(stream)? else {
            return Ok(());
        };
        ensure!(
            frame.request_id == request_id,
            "STT frame request id {} does not match active request {request_id}",
            frame.request_id
        );
        match frame.kind {
            Kind::SttPcm => {
                // Whisper inference is synchronous, so a queued CANCEL or EOF
                // is observed after the current (at most ten-second) window.
                session.push_pcm(engine, &frame.payload, |chunk| {
                    write_stt_chunk(stream, request_id, chunk)
                })?;
            }
            Kind::SttFinish => {
                ensure!(frame.payload.is_empty(), "STT_FINISH payload must be empty");
                session.finish(engine, true, |chunk| {
                    write_stt_chunk(stream, request_id, chunk)
                })?;
                write_json(
                    stream,
                    Kind::SttDone,
                    request_id,
                    &Done {
                        reason: "finished".to_owned(),
                    },
                )?;
                return Ok(());
            }
            Kind::SttCancel => {
                ensure!(frame.payload.is_empty(), "STT_CANCEL payload must be empty");
                write_json(
                    stream,
                    Kind::SttDone,
                    request_id,
                    &Done {
                        reason: "cancelled".to_owned(),
                    },
                )?;
                return Ok(());
            }
            Kind::Ping => {
                ensure!(frame.payload.is_empty(), "PING payload must be empty");
                write_frame(stream, &Frame::empty(Kind::Pong, frame.request_id))?;
            }
            other => bail!("unexpected {other:?} frame during STT session"),
        }
    }
}

fn write_stt_chunk(stream: &mut TcpStream, request_id: u32, chunk: TranscriptChunk) -> Result<()> {
    write_json(
        stream,
        Kind::SttText,
        request_id,
        &SttText {
            sequence: chunk.sequence,
            text: chunk.text,
            utterance_end: chunk.utterance_end,
        },
    )
}

fn serve_tts_request(
    engine: &mut KokoroEngine,
    model_dir: &std::path::Path,
    stream: &mut TcpStream,
    request_id: u32,
    request: TtsSpeak,
) -> Result<()> {
    ensure!(
        engine.list_voices().contains(&request.voice.as_str()),
        "Kokoro voice {:?} is not present in {}",
        request.voice,
        model_dir.display()
    );
    write_frame(stream, &Frame::empty(Kind::TtsAccepted, request_id))?;

    let inference = KokoroInferenceParams {
        voice: request.voice,
        speed: request.speed,
        style_index: None,
    };
    let mut player = PlaybackStream::start(SAMPLE_RATE)?;
    let mut sample_count = 0_usize;
    engine
        .synthesize_streaming(&request.text, Some(inference), |chunk| {
            sample_count = sample_count.saturating_add(chunk.samples.len());
            player.enqueue(chunk)
        })
        .map_err(|error| anyhow!("Kokoro streaming synthesis failed: {error}"))?;
    player.finish()?;

    let duration_ms = u64::try_from(sample_count)
        .unwrap_or(u64::MAX)
        .saturating_mul(1_000)
        / u64::from(SAMPLE_RATE);
    write_json(
        stream,
        Kind::TtsDone,
        request_id,
        &TtsDone {
            reason: "played".to_owned(),
            duration_ms,
        },
    )
}

fn write_json<T: serde::Serialize>(
    stream: &mut TcpStream,
    kind: Kind,
    request_id: u32,
    value: &T,
) -> Result<()> {
    let frame = Frame::json(kind, request_id, value)?;
    write_frame(stream, &frame)?;
    Ok(())
}

fn write_error(
    stream: &mut TcpStream,
    request_id: u32,
    code: &str,
    message: &str,
    fatal: bool,
) -> Result<()> {
    write_json(
        stream,
        Kind::Error,
        request_id,
        &ErrorMessage {
            code: code.to_owned(),
            message: message.to_owned(),
            fatal,
        },
    )
}

#[cfg(test)]
mod tests {
    use std::net::{TcpListener, TcpStream};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{Receiver, sync_channel};
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    use trueos_ttstt_protocol::{
        ErrorMessage, Frame, Hello, HelloOk, Kind, PROTOCOL_VERSION, SttOpen, TtsSpeak, read_frame,
        write_frame,
    };

    use super::{
        SttJob, TtsJob, Workers, handle_connection, validate_stt_request, validate_tts_request,
    };

    #[test]
    fn protocol_request_validation_rejects_non_finite_numbers() {
        let stt = SttOpen {
            vad_threshold: f32::NAN,
            ..SttOpen::default()
        };
        assert!(validate_stt_request(&stt).is_err());

        let tts = TtsSpeak {
            text: "hello".to_owned(),
            voice: "af_heart".to_owned(),
            speed: f32::INFINITY,
        };
        assert!(validate_tts_request(&tts).is_err());
    }

    #[test]
    fn protocol_request_validation_accepts_defaults() {
        assert!(validate_stt_request(&SttOpen::default()).is_ok());
        assert!(
            validate_stt_request(&SttOpen {
                language: String::new(),
                detect_language: true,
                ..SttOpen::default()
            })
            .is_ok()
        );
        assert!(
            validate_tts_request(&TtsSpeak {
                text: "hello".to_owned(),
                voice: "af_heart".to_owned(),
                speed: 1.0,
            })
            .is_ok()
        );
    }

    #[test]
    fn hello_then_tts_request_is_dispatched_with_the_same_id() {
        let (workers, _stt_jobs, tts_jobs) = fake_workers();
        let (mut client, handler) = start_handler(workers);
        handshake(&mut client);
        write_frame(
            &mut client,
            &Frame::json(
                Kind::TtsSpeak,
                17,
                &TtsSpeak {
                    text: "hello".to_owned(),
                    voice: "af_heart".to_owned(),
                    speed: 1.0,
                },
            )
            .unwrap(),
        )
        .unwrap();

        handler.join().unwrap().unwrap();
        let job = tts_jobs.recv().unwrap();
        assert_eq!(job.request_id, 17);
        assert_eq!(job.request.text, "hello");
    }

    #[test]
    fn wrong_first_frame_receives_a_fatal_protocol_error() {
        let (workers, _stt_jobs, _tts_jobs) = fake_workers();
        let (mut client, handler) = start_handler(workers);
        write_frame(&mut client, &Frame::empty(Kind::Ping, 0)).unwrap();

        let response = read_frame(&mut client).unwrap().unwrap();
        assert_eq!(response.kind, Kind::Error);
        let error: ErrorMessage = response.decode_json().unwrap();
        assert_eq!(error.code, "hello_required");
        assert!(error.fatal);
        handler.join().unwrap().unwrap();
    }

    #[test]
    fn tts_cancel_is_explicitly_reserved_and_unsupported() {
        let (workers, _stt_jobs, _tts_jobs) = fake_workers();
        let (mut client, handler) = start_handler(workers);
        handshake(&mut client);
        write_frame(&mut client, &Frame::empty(Kind::TtsCancel, 23)).unwrap();

        let response = read_frame(&mut client).unwrap().unwrap();
        let error: ErrorMessage = response.decode_json().unwrap();
        assert_eq!(error.code, "tts_cancel_unsupported");
        handler.join().unwrap().unwrap();
    }

    #[test]
    fn dropping_a_queued_stt_job_releases_the_single_session_lease() {
        let (workers, stt_jobs, _tts_jobs) = fake_workers();
        let active = Arc::clone(&workers.stt_active);
        let (mut client, handler) = start_handler(workers);
        handshake(&mut client);
        write_frame(
            &mut client,
            &Frame::json(Kind::SttOpen, 31, &SttOpen::default()).unwrap(),
        )
        .unwrap();

        handler.join().unwrap().unwrap();
        let job = stt_jobs.recv().unwrap();
        assert!(active.load(Ordering::Acquire));
        drop(job);
        assert!(!active.load(Ordering::Acquire));
    }

    fn fake_workers() -> (Workers, Receiver<SttJob>, Receiver<TtsJob>) {
        let (stt, stt_jobs) = sync_channel(1);
        let (tts, tts_jobs) = sync_channel(1);
        (
            Workers {
                stt,
                tts,
                stt_active: Arc::new(AtomicBool::new(false)),
            },
            stt_jobs,
            tts_jobs,
        )
    }

    fn start_handler(workers: Workers) -> (TcpStream, JoinHandle<anyhow::Result<()>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let client = TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let (server, _) = listener.accept().unwrap();
        let handler = thread::spawn(move || handle_connection(server, workers));
        (client, handler)
    }

    fn handshake(client: &mut TcpStream) {
        write_frame(
            client,
            &Frame::json(
                Kind::Hello,
                0,
                &Hello {
                    client: "server-test".to_owned(),
                },
            )
            .unwrap(),
        )
        .unwrap();
        let response = read_frame(client).unwrap().unwrap();
        assert_eq!(response.kind, Kind::HelloOk);
        let hello: HelloOk = response.decode_json().unwrap();
        assert_eq!(hello.protocol, PROTOCOL_VERSION);
    }
}
