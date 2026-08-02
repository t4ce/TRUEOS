use std::fs::File;
use std::io::{self, BufWriter, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, sync_channel};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail, ensure};
use signal_hook::consts::SIGINT;
use signal_hook::{flag, low_level};
use transcribe_rs::vad::{EnergyVad, SmoothedVad, Vad};

use crate::audio::WHISPER_SAMPLE_RATE;
use crate::cli::{ListenArgs, OutputFormat};
use crate::status;
use crate::whisper::{Engine, InferenceParams, Ownership, short_audio_context};

const FRAME_SAMPLES: usize = 480;
const PREFILL_FRAMES: usize = 15;
const HANGOVER_FRAMES: usize = 15;
const ONSET_FRAMES: usize = 2;
const MAX_WINDOW_SAMPLES: usize = (160_000 / FRAME_SAMPLES) * FRAME_SAMPLES;
const OVERLAP_SAMPLES: usize = 3 * WHISPER_SAMPLE_RATE as usize;
const INPUT_BUFFER_BYTES: usize = 8 * 1024;
const INPUT_QUEUE_CHUNKS: usize = 16;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);

enum InputEvent {
    Audio(Vec<u8>),
    Eof,
    Error(io::Error),
}

#[derive(Clone, Debug)]
pub(crate) struct LiveParams {
    pub language: Option<String>,
    pub translate: bool,
    pub prompt: Option<String>,
    pub vad_threshold: f32,
    pub no_speech_threshold: f32,
}

impl From<&ListenArgs> for LiveParams {
    fn from(args: &ListenArgs) -> Self {
        Self {
            language: (!args.detect_language).then(|| args.language.clone()),
            translate: args.translate,
            prompt: args.prompt.clone(),
            vad_threshold: args.vad_threshold,
            no_speech_threshold: args.no_speech_threshold,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TranscriptChunk {
    pub sequence: u64,
    pub text: String,
    pub utterance_end: bool,
}

pub(crate) struct LiveSession {
    params: LiveParams,
    decoder: S16LeDecoder,
    segmenter: Segmenter,
    pending_samples: Vec<f32>,
    sequence: u64,
    utterance_open: bool,
    finished: bool,
}

impl LiveSession {
    pub(crate) fn new(params: LiveParams) -> Self {
        let vad_threshold = params.vad_threshold;
        Self {
            params,
            decoder: S16LeDecoder::default(),
            segmenter: Segmenter::new(vad_threshold),
            pending_samples: Vec::new(),
            sequence: 0,
            utterance_open: false,
            finished: false,
        }
    }

    pub(crate) fn push_pcm<F>(
        &mut self,
        engine: &mut Engine,
        bytes: &[u8],
        mut emit: F,
    ) -> Result<()>
    where
        F: FnMut(TranscriptChunk) -> Result<()>,
    {
        ensure!(!self.finished, "cannot add PCM to a finished STT session");
        self.decoder.decode(bytes, &mut self.pending_samples);
        self.process_complete_frames(engine, &mut emit)
    }

    pub(crate) fn finish<F>(
        &mut self,
        engine: &mut Engine,
        validate_complete_sample: bool,
        mut emit: F,
    ) -> Result<()>
    where
        F: FnMut(TranscriptChunk) -> Result<()>,
    {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        if validate_complete_sample {
            self.decoder.finish()?;
        }
        if !self.pending_samples.is_empty() {
            self.segmenter.push_tail(&self.pending_samples);
            self.pending_samples.clear();
        }
        if let Some(window) = self.segmenter.finish() {
            self.emit_window(engine, window, &mut emit)?;
        } else if self.utterance_open {
            self.commit_text("", true, &mut emit)?;
        }
        Ok(())
    }

    fn process_complete_frames<F>(&mut self, engine: &mut Engine, emit: &mut F) -> Result<()>
    where
        F: FnMut(TranscriptChunk) -> Result<()>,
    {
        let complete = self.pending_samples.len() / FRAME_SAMPLES * FRAME_SAMPLES;
        let remainder = self.pending_samples.split_off(complete);
        let samples = std::mem::replace(&mut self.pending_samples, remainder);
        for frame in samples.as_chunks::<FRAME_SAMPLES>().0 {
            if let Some(window) = self.segmenter.push_frame(frame)? {
                self.emit_window(engine, window, emit)?;
            }
        }
        Ok(())
    }

    fn emit_window<F>(
        &mut self,
        engine: &mut Engine,
        window: InferenceWindow,
        emit: &mut F,
    ) -> Result<()>
    where
        F: FnMut(TranscriptChunk) -> Result<()>,
    {
        if window.samples.is_empty() {
            if window.finalizes_utterance {
                self.commit_text("", true, emit)?;
            }
            return Ok(());
        }

        let audio_context = short_audio_context(window.samples.len()).with_context(|| {
            format!(
                "internal listen window exceeded 10 seconds ({} samples)",
                window.samples.len()
            )
        })?;
        engine.transcribe(
            &window.samples,
            InferenceParams {
                language: self.params.language.as_deref(),
                translate: self.params.translate,
                prompt: self.params.prompt.as_deref(),
                no_speech_threshold: self.params.no_speech_threshold,
                audio_context: Some(audio_context),
                token_timestamps: true,
            },
        )?;
        let text = engine.owned_text(window.ownership)?;
        self.commit_text(&text, window.finalizes_utterance, emit)
    }

    fn commit_text<F>(&mut self, text: &str, utterance_end: bool, emit: &mut F) -> Result<()>
    where
        F: FnMut(TranscriptChunk) -> Result<()>,
    {
        let delta = if self.utterance_open {
            text.to_owned()
        } else {
            text.trim_start().to_owned()
        };
        let emit_boundary = utterance_end && self.utterance_open && delta.is_empty();
        if !delta.is_empty() || emit_boundary {
            emit(TranscriptChunk {
                sequence: self.sequence,
                text: delta.clone(),
                utterance_end,
            })?;
            self.sequence = self.sequence.saturating_add(1);
        }

        if utterance_end {
            self.utterance_open = false;
        } else {
            self.utterance_open |= !delta.is_empty();
        }
        Ok(())
    }
}

pub(crate) fn run(mut args: ListenArgs, quiet: bool, default_model: PathBuf) -> Result<()> {
    ensure!(
        matches!(args.format, OutputFormat::Text | OutputFormat::Jsonl),
        "continuous STT supports --format text or --format jsonl"
    );
    ensure!(
        !io::stdin().is_terminal(),
        "continuous STT expects raw 16 kHz mono s16le audio on stdin; for example: \
         arecord -q -t raw -f S16_LE -r 16000 -c 1 | trueos-ttstt stt"
    );

    let model = args.model.take().unwrap_or(default_model);
    ensure!(
        model.is_file(),
        "Whisper model {} does not exist; download it as described in README.md, pass --model FILE, or set TTSTT_STT_MODEL",
        model.display()
    );

    status(
        quiet,
        format!("Loading Whisper model from {} ...", model.display()),
    );
    let load_started = Instant::now();
    let mut engine = Engine::load(&model)?;
    let params = LiveParams::from(&args);
    engine.validate(params.language.as_deref(), params.translate)?;
    status(
        quiet,
        format!("Whisper model loaded in {:.2?}.", load_started.elapsed()),
    );

    let mut sink = TextSink::new(args.output.as_deref(), args.format)?;
    let mut session = LiveSession::new(params);
    let stop_requested = Arc::new(AtomicBool::new(false));
    let input = spawn_input_pump(Arc::clone(&stop_requested))?;
    let signal_id = flag::register(SIGINT, Arc::clone(&stop_requested))
        .context("failed to install Ctrl+C handler")?;

    status(
        quiet,
        "Listening to raw 16 kHz mono s16le stdin; press Ctrl+C to finish ...".to_owned(),
    );
    let result = listen_loop(
        input,
        &mut engine,
        &mut session,
        &mut sink,
        &stop_requested,
        quiet,
    );
    low_level::unregister(signal_id);
    result
}

fn spawn_input_pump(stop_requested: Arc<AtomicBool>) -> Result<Receiver<InputEvent>> {
    let (sender, receiver) = sync_channel(INPUT_QUEUE_CHUNKS);
    thread::Builder::new()
        .name("trueos-ttstt-audio-input".to_owned())
        .spawn(move || {
            let stdin = io::stdin();
            let mut reader = stdin.lock();
            while !stop_requested.load(Ordering::Relaxed) {
                let mut input = vec![0_u8; INPUT_BUFFER_BYTES];
                match reader.read(&mut input) {
                    Ok(0) => {
                        let _ = sender.send(InputEvent::Eof);
                        break;
                    }
                    Ok(read) => {
                        input.truncate(read);
                        if sender.send(InputEvent::Audio(input)).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                        if stop_requested.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(error) => {
                        let _ = sender.send(InputEvent::Error(error));
                        break;
                    }
                }
            }
        })
        .context("failed to start the audio input worker")?;
    Ok(receiver)
}

fn listen_loop(
    input: Receiver<InputEvent>,
    engine: &mut Engine,
    session: &mut LiveSession,
    sink: &mut TextSink,
    stop_requested: &AtomicBool,
    quiet: bool,
) -> Result<()> {
    let mut reached_eof = false;

    while !stop_requested.load(Ordering::Relaxed) {
        let bytes = match input.recv_timeout(INPUT_POLL_INTERVAL) {
            Ok(InputEvent::Audio(bytes)) => bytes,
            Ok(InputEvent::Eof) => {
                reached_eof = true;
                break;
            }
            Ok(InputEvent::Error(error)) => {
                return Err(error).context("failed to read raw PCM from stdin");
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                bail!("audio input worker stopped unexpectedly")
            }
        };

        session.push_pcm(engine, &bytes, |chunk| sink.write_chunk(&chunk))?;
    }

    // The input worker may have drained PCM while Whisper was finishing an
    // inference. On Ctrl+C, consume that bounded backlog before committing the
    // final utterance instead of silently losing the most recent audio.
    if stop_requested.load(Ordering::Relaxed) {
        loop {
            match input.recv_timeout(INPUT_POLL_INTERVAL) {
                Ok(InputEvent::Audio(bytes)) => {
                    session.push_pcm(engine, &bytes, |chunk| sink.write_chunk(&chunk))?;
                }
                Ok(InputEvent::Eof) => {
                    reached_eof = true;
                    break;
                }
                Ok(InputEvent::Error(error)) => {
                    return Err(error).context("failed to read raw PCM from stdin");
                }
                Err(RecvTimeoutError::Timeout | RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    session.finish(engine, reached_eof, |chunk| sink.write_chunk(&chunk))?;
    sink.finish()?;
    status(quiet, "Listening stopped.".to_owned());
    Ok(())
}

struct InferenceWindow {
    samples: Vec<f32>,
    ownership: Ownership,
    finalizes_utterance: bool,
}

struct Segmenter {
    vad: SmoothedVad,
    buffer: Vec<f32>,
    window_start_sample: u64,
    emit_after_sample: u64,
    stream_samples: u64,
    in_speech: bool,
}

impl Segmenter {
    fn new(vad_threshold: f32) -> Self {
        Self {
            vad: SmoothedVad::new(
                Box::new(EnergyVad::new(FRAME_SAMPLES, vad_threshold)),
                PREFILL_FRAMES,
                HANGOVER_FRAMES,
                ONSET_FRAMES,
            ),
            buffer: Vec::with_capacity(MAX_WINDOW_SAMPLES),
            window_start_sample: 0,
            emit_after_sample: 0,
            stream_samples: 0,
            in_speech: false,
        }
    }

    fn push_frame(&mut self, frame: &[f32]) -> Result<Option<InferenceWindow>> {
        ensure!(
            frame.len() == FRAME_SAMPLES,
            "listen frame must contain {FRAME_SAMPLES} samples"
        );
        let frame_start = self.stream_samples;
        self.stream_samples = self.stream_samples.saturating_add(frame.len() as u64);
        let is_speech = self.vad.is_speech(frame)?;

        if is_speech {
            if !self.in_speech {
                let prefill = self.vad.drain_prefill();
                self.window_start_sample = frame_start.saturating_sub(prefill.len() as u64);
                self.emit_after_sample = self.window_start_sample;
                self.buffer.clear();
                self.buffer.extend_from_slice(&prefill);
            }
            self.buffer.extend_from_slice(frame);
            self.in_speech = true;

            if self.buffer.len() >= MAX_WINDOW_SAMPLES {
                return Ok(Some(self.force_window()));
            }
        } else if self.in_speech {
            self.in_speech = false;
            let window = self.final_window();
            self.vad.reset();
            return Ok(window);
        }

        Ok(None)
    }

    fn push_tail(&mut self, samples: &[f32]) {
        self.stream_samples = self.stream_samples.saturating_add(samples.len() as u64);
        if self.in_speech {
            self.buffer.extend_from_slice(samples);
        }
    }

    fn finish(&mut self) -> Option<InferenceWindow> {
        self.in_speech = false;
        let window = self.final_window();
        self.vad.reset();
        window
    }

    fn force_window(&mut self) -> InferenceWindow {
        let samples = std::mem::take(&mut self.buffer);
        let window_end = self
            .window_start_sample
            .saturating_add(samples.len() as u64);
        let boundary = self
            .window_start_sample
            .saturating_add(lowest_energy_boundary(&samples) as u64);
        let retained_start = samples.len().saturating_sub(OVERLAP_SAMPLES);
        self.buffer.extend_from_slice(&samples[retained_start..]);

        let ownership = Ownership {
            window_start_sample: self.window_start_sample,
            emit_after_sample: self.emit_after_sample,
            emit_before_sample: boundary,
        };
        self.window_start_sample = window_end.saturating_sub(self.buffer.len() as u64);
        self.emit_after_sample = boundary;

        InferenceWindow {
            samples,
            ownership,
            finalizes_utterance: false,
        }
    }

    fn final_window(&mut self) -> Option<InferenceWindow> {
        if self.buffer.is_empty() {
            return None;
        }
        let samples = std::mem::take(&mut self.buffer);
        Some(InferenceWindow {
            samples,
            ownership: Ownership {
                window_start_sample: self.window_start_sample,
                emit_after_sample: self.emit_after_sample,
                emit_before_sample: u64::MAX,
            },
            finalizes_utterance: true,
        })
    }
}

fn lowest_energy_boundary(samples: &[f32]) -> usize {
    let search_start = samples.len().saturating_sub(OVERLAP_SAMPLES);
    let search_start = search_start.div_ceil(FRAME_SAMPLES) * FRAME_SAMPLES;
    let mut minimum_energy = f64::INFINITY;
    let mut boundary = samples.len();

    for (index, frame) in samples[search_start..]
        .as_chunks::<FRAME_SAMPLES>()
        .0
        .iter()
        .enumerate()
    {
        let energy = frame
            .iter()
            .map(|sample| f64::from(*sample) * f64::from(*sample))
            .sum::<f64>()
            / frame.len() as f64;
        // Prefer the latest equally quiet frame, retaining the largest stable
        // prefix when a long silence spans much of the overlap.
        if energy <= minimum_energy {
            minimum_energy = energy;
            boundary = search_start + (index + 1) * FRAME_SAMPLES;
        }
    }

    boundary.min(samples.len())
}

#[derive(Default)]
struct S16LeDecoder {
    carry: Option<u8>,
}

impl S16LeDecoder {
    fn decode(&mut self, bytes: &[u8], output: &mut Vec<f32>) {
        let mut index = 0;
        if let Some(low) = self.carry.take() {
            if let Some(&high) = bytes.first() {
                output.push(f32::from(i16::from_le_bytes([low, high])) / 32_768.0);
                index = 1;
            } else {
                self.carry = Some(low);
                return;
            }
        }

        while index + 1 < bytes.len() {
            output.push(f32::from(i16::from_le_bytes([bytes[index], bytes[index + 1]])) / 32_768.0);
            index += 2;
        }
        if index < bytes.len() {
            self.carry = Some(bytes[index]);
        }
    }

    fn finish(&self) -> Result<()> {
        if self.carry.is_some() {
            bail!("raw PCM input ended with an incomplete 16-bit sample");
        }
        Ok(())
    }
}

struct TextSink {
    writer: Box<dyn Write>,
    format: OutputFormat,
}

impl TextSink {
    fn new(destination: Option<&Path>, format: OutputFormat) -> Result<Self> {
        let writer: Box<dyn Write> = match destination {
            None => Box::new(BufWriter::new(io::stdout())),
            Some(path) if path == Path::new("-") => Box::new(BufWriter::new(io::stdout())),
            Some(path) => {
                Box::new(BufWriter::new(File::create(path).with_context(|| {
                    format!("failed to create {}", path.display())
                })?))
            }
        };
        Ok(Self::with_writer(writer, format))
    }

    fn with_writer(writer: Box<dyn Write>, format: OutputFormat) -> Self {
        Self { writer, format }
    }

    fn write_chunk(&mut self, chunk: &TranscriptChunk) -> Result<()> {
        if self.format == OutputFormat::Text && chunk.text.is_empty() {
            return Ok(());
        }
        let record = render_stream_record(
            self.format,
            chunk.sequence,
            &chunk.text,
            chunk.utterance_end,
        );
        self.writer
            .write_all(record.as_bytes())
            .context("failed to write live transcription")?;
        self.writer
            .flush()
            .context("failed to flush live transcription")?;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.writer
            .flush()
            .context("failed to flush live transcription")
    }
}

fn render_stream_record(
    format: OutputFormat,
    sequence: u64,
    text: &str,
    utterance_end: bool,
) -> String {
    match format {
        OutputFormat::Text => format!("{text}\n"),
        OutputFormat::Jsonl => format!(
            "{}\n",
            serde_json::to_string(&serde_json::json!({
                "sequence": sequence,
                "text": text,
                "utterance_end": utterance_end,
            }))
            .expect("live transcription record is JSON-serializable")
        ),
        OutputFormat::Json | OutputFormat::Srt | OutputFormat::Vtt => {
            unreachable!("unsupported continuous format was rejected before creating the sink")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::io::{self, Write};
    use std::rc::Rc;

    use super::{
        FRAME_SAMPLES, LiveParams, LiveSession, MAX_WINDOW_SAMPLES, OVERLAP_SAMPLES, S16LeDecoder,
        Segmenter, TextSink, TranscriptChunk, lowest_energy_boundary, render_stream_record,
    };
    use crate::cli::OutputFormat;

    #[derive(Default)]
    struct State {
        bytes: Vec<u8>,
        flushes: usize,
    }

    struct SharedWriter(Rc<RefCell<State>>);

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.borrow_mut().bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.0.borrow_mut().flushes += 1;
            Ok(())
        }
    }

    fn session() -> LiveSession {
        LiveSession::new(LiveParams {
            language: Some("en".to_owned()),
            translate: false,
            prompt: None,
            vad_threshold: 0.01,
            no_speech_threshold: 0.2,
        })
    }

    fn commit(session: &mut LiveSession, text: &str, utterance_end: bool) -> TranscriptChunk {
        let mut chunks = Vec::new();
        session
            .commit_text(text, utterance_end, &mut |chunk| {
                chunks.push(chunk);
                Ok(())
            })
            .unwrap();
        assert_eq!(chunks.len(), 1);
        chunks.pop().unwrap()
    }

    #[test]
    fn decoder_handles_odd_read_boundaries() {
        let mut decoder = S16LeDecoder::default();
        let mut samples = Vec::new();
        decoder.decode(&[0], &mut samples);
        decoder.decode(&[128, 255, 127], &mut samples);
        decoder.finish().unwrap();
        assert_eq!(samples, [-1.0, 32_767.0 / 32_768.0]);
    }

    #[test]
    fn decoder_rejects_an_odd_byte_at_eof() {
        let mut decoder = S16LeDecoder::default();
        decoder.decode(&[1], &mut Vec::new());
        assert!(decoder.finish().is_err());
    }

    #[test]
    fn energy_search_uses_the_latest_quiet_frame() {
        let mut samples = vec![0.5; OVERLAP_SAMPLES];
        let quiet_start = samples.len() - 2 * FRAME_SAMPLES;
        samples[quiet_start..].fill(0.0);
        assert_eq!(lowest_energy_boundary(&samples), samples.len());
    }

    #[test]
    fn forced_window_retains_exactly_three_seconds() {
        let mut segmenter = Segmenter::new(0.01);
        let speech = vec![0.1; FRAME_SAMPLES];
        let mut window = None;
        while window.is_none() {
            window = segmenter.push_frame(&speech).unwrap();
        }
        let window = window.unwrap();
        assert_eq!(window.samples.len(), MAX_WINDOW_SAMPLES);
        assert_eq!(segmenter.buffer.len(), OVERLAP_SAMPLES);
        assert_eq!(
            segmenter.emit_after_sample,
            window.ownership.emit_before_sample
        );
    }

    #[test]
    fn text_stream_records_are_line_framed() {
        assert_eq!(
            render_stream_record(OutputFormat::Text, 7, "hello", false),
            "hello\n"
        );
    }

    #[test]
    fn jsonl_stream_records_describe_order_and_boundaries() {
        let rendered = render_stream_record(OutputFormat::Jsonl, 7, "hello", true);
        assert_eq!(rendered.lines().count(), 1);
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["sequence"], 7);
        assert_eq!(value["text"], "hello");
        assert_eq!(value["utterance_end"], true);
    }

    #[test]
    fn sink_flushes_every_jsonl_record() {
        let state = Rc::new(RefCell::new(State::default()));
        let mut sink = TextSink::with_writer(
            Box::new(SharedWriter(Rc::clone(&state))),
            OutputFormat::Jsonl,
        );
        let mut session = session();
        for chunk in [
            commit(&mut session, " Hello", false),
            commit(&mut session, ",", false),
            commit(&mut session, " world", true),
        ] {
            sink.write_chunk(&chunk).unwrap();
        }

        let state = state.borrow();
        assert_eq!(state.flushes, 3);
        let records = String::from_utf8(state.bytes.clone()).unwrap();
        let records = records.lines().collect::<Vec<_>>();
        assert_eq!(records.len(), 3);
        let records = records
            .iter()
            .map(|record| serde_json::from_str::<serde_json::Value>(record).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(records[0]["sequence"], 0);
        assert_eq!(records[1]["sequence"], 1);
        assert_eq!(records[2]["utterance_end"], true);
        let reconstructed = records
            .iter()
            .map(|record| record["text"].as_str().unwrap())
            .collect::<String>();
        assert_eq!(reconstructed, "Hello, world");
    }

    #[test]
    fn append_deltas_preserve_subword_boundaries() {
        let mut session = session();
        let chunks = [
            commit(&mut session, " playing", false),
            commit(&mut session, "ly", true),
        ];
        let reconstructed = chunks
            .iter()
            .map(|chunk| chunk.text.as_str())
            .collect::<String>();
        assert_eq!(reconstructed, "playingly");
    }
}
