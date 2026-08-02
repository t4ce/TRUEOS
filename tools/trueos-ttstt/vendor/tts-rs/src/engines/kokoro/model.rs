use std::collections::HashMap;
use std::path::Path;
#[cfg(feature = "kokoro-rten")]
use std::sync::Arc;

use ndarray::Array2;
use ort::execution_providers::CPUExecutionProvider;
use ort::inputs;
use ort::session::builder::GraphOptimizationLevel;
use ort::session::Session;
use ort::value::TensorRef;
#[cfg(feature = "kokoro-rten")]
use rten::{DataType, Model, ModelOptions, NodeId, RunOptions, ThreadPool, ValueType};
#[cfg(feature = "kokoro-rten")]
use rten_tensor::{NdTensor, Tensor};

use super::phonemizer::{phonemize, voice_lang, EspeakConfig};
use super::voices::VoiceStore;

/// Maximum number of phoneme tokens per chunk (before padding).
pub const MAX_PHONEME_LEN: usize = 510;

/// Style vector dimension for Kokoro.
pub const STYLE_DIM: usize = 256;

/// Output sample rate from the Kokoro model.
pub const SAMPLE_RATE: u32 = 24000;

/// CPU inference implementation used for the Kokoro graph.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum KokoroBackend {
    /// ONNX Runtime, retained as the compatibility and numerical oracle path.
    #[default]
    OnnxRuntime,
    /// RTen's native Rust CPU runtime.
    #[cfg(feature = "kokoro-rten")]
    Rten,
}

/// Crossfade used when joining independently synthesized model chunks.
pub const CHUNK_CROSSFADE_SAMPLES: usize = 240; // 10ms @ 24kHz

/// Earliest punctuation split within a full 510-phoneme window.
pub const PREFERRED_SPLIT_MIN_PHONEMES: usize = MAX_PHONEME_LEN * 4 / 5;

#[derive(thiserror::Error, Debug)]
pub enum KokoroError {
    #[error("ONNX runtime error: {0}")]
    Ort(#[from] ort::Error),
    #[cfg(feature = "kokoro-rten")]
    #[error("RTen model load failed: {0}")]
    RtenLoad(String),
    #[cfg(feature = "kokoro-rten")]
    #[error("RTen inference failed: {0}")]
    RtenRun(String),
    #[cfg(feature = "kokoro-rten")]
    #[error(
        "RTen requires the bridged graph at {0}; run `python3 tools/prepare_kokoro_rten.py \
         .ttstt/models/kokoro/kokoro-quant-convinteger.onnx {0}`"
    )]
    RtenBridgeMissing(std::path::PathBuf),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Array shape error: {0}")]
    Shape(#[from] ndarray::ShapeError),
    #[error(
        "espeak-ng not found. Install: Linux: `sudo apt-get install espeak-ng`, \
         macOS: `brew install espeak-ng`, Windows: https://espeak-ng.org/download"
    )]
    EspeakNotFound,
    #[error("Phonemization failed: {0}")]
    PhonemizerFailed(String),
    #[error("Voice '{0}' not found. Call list_voices() to see available voices.")]
    VoiceNotFound(String),
    #[error("Model not loaded. Call load_model() first.")]
    ModelNotLoaded,
    #[error("Invalid config.json: {0}")]
    Config(String),
    #[error("Failed to parse voice file: {0}")]
    VoiceParse(String),
}

/// Error returned while synthesizing audio through a chunk callback.
#[derive(thiserror::Error, Debug)]
pub enum KokoroStreamError<E> {
    /// Kokoro could not synthesize the next audio chunk.
    #[error("speech synthesis failed: {0}")]
    Synthesis(#[source] KokoroError),
    /// The consumer rejected a completed audio chunk.
    #[error("audio consumer failed: {0}")]
    Callback(#[source] E),
}

enum InferenceRuntime {
    OnnxRuntime(Session),
    #[cfg(feature = "kokoro-rten")]
    Rten {
        model: Model,
        tokens: NodeId,
        style: NodeId,
        speed: NodeId,
        output: NodeId,
        speed_is_int32: bool,
        run_options: RunOptions,
    },
}

/// Internal Kokoro model state.
pub struct KokoroModel {
    runtime: InferenceRuntime,
    voice_store: VoiceStore,
    vocab: HashMap<char, i64>,
    /// Detected input name: "input_ids" or "tokens"
    tokens_input_name: String,
    /// True if the speed input expects int32, false for float32
    speed_is_int32: bool,
}

impl KokoroModel {
    /// Load the Kokoro model from a directory.
    ///
    /// The directory must contain:
    /// - An `.onnx` file (preferably `kokoro-quant-convinteger.onnx`)
    /// - A `voices-v1.0.bin` voice archive
    /// - Optionally a `config.json` for vocabulary (falls back to hardcoded)
    pub fn load(
        model_dir: &Path,
        backend: KokoroBackend,
        num_threads: Option<usize>,
        optimized_cache_path: Option<&Path>,
    ) -> Result<Self, KokoroError> {
        let (runtime, tokens_input_name, speed_is_int32) = match backend {
            KokoroBackend::OnnxRuntime => {
                let onnx_path = find_onnx_file(model_dir)?;
                log::info!("Loading Kokoro model from {}", onnx_path.display());
                let session = init_session(&onnx_path, num_threads, optimized_cache_path)?;
                let tokens_input_name = detect_tokens_input(&session);
                let speed_is_int32 = detect_speed_type(&session);
                (
                    InferenceRuntime::OnnxRuntime(session),
                    tokens_input_name,
                    speed_is_int32,
                )
            }
            #[cfg(feature = "kokoro-rten")]
            KokoroBackend::Rten => {
                let onnx_path = model_dir.join("kokoro-rten.onnx");
                if !onnx_path.is_file() {
                    return Err(KokoroError::RtenBridgeMissing(onnx_path));
                }
                if optimized_cache_path.is_some() {
                    log::warn!("optimized_model_cache_path is ignored by the RTen backend");
                }
                log::info!("Loading bridged Kokoro graph with RTen from {}", onnx_path.display());
                let runtime = init_rten_model(&onnx_path, num_threads)?;
                let tokens_input_name = runtime.tokens_name.clone();
                let speed_is_int32 = runtime.speed_is_int32;
                (
                    InferenceRuntime::Rten {
                        model: runtime.model,
                        tokens: runtime.tokens,
                        style: runtime.style,
                        speed: runtime.speed,
                        output: runtime.output,
                        speed_is_int32,
                        run_options: runtime.run_options,
                    },
                    tokens_input_name,
                    speed_is_int32,
                )
            }
        };

        log::info!(
            "Detected: tokens_input='{}', speed_is_int32={}",
            tokens_input_name,
            speed_is_int32
        );

        // Load voices
        let voices_path = model_dir.join("voices-v1.0.bin");
        if !voices_path.exists() {
            return Err(KokoroError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "Voice file not found at {}. Download it from the Kokoro model repository.",
                    voices_path.display()
                ),
            )));
        }
        let voice_store = VoiceStore::load(&voices_path)?;

        // Load vocabulary
        let config_path = model_dir.join("config.json");
        let vocab = if config_path.exists() {
            log::info!("Loading vocab from config.json");
            super::vocab::load_vocab(&config_path)?
        } else {
            log::warn!("config.json not found, using hardcoded vocab");
            super::vocab::hardcoded_vocab()
        };

        Ok(Self {
            runtime,
            voice_store,
            vocab,
            tokens_input_name,
            speed_is_int32,
        })
    }

    /// Synthesize audio from text using the given voice and speed.
    pub fn synthesize_text(
        &mut self,
        text: &str,
        voice_name: &str,
        speed: f32,
        style_idx_override: Option<usize>,
        espeak: &EspeakConfig,
    ) -> Result<Vec<f32>, KokoroError> {
        let mut combined = Vec::new();
        match self.synthesize_text_streaming(
            text,
            voice_name,
            speed,
            style_idx_override,
            espeak,
            |samples| {
                combined.extend(samples);
                Ok::<(), std::convert::Infallible>(())
            },
        ) {
            Ok(()) => Ok(combined),
            Err(KokoroStreamError::Synthesis(error)) => Err(error),
            Err(KokoroStreamError::Callback(never)) => match never {},
        }
    }

    /// Synthesize a pre-phonemized IPA string without invoking espeak-ng.
    pub fn synthesize_phonemes(
        &mut self,
        phonemes: &str,
        voice_name: &str,
        speed: f32,
        style_idx_override: Option<usize>,
    ) -> Result<Vec<f32>, KokoroError> {
        let mut combined = Vec::new();
        match self.synthesize_phonemes_streaming(
            phonemes,
            voice_name,
            speed,
            style_idx_override,
            |samples| {
                combined.extend(samples);
                Ok::<(), std::convert::Infallible>(())
            },
        ) {
            Ok(()) => Ok(combined),
            Err(KokoroStreamError::Synthesis(error)) => Err(error),
            Err(KokoroStreamError::Callback(never)) => match never {},
        }
    }

    /// Synthesize text and hand off finalized audio as each model chunk completes.
    ///
    /// Kokoro inference is blocking for each sequence of at most
    /// [`MAX_PHONEME_LEN`] phonemes. For longer text, this method invokes
    /// `on_chunk` between inference calls while retaining the samples needed for
    /// the next crossfade. Concatenating every callback value is sample-for-sample
    /// equivalent to [`Self::synthesize_text`].
    pub fn synthesize_text_streaming<E, F>(
        &mut self,
        text: &str,
        voice_name: &str,
        speed: f32,
        style_idx_override: Option<usize>,
        espeak: &EspeakConfig,
        on_chunk: F,
    ) -> Result<(), KokoroStreamError<E>>
    where
        F: FnMut(Vec<f32>) -> Result<(), E>,
    {
        let lang = voice_lang(voice_name);
        let ids =
            phonemize(text, lang, &self.vocab, espeak).map_err(KokoroStreamError::Synthesis)?;

        if ids.is_empty() {
            log::warn!("No phoneme tokens produced for text: {text:?}");
            return Ok(());
        }

        self.synthesize_ids_streaming(
            ids,
            voice_name,
            speed,
            style_idx_override,
            on_chunk,
        )
    }

    /// Synthesize an IPA string and deliver finalized model chunks.
    pub fn synthesize_phonemes_streaming<E, F>(
        &mut self,
        phonemes: &str,
        voice_name: &str,
        speed: f32,
        style_idx_override: Option<usize>,
        on_chunk: F,
    ) -> Result<(), KokoroStreamError<E>>
    where
        F: FnMut(Vec<f32>) -> Result<(), E>,
    {
        let ids = phonemes
            .chars()
            .filter_map(|phoneme| self.vocab.get(&phoneme).copied())
            .collect::<Vec<_>>();
        if ids.is_empty() {
            log::warn!("No Kokoro tokens found in phoneme string: {phonemes:?}");
            return Ok(());
        }
        self.synthesize_ids_streaming(
            ids,
            voice_name,
            speed,
            style_idx_override,
            on_chunk,
        )
    }

    fn synthesize_ids_streaming<E, F>(
        &mut self,
        ids: Vec<i64>,
        voice_name: &str,
        speed: f32,
        style_idx_override: Option<usize>,
        mut on_chunk: F,
    ) -> Result<(), KokoroStreamError<E>>
    where
        F: FnMut(Vec<f32>) -> Result<(), E>,
    {
        // Split into chunks if needed. Keep a stable style index so adjacent chunks
        // don't change style/prosody based on chunk length.
        let style_idx = style_idx_override.unwrap_or(ids.len());
        let chunks = if ids.len() > MAX_PHONEME_LEN {
            log::debug!(
                "Kokoro phoneme sequence exceeded limit ({} > {}), chunking",
                ids.len(),
                MAX_PHONEME_LEN
            );
            split_chunks(&ids)
        } else {
            vec![ids]
        };

        let mut pending = Vec::new();

        for chunk_ids in chunks.iter() {
            let style = self
                .voice_store
                .get_style(voice_name, style_idx)
                .map_err(KokoroStreamError::Synthesis)?;
            let audio = self
                .synthesize_chunk(chunk_ids, &style, speed)
                .map_err(KokoroStreamError::Synthesis)?;
            if audio.is_empty() {
                continue;
            }

            append_owned_with_crossfade(&mut pending, audio, CHUNK_CROSSFADE_SAMPLES);
            emit_finalized_prefix(&mut pending, CHUNK_CROSSFADE_SAMPLES, &mut on_chunk)
                .map_err(KokoroStreamError::Callback)?;
        }

        if !pending.is_empty() {
            on_chunk(pending).map_err(KokoroStreamError::Callback)?;
        }

        Ok(())
    }

    /// Run ONNX inference on a single chunk of phoneme token IDs.
    fn synthesize_chunk(
        &mut self,
        tokens: &[i64],
        style: &[f32; STYLE_DIM],
        speed: f32,
    ) -> Result<Vec<f32>, KokoroError> {
        let seq_len = tokens.len() + 2; // +2 for padding tokens

        // Build tokens tensor: [[0, t1..tN, 0]]
        let mut padded = vec![0i64; seq_len];
        padded[1..seq_len - 1].copy_from_slice(tokens);
        match &mut self.runtime {
            InferenceRuntime::OnnxRuntime(session) => {
                let tokens_arr = Array2::from_shape_vec((1, seq_len), padded)?;
                // Build style tensor: [[s0..s255]] — use a view to avoid
                // copying the 256-float array.
                let style_view =
                    ndarray::ArrayView2::from_shape((1, STYLE_DIM), style.as_slice())?;
                let output = if self.speed_is_int32 {
                    let speed_arr = ndarray::arr1(&[speed as i32]);
                    let inputs = inputs![
                        self.tokens_input_name.as_str() => TensorRef::from_array_view(tokens_arr.view())?,
                        "style" => TensorRef::from_array_view(style_view)?,
                        "speed" => TensorRef::from_array_view(speed_arr.view())?,
                    ];
                    session.run(inputs)?
                } else {
                    let speed_arr = ndarray::arr1(&[speed]);
                    let inputs = inputs![
                        self.tokens_input_name.as_str() => TensorRef::from_array_view(tokens_arr.view())?,
                        "style" => TensorRef::from_array_view(style_view)?,
                        "speed" => TensorRef::from_array_view(speed_arr.view())?,
                    ];
                    session.run(inputs)?
                };

                let first_output = output
                    .iter()
                    .next()
                    .ok_or_else(|| KokoroError::Ort(ort::Error::new("No output from model")))?;
                let waveform = first_output.1.try_extract_array::<f32>()?;
                Ok(waveform.as_slice().unwrap_or(&[]).to_vec())
            }
            #[cfg(feature = "kokoro-rten")]
            InferenceRuntime::Rten {
                model,
                tokens: tokens_id,
                style: style_id,
                speed: speed_id,
                output: output_id,
                speed_is_int32,
                run_options,
            } => {
                let token_data = padded
                    .into_iter()
                    .map(|token| {
                        i32::try_from(token).map_err(|_| {
                            KokoroError::RtenRun(format!(
                                "phoneme token {token} cannot be represented as i32"
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let tokens = NdTensor::from_data([1, seq_len], token_data);
                let style = NdTensor::from_data([1, STYLE_DIM], style.to_vec());

                let inputs = if *speed_is_int32 {
                    let speed = NdTensor::from([speed as i32]);
                    vec![
                        (*tokens_id, tokens.into()),
                        (*style_id, style.into()),
                        (*speed_id, speed.into()),
                    ]
                } else {
                    let speed = NdTensor::from([speed]);
                    vec![
                        (*tokens_id, tokens.into()),
                        (*style_id, style.into()),
                        (*speed_id, speed.into()),
                    ]
                };
                let [output] = model
                    .run_n(inputs, [*output_id], Some(run_options.clone()))
                    .map_err(|error| KokoroError::RtenRun(error.to_string()))?;
                let waveform: Tensor<f32> = output.try_into().map_err(|error| {
                    KokoroError::RtenRun(format!("audio output is not an f32 tensor: {error}"))
                })?;
                Ok(waveform.into_data())
            }
        }
    }

    /// List all available voice names.
    pub fn list_voices(&self) -> Vec<&str> {
        self.voice_store.list_voices()
    }
}

/// Find the ONNX model file in the given directory.
///
/// Prefers `kokoro-quant-convinteger.onnx`, then falls back to the first `.onnx` file found.
fn find_onnx_file(model_dir: &Path) -> Result<std::path::PathBuf, KokoroError> {
    let preferred = model_dir.join("kokoro-quant-convinteger.onnx");
    if preferred.exists() {
        return Ok(preferred);
    }

    // Scan for any .onnx file
    for entry in std::fs::read_dir(model_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("onnx") {
            log::info!("Using ONNX file: {}", path.display());
            return Ok(path);
        }
    }

    Err(KokoroError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("No .onnx file found in {}", model_dir.display()),
    )))
}

#[cfg(feature = "kokoro-rten")]
struct RtenModelInit {
    model: Model,
    tokens: NodeId,
    tokens_name: String,
    style: NodeId,
    speed: NodeId,
    output: NodeId,
    speed_is_int32: bool,
    run_options: RunOptions,
}

#[cfg(feature = "kokoro-rten")]
fn init_rten_model(
    onnx_path: &Path,
    num_threads: Option<usize>,
) -> Result<RtenModelInit, KokoroError> {
    let mut options = ModelOptions::with_all_ops();
    // The persistent ttstt service runs the same graph repeatedly. Packing its
    // constant matrices once at load time is worth the additional resident
    // memory and also makes the local reference less timing-noisy.
    options.prepack_weights(true);
    let model = options
        .load_file(onnx_path)
        .map_err(|error| KokoroError::RtenLoad(error.to_string()))?;

    let (tokens_name, tokens) = ["tokens", "input_ids"]
        .into_iter()
        .find_map(|name| model.find_node(name).map(|id| (name.to_owned(), id)))
        .ok_or_else(|| {
            KokoroError::RtenLoad("graph has neither a `tokens` nor `input_ids` input".to_owned())
        })?;
    let style = model
        .find_node("style")
        .ok_or_else(|| KokoroError::RtenLoad("graph has no `style` input".to_owned()))?;
    let speed = model
        .find_node("speed")
        .ok_or_else(|| KokoroError::RtenLoad("graph has no `speed` input".to_owned()))?;
    let output = ["audio", "waveform"]
        .into_iter()
        .find_map(|name| model.find_node(name))
        .or_else(|| model.output_ids().first().copied())
        .ok_or_else(|| KokoroError::RtenLoad("graph has no audio output".to_owned()))?;
    let speed_is_int32 = matches!(
        model.node_info(speed).and_then(|info| info.dtype()),
        Some(ValueType::Tensor(DataType::Int32))
    );

    let run_options = match num_threads {
        Some(threads) => RunOptions::default()
            .with_thread_pool(Some(Arc::new(ThreadPool::with_num_threads(threads)))),
        None => RunOptions::default(),
    };

    Ok(RtenModelInit {
        model,
        tokens,
        tokens_name,
        style,
        speed,
        output,
        speed_is_int32,
        run_options,
    })
}

/// Initialize an ONNX session with optional on-disk graph caching.
///
/// The first time a model is loaded, ORT runs Level3 graph optimization (5–10 s)
/// and serialises the result to `optimized_cache_path`.  Every subsequent load
/// reads the pre-optimized file directly at `Disable` optimization level, cutting
/// cold-start time to under one second.
///
/// If `optimized_cache_path` is `None` the original behaviour (always Level3) is
/// preserved, which is useful for unit-testing or read-only deployments.
fn init_session(
    onnx_path: &Path,
    num_threads: Option<usize>,
    optimized_cache_path: Option<&Path>,
) -> Result<Session, KokoroError> {
    let providers = vec![CPUExecutionProvider::default().build()];

    // Choose load path and optimization level depending on cache state.
    let (load_path, opt_level, write_cache) = match optimized_cache_path {
        // Pre-optimized graph already on disk → load it directly, skip optimization.
        Some(cache) if cache.exists() => {
            log::info!(
                "Loading pre-optimized Kokoro graph ({:.1} MB) from {:?} — skipping Level3",
                cache
                    .metadata()
                    .map(|m| m.len() as f64 / 1_048_576.0)
                    .unwrap_or(0.0),
                cache
            );
            (cache, GraphOptimizationLevel::Disable, false)
        }
        // Cache path given but file does not exist yet → build + persist.
        Some(cache) => {
            log::info!(
                "First load: running Level3 optimization; saving graph to {:?}",
                cache
            );
            (onnx_path, GraphOptimizationLevel::Level3, true)
        }
        // No cache path → original behaviour.
        None => (onnx_path, GraphOptimizationLevel::Level3, false),
    };

    let mut builder = Session::builder()?
        .with_optimization_level(opt_level)?
        .with_execution_providers(providers)?
        .with_parallel_execution(true)?;

    if write_cache {
        // Serialise the optimized graph so the next launch can skip optimization.
        let cache = optimized_cache_path.unwrap();
        builder = builder.with_optimized_model_path(cache)?;
    }

    if let Some(threads) = num_threads {
        builder = builder
            .with_intra_threads(threads)?
            .with_inter_threads(threads)?;
    }

    Ok(builder.commit_from_file(load_path)?)
}

/// Detect the token input name ("input_ids" or "tokens") from session inputs.
fn detect_tokens_input(session: &Session) -> String {
    for input in session.inputs() {
        if input.name() == "input_ids" || input.name() == "tokens" {
            return input.name().to_string();
        }
    }
    // Default to "input_ids" if neither is found
    "input_ids".to_string()
}

/// Detect whether the speed input expects int32 (true) or float32 (false).
fn detect_speed_type(session: &Session) -> bool {
    for input in session.inputs() {
        if input.name() == "speed" {
            // Check the type description
            let type_str = format!("{:?}", input.dtype());
            return type_str.contains("Int32") || type_str.contains("int32");
        }
    }
    // Default: modern Kokoro models use int32
    true
}

/// Split phoneme IDs into chunks of at most `MAX_PHONEME_LEN`, preferring punctuation.
fn split_chunks(ids: &[i64]) -> Vec<Vec<i64>> {
    // An extremely early punctuation split can finish playing before inference
    // of the next full chunk. Only prefer punctuation in the final fifth of the
    // window; otherwise the fixed-size boundary keeps streaming well buffered.
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < ids.len() {
        let end = (start + MAX_PHONEME_LEN).min(ids.len());
        if end == ids.len() {
            chunks.push(ids[start..end].to_vec());
            break;
        }

        // Try to find a good split point (last punctuation before `end`).
        // Punctuation IDs (hardcoded vocab): ';':1 ':':2 ',':3 '.':4 '!':5 '?':6
        const PUNCT_IDS: &[i64] = &[1, 2, 3, 4, 5, 6];
        let preferred_start = start + PREFERRED_SPLIT_MIN_PHONEMES;
        let split = ids[preferred_start..end]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, &id)| PUNCT_IDS.contains(&id))
            .map(|(i, _)| preferred_start + i + 1)
            .unwrap_or(end);

        chunks.push(ids[start..split].to_vec());
        start = split;
    }

    chunks
}

#[cfg(test)]
fn append_with_crossfade(dst: &mut Vec<f32>, src: &[f32], crossfade_samples: usize) {
    let overlap = crossfade_samples.min(dst.len()).min(src.len());
    if overlap == 0 {
        dst.extend_from_slice(src);
        return;
    }

    let dst_start = dst.len() - overlap;
    for i in 0..overlap {
        let t = (i + 1) as f32 / (overlap as f32 + 1.0);
        let left = dst[dst_start + i] * (1.0 - t);
        let right = src[i] * t;
        dst[dst_start + i] = left + right;
    }

    dst.extend_from_slice(&src[overlap..]);
}

fn append_owned_with_crossfade(dst: &mut Vec<f32>, mut src: Vec<f32>, crossfade_samples: usize) {
    if dst.is_empty() {
        *dst = src;
        return;
    }

    let overlap = crossfade_samples.min(dst.len()).min(src.len());
    if overlap == 0 {
        dst.append(&mut src);
        return;
    }

    let dst_start = dst.len() - overlap;
    for i in 0..overlap {
        let t = (i + 1) as f32 / (overlap as f32 + 1.0);
        let left = dst[dst_start + i] * (1.0 - t);
        src[i] = left + src[i] * t;
    }

    if dst_start == 0 {
        *dst = src;
    } else {
        dst.truncate(dst_start);
        dst.append(&mut src);
    }
}

fn emit_finalized_prefix<E, F>(
    pending: &mut Vec<f32>,
    retained_samples: usize,
    on_chunk: &mut F,
) -> Result<(), E>
where
    F: FnMut(Vec<f32>) -> Result<(), E>,
{
    let finalized_len = pending.len().saturating_sub(retained_samples);
    if finalized_len == 0 {
        return Ok(());
    }

    let tail = pending.split_off(finalized_len);
    let finalized = std::mem::replace(pending, tail);
    on_chunk(finalized)
}

#[cfg(test)]
mod streaming_tests {
    use super::{
        append_owned_with_crossfade, append_with_crossfade, emit_finalized_prefix, split_chunks,
        CHUNK_CROSSFADE_SAMPLES, MAX_PHONEME_LEN,
    };

    fn streamed(chunks: &[Vec<f32>]) -> Vec<f32> {
        let mut pending = Vec::new();
        let mut output = Vec::new();
        for chunk in chunks {
            if chunk.is_empty() {
                continue;
            }
            append_owned_with_crossfade(&mut pending, chunk.clone(), CHUNK_CROSSFADE_SAMPLES);
            emit_finalized_prefix(&mut pending, CHUNK_CROSSFADE_SAMPLES, &mut |samples| {
                output.extend(samples);
                Ok::<(), ()>(())
            })
            .unwrap();
        }
        output.extend(pending);
        output
    }

    fn collected(chunks: &[Vec<f32>]) -> Vec<f32> {
        let mut output = Vec::new();
        for chunk in chunks {
            if output.is_empty() {
                output.extend_from_slice(chunk);
            } else if !chunk.is_empty() {
                append_with_crossfade(&mut output, chunk, CHUNK_CROSSFADE_SAMPLES);
            }
        }
        output
    }

    #[test]
    fn streamed_chunks_preserve_the_existing_crossfade_exactly() {
        let cases = [
            vec![vec![0.25; 1_000]],
            vec![vec![0.25; 1_000], vec![-0.5; 800], vec![0.75; 600]],
            vec![vec![0.1; 100], vec![0.2; 80], vec![0.3; 400]],
            vec![vec![0.4; 700], Vec::new(), vec![-0.2; 700]],
        ];

        for chunks in cases {
            assert_eq!(streamed(&chunks), collected(&chunks));
        }
    }

    #[test]
    fn callback_failure_is_returned_immediately() {
        let mut pending = vec![0.0; CHUNK_CROSSFADE_SAMPLES + 1];
        let error = emit_finalized_prefix(&mut pending, CHUNK_CROSSFADE_SAMPLES, &mut |_| {
            Err::<(), _>("stopped")
        })
        .unwrap_err();

        assert_eq!(error, "stopped");
    }

    #[test]
    fn chunking_ignores_punctuation_that_would_starve_playback() {
        let mut ids = vec![7; MAX_PHONEME_LEN * 2];
        ids[10] = 4;

        let chunks = split_chunks(&ids);
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), [510, 510]);
    }

    #[test]
    fn chunking_still_prefers_late_punctuation() {
        let mut ids = vec![7; MAX_PHONEME_LEN + 200];
        ids[450] = 4;

        let chunks = split_chunks(&ids);
        assert_eq!(chunks.iter().map(Vec::len).collect::<Vec<_>>(), [451, 259]);
    }
}
