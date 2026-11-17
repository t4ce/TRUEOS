//! Compressed-picture queue. This task owns HTTP and can refill while the
//! playback task is decoding, converting, or waiting for the display latch.
use super::*;
use alloc::{collections::VecDeque, sync::Arc};
use atomic_waker::AtomicWaker;
use core::sync::atomic::AtomicBool;
use core::task::Poll;
use spin::Mutex;

const LOW_WATER_US: u64 = 250_000;
const HIGH_WATER_US: u64 = 750_000;
const MAX_QUEUED_PICTURES: usize = 64;
const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;

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
    done: bool,
    error: Option<&'static str>,
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
        *shared.reader.lock() = Some(self);
        worker.spawn(task);
        Ok(BufferedHttpMp4Reader { shared, timing: Vec::new() })
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
            if state.done {
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

    /// Stop admission and await the HTTP task's TLS cancellation fence before
    /// the playback reservation is released. A timeout never drops live I/O.
    pub(crate) async fn close(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
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
}

impl Drop for BufferedHttpMp4Reader {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        self.shared.producer.wake();
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

async fn run_prefetch(mut reader: HttpMp4Reader, shared: Arc<PrefetchShared>) {
    let mut timing_sent = 0usize;
    let mut refilling = true;
    let mut error = None;
    loop {
        let admitted = core::future::poll_fn(|cx| {
            shared.producer.register(cx.waker());
            if reader.source.stopped() { return Poll::Ready(false); }
            let state = shared.state.lock();
            if !refilling && (state.duration_us <= LOW_WATER_US || state.queue.is_empty()) {
                refilling = true;
            }
            if refilling && state.queue.len() < MAX_QUEUED_PICTURES
                && state.bytes < MAX_QUEUED_BYTES
            {
                Poll::Ready(true)
            } else {
                Poll::Pending
            }
        }).await;
        if !admitted { break; }
        let (bytes, timing) = match reader.next_sample().await {
            Ok(Some(sample)) => sample,
            Ok(None) => break,
            Err(reason) => { error = Some(reason); break; }
        };
        if bytes.len() > MAX_QUEUED_BYTES {
            error = Some("HTTP buffered picture exceeds byte budget");
            break;
        }
        // At most one bounded picture is held outside the queue while waiting
        // for byte capacity. Never fetch an unbounded number of large IDRs.
        let fits = core::future::poll_fn(|cx| {
            shared.producer.register(cx.waker());
            if reader.source.stopped() { return Poll::Ready(false); }
            let state = shared.state.lock();
            if state.bytes.saturating_add(bytes.len()) <= MAX_QUEUED_BYTES {
                Poll::Ready(true)
            } else {
                Poll::Pending
            }
        }).await;
        if !fits { break; }
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
    reader.source.transport.close().await;
    let mut state = shared.state.lock();
    state.error = if reader.source.stopped() { None } else { error };
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
