mod audio;
mod cli;
mod listen;
mod output;
mod playback;
mod server;
mod whisper;

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail, ensure};
use clap::Parser;
use cli::{Cli, Command, ListenArgs, OutputFormat, SttArgs, TtsArgs, TtsBackend};
use transcribe_rs::TranscriptionResult;
use tts_rs::SynthesisEngine;
use tts_rs::engines::kokoro::{
    KokoroBackend, KokoroEngine, KokoroInferenceParams, KokoroModelParams, SAMPLE_RATE,
};
use whisper::{Engine as WhisperEngine, InferenceParams as WhisperInferenceParams};

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if is_broken_pipe(&error) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Tts(args) => run_tts(args, cli.quiet),
        Command::Stt(args) => run_stt_command(args, cli.quiet),
        Command::Listen(args) => listen::run(args, cli.quiet, default_stt_model()),
        Command::Serve(args) => server::run(
            args,
            cli.quiet,
            default_stt_model(),
            default_tts_model_dir(),
        ),
        Command::Paths => print_default_paths(),
    }
}

fn run_stt_command(mut args: SttArgs, quiet: bool) -> Result<()> {
    let continuous = args
        .input
        .as_deref()
        .is_none_or(|path| path == Path::new("-"));
    if !continuous {
        return run_stt(args, quiet);
    }

    ensure!(
        matches!(args.format, OutputFormat::Text | OutputFormat::Jsonl),
        "continuous STT supports --format text or --format jsonl"
    );
    let live_args = ListenArgs {
        model: args.model.take(),
        language: args.language.take().unwrap_or_else(|| "en".to_owned()),
        detect_language: args.detect_language,
        translate: args.translate,
        prompt: args.prompt.take(),
        output: args.output.take(),
        format: args.format,
        vad_threshold: args.vad_threshold,
        no_speech_threshold: args.no_speech_threshold,
    };
    listen::run(live_args, quiet, default_stt_model())
}

fn run_tts(mut args: TtsArgs, quiet: bool) -> Result<()> {
    let model_dir = args.model_dir.take().unwrap_or_else(default_tts_model_dir);
    validate_kokoro_model_dir(&model_dir)?;

    let text = if args.list_voices {
        None
    } else {
        Some(read_text(&args)?)
    };

    status(
        quiet,
        format!("Loading Kokoro model from {} ...", model_dir.display()),
    );
    let load_started = Instant::now();
    let mut engine = KokoroEngine::with_espeak(args.espeak_bin, args.espeak_data);
    engine
        .load_model_with_params(
            &model_dir,
            KokoroModelParams {
                backend: match args.backend {
                    TtsBackend::Ort => KokoroBackend::OnnxRuntime,
                    TtsBackend::Rten => KokoroBackend::Rten,
                },
                num_threads: args.threads,
                optimized_model_cache_path: args.optimized_model_cache,
            },
        )
        .map_err(|error| {
            anyhow!(
                "failed to load Kokoro model from {}: {error}",
                model_dir.display()
            )
        })?;
    status(
        quiet,
        format!("Kokoro model loaded in {:.2?}.", load_started.elapsed()),
    );

    if args.list_voices {
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        for voice in engine.list_voices() {
            writeln!(stdout, "{voice}").context("failed to write voices to standard output")?;
        }
        return Ok(());
    }

    ensure!(
        engine.list_voices().contains(&args.voice.as_str()),
        "Kokoro voice {:?} is not present in {}; run `trueos-ttstt tts --list-voices`",
        args.voice,
        model_dir.display()
    );

    let text = text.expect("text is present unless voices were listed");
    let inference = KokoroInferenceParams {
        voice: args.voice,
        speed: args.speed,
        style_index: None,
    };

    if let Some(output) = args.output.as_deref() {
        status(
            quiet,
            format!("Synthesizing with voice {} ...", inference.voice),
        );
        let synthesis_started = Instant::now();
        let result = if args.phonemes {
            engine
                .synthesize_phonemes(&text, Some(inference))
                .map_err(|error| anyhow!("Kokoro synthesis failed: {error}"))?
        } else {
            engine
                .synthesize(&text, Some(inference))
                .map_err(|error| anyhow!("Kokoro synthesis failed: {error}"))?
        };
        result
            .write_wav(output)
            .map_err(|error| anyhow!("failed to write {}: {error}", output.display()))?;

        status(
            quiet,
            format!(
                "Wrote {:.2}s of speech to {} in {:.2?}.",
                result.duration_secs(),
                output.display(),
                synthesis_started.elapsed()
            ),
        );
        return Ok(());
    }

    status(
        quiet,
        format!(
            "Synthesizing and playing with voice {} ...",
            inference.voice
        ),
    );
    let total_started = Instant::now();
    let mut player = playback::PlaybackStream::start(SAMPLE_RATE)?;
    let synthesis_started = Instant::now();
    let mut sample_count = 0_usize;
    let mut enqueue = |chunk: tts_rs::SynthesisResult| {
        sample_count += chunk.samples.len();
        player.enqueue(chunk)
    };
    if args.phonemes {
        engine
            .synthesize_phonemes_streaming(&text, Some(inference), &mut enqueue)
            .map_err(|error| anyhow!("Kokoro streaming synthesis failed: {error}"))?;
    } else {
        engine
            .synthesize_streaming(&text, Some(inference), &mut enqueue)
            .map_err(|error| anyhow!("Kokoro streaming synthesis failed: {error}"))?;
    }
    let synthesis_elapsed = synthesis_started.elapsed();
    let duration = sample_count as f64 / f64::from(SAMPLE_RATE);

    status(
        quiet,
        format!(
            "Synthesized {duration:.2}s of speech in {synthesis_elapsed:.2?}; finishing playback ..."
        ),
    );
    player.finish()?;
    status(
        quiet,
        format!(
            "Played and discarded {duration:.2}s of speech in {:.2?} total.",
            total_started.elapsed()
        ),
    );
    Ok(())
}

fn run_stt(mut args: SttArgs, quiet: bool) -> Result<()> {
    let model = args.model.take().unwrap_or_else(default_stt_model);
    let input = args.input.as_deref().expect("file STT has an input path");
    ensure!(
        input.is_file(),
        "audio file {} does not exist",
        input.display()
    );
    ensure!(
        model.is_file(),
        "Whisper model {} does not exist; download it as described in README.md, pass --model FILE, or set TTSTT_STT_MODEL",
        resolved_path(&model).display()
    );
    ensure!(
        model.to_str().is_some(),
        "Whisper model path must be valid UTF-8 (upstream whisper.cpp limitation)"
    );
    status(
        quiet,
        format!("Reading and converting {} ...", input.display()),
    );
    let samples = audio::read_for_whisper(input)?;
    let audio_duration = samples.len() as f64 / audio::WHISPER_SAMPLE_RATE as f64;
    let audio_context = whisper::short_audio_context(samples.len());

    status(
        quiet,
        format!("Loading Whisper model from {} ...", model.display()),
    );
    let load_started = Instant::now();
    let mut engine = WhisperEngine::load(&model)?;
    status(
        quiet,
        format!("Whisper model loaded in {:.2?}.", load_started.elapsed()),
    );

    let context_description = audio_context.map_or_else(
        || "the full model context".to_owned(),
        |context| format!("a {:.2}s context", whisper::context_seconds(context)),
    );
    status(
        quiet,
        format!("Transcribing {audio_duration:.2}s of audio with {context_description} ..."),
    );
    let transcribe_started = Instant::now();
    let result = engine.transcribe(
        &samples,
        WhisperInferenceParams {
            language: args.language.as_deref(),
            translate: args.translate,
            prompt: args.prompt.as_deref(),
            no_speech_threshold: args.no_speech_threshold,
            audio_context,
            token_timestamps: false,
        },
    )?;
    status(
        quiet,
        format!(
            "Transcription completed in {:.2?}.",
            transcribe_started.elapsed()
        ),
    );

    write_transcription(&result, args.format, args.output.as_deref())
}

fn read_text(args: &TtsArgs) -> Result<String> {
    let text = if !args.text.is_empty() {
        args.text.join(" ")
    } else if let Some(path) = &args.input {
        if path == Path::new("-") {
            read_stdin()?
        } else {
            fs::read_to_string(path)
                .with_context(|| format!("failed to read text file {}", path.display()))?
        }
    } else if !io::stdin().is_terminal() {
        read_stdin()?
    } else {
        bail!("no text provided; pass TEXT, use --input FILE, or pipe text on standard input")
    };

    let text = text.trim();
    ensure!(!text.is_empty(), "input text is empty");
    Ok(text.to_owned())
}

fn read_stdin() -> Result<String> {
    let mut text = String::new();
    io::stdin()
        .read_to_string(&mut text)
        .context("failed to read text from standard input")?;
    Ok(text)
}

fn validate_kokoro_model_dir(model_dir: &Path) -> Result<()> {
    ensure!(
        model_dir.is_dir(),
        "Kokoro model directory {} does not exist; download it as described in README.md, pass --model-dir DIR, or set TTSTT_TTS_MODEL_DIR (relative paths use the current working directory)",
        resolved_path(model_dir).display()
    );

    let voices = model_dir.join("voices-v1.0.bin");
    ensure!(
        voices.is_file(),
        "Kokoro voice archive {} is missing; see README.md for model setup",
        voices.display()
    );

    let preferred_model = model_dir.join("kokoro-quant-convinteger.onnx");
    if preferred_model.is_file() {
        return Ok(());
    }

    let onnx_files = fs::read_dir(model_dir)
        .with_context(|| format!("failed to inspect {}", model_dir.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "onnx")
        })
        .collect::<Vec<_>>();

    match onnx_files.as_slice() {
        [] => bail!(
            "no .onnx model found in {}; see README.md for model setup",
            model_dir.display()
        ),
        [_] => Ok(()),
        _ => bail!(
            "multiple .onnx models found in {}; keep one model or name the preferred one kokoro-quant-convinteger.onnx",
            model_dir.display()
        ),
    }
}

fn resolved_path(path: &Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    }
}

fn default_model_root() -> PathBuf {
    project_state_dir().join("models")
}

fn default_tts_model_dir() -> PathBuf {
    if nonempty_env_path("TTSTT_HOME").is_some() {
        return default_model_root().join("kokoro");
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../ttstt/models/kokoro")
}

fn default_stt_model() -> PathBuf {
    default_model_root().join("whisper").join("ggml-base.bin")
}

fn project_state_dir() -> PathBuf {
    nonempty_env_path("TTSTT_HOME")
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".ttstt"))
}

fn nonempty_env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn print_default_paths() -> Result<()> {
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    writeln!(
        stdout,
        "TTS model directory: {}",
        default_tts_model_dir().display()
    )
    .context("failed to write model paths to standard output")?;
    writeln!(stdout, "STT model file: {}", default_stt_model().display())
        .context("failed to write model paths to standard output")?;
    Ok(())
}

fn write_transcription(
    result: &TranscriptionResult,
    format: cli::OutputFormat,
    destination: Option<&Path>,
) -> Result<()> {
    let rendered = output::render(result, format);
    match destination {
        None => io::stdout()
            .write_all(rendered.as_bytes())
            .context("failed to write transcription to standard output"),
        Some(path) if path == Path::new("-") => io::stdout()
            .write_all(rendered.as_bytes())
            .context("failed to write transcription to standard output"),
        Some(path) => fs::write(path, rendered)
            .with_context(|| format!("failed to write transcription to {}", path.display())),
    }
}

fn status(quiet: bool, message: String) {
    if !quiet {
        eprintln!("{message}");
    }
}

fn is_broken_pipe(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<io::Error>()
            .is_some_and(|error| error.kind() == io::ErrorKind::BrokenPipe)
    })
}
