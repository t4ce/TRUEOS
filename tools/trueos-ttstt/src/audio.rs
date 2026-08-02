use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use hound::{SampleFormat, WavReader};
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

pub const WHISPER_SAMPLE_RATE: u32 = 16_000;

/// Read PCM or IEEE-float WAV audio, downmix it to mono, and resample to 16 kHz.
pub fn read_for_whisper(path: &Path) -> Result<Vec<f32>> {
    let mut reader = WavReader::open(path)
        .with_context(|| format!("failed to open WAV file {}", path.display()))?;
    let spec = reader.spec();

    ensure!(spec.channels > 0, "WAV file has no channels");
    ensure!(spec.sample_rate > 0, "WAV file has an invalid sample rate");

    let interleaved = match spec.sample_format {
        SampleFormat::Float => {
            ensure!(
                spec.bits_per_sample == 32,
                "unsupported {}-bit float WAV; expected 32-bit float",
                spec.bits_per_sample
            );
            reader
                .samples::<f32>()
                .map(|sample| {
                    sample.map(|value| {
                        if value.is_finite() {
                            value.clamp(-1.0, 1.0)
                        } else {
                            0.0
                        }
                    })
                })
                .collect::<Result<Vec<_>, _>>()
                .context("failed to decode float WAV samples")?
        }
        SampleFormat::Int => {
            ensure!(
                (1..=32).contains(&spec.bits_per_sample),
                "unsupported {}-bit integer WAV",
                spec.bits_per_sample
            );
            let scale = (1_u64 << (spec.bits_per_sample - 1)) as f32;
            reader
                .samples::<i32>()
                .map(|sample| {
                    sample
                        .map(|value| (value as f32 / scale).clamp(-1.0, 1.0))
                        .map_err(anyhow::Error::from)
                })
                .collect::<Result<Vec<_>>>()
                .context("failed to decode integer WAV samples")?
        }
    };

    let mono = downmix(&interleaved, spec.channels as usize)?;
    ensure!(!mono.is_empty(), "WAV file contains no audio samples");

    resample(&mono, spec.sample_rate, WHISPER_SAMPLE_RATE)
        .context("failed to resample WAV audio for Whisper")
}

fn downmix(interleaved: &[f32], channels: usize) -> Result<Vec<f32>> {
    if channels == 0 {
        bail!("audio has no channels");
    }
    ensure!(
        interleaved.len().is_multiple_of(channels),
        "WAV sample data ends in a partial frame"
    );

    if channels == 1 {
        return Ok(interleaved.to_vec());
    }

    Ok(interleaved
        .chunks_exact(channels)
        .map(|frame| frame.iter().sum::<f32>() / channels as f32)
        .collect())
}

fn resample(input: &[f32], source_rate: u32, target_rate: u32) -> Result<Vec<f32>> {
    if input.is_empty() || source_rate == target_rate {
        return Ok(input.to_vec());
    }

    let input_buffer = InterleavedSlice::new(input, 1, input.len())
        .context("failed to prepare the resampler input")?;
    let mut resampler = Fft::<f32>::new(
        source_rate as usize,
        target_rate as usize,
        1_024,
        1,
        FixedSync::Both,
    )
    .context("failed to initialize the audio resampler")?;
    let output = resampler
        .process_all(&input_buffer, input.len(), None)
        .context("audio resampling failed")?;
    Ok(output.take_data())
}

#[cfg(test)]
mod tests {
    use hound::{SampleFormat, WavSpec, WavWriter};
    use tempfile::NamedTempFile;

    use super::{downmix, read_for_whisper, resample};

    #[test]
    fn downmixes_stereo_by_averaging_channels() {
        let mono = downmix(&[1.0, -1.0, 0.5, 0.25], 2).unwrap();
        assert_eq!(mono, [0.0, 0.375]);
    }

    #[test]
    fn rejects_partial_audio_frame() {
        assert!(downmix(&[0.0, 1.0, 2.0], 2).is_err());
    }

    #[test]
    fn resampling_preserves_duration() {
        let input = vec![0.25; 24_000];
        let output = resample(&input, 24_000, 16_000).unwrap();
        assert_eq!(output.len(), 16_000);
        assert!((output[8_000] - 0.25).abs() < 0.001);
        assert!(output.iter().all(|sample| sample.is_finite()));
    }

    #[test]
    fn same_rate_is_unchanged() {
        let input = vec![-1.0, 0.0, 1.0];
        assert_eq!(resample(&input, 16_000, 16_000).unwrap(), input);
    }

    #[test]
    fn reads_the_float_wav_format_produced_by_tts_rs() {
        let file = NamedTempFile::new().unwrap();
        let mut writer = WavWriter::create(
            file.path(),
            WavSpec {
                channels: 1,
                sample_rate: 24_000,
                bits_per_sample: 32,
                sample_format: SampleFormat::Float,
            },
        )
        .unwrap();
        for index in 0..2_400 {
            writer
                .write_sample((index as f32 / 20.0).sin() * 0.5)
                .unwrap();
        }
        writer.finalize().unwrap();

        let samples = read_for_whisper(file.path()).unwrap();
        assert_eq!(samples.len(), 1_600);
        assert!(samples.iter().all(|sample| sample.is_finite()));
    }
}
