use std::path::Path;

use anyhow::{Context, Result, ensure};
use transcribe_rs::{TranscriptionResult, TranscriptionSegment};
use whisper_rs::{
    FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters, WhisperState,
};

const WHISPER_THREADS: i32 = 3;
const FIVE_SECONDS: usize = 80_000;
const SEVEN_AND_A_HALF_SECONDS: usize = 120_000;
const TEN_SECONDS: usize = 160_000;

pub(crate) struct Engine {
    context: WhisperContext,
    state: WhisperState,
    is_multilingual: bool,
}

pub(crate) struct InferenceParams<'a> {
    pub language: Option<&'a str>,
    pub translate: bool,
    pub prompt: Option<&'a str>,
    pub no_speech_threshold: f32,
    pub audio_context: Option<i32>,
    pub token_timestamps: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Ownership {
    pub window_start_sample: u64,
    pub emit_after_sample: u64,
    pub emit_before_sample: u64,
}

impl Engine {
    pub(crate) fn load(model: &Path) -> Result<Self> {
        whisper_rs::install_logging_hooks();
        let context_params = WhisperContextParameters {
            use_gpu: false,
            flash_attn: true,
            gpu_device: 0,
            ..Default::default()
        };
        let context = WhisperContext::new_with_params(model, context_params)
            .with_context(|| format!("failed to load Whisper model {}", model.display()))?;
        let is_multilingual = context.is_multilingual();
        let state = context
            .create_state()
            .context("failed to create Whisper inference state")?;

        Ok(Self {
            context,
            state,
            is_multilingual,
        })
    }

    pub(crate) fn validate(&self, language: Option<&str>, translate: bool) -> Result<()> {
        if let Some(language) = language {
            ensure!(
                whisper_rs::get_lang_id(language).is_some(),
                "Whisper does not recognize language {language:?}"
            );
            ensure!(
                self.is_multilingual || language == "en",
                "model does not support language {language:?}; this may be an English-only Whisper model"
            );
        }
        ensure!(
            !translate || self.is_multilingual,
            "this Whisper model cannot translate; use a multilingual model"
        );
        Ok(())
    }

    pub(crate) fn transcribe(
        &mut self,
        samples: &[f32],
        params: InferenceParams<'_>,
    ) -> Result<TranscriptionResult> {
        ensure!(!samples.is_empty(), "cannot transcribe empty audio");
        self.validate(params.language, params.translate)?;

        if let Some(audio_context) = params.audio_context {
            ensure!(
                self.context.n_audio_ctx() >= audio_context,
                "Whisper model supports an audio context of {}, but this input requires {audio_context}",
                self.context.n_audio_ctx()
            );
        }

        let mut full_params = FullParams::new(SamplingStrategy::BeamSearch {
            beam_size: 3,
            patience: -1.0,
        });
        if let Some(audio_context) = params.audio_context {
            full_params.set_audio_ctx(audio_context);
        }
        full_params.set_language(params.language);
        full_params.set_translate(params.translate);
        full_params.set_print_special(false);
        full_params.set_print_progress(false);
        full_params.set_print_realtime(false);
        full_params.set_print_timestamps(false);
        full_params.set_suppress_blank(true);
        full_params.set_suppress_nst(true);
        full_params.set_no_speech_thold(params.no_speech_threshold);
        full_params.set_n_threads(WHISPER_THREADS);
        // Repeated audio supplies the acoustic overlap explicitly. Keeping the
        // decoder's previous prompt as well encourages duplicated phrases.
        full_params.set_no_context(true);
        full_params.set_token_timestamps(params.token_timestamps);
        full_params.set_split_on_word(params.token_timestamps);
        if let Some(prompt) = params.prompt {
            full_params.set_initial_prompt(prompt);
        }

        self.state
            .full(full_params, samples)
            .context("Whisper transcription failed")?;
        transcription_result(&self.state)
    }

    pub(crate) fn owned_text(&self, ownership: Ownership) -> Result<String> {
        let token_eot = self.context.token_eot();
        let mut bytes = Vec::new();

        for segment_index in 0..self.state.full_n_segments() {
            let segment = self
                .state
                .get_segment(segment_index)
                .with_context(|| format!("Whisper segment {segment_index} is out of bounds"))?;
            let segment_midpoint =
                midpoint_samples(segment.start_timestamp(), segment.end_timestamp());

            for token_index in 0..segment.n_tokens() {
                let token = segment.get_token(token_index).with_context(|| {
                    format!(
                        "Whisper token {token_index} in segment {segment_index} is out of bounds"
                    )
                })?;
                if token.token_id() >= token_eot {
                    continue;
                }

                let data = token.token_data();
                let local_midpoint = if data.t0 >= 0 && data.t1 >= data.t0 {
                    midpoint_samples(data.t0, data.t1)
                } else {
                    segment_midpoint
                };
                let global_midpoint = ownership.window_start_sample.saturating_add(local_midpoint);
                if global_midpoint >= ownership.emit_after_sample
                    && global_midpoint < ownership.emit_before_sample
                {
                    bytes
                        .extend_from_slice(token.to_bytes().context("invalid Whisper token text")?);
                }
            }
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

pub(crate) fn short_audio_context(sample_count: usize) -> Option<i32> {
    if sample_count <= FIVE_SECONDS {
        Some(256)
    } else if sample_count <= SEVEN_AND_A_HALF_SECONDS {
        Some(384)
    } else if sample_count <= TEN_SECONDS {
        Some(512)
    } else {
        None
    }
}

pub(crate) fn context_seconds(audio_context: i32) -> f64 {
    f64::from(audio_context) / 50.0
}

fn midpoint_samples(start_centiseconds: i64, end_centiseconds: i64) -> u64 {
    let sum = start_centiseconds.saturating_add(end_centiseconds);
    u64::try_from(sum).unwrap_or(0).saturating_mul(80)
}

fn transcription_result(state: &WhisperState) -> Result<TranscriptionResult> {
    let mut segments = Vec::with_capacity(state.full_n_segments().max(0) as usize);
    let mut full_text = String::new();

    for index in 0..state.full_n_segments() {
        let segment = state
            .get_segment(index)
            .with_context(|| format!("Whisper segment {index} is out of bounds"))?;
        let text = segment
            .to_str()
            .with_context(|| format!("Whisper segment {index} contains invalid text"))?;
        full_text.push_str(text);
        segments.push(TranscriptionSegment {
            start: segment.start_timestamp() as f32 / 100.0,
            end: segment.end_timestamp() as f32 / 100.0,
            text: text.to_owned(),
        });
    }

    Ok(TranscriptionResult {
        text: full_text.trim().to_owned(),
        segments: Some(segments),
    })
}

#[cfg(test)]
mod tests {
    use super::{context_seconds, midpoint_samples, short_audio_context};

    #[test]
    fn context_buckets_cover_short_audio_boundaries() {
        assert_eq!(short_audio_context(0), Some(256));
        assert_eq!(short_audio_context(80_000), Some(256));
        assert_eq!(short_audio_context(80_001), Some(384));
        assert_eq!(short_audio_context(120_000), Some(384));
        assert_eq!(short_audio_context(120_001), Some(512));
        assert_eq!(short_audio_context(160_000), Some(512));
        assert_eq!(short_audio_context(160_001), None);
    }

    #[test]
    fn context_tokens_convert_to_seconds() {
        assert_eq!(context_seconds(256), 5.12);
        assert_eq!(context_seconds(384), 7.68);
        assert_eq!(context_seconds(512), 10.24);
    }

    #[test]
    fn centisecond_midpoint_converts_to_samples() {
        assert_eq!(midpoint_samples(10, 20), 2_400);
        assert_eq!(midpoint_samples(-1, 20), 1_520);
    }
}
