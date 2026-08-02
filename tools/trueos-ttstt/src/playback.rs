use std::env;
use std::fmt;
use std::io::{self, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result, anyhow, bail};
use tts_rs::SynthesisResult;

const QUEUED_CHUNKS: usize = 2;
const PCM_BUFFER_SAMPLES: usize = 4_096;

pub(crate) struct PlaybackStream {
    backend: Backend,
    sample_rate: u32,
    sender: Option<SyncSender<Vec<f32>>>,
    writer: Option<JoinHandle<io::Result<()>>>,
    child: Option<Child>,
}

impl PlaybackStream {
    pub(crate) fn start(sample_rate: u32) -> Result<Self> {
        let (backend, mut child) = spawn_player(sample_rate)?;
        let Some(stdin) = child.stdin.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow!(
                "{} did not provide a standard-input pipe; pass --output FILE to write a WAV file instead",
                backend
            ));
        };
        let (sender, receiver) = sync_channel(QUEUED_CHUNKS);
        let writer = match thread::Builder::new()
            .name("trueos-ttstt-playback".to_owned())
            .spawn(move || write_queued_pcm(stdin, receiver))
        {
            Ok(writer) => writer,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error).context("failed to start the audio playback worker");
            }
        };

        Ok(Self {
            backend,
            sample_rate,
            sender: Some(sender),
            writer: Some(writer),
            child: Some(child),
        })
    }

    pub(crate) fn enqueue(&mut self, result: SynthesisResult) -> io::Result<()> {
        if result.sample_rate != self.sample_rate {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "audio sample rate changed from {} Hz to {} Hz",
                    self.sample_rate, result.sample_rate
                ),
            ));
        }

        self.sender
            .as_ref()
            .expect("playback sender is present until finish")
            .send(result.samples)
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    format!(
                        "{} stopped accepting audio; pass --output FILE to write a WAV file instead",
                        self.backend
                    ),
                )
            })
    }

    pub(crate) fn finish(mut self) -> Result<()> {
        self.sender.take();
        let writer_result = join_writer(self.writer.take());
        let status = self
            .child
            .as_mut()
            .expect("audio player is present until finish")
            .wait()
            .with_context(|| format!("failed to wait for {}", self.backend))?;
        self.child.take();

        if !status.success() {
            bail!(
                "{} exited with {status}; pass --output FILE to write a WAV file instead",
                self.backend
            );
        }

        writer_result.with_context(|| {
            format!(
                "failed to send audio to {}; pass --output FILE to write a WAV file instead",
                self.backend
            )
        })
    }
}

impl Drop for PlaybackStream {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
        }
        let _ = join_writer(self.writer.take());
        if let Some(mut child) = self.child.take() {
            let _ = child.wait();
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Backend {
    Aplay,
    Ffplay,
}

impl Backend {
    const CANDIDATES: [Self; 2] = [Self::Aplay, Self::Ffplay];

    fn command(self, sample_rate: u32) -> CommandSpec {
        let rate = sample_rate.to_string();
        match self {
            Self::Aplay => CommandSpec {
                program: "aplay",
                args: vec![
                    "--quiet".to_owned(),
                    "--file-type".to_owned(),
                    "raw".to_owned(),
                    "--format".to_owned(),
                    "FLOAT_LE".to_owned(),
                    "--channels".to_owned(),
                    "1".to_owned(),
                    "--rate".to_owned(),
                    rate,
                ],
            },
            Self::Ffplay => CommandSpec {
                program: "ffplay",
                args: vec![
                    "-v".to_owned(),
                    "error".to_owned(),
                    "-nodisp".to_owned(),
                    "-autoexit".to_owned(),
                    "-f".to_owned(),
                    "f32le".to_owned(),
                    // Use the raw demuxer's long-form option. Current ffplay
                    // no longer accepts the ffmpeg-style -ar/-ac aliases;
                    // f32le is mono by default.
                    "-sample_rate".to_owned(),
                    rate,
                    "-i".to_owned(),
                    "pipe:0".to_owned(),
                ],
            },
        }
    }
}

impl fmt::Display for Backend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Aplay => "aplay",
            Self::Ffplay => "ffplay",
        })
    }
}

#[derive(Debug, Eq, PartialEq)]
struct CommandSpec {
    program: &'static str,
    args: Vec<String>,
}

fn spawn_player(sample_rate: u32) -> Result<(Backend, Child)> {
    let configured = configured_backend()?;
    let forced;
    let candidates = if let Some(backend) = configured {
        forced = [backend];
        &forced[..]
    } else {
        &Backend::CANDIDATES[..]
    };

    for &backend in candidates {
        let spec = backend.command(sample_rate);
        match Command::new(spec.program)
            .args(&spec.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => return Ok((backend, child)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).with_context(|| format!("failed to start {backend}"));
            }
        }
    }

    if let Some(backend) = configured {
        bail!(
            "TTSTT_PLAYER selected {backend}, but it was not found on PATH; install it or pass --output FILE to write a WAV file"
        );
    }

    bail!(
        "no supported audio player found on PATH; install alsa-utils (aplay) or FFmpeg (ffplay), or pass --output FILE to write a WAV file"
    )
}

fn configured_backend() -> Result<Option<Backend>> {
    let Some(value) = env::var_os("TTSTT_PLAYER") else {
        return Ok(None);
    };
    match value.to_str() {
        Some("aplay") => Ok(Some(Backend::Aplay)),
        Some("ffplay") => Ok(Some(Backend::Ffplay)),
        Some(value) => bail!("unsupported TTSTT_PLAYER value {value:?}; use aplay or ffplay"),
        None => bail!("TTSTT_PLAYER must be valid UTF-8 and set to aplay or ffplay"),
    }
}

fn write_queued_pcm(mut writer: impl Write, receiver: Receiver<Vec<f32>>) -> io::Result<()> {
    for samples in receiver {
        write_pcm(&mut writer, &samples)?;
    }
    writer.flush()
}

fn write_pcm(mut writer: impl Write, samples: &[f32]) -> io::Result<()> {
    let mut bytes = Vec::with_capacity(PCM_BUFFER_SAMPLES * size_of::<f32>());
    for samples in samples.chunks(PCM_BUFFER_SAMPLES) {
        bytes.clear();
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        writer.write_all(&bytes)?;
    }
    Ok(())
}

fn join_writer(writer: Option<JoinHandle<io::Result<()>>>) -> io::Result<()> {
    let Some(writer) = writer else {
        return Ok(());
    };
    writer
        .join()
        .map_err(|_| io::Error::other("audio playback worker panicked"))?
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc::sync_channel;

    use super::{Backend, write_pcm, write_queued_pcm};

    #[test]
    fn aplay_receives_raw_float_arguments() {
        let spec = Backend::Aplay.command(24_000);
        assert_eq!(spec.program, "aplay");
        assert_eq!(
            spec.args,
            [
                "--quiet",
                "--file-type",
                "raw",
                "--format",
                "FLOAT_LE",
                "--channels",
                "1",
                "--rate",
                "24000",
            ]
        );
    }

    #[test]
    fn ffplay_receives_raw_float_arguments() {
        let spec = Backend::Ffplay.command(22_050);
        assert_eq!(spec.program, "ffplay");
        assert_eq!(
            spec.args,
            [
                "-v",
                "error",
                "-nodisp",
                "-autoexit",
                "-f",
                "f32le",
                "-sample_rate",
                "22050",
                "-i",
                "pipe:0",
            ]
        );
    }

    #[test]
    fn pcm_is_always_little_endian_f32() {
        let samples = [0.0_f32, 1.0, -0.5];
        let mut bytes = Vec::new();
        write_pcm(&mut bytes, &samples).unwrap();

        let expected: Vec<u8> = samples.into_iter().flat_map(f32::to_le_bytes).collect();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn queued_pcm_flushes_a_single_chunk_when_the_stream_ends() {
        let (sender, receiver) = sync_channel(2);
        sender.send(vec![0.25, -0.25]).unwrap();
        drop(sender);

        let mut bytes = Vec::new();
        write_queued_pcm(&mut bytes, receiver).unwrap();

        let expected: Vec<u8> = [0.25_f32, -0.25]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert_eq!(bytes, expected);
    }
}
