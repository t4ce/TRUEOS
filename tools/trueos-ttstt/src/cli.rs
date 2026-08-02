use std::net::SocketAddr;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "trueos-ttstt",
    version,
    about = "Local text-to-speech and speech-to-text",
    long_about = "Convert text to Kokoro speech, transcribe WAV audio, or continuously transcribe raw PCM with Whisper.\n\
                  Inference runs locally after you download the model files.",
    arg_required_else_help = true
)]
pub struct Cli {
    /// Suppress progress messages (playback, output, and errors are unaffected).
    #[arg(short, long, global = true)]
    pub quiet: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Convert text to speech with Kokoro (played by default).
    #[command(visible_alias = "speak")]
    Tts(TtsArgs),

    /// Transcribe a WAV file, or continuously transcribe raw PCM from stdin.
    #[command(visible_alias = "transcribe")]
    Stt(SttArgs),

    /// Continuously transcribe raw 16 kHz mono s16le audio from standard input.
    #[command(visible_alias = "stream")]
    Listen(ListenArgs),

    /// Run the persistent localhost STT/TTS service.
    Serve(ServeArgs),

    /// Print the configured project-local model locations.
    Paths,
}

#[derive(Debug, Args)]
pub struct TtsArgs {
    /// Text to synthesize. If omitted, read --input or standard input.
    #[arg(value_name = "TEXT", num_args = 0.., conflicts_with = "input")]
    pub text: Vec<String>,

    /// Treat input as Kokoro IPA phonemes and do not invoke espeak-ng.
    #[arg(long)]
    pub phonemes: bool,

    /// Read text from this UTF-8 file; use - for standard input.
    #[arg(short = 'f', long, value_name = "FILE", conflicts_with = "text")]
    pub input: Option<PathBuf>,

    /// Directory containing one Kokoro .onnx file and voices-v1.0.bin.
    #[arg(short = 'm', long, env = "TTSTT_TTS_MODEL_DIR", value_name = "DIR")]
    pub model_dir: Option<PathBuf>,

    /// CPU inference runtime. RTen requires a prepared kokoro-rten.onnx graph.
    #[arg(long, value_enum, env = "TTSTT_TTS_BACKEND", default_value_t = TtsBackend::Ort)]
    pub backend: TtsBackend,

    /// Write a WAV file instead of playing through the default audio device.
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Kokoro voice name.
    #[arg(short, long, default_value = "af_heart")]
    pub voice: String,

    /// Speech speed multiplier (0.5 through 2.0).
    #[arg(short, long, default_value_t = 1.0, value_parser = parse_speed)]
    pub speed: f32,

    /// Number of CPU inference threads.
    #[arg(long, value_parser = parse_positive_usize)]
    pub threads: Option<usize>,

    /// Cache the optimized ONNX graph at this writable path.
    #[arg(long, value_name = "FILE")]
    pub optimized_model_cache: Option<PathBuf>,

    /// Explicit espeak-ng executable instead of searching PATH.
    #[arg(long, value_name = "FILE")]
    pub espeak_bin: Option<PathBuf>,

    /// Explicit espeak-ng-data directory.
    #[arg(long, value_name = "DIR")]
    pub espeak_data: Option<PathBuf>,

    /// Load the model, print its available voices, and exit.
    #[arg(long)]
    pub list_voices: bool,
}

#[derive(Debug, Args)]
pub struct SttArgs {
    /// WAV audio to transcribe; omit it or use - for continuous raw PCM on stdin.
    #[arg(value_name = "AUDIO.wav")]
    pub input: Option<PathBuf>,

    /// Whisper GGML/GGUF model file.
    #[arg(short = 'm', long, env = "TTSTT_STT_MODEL", value_name = "FILE")]
    pub model: Option<PathBuf>,

    /// Language hint; file mode auto-detects if omitted, continuous mode uses en.
    #[arg(short, long, value_name = "CODE")]
    pub language: Option<String>,

    /// Auto-detect the spoken language; continuous mode otherwise uses English.
    #[arg(long, conflicts_with = "language")]
    pub detect_language: bool,

    /// Translate multilingual speech to English.
    #[arg(long)]
    pub translate: bool,

    /// Initial text prompt to bias Whisper's decoding.
    #[arg(long, value_name = "TEXT")]
    pub prompt: Option<String>,

    /// Output representation; continuous mode supports text and jsonl.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// Write the result or committed chunks here instead of stdout; use - for stdout.
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Silence/no-speech threshold (0.0 through 1.0).
    #[arg(long, default_value_t = 0.2, value_parser = parse_probability)]
    pub no_speech_threshold: f32,

    /// Continuous-input RMS level above which a 30 ms frame counts as speech.
    #[arg(long, default_value_t = 0.01, value_parser = parse_probability)]
    pub vad_threshold: f32,
}

#[derive(Debug, Args)]
pub struct ListenArgs {
    /// Whisper GGML/GGUF model file.
    #[arg(short = 'm', long, env = "TTSTT_STT_MODEL", value_name = "FILE")]
    pub model: Option<PathBuf>,

    /// Spoken language code. English is the fast default for continuous use.
    #[arg(short, long, default_value = "en", value_name = "CODE")]
    pub language: String,

    /// Auto-detect the spoken language instead of using --language.
    #[arg(long, conflicts_with = "language")]
    pub detect_language: bool,

    /// Translate multilingual speech to English.
    #[arg(long)]
    pub translate: bool,

    /// Initial text prompt to bias each Whisper window.
    #[arg(long, value_name = "TEXT")]
    pub prompt: Option<String>,

    /// Write committed text here instead of stdout; use - for stdout.
    #[arg(short, long, value_name = "FILE")]
    pub output: Option<PathBuf>,

    /// Stream framing. Continuous mode supports text and jsonl.
    #[arg(long, value_enum, default_value_t = OutputFormat::Text)]
    pub format: OutputFormat,

    /// RMS level above which a 30 ms input frame is considered speech.
    #[arg(long, default_value_t = 0.01, value_parser = parse_probability)]
    pub vad_threshold: f32,

    /// Whisper silence/no-speech threshold (0.0 through 1.0).
    #[arg(long, default_value_t = 0.2, value_parser = parse_probability)]
    pub no_speech_threshold: f32,
}

#[derive(Debug, Args)]
pub struct ServeArgs {
    /// TCP address for the service. The loopback-only default is intentionally local.
    #[arg(long, env = "TTSTT_BIND", default_value = "127.0.0.1:1100")]
    pub bind: SocketAddr,

    /// Whisper GGML/GGUF model file loaded once by the STT worker.
    #[arg(long, env = "TTSTT_STT_MODEL", value_name = "FILE")]
    pub stt_model: Option<PathBuf>,

    /// Kokoro model directory loaded once by the TTS worker.
    #[arg(long, env = "TTSTT_TTS_MODEL_DIR", value_name = "DIR")]
    pub tts_model_dir: Option<PathBuf>,

    /// CPU inference runtime for the persistent TTS worker.
    #[arg(long, value_enum, env = "TTSTT_TTS_BACKEND", default_value_t = TtsBackend::Ort)]
    pub tts_backend: TtsBackend,

    /// Number of CPU inference threads used by the TTS worker.
    #[arg(long, default_value_t = 3, value_parser = parse_positive_usize)]
    pub tts_threads: usize,

    /// Cache the optimized Kokoro ONNX graph at this writable path.
    #[arg(long, value_name = "FILE")]
    pub tts_optimized_model_cache: Option<PathBuf>,

    /// Explicit espeak-ng executable instead of searching PATH.
    #[arg(long, value_name = "FILE")]
    pub espeak_bin: Option<PathBuf>,

    /// Explicit espeak-ng-data directory.
    #[arg(long, value_name = "DIR")]
    pub espeak_data: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
    #[value(alias = "ndjson")]
    Jsonl,
    Srt,
    Vtt,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum TtsBackend {
    /// ONNX Runtime compatibility path and numerical oracle.
    #[default]
    Ort,
    /// Native Rust RTen path using the prepared graph.
    Rten,
}

fn parse_speed(value: &str) -> Result<f32, String> {
    parse_bounded_f32(value, 0.5, 2.0, "speed")
}

fn parse_probability(value: &str) -> Result<f32, String> {
    parse_bounded_f32(value, 0.0, 1.0, "probability")
}

fn parse_bounded_f32(value: &str, min: f32, max: f32, name: &str) -> Result<f32, String> {
    let parsed = value
        .parse::<f32>()
        .map_err(|_| format!("{name} must be a number"))?;
    if parsed.is_finite() && (min..=max).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!("{name} must be between {min} and {max}"))
    }
}

fn parse_positive_usize(value: &str) -> Result<usize, String> {
    let parsed = value
        .parse::<usize>()
        .map_err(|_| "thread count must be a positive integer".to_string())?;
    if parsed == 0 {
        Err("thread count must be at least 1".to_string())
    } else {
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{Cli, Command, OutputFormat, TtsBackend};

    #[test]
    fn speak_alias_parses() {
        let cli = Cli::try_parse_from(["trueos-ttstt", "speak", "hello", "world"]).unwrap();
        let Command::Tts(args) = cli.command else {
            panic!("expected tts command");
        };
        assert_eq!(args.text, ["hello", "world"]);
        assert!(args.output.is_none());
        assert_eq!(args.backend, TtsBackend::Ort);
    }

    #[test]
    fn tts_output_preserves_the_file_only_path() {
        let cli =
            Cli::try_parse_from(["trueos-ttstt", "tts", "-o", "saved.wav", "hello"])
                .unwrap();
        let Command::Tts(args) = cli.command else {
            panic!("expected tts command");
        };
        assert_eq!(
            args.output.as_deref(),
            Some(std::path::Path::new("saved.wav"))
        );
    }

    #[test]
    fn tts_rten_backend_parses() {
        let cli =
            Cli::try_parse_from(["trueos-ttstt", "tts", "--backend", "rten", "hello"])
                .unwrap();
        let Command::Tts(args) = cli.command else {
            panic!("expected tts command");
        };
        assert_eq!(args.backend, TtsBackend::Rten);
    }

    #[test]
    fn transcribe_alias_and_format_parse() {
        let cli =
            Cli::try_parse_from([
                "trueos-ttstt",
                "transcribe",
                "voice.wav",
                "--format",
                "srt",
            ])
            .unwrap();
        let Command::Stt(args) = cli.command else {
            panic!("expected stt command");
        };
        assert_eq!(args.format, OutputFormat::Srt);
        assert_eq!(
            args.input.as_deref(),
            Some(std::path::Path::new("voice.wav"))
        );
    }

    #[test]
    fn bare_stt_selects_continuous_stdin() {
        let cli =
            Cli::try_parse_from(["trueos-ttstt", "stt", "--format", "jsonl"]).unwrap();
        let Command::Stt(args) = cli.command else {
            panic!("expected stt command");
        };
        assert!(args.input.is_none());
        assert_eq!(args.format, OutputFormat::Jsonl);
    }

    #[test]
    fn ndjson_is_an_alias_for_jsonl() {
        let cli =
            Cli::try_parse_from(["trueos-ttstt", "stt", "--format", "ndjson"]).unwrap();
        let Command::Stt(args) = cli.command else {
            panic!("expected stt command");
        };
        assert_eq!(args.format, OutputFormat::Jsonl);
    }

    #[test]
    fn stream_alias_uses_english_by_default() {
        let cli = Cli::try_parse_from(["trueos-ttstt", "stream"]).unwrap();
        let Command::Listen(args) = cli.command else {
            panic!("expected listen command");
        };
        assert_eq!(args.language, "en");
        assert!(!args.detect_language);
    }

    #[test]
    fn continuous_stt_can_request_language_detection() {
        let cli =
            Cli::try_parse_from(["trueos-ttstt", "stt", "--detect-language"]).unwrap();
        let Command::Stt(args) = cli.command else {
            panic!("expected stt command");
        };
        assert!(args.detect_language);
        assert!(args.language.is_none());

        let cli =
            Cli::try_parse_from(["trueos-ttstt", "stream", "--detect-language"]).unwrap();
        let Command::Listen(args) = cli.command else {
            panic!("expected listen command");
        };
        assert!(args.detect_language);

        assert!(
            Cli::try_parse_from([
                "trueos-ttstt",
                "stt",
                "--detect-language",
                "--language",
                "de",
            ])
                .is_err()
        );
    }

    #[test]
    fn serve_uses_loopback_and_three_tts_threads_by_default() {
        let cli = Cli::try_parse_from(["trueos-ttstt", "serve"]).unwrap();
        let Command::Serve(args) = cli.command else {
            panic!("expected serve command");
        };
        assert_eq!(args.bind.to_string(), "127.0.0.1:1100");
        assert_eq!(args.tts_threads, 3);
    }

    #[test]
    fn invalid_speed_is_rejected() {
        let error = Cli::try_parse_from(["trueos-ttstt", "tts", "--speed", "3", "hello"])
            .unwrap_err()
            .to_string();
        assert!(error.contains("speed must be between 0.5 and 2"));
    }

    #[test]
    fn tts_text_and_file_conflict() {
        assert!(
            Cli::try_parse_from([
                "trueos-ttstt",
                "tts",
                "hello",
                "--input",
                "script.txt"
            ])
            .is_err()
        );
    }

    #[test]
    fn stt_thread_override_is_not_exposed() {
        assert!(
            Cli::try_parse_from(["trueos-ttstt", "stt", "audio.wav", "--threads", "4"])
                .is_err()
        );
        assert!(
            Cli::try_parse_from(["trueos-ttstt", "listen", "--threads", "4"]).is_err()
        );
    }
}
