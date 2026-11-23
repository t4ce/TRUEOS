//! Compressed-picture queue. This task owns HTTP and can refill while the
//! playback task is decoding, converting, or waiting for the display latch.
use super::*;
use alloc::{collections::VecDeque, sync::Arc};
use atomic_waker::AtomicWaker;
use core::sync::atomic::AtomicBool;
use core::task::Poll;
use spin::Mutex;
use crate::r::services::audio_playback::VideoPcm;

const LOW_WATER_US: u64 = 250_000;
const HIGH_WATER_US: u64 = 750_000;
const MAX_QUEUED_PICTURES: usize = 64;
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
const SOUNDTRACK_PREROLL_FRAMES: usize = 5_760; // 120 ms
const SOUNDTRACK_BUFFER_FRAMES: usize = 11_520;
const SOUNDTRACK_REFILL_MS: u64 = 10;

struct BufferedSample {
    bytes: Vec<u8>,
    timing: H264SampleTiming,
    // A complete fragment's presentation order must be known before its first
    // picture is decoded. Later samples in the batch carry no duplicate table.
    added_timing: Vec<H264SampleTiming>,
    duration_us: u64,
}

#[derive(Default)]
struct PrefetchState {
    queue: VecDeque<BufferedSample>,
    bytes: usize,
    duration_us: u64,
    video_done: bool,
    done: bool,
    error: Option<&'static str>,
    audio: Option<VideoPcm>,
    max_bytes: usize,
    max_pictures: usize,
    max_duration_us: u64,
    consumer_waits: usize,
}

struct PrefetchShared {
    state: Mutex<PrefetchState>,
    reader: Mutex<Option<HttpMp4Reader>>,
    stop: Arc<AtomicBool>,
    producer: AtomicWaker,
    consumer: AtomicWaker,
}

pub(crate) struct BufferedHttpMp4Reader {
    shared: Arc<PrefetchShared>,
    session: crate::ui4::VideoPlaybackSession,
    pub(crate) timing: Vec<H264SampleTiming>,
}

impl HttpMp4Reader {
    pub(crate) async fn into_buffered(mut self) -> Result<BufferedHttpMp4Reader, &'static str> {
        let Some(worker) = crate::workers::pick_background_spawner() else {
            self.close().await;
            return Err("HTTP video prefetch worker unavailable");
        };
        let shared = Arc::new(PrefetchShared {
            state: Mutex::new(PrefetchState::default()),
            reader: Mutex::new(None),
            stop: Arc::new(AtomicBool::new(false)),
            producer: AtomicWaker::new(),
            consumer: AtomicWaker::new(),
        });
        self.source.stop = Some(shared.stop.clone());
        // Reserve the task slot before transferring the parser. Failed
        // admission can then await its TLS fence with ownership intact.
        let task = match prefetch_task(shared.clone()) {
            Ok(task) => task,
            Err(_) => {
                self.close().await;
                return Err("HTTP video prefetch task limit");
            }
        };
        let session = self.source.session;
        *shared.reader.lock() = Some(self);
        worker.spawn(task);
        Ok(BufferedHttpMp4Reader { shared, session, timing: Vec::new() })
    }
}

impl BufferedHttpMp4Reader {
    pub(crate) async fn next_sample(
        &mut self,
    ) -> Result<Option<(Vec<u8>, H264SampleTiming)>, &'static str> {
        let shared = self.shared.clone();
        let mut counted_wait = false;
        let sample = core::future::poll_fn(|cx| {
            shared.consumer.register(cx.waker());
            let mut state = shared.state.lock();
            if let Some(sample) = state.queue.pop_front() {
                state.bytes -= sample.bytes.len();
                state.duration_us = state.duration_us.saturating_sub(sample.duration_us);
                drop(state);
                shared.producer.wake();
                return Poll::Ready(Ok(Some(sample)));
            }
            if state.video_done || state.done {
                return Poll::Ready(match state.error {
                    Some(error) => Err(error),
                    None => Ok(None),
                });
            }
            if !counted_wait {
                state.consumer_waits += 1;
                counted_wait = true;
            }
            Poll::Pending
        }).await?;
        let Some(sample) = sample else { return Ok(None); };
        self.timing.extend_from_slice(&sample.added_timing);
        Ok(Some((sample.bytes, sample.timing)))
    }

    /// Called after the final displayed picture, independently of video EOF.
    pub(crate) async fn finish_audio(&mut self) {
        let audio = loop {
            {
                let state = self.shared.state.lock();
                if state.done {
                    break state.audio;
                }
            }
            if !self.session.wait_until_playing().await {
                return;
            }
            Timer::after_millis(SOUNDTRACK_REFILL_MS).await;
        };
        if let Some(audio) = audio {
            while audio.queued_frames() != 0 && self.session.wait_until_playing().await {
                Timer::after_millis(SOUNDTRACK_REFILL_MS).await;
            }
        }
    }

    /// Stop admission and await the HTTP task's TLS cancellation fence before
    /// the playback reservation is released. A timeout never drops live I/O.
    pub(crate) async fn close(&mut self) {
        self.stop();
        {
            let mut state = self.shared.state.lock();
            state.queue.clear();
            state.bytes = 0;
            state.duration_us = 0;
        }
        self.shared.producer.wake();
        core::future::poll_fn(|cx| {
            self.shared.consumer.register(cx.waker());
            if self.shared.state.lock().done { Poll::Ready(()) } else { Poll::Pending }
        }).await;
    }

    fn stop(&self) {
        self.shared.stop.store(true, Ordering::Release);
        let audio = self.shared.state.lock().audio.take();
        if let Some(audio) = audio {
            audio.abort();
        }
        self.shared.producer.wake();
    }
}

impl Drop for BufferedHttpMp4Reader {
    fn drop(&mut self) {
        self.stop();
    }
}

fn sample_duration_us(timing: H264SampleTiming) -> u64 {
    if timing.duration == 0 || timing.timescale == 0 {
        16_667
    } else {
        (u64::from(timing.duration).saturating_mul(1_000_000)
            / u64::from(timing.timescale)).max(1)
    }
}

struct Soundtrack {
    track: audio::Mp4Audio,
    pcm: VideoPcm,
    writer: audio::PcmWriter,
    pending: Option<audio::DecodedAudio>,
}

impl Soundtrack {
    fn begin(track: audio::Mp4Audio, session: crate::ui4::VideoPlaybackSession) -> Option<Self> {
        match session.begin_audio() {
            Ok(pcm) => Some(Self { track, pcm, writer: audio::PcmWriter::default(), pending: None }),
            Err(error) => {
                crate::log_warn!(target: "intel-media";
                    "video/http: soundtrack admission failed error={}\n", error);
                None
            }
        }
    }

    async fn feed(&mut self, source: &mut HttpSource) -> Result<bool, &'static str> {
        if self.pending.is_none() {
            let Some(sample) = self.track.next_sample() else { return Ok(false) };
            let compressed = source.read(sample.offset, sample.size, ReadWindow::Audio).await?;
            self.pending = self.track.decode_sample(compressed)?;
        }
        if let Some(packet) = &self.pending
            && self.writer.write(packet, |bytes| self.pcm.write_pcm(bytes))
                .map_err(|_| "video soundtrack stream stopped")?
        {
            self.pending = None;
        }
        Ok(true)
    }
}

enum Work {
    Audio,
    Video,
    QueuePicture,
    Recheck,
    Stop,
}

async fn run_prefetch(mut reader: HttpMp4Reader, shared: Arc<PrefetchShared>) {
    let mut timing_sent = 0usize;
    let mut refilling = true;
    let mut video_done = false;
    let session = reader.source.session;
    let mut audio = reader.audio.take().and_then(|track| Soundtrack::begin(track, session));
    shared.state.lock().audio = audio.as_ref().map(|audio| audio.pcm);
    let mut prefer_audio = true;
    let mut pending_picture: Option<(Vec<u8>, H264SampleTiming)> = None;
    loop {
        let mut retry_audio = core::pin::pin!(Timer::after_millis(SOUNDTRACK_REFILL_MS));
        let work = core::future::poll_fn(|cx| {
            use core::future::Future;
            shared.producer.register(cx.waker());
            if reader.source.stopped() || (video_done && audio.is_none()) {
                return Poll::Ready(Work::Stop);
            }
            let state = shared.state.lock();
            if let Some((picture, _)) = &pending_picture
                && state.bytes.saturating_add(picture.len()) <= MAX_QUEUED_BYTES
            {
                return Poll::Ready(Work::QueuePicture);
            }
            if !refilling && (state.duration_us <= LOW_WATER_US || state.queue.is_empty()) {
                refilling = true;
            }
            let video_ready = !video_done && pending_picture.is_none() && refilling
                && state.queue.len() < MAX_QUEUED_PICTURES && state.bytes < MAX_QUEUED_BYTES;
            drop(state);
            let preroll = !session.has_published();
            let audio_ready = audio.as_ref().is_some_and(|audio| {
                let limit = if preroll { SOUNDTRACK_PREROLL_FRAMES } else { SOUNDTRACK_BUFFER_FRAMES };
                audio.pcm.queued_frames() < limit
            });
            if audio_ready && (preroll || prefer_audio || !video_ready) {
                return Poll::Ready(Work::Audio);
            }
            if video_ready {
                return Poll::Ready(Work::Video);
            }
            if audio.is_some() && retry_audio.as_mut().poll(cx).is_ready() {
                return Poll::Ready(Work::Recheck);
            }
            Poll::Pending
        }).await;
        match work {
            Work::Stop => break,
            Work::Recheck => continue,
            Work::Audio => {
                prefer_audio = false;
                let soundtrack = audio.as_mut().unwrap();
                match soundtrack.feed(&mut reader.source).await {
                    Ok(true) => {}
                    Ok(false) => {
                        soundtrack.pcm.finish();
                        audio = None;
                    }
                    Err(reason) => {
                        soundtrack.pcm.abort();
                        audio = None;
                        if !reader.source.stopped() {
                            crate::log_warn!(target: "intel-media";
                                "video/http: soundtrack stopped reason={}\n", reason);
                        }
                    }
                }
                continue;
            }
            Work::Video => {
                prefer_audio = true;
                let error = match reader.next_sample().await {
                    Ok(Some(sample)) if sample.0.len() <= MAX_QUEUED_BYTES => {
                        pending_picture = Some(sample);
                        continue;
                    }
                    Ok(Some(_)) => Some("HTTP buffered picture exceeds byte budget"),
                    Ok(None) => None,
                    Err(reason) => Some(reason),
                };
                video_done = true;
                let mut state = shared.state.lock();
                state.video_done = true;
                state.error = error.filter(|_| !reader.source.stopped());
                drop(state);
                shared.consumer.wake();
                if error.is_some() {
                    break;
                }
                continue;
            }
            Work::QueuePicture => {}
        }
        let (bytes, timing) = pending_picture.take().unwrap();
        let added_timing = reader.timing[timing_sent..].to_vec();
        timing_sent = reader.timing.len();
        let duration_us = sample_duration_us(timing);
        {
            let mut state = shared.state.lock();
            state.bytes += bytes.len();
            state.duration_us = state.duration_us.saturating_add(duration_us);
            state.queue.push_back(BufferedSample { bytes, timing, added_timing, duration_us });
            state.max_bytes = state.max_bytes.max(state.bytes);
            state.max_pictures = state.max_pictures.max(state.queue.len());
            state.max_duration_us = state.max_duration_us.max(state.duration_us);
            if state.duration_us >= HIGH_WATER_US
                || state.queue.len() >= MAX_QUEUED_PICTURES
                || state.bytes >= MAX_QUEUED_BYTES
            {
                refilling = false;
            }
        }
        shared.consumer.wake();
    }
    if let Some(audio) = audio {
        audio.pcm.abort();
    }
    reader.source.transport.close().await;
    let mut state = shared.state.lock();
    state.done = true;
    crate::log_info!(target: "intel-media";
        "video/http: prefetch done fetched_bytes={} consumer_waits={} max_queued_bytes={} max_queued_pictures={} max_queued_us={} low_water_us={} high_water_us={}\n",
        reader.source.bytes_fetched, state.consumer_waits, state.max_bytes,
        state.max_pictures, state.max_duration_us, LOW_WATER_US, HIGH_WATER_US);
    drop(state);
    shared.consumer.wake();
}

#[trueos_executor::task(pool_size = 3)]
async fn prefetch_task(shared: Arc<PrefetchShared>) {
    let reader = shared.reader.lock().take();
    if let Some(reader) = reader {
        run_prefetch(reader, shared).await;
    }
}
