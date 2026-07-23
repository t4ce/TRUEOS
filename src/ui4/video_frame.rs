//! UI4 ownership wrapper for decoded video.
//!
//! The decoder remains a native media-Y-tiled NV12 producer only until one
//! SIMD16 GuC dispatch writes an exact broker-owned RGBA buffer. From that
//! completion onward the broker, compositor, and display exchange RGBA only.
//! The streaming Frame has distinct producer-write, broker-pending, and
//! display-live allocations; display ownership independently ends at SURFLIVE.
//! The older `Tile64` Rust/kernel symbol names are retained as an artifact ABI;
//! the shader uses the proven 128x32 media Y-tile byte layout.

use alloc::{collections::VecDeque, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use spin::Mutex;

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    ScanoutFormat, Ui4InputEvent, WindowCreate, WindowId, WindowOwner, WindowPlacement,
    WindowSessionCloseRequest, WindowSessionId, acquire_frame_buffer, begin_window_session,
    cancel_frame_buffer, create_frame, create_window, destroy_frame, finish_window_session,
    finish_window_session_with_request, gpgpu_rgba_surface, publish_gpgpu_video_frame_buffer,
    publish_window_frame, take_owner_input_events, wait_frame_buffer_release,
};

// The decoded-video producer owns one ordinary broker window independently of
// the compositor service.
const VIDEO_OWNER: WindowOwner = WindowOwner::VIDEO_PLAYER;
const VIDEO_OUTPUT: OutputId = OutputId::from_slot(0).unwrap();
const VIDEO_PLANE_SLOT: usize = super::ALPHA_OVERLAY_PLANE_SLOT;
const VIDEO_RGBA_BUFFERING: FrameBuffering = FrameBuffering::Triple;
const VIDEO_RGBA_BUFFER_COUNT: usize = VIDEO_RGBA_BUFFERING.count();
/// At most one conversion may execute while one decoded DPB surface waits.
/// The current AVC path retains three references in four slots; the playback
/// loop additionally drains this queue before every later IDR reuses slot 0.
const VIDEO_CONVERSION_OUTSTANDING_CAP: usize = 2;
const VIDEO_CONVERSION_ERROR_LOG_INTERVAL_TICKS: u64 = embassy_time::TICK_HZ * 10;
const VIDEO_CONVERSION_PRESENT_ERROR: i32 = -34;
const VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKET_US: u64 = 250;
const VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKETS: usize = 128;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedNv12Source {
    pub(crate) decode_sequence: u64,
    pub(crate) gpu: u64,
    pub(crate) phys: u64,
    pub(crate) virt: usize,
    pub(crate) byte_len: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) visible_width: u32,
    pub(crate) visible_height: u32,
    pub(crate) pitch_bytes: usize,
    pub(crate) uv_offset: usize,
}

/// Bounded live-path probe for the ordered decode-to-publication architecture.
/// It submits no diagnostic GPU work and allocates nothing per frame: samples
/// are taken only at existing ownership transitions and folded into fixed
/// histograms after the request completes.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DecodedVideoConversionProbeReport {
    pub(crate) samples: usize,
    pub(crate) rcs_samples: usize,
    pub(crate) quantile_bucket_us: u64,
    pub(crate) avg_queue_wait_us: u64,
    pub(crate) max_queue_wait_us: u64,
    pub(crate) avg_end_to_end_us: u64,
    pub(crate) max_end_to_end_us: u64,
    pub(crate) p50_end_to_end_us: u64,
    pub(crate) p95_end_to_end_us: u64,
    pub(crate) p99_end_to_end_us: u64,
    pub(crate) avg_bind_layout_us: u64,
    pub(crate) max_bind_layout_us: u64,
    pub(crate) avg_rgba_acquire_us: u64,
    pub(crate) max_rgba_acquire_us: u64,
    pub(crate) avg_surface_prepare_us: u64,
    pub(crate) max_surface_prepare_us: u64,
    pub(crate) avg_rcs_queue_us: u64,
    pub(crate) max_rcs_queue_us: u64,
    pub(crate) avg_rcs_completion_us: u64,
    pub(crate) max_rcs_completion_us: u64,
    pub(crate) avg_publish_us: u64,
    pub(crate) max_publish_us: u64,
    pub(crate) avg_rcs_queue_prepare_us: u64,
    pub(crate) max_rcs_queue_prepare_us: u64,
    pub(crate) p50_rcs_queue_prepare_us: u64,
    pub(crate) p95_rcs_queue_prepare_us: u64,
    pub(crate) p99_rcs_queue_prepare_us: u64,
    pub(crate) avg_rcs_queue_total_us: u64,
    pub(crate) avg_rcs_forcewake_us: u64,
    pub(crate) avg_rcs_state_map_us: u64,
    pub(crate) avg_rcs_ppgtt_init_us: u64,
    pub(crate) avg_rcs_kernel_map_us: u64,
    pub(crate) avg_rcs_source_map_us: u64,
    pub(crate) avg_rcs_destination_map_us: u64,
    pub(crate) avg_rcs_batch_encode_us: u64,
    pub(crate) avg_rcs_admission_us: u64,
    pub(crate) avg_rcs_submit_to_marker_us: u64,
    pub(crate) max_rcs_submit_to_marker_us: u64,
    pub(crate) p50_rcs_submit_to_marker_us: u64,
    pub(crate) p95_rcs_submit_to_marker_us: u64,
    pub(crate) p99_rcs_submit_to_marker_us: u64,
    pub(crate) avg_completion_polls: u64,
    pub(crate) max_completion_polls: u64,
    pub(crate) gpu_timestamp_samples: usize,
    pub(crate) gpu_timestamp_frequency_hz: u64,
    pub(crate) avg_gpu_walker_us: u64,
    pub(crate) max_gpu_walker_us: u64,
    pub(crate) p50_gpu_walker_us: u64,
    pub(crate) p95_gpu_walker_us: u64,
    pub(crate) p99_gpu_walker_us: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct DecodedVideoConversionReport {
    pub(crate) generation: u64,
    pub(crate) queued: usize,
    pub(crate) completed: usize,
    pub(crate) published: usize,
    pub(crate) first_failure_frame: usize,
    pub(crate) first_failure_error: i32,
    pub(crate) backpressure_events: usize,
    pub(crate) rgba_buffer_wait_events: usize,
    pub(crate) rcs_submit_wait_events: usize,
    pub(crate) max_outstanding: usize,
    pub(crate) total_conversion_us: u64,
    pub(crate) max_conversion_us: u64,
    pub(crate) probe: DecodedVideoConversionProbeReport,
}

impl DecodedVideoConversionReport {
    pub(crate) const fn avg_conversion_us(self) -> u64 {
        if self.completed == 0 {
            0
        } else {
            self.total_conversion_us / self.completed as u64
        }
    }
}

#[derive(Copy, Clone)]
struct DecodedVideoConversionRequest {
    generation: u64,
    playback_frame: usize,
    enqueued_tick: u64,
    source: DecodedNv12Source,
}

#[derive(Copy, Clone, Default)]
struct VideoConversionProbeMetric {
    total_us: u64,
    max_us: u64,
}

impl VideoConversionProbeMetric {
    const fn new() -> Self {
        Self {
            total_us: 0,
            max_us: 0,
        }
    }

    fn record(&mut self, value_us: u64) {
        self.total_us = self.total_us.saturating_add(value_us);
        self.max_us = self.max_us.max(value_us);
    }

    const fn average(self, samples: usize) -> u64 {
        if samples == 0 {
            0
        } else {
            self.total_us / samples as u64
        }
    }
}

#[derive(Copy, Clone)]
struct VideoConversionProbeHistogram {
    buckets: [u32; VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKETS],
    overflow: u32,
    samples: u64,
}

impl VideoConversionProbeHistogram {
    const fn new() -> Self {
        Self {
            buckets: [0; VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKETS],
            overflow: 0,
            samples: 0,
        }
    }

    fn record(&mut self, value_us: u64) {
        self.samples = self.samples.saturating_add(1);
        let bucket = (value_us / VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKET_US) as usize;
        if let Some(count) = self.buckets.get_mut(bucket) {
            *count = count.saturating_add(1);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    fn percentile(&self, percent: u64, overflow_value_us: u64) -> u64 {
        if self.samples == 0 {
            return 0;
        }
        let rank = self.samples.saturating_mul(percent).saturating_add(99) / 100;
        let mut observed = 0u64;
        for (index, count) in self.buckets.iter().enumerate() {
            observed = observed.saturating_add(u64::from(*count));
            if observed >= rank {
                return (index as u64 + 1)
                    .saturating_mul(VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKET_US);
            }
        }
        overflow_value_us
    }
}

#[derive(Copy, Clone, Default)]
struct DecodedVideoConversionProbeSample {
    queue_wait_us: u64,
    end_to_end_us: u64,
    bind_layout_us: u64,
    rgba_acquire_us: u64,
    surface_prepare_us: u64,
    rcs_queue_us: u64,
    rcs_completion_us: u64,
    publish_us: u64,
    rcs: Option<crate::intel::gpgpu::GpgpuSubmissionProbe>,
}

#[derive(Copy, Clone)]
struct DecodedVideoConversionOutcome {
    published: bool,
    probe: DecodedVideoConversionProbeSample,
}

impl DecodedVideoConversionProbeSample {
    const fn finish(self, published: bool) -> DecodedVideoConversionOutcome {
        DecodedVideoConversionOutcome {
            published,
            probe: self,
        }
    }
}

#[derive(Copy, Clone)]
struct DecodedVideoConversionProbeState {
    samples: usize,
    rcs_samples: usize,
    queue_wait: VideoConversionProbeMetric,
    end_to_end: VideoConversionProbeMetric,
    bind_layout: VideoConversionProbeMetric,
    rgba_acquire: VideoConversionProbeMetric,
    surface_prepare: VideoConversionProbeMetric,
    rcs_queue: VideoConversionProbeMetric,
    rcs_completion: VideoConversionProbeMetric,
    publish: VideoConversionProbeMetric,
    rcs_queue_prepare: VideoConversionProbeMetric,
    rcs_queue_total: VideoConversionProbeMetric,
    rcs_forcewake: VideoConversionProbeMetric,
    rcs_state_map: VideoConversionProbeMetric,
    rcs_ppgtt_init: VideoConversionProbeMetric,
    rcs_kernel_map: VideoConversionProbeMetric,
    rcs_source_map: VideoConversionProbeMetric,
    rcs_destination_map: VideoConversionProbeMetric,
    rcs_batch_encode: VideoConversionProbeMetric,
    rcs_admission: VideoConversionProbeMetric,
    rcs_submit_to_marker: VideoConversionProbeMetric,
    end_to_end_histogram: VideoConversionProbeHistogram,
    rcs_queue_prepare_histogram: VideoConversionProbeHistogram,
    rcs_submit_to_marker_histogram: VideoConversionProbeHistogram,
    gpu_timestamp_samples: usize,
    gpu_timestamp_frequency_hz: u64,
    gpu_walker: VideoConversionProbeMetric,
    gpu_walker_histogram: VideoConversionProbeHistogram,
    total_completion_polls: u64,
    max_completion_polls: u64,
}

impl DecodedVideoConversionProbeState {
    const fn new() -> Self {
        Self {
            samples: 0,
            rcs_samples: 0,
            queue_wait: VideoConversionProbeMetric::new(),
            end_to_end: VideoConversionProbeMetric::new(),
            bind_layout: VideoConversionProbeMetric::new(),
            rgba_acquire: VideoConversionProbeMetric::new(),
            surface_prepare: VideoConversionProbeMetric::new(),
            rcs_queue: VideoConversionProbeMetric::new(),
            rcs_completion: VideoConversionProbeMetric::new(),
            publish: VideoConversionProbeMetric::new(),
            rcs_queue_prepare: VideoConversionProbeMetric::new(),
            rcs_queue_total: VideoConversionProbeMetric::new(),
            rcs_forcewake: VideoConversionProbeMetric::new(),
            rcs_state_map: VideoConversionProbeMetric::new(),
            rcs_ppgtt_init: VideoConversionProbeMetric::new(),
            rcs_kernel_map: VideoConversionProbeMetric::new(),
            rcs_source_map: VideoConversionProbeMetric::new(),
            rcs_destination_map: VideoConversionProbeMetric::new(),
            rcs_batch_encode: VideoConversionProbeMetric::new(),
            rcs_admission: VideoConversionProbeMetric::new(),
            rcs_submit_to_marker: VideoConversionProbeMetric::new(),
            end_to_end_histogram: VideoConversionProbeHistogram::new(),
            rcs_queue_prepare_histogram: VideoConversionProbeHistogram::new(),
            rcs_submit_to_marker_histogram: VideoConversionProbeHistogram::new(),
            gpu_timestamp_samples: 0,
            gpu_timestamp_frequency_hz: 0,
            gpu_walker: VideoConversionProbeMetric::new(),
            gpu_walker_histogram: VideoConversionProbeHistogram::new(),
            total_completion_polls: 0,
            max_completion_polls: 0,
        }
    }

    fn record(&mut self, sample: DecodedVideoConversionProbeSample) {
        self.samples = self.samples.saturating_add(1);
        self.queue_wait.record(sample.queue_wait_us);
        self.end_to_end.record(sample.end_to_end_us);
        self.end_to_end_histogram.record(sample.end_to_end_us);
        self.bind_layout.record(sample.bind_layout_us);
        self.rgba_acquire.record(sample.rgba_acquire_us);
        self.surface_prepare.record(sample.surface_prepare_us);
        self.rcs_queue.record(sample.rcs_queue_us);
        self.rcs_completion.record(sample.rcs_completion_us);
        self.publish.record(sample.publish_us);
        let Some(rcs) = sample.rcs else {
            return;
        };
        self.rcs_samples = self.rcs_samples.saturating_add(1);
        self.rcs_queue_prepare.record(rcs.queue_prepare_us);
        self.rcs_queue_prepare_histogram
            .record(rcs.queue_prepare_us);
        self.rcs_queue_total.record(rcs.queue_total_us);
        self.rcs_forcewake.record(rcs.forcewake_us);
        self.rcs_state_map.record(rcs.state_map_us);
        self.rcs_ppgtt_init.record(rcs.ppgtt_init_us);
        self.rcs_kernel_map.record(rcs.kernel_map_us);
        self.rcs_source_map.record(rcs.source_map_us);
        self.rcs_destination_map.record(rcs.destination_map_us);
        self.rcs_batch_encode.record(rcs.batch_encode_us);
        self.rcs_admission.record(rcs.admission_us);
        self.rcs_submit_to_marker.record(rcs.submit_to_marker_us);
        self.rcs_submit_to_marker_histogram
            .record(rcs.submit_to_marker_us);
        if rcs.gpu_walker_timestamp_valid {
            if self.gpu_timestamp_samples == 0 {
                self.gpu_timestamp_frequency_hz = rcs.gpu_timestamp_frequency_hz;
            } else if self.gpu_timestamp_frequency_hz != rcs.gpu_timestamp_frequency_hz {
                self.gpu_timestamp_frequency_hz = 0;
            }
            self.gpu_timestamp_samples = self.gpu_timestamp_samples.saturating_add(1);
            self.gpu_walker.record(rcs.gpu_walker_us);
            self.gpu_walker_histogram.record(rcs.gpu_walker_us);
        }
        self.total_completion_polls = self
            .total_completion_polls
            .saturating_add(rcs.completion_polls);
        self.max_completion_polls = self.max_completion_polls.max(rcs.completion_polls);
    }

    fn report(&self) -> DecodedVideoConversionProbeReport {
        let samples = self.samples;
        let rcs_samples = self.rcs_samples;
        DecodedVideoConversionProbeReport {
            samples,
            rcs_samples,
            quantile_bucket_us: VIDEO_CONVERSION_PROBE_HISTOGRAM_BUCKET_US,
            avg_queue_wait_us: self.queue_wait.average(samples),
            max_queue_wait_us: self.queue_wait.max_us,
            avg_end_to_end_us: self.end_to_end.average(samples),
            max_end_to_end_us: self.end_to_end.max_us,
            p50_end_to_end_us: self
                .end_to_end_histogram
                .percentile(50, self.end_to_end.max_us),
            p95_end_to_end_us: self
                .end_to_end_histogram
                .percentile(95, self.end_to_end.max_us),
            p99_end_to_end_us: self
                .end_to_end_histogram
                .percentile(99, self.end_to_end.max_us),
            avg_bind_layout_us: self.bind_layout.average(samples),
            max_bind_layout_us: self.bind_layout.max_us,
            avg_rgba_acquire_us: self.rgba_acquire.average(samples),
            max_rgba_acquire_us: self.rgba_acquire.max_us,
            avg_surface_prepare_us: self.surface_prepare.average(samples),
            max_surface_prepare_us: self.surface_prepare.max_us,
            avg_rcs_queue_us: self.rcs_queue.average(samples),
            max_rcs_queue_us: self.rcs_queue.max_us,
            avg_rcs_completion_us: self.rcs_completion.average(samples),
            max_rcs_completion_us: self.rcs_completion.max_us,
            avg_publish_us: self.publish.average(samples),
            max_publish_us: self.publish.max_us,
            avg_rcs_queue_prepare_us: self.rcs_queue_prepare.average(rcs_samples),
            max_rcs_queue_prepare_us: self.rcs_queue_prepare.max_us,
            p50_rcs_queue_prepare_us: self
                .rcs_queue_prepare_histogram
                .percentile(50, self.rcs_queue_prepare.max_us),
            p95_rcs_queue_prepare_us: self
                .rcs_queue_prepare_histogram
                .percentile(95, self.rcs_queue_prepare.max_us),
            p99_rcs_queue_prepare_us: self
                .rcs_queue_prepare_histogram
                .percentile(99, self.rcs_queue_prepare.max_us),
            avg_rcs_queue_total_us: self.rcs_queue_total.average(rcs_samples),
            avg_rcs_forcewake_us: self.rcs_forcewake.average(rcs_samples),
            avg_rcs_state_map_us: self.rcs_state_map.average(rcs_samples),
            avg_rcs_ppgtt_init_us: self.rcs_ppgtt_init.average(rcs_samples),
            avg_rcs_kernel_map_us: self.rcs_kernel_map.average(rcs_samples),
            avg_rcs_source_map_us: self.rcs_source_map.average(rcs_samples),
            avg_rcs_destination_map_us: self.rcs_destination_map.average(rcs_samples),
            avg_rcs_batch_encode_us: self.rcs_batch_encode.average(rcs_samples),
            avg_rcs_admission_us: self.rcs_admission.average(rcs_samples),
            avg_rcs_submit_to_marker_us: self.rcs_submit_to_marker.average(rcs_samples),
            max_rcs_submit_to_marker_us: self.rcs_submit_to_marker.max_us,
            p50_rcs_submit_to_marker_us: self
                .rcs_submit_to_marker_histogram
                .percentile(50, self.rcs_submit_to_marker.max_us),
            p95_rcs_submit_to_marker_us: self
                .rcs_submit_to_marker_histogram
                .percentile(95, self.rcs_submit_to_marker.max_us),
            p99_rcs_submit_to_marker_us: self
                .rcs_submit_to_marker_histogram
                .percentile(99, self.rcs_submit_to_marker.max_us),
            avg_completion_polls: if rcs_samples == 0 {
                0
            } else {
                self.total_completion_polls / rcs_samples as u64
            },
            max_completion_polls: self.max_completion_polls,
            gpu_timestamp_samples: self.gpu_timestamp_samples,
            gpu_timestamp_frequency_hz: self.gpu_timestamp_frequency_hz,
            avg_gpu_walker_us: self.gpu_walker.average(self.gpu_timestamp_samples),
            max_gpu_walker_us: self.gpu_walker.max_us,
            p50_gpu_walker_us: self
                .gpu_walker_histogram
                .percentile(50, self.gpu_walker.max_us),
            p95_gpu_walker_us: self
                .gpu_walker_histogram
                .percentile(95, self.gpu_walker.max_us),
            p99_gpu_walker_us: self
                .gpu_walker_histogram
                .percentile(99, self.gpu_walker.max_us),
        }
    }
}

struct DecodedVideoConversionState {
    generation: u64,
    online: bool,
    active: bool,
    queue: VecDeque<DecodedVideoConversionRequest>,
    queued: usize,
    completed: usize,
    published: usize,
    first_failure_frame: usize,
    first_failure_error: i32,
    backpressure_events: usize,
    rgba_buffer_wait_events: usize,
    rcs_submit_wait_events: usize,
    max_outstanding: usize,
    total_conversion_ticks: u64,
    max_conversion_ticks: u64,
    probe: DecodedVideoConversionProbeState,
}

impl DecodedVideoConversionState {
    const fn new() -> Self {
        Self {
            generation: 0,
            online: false,
            active: false,
            queue: VecDeque::new(),
            queued: 0,
            completed: 0,
            published: 0,
            first_failure_frame: 0,
            first_failure_error: 0,
            backpressure_events: 0,
            rgba_buffer_wait_events: 0,
            rcs_submit_wait_events: 0,
            max_outstanding: 0,
            total_conversion_ticks: 0,
            max_conversion_ticks: 0,
            probe: DecodedVideoConversionProbeState::new(),
        }
    }

    fn outstanding(&self) -> usize {
        self.queued.saturating_sub(self.completed)
    }

    fn reset_batch(&mut self) -> bool {
        if self.active || !self.queue.is_empty() || self.outstanding() != 0 {
            return false;
        }
        self.generation = self.generation.wrapping_add(1).max(1);
        self.queued = 0;
        self.completed = 0;
        self.published = 0;
        self.first_failure_frame = 0;
        self.first_failure_error = 0;
        self.backpressure_events = 0;
        self.rgba_buffer_wait_events = 0;
        self.rcs_submit_wait_events = 0;
        self.max_outstanding = 0;
        self.total_conversion_ticks = 0;
        self.max_conversion_ticks = 0;
        self.probe = DecodedVideoConversionProbeState::new();
        true
    }

    fn report(&self) -> DecodedVideoConversionReport {
        DecodedVideoConversionReport {
            generation: self.generation,
            queued: self.queued,
            completed: self.completed,
            published: self.published,
            first_failure_frame: self.first_failure_frame,
            first_failure_error: self.first_failure_error,
            backpressure_events: self.backpressure_events,
            rgba_buffer_wait_events: self.rgba_buffer_wait_events,
            rcs_submit_wait_events: self.rcs_submit_wait_events,
            max_outstanding: self.max_outstanding,
            total_conversion_us: video_conversion_ticks_to_micros(self.total_conversion_ticks),
            max_conversion_us: video_conversion_ticks_to_micros(self.max_conversion_ticks),
            probe: self.probe.report(),
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct NativeViewportLayout {
    pub(crate) source_x: u32,
    pub(crate) source_y: u32,
    pub(crate) destination_x: u32,
    pub(crate) destination_y: u32,
    pub(crate) width: u32,
    pub(crate) height: u32,
}

/// Dimensions used to bind one decoded picture to its UI4 viewport. Pixel
/// storage stays decoder-owned until GuC completion; only the converted RGBA
/// allocation enters the UI4 publication lifetime.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct DecodedVideoFrameSpec {
    pub(crate) coded_width: u32,
    pub(crate) coded_height: u32,
    pub(crate) visible_width: u32,
    pub(crate) visible_height: u32,
}

impl DecodedVideoFrameSpec {
    const fn from_nv12_source(source: DecodedNv12Source) -> Self {
        Self {
            coded_width: source.width,
            coded_height: source.height,
            visible_width: source.visible_width,
            visible_height: source.visible_height,
        }
    }

    const fn valid(self) -> bool {
        self.coded_width != 0
            && self.coded_height != 0
            && self.visible_width != 0
            && self.visible_height != 0
            && self.visible_width <= self.coded_width
            && self.visible_height <= self.coded_height
    }
}

#[derive(Copy, Clone)]
struct VideoStream {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    source_width: u32,
    source_height: u32,
    visible_width: u32,
    visible_height: u32,
    frame_width: u32,
    frame_height: u32,
    pan_x: u32,
    pan_y: u32,
    active_pan_source: Option<super::Ui4CursorSource>,
}

static VIDEO_STREAM: Mutex<Option<VideoStream>> = Mutex::new(None);
static VIDEO_RETIRED_FRAMES: Mutex<Vec<FrameHandle>> = Mutex::new(Vec::new());
static VIDEO_PUBLISH_SEQ: AtomicU64 = AtomicU64::new(0);
static VIDEO_LIFECYCLE_RESERVED: AtomicBool = AtomicBool::new(false);
static VIDEO_CONVERSION_STATE: Mutex<DecodedVideoConversionState> =
    Mutex::new(DecodedVideoConversionState::new());
static VIDEO_CONVERSION_WORK: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();
static VIDEO_CONVERSION_LAST_ERROR_LOG_TICK: AtomicU64 = AtomicU64::new(0);

fn decoded_video_conversion_idle() -> bool {
    let state = VIDEO_CONVERSION_STATE.lock();
    !state.active && state.queue.is_empty() && state.outstanding() == 0
}

fn video_conversion_ticks_to_micros(ticks: u64) -> u64 {
    ((ticks as u128).saturating_mul(1_000_000) / embassy_time::TICK_HZ.max(1) as u128) as u64
}

fn should_log_video_conversion_error() -> bool {
    let now = Instant::now().as_ticks().max(1);
    loop {
        let previous = VIDEO_CONVERSION_LAST_ERROR_LOG_TICK.load(Ordering::Acquire);
        if previous != 0 && now.saturating_sub(previous) < VIDEO_CONVERSION_ERROR_LOG_INTERVAL_TICKS
        {
            return false;
        }
        if VIDEO_CONVERSION_LAST_ERROR_LOG_TICK
            .compare_exchange_weak(previous, now, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return true;
        }
    }
}

fn log_video_conversion_backpressure(
    phase: &str,
    playback_frame: usize,
    report: DecodedVideoConversionReport,
    worker_online: bool,
) {
    if !should_log_video_conversion_error() {
        return;
    }
    crate::log_error!(
        target: "ui4";
        "ui4 video-conversion backpressure generation={} phase={} playback_frame={} queued={} completed={} outstanding={} cap={} handoff_wait_events={} rgba_buffer_wait_events={} rcs_submit_wait_events={} worker_online={} policy=wait-ordered-no-drop rate_limit_ms=10000\n",
        report.generation,
        phase,
        playback_frame,
        report.queued,
        report.completed,
        report.queued.saturating_sub(report.completed),
        VIDEO_CONVERSION_OUTSTANDING_CAP,
        report.backpressure_events,
        report.rgba_buffer_wait_events,
        report.rcs_submit_wait_events,
        worker_online as u8,
    );
}

/// Start a fresh accounting batch without changing the broker Frame/window.
/// Loop playback calls this only after the prior batch drained completely.
pub(crate) fn begin_decoded_nv12_conversion_batch() -> bool {
    let mut state = VIDEO_CONVERSION_STATE.lock();
    let reset = state.reset_batch();
    if reset {
        crate::log_info!(
            target: "ui4";
            "ui4 video-conversion batch generation={} worker=independent-rcs ordered=1 outstanding_cap={} no_drop=1 idr_drain=1 worker_online={}\n",
            state.generation,
            VIDEO_CONVERSION_OUTSTANDING_CAP,
            state.online as u8,
        );
    }
    reset
}

/// Hand one decoder-retired NV12 surface to the independent RCS conversion
/// worker. Capacity is deliberately bounded; saturation waits and reports an
/// error at most once per ten seconds, never discarding or replacing a frame.
pub(crate) async fn enqueue_decoded_nv12_stream_frame(
    source: DecodedNv12Source,
    playback_frame: usize,
) -> bool {
    if !valid_source(source) || !VIDEO_LIFECYCLE_RESERVED.load(Ordering::Acquire) {
        return false;
    }
    let mut backpressure_counted = false;
    loop {
        let (enqueued, report, worker_online) = {
            let mut state = VIDEO_CONVERSION_STATE.lock();
            let outstanding = state.outstanding();
            if outstanding < VIDEO_CONVERSION_OUTSTANDING_CAP {
                let generation = state.generation;
                state.queue.push_back(DecodedVideoConversionRequest {
                    generation,
                    playback_frame,
                    enqueued_tick: Instant::now().as_ticks(),
                    source,
                });
                state.queued = state.queued.saturating_add(1);
                state.max_outstanding = state.max_outstanding.max(state.outstanding());
                (true, state.report(), state.online)
            } else {
                if !backpressure_counted {
                    state.backpressure_events = state.backpressure_events.saturating_add(1);
                    backpressure_counted = true;
                }
                (false, state.report(), state.online)
            }
        };
        if enqueued {
            VIDEO_CONVERSION_WORK.signal(());
            return true;
        }
        log_video_conversion_backpressure("enqueue", playback_frame, report, worker_online);
        Timer::after(Duration::from_millis(1)).await;
        if !VIDEO_LIFECYCLE_RESERVED.load(Ordering::Acquire) {
            return false;
        }
    }
}

/// Wait for all conversion requests in the current batch. This is used at IDR
/// reuse boundaries and once at EOS; it does not reset cumulative accounting.
pub(crate) async fn wait_decoded_nv12_conversion_idle() -> DecodedVideoConversionReport {
    loop {
        let (idle, report, worker_online) = {
            let state = VIDEO_CONVERSION_STATE.lock();
            (
                !state.active && state.queue.is_empty() && state.outstanding() == 0,
                state.report(),
                state.online,
            )
        };
        if idle {
            return report;
        }
        if !worker_online {
            log_video_conversion_backpressure("drain-worker-offline", 0, report, worker_online);
        }
        Timer::after(Duration::from_millis(1)).await;
    }
}

fn take_decoded_video_conversion_request() -> Option<DecodedVideoConversionRequest> {
    let mut state = VIDEO_CONVERSION_STATE.lock();
    let request = state.queue.pop_front()?;
    state.active = true;
    Some(request)
}

fn complete_decoded_video_conversion(
    request: DecodedVideoConversionRequest,
    outcome: DecodedVideoConversionOutcome,
    elapsed_ticks: u64,
) {
    let mut state = VIDEO_CONVERSION_STATE.lock();
    if state.generation == request.generation {
        state.completed = state.completed.saturating_add(1);
        if outcome.published {
            state.published = state.published.saturating_add(1);
        } else if state.first_failure_frame == 0 {
            state.first_failure_frame = request.playback_frame;
            state.first_failure_error = VIDEO_CONVERSION_PRESENT_ERROR;
        }
        state.total_conversion_ticks = state.total_conversion_ticks.saturating_add(elapsed_ticks);
        state.max_conversion_ticks = state.max_conversion_ticks.max(elapsed_ticks);
        state.probe.record(outcome.probe);
    }
    state.active = false;
}

#[embassy_executor::task(pool_size = 1)]
pub(crate) async fn ui4_video_conversion_service_task(worker_slot: u32) {
    {
        let mut state = VIDEO_CONVERSION_STATE.lock();
        state.online = true;
    }
    crate::log_info!(
        target: "ui4";
        "ui4 video-conversion worker online producer=decoded-nv12 consumer=broker-rgba engine=guc-rcs assigned_slot={} current_slot={} ordered=1 outstanding_cap={} no_drop=1 backpressure=wait error_rate_limit_ms=10000\n",
        worker_slot,
        crate::percpu::current_slot(),
        VIDEO_CONVERSION_OUTSTANDING_CAP,
    );
    loop {
        let Some(request) = take_decoded_video_conversion_request() else {
            VIDEO_CONVERSION_WORK.wait().await;
            continue;
        };
        let started = Instant::now();
        let mut outcome =
            convert_publish_decoded_nv12_stream_frame(request.source, request.playback_frame).await;
        let finished = Instant::now();
        let elapsed_ticks = finished.saturating_duration_since(started).as_ticks();
        outcome.probe.queue_wait_us = video_conversion_ticks_to_micros(
            started.as_ticks().saturating_sub(request.enqueued_tick),
        );
        outcome.probe.end_to_end_us = video_conversion_ticks_to_micros(
            finished.as_ticks().saturating_sub(request.enqueued_tick),
        );
        complete_decoded_video_conversion(request, outcome, elapsed_ticks);
    }
}

/// Reserve the decoded-video lifetime and ask UI4 for its streaming RGBA
/// Frame/window before filesystem or decoder work begins. No placeholder is
/// published: the first visible buffer remains a fully converted and
/// GuC-released decoded picture.
pub(crate) fn begin_shell_decoded_video_player(desired_width: u32, desired_height: u32) -> bool {
    if !super::video_frame_extent_admitted(desired_width, desired_height) {
        crate::log_warn!(
            target: "ui4";
            "ui4 video-player frame request rejected requested={}x{} pixels={} softcap_pixels={} reason=decoded-video-pixel-softcap\n",
            desired_width,
            desired_height,
            u64::from(desired_width) * u64::from(desired_height),
            super::VIDEO_FRAME_MAX_PIXELS,
        );
        return false;
    }
    if !decoded_video_conversion_idle() {
        crate::log_warn!(
            target: "ui4";
            "ui4 video-player frame request rejected requested={}x{} reason=prior-conversion-batch-not-drained\n",
            desired_width,
            desired_height,
        );
        return false;
    }
    if VIDEO_LIFECYCLE_RESERVED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let requested_spec = DecodedVideoFrameSpec {
        coded_width: desired_width,
        coded_height: desired_height,
        visible_width: desired_width,
        visible_height: desired_height,
    };
    let Some(stream) = create_stream(requested_spec, desired_width, desired_height) else {
        VIDEO_LIFECYCLE_RESERVED.store(false, Ordering::Release);
        crate::log_warn!(
            target: "ui4";
            "ui4 video-player frame request rejected requested={}x{} pixels={} softcap_pixels={} reason=broker-frame-window-create-failed\n",
            desired_width,
            desired_height,
            u64::from(desired_width) * u64::from(desired_height),
            super::VIDEO_FRAME_MAX_PIXELS,
        );
        return false;
    };
    let mut slot = VIDEO_STREAM.lock();
    if slot.is_some() {
        drop(slot);
        cleanup_uninstalled_stream(stream);
        VIDEO_LIFECYCLE_RESERVED.store(false, Ordering::Release);
        return false;
    }
    *slot = Some(stream);
    drop(slot);
    crate::log_info!(
        target: "ui4";
        "ui4 video-player initialized owner={:?} playback=playing control=broker-pan source=await-first-decoded-frame lifecycle_owner=shell2-vid-task frame={} window={} requested={}x{} admitted={}x{} pixels={} softcap_pixels={} rgba_buffers={} rgba_ownership=producer-write+broker-pending+display-live broker_state=frame-window-ready placeholder_present=0\n",
        VIDEO_OWNER,
        stream.frame.raw(),
        stream.window.raw(),
        desired_width,
        desired_height,
        stream.frame_width,
        stream.frame_height,
        u64::from(stream.frame_width) * u64::from(stream.frame_height),
        super::VIDEO_FRAME_MAX_PIXELS,
        VIDEO_RGBA_BUFFER_COUNT,
    );
    true
}

fn poll_decoded_video_player_input() {
    reap_retired_video_frames();
    for event in take_owner_input_events(VIDEO_OWNER) {
        match event {
            Ui4InputEvent::Pan(event) => pan_video_viewport(event),
            Ui4InputEvent::Resize(event) => crate::log_warn!(
                target: "ui4";
                "ui4 video-player resize ignored window={} extent={}x{} reason=fixed-shell-vid-frame no-placeholder-publish=1\n",
                event.window.raw(), event.width, event.height,
            ),
            _ => {}
        }
    }
}

fn pan_video_viewport(event: super::Ui4PanEvent) {
    let mut slot = VIDEO_STREAM.lock();
    let Some(stream) = slot.as_mut() else {
        return;
    };
    if event.window != stream.window {
        return;
    }
    match event.phase {
        super::Ui4PanPhase::Begin => stream.active_pan_source = Some(event.source),
        super::Ui4PanPhase::Update if stream.active_pan_source == Some(event.source) => {
            stream.pan_x = move_crop_origin(
                stream.pan_x,
                event.dx,
                stream.visible_width.saturating_sub(stream.frame_width),
            );
            stream.pan_y = move_crop_origin(
                stream.pan_y,
                event.dy,
                stream.visible_height.saturating_sub(stream.frame_height),
            );
        }
        super::Ui4PanPhase::End if stream.active_pan_source == Some(event.source) => {
            stream.active_pan_source = None;
            crate::log_info!(
                target: "ui4";
                "ui4 video-player pan ended window={} native={}x{} viewport={}x{} crop_origin={},{} scaling=none-1to1\n",
                stream.window.raw(),
                stream.visible_width,
                stream.visible_height,
                stream.frame_width,
                stream.frame_height,
                stream.pan_x,
                stream.pan_y,
            );
        }
        _ => {}
    }
}

fn move_crop_origin(origin: u32, drag_delta: i32, maximum: u32) -> u32 {
    (i64::from(origin) - i64::from(drag_delta)).clamp(0, i64::from(maximum)) as u32
}

fn cleanup_uninstalled_stream(stream: VideoStream) {
    let _ = finish_window_session(VIDEO_OWNER, stream.session);
    retire_video_frame(stream.frame);
}

fn retire_video_frame(frame: FrameHandle) {
    match destroy_frame(frame) {
        Ok(()) | Err(super::FramePoolError::InvalidHandle) => {}
        Err(super::FramePoolError::Busy) => {
            let mut retired = VIDEO_RETIRED_FRAMES.lock();
            if !retired.contains(&frame) {
                retired.push(frame);
            }
        }
        Err(error) => crate::log_warn!(
            target: "ui4";
            "ui4 video-frame retire abandoned frame={} error={:?}\n",
            frame.raw(),
            error,
        ),
    }
}

fn reap_retired_video_frames() {
    VIDEO_RETIRED_FRAMES
        .lock()
        .retain(|frame| matches!(destroy_frame(*frame), Err(super::FramePoolError::Busy)));
}

/// Worker-side conversion of one decoder picture into an exact UI4 RGBA
/// backbuffer. The caller is the dedicated RCS service, never the VDBOX
/// playback loop. It returns after the GuC read of NV12 has retired and the
/// RGBA publication is visible to the broker; display SURFLIVE remains
/// independently owned by UI4.
async fn convert_publish_decoded_nv12_stream_frame(
    source: DecodedNv12Source,
    playback_frame: usize,
) -> DecodedVideoConversionOutcome {
    let reason = "independent-rcs-worker";
    let mut probe = DecodedVideoConversionProbeSample::default();
    let bind_layout_started = Instant::now();
    if !valid_source(source) {
        probe.bind_layout_us =
            video_conversion_ticks_to_micros(bind_layout_started.elapsed().as_ticks());
        return probe.finish(false);
    }
    // Shell-driven playback owns the same application window as boot playback;
    // drain its broker queue at frame cadence so move/resize/pan never depends
    // on the boot-only pause gate.
    poll_decoded_video_player_input();
    let Some(stream) =
        bind_decoded_source_stream(DecodedVideoFrameSpec::from_nv12_source(source), reason)
    else {
        probe.bind_layout_us =
            video_conversion_ticks_to_micros(bind_layout_started.elapsed().as_ticks());
        return probe.finish(false);
    };
    let Some(layout) = native_viewport_layout(
        stream.visible_width,
        stream.visible_height,
        stream.frame_width,
        stream.frame_height,
        stream.pan_x,
        stream.pan_y,
    ) else {
        probe.bind_layout_us =
            video_conversion_ticks_to_micros(bind_layout_started.elapsed().as_ticks());
        return probe.finish(false);
    };
    probe.bind_layout_us =
        video_conversion_ticks_to_micros(bind_layout_started.elapsed().as_ticks());

    let rgba_acquire_started = Instant::now();
    let mut rgba_buffer_wait_counted = false;
    let write = loop {
        match acquire_frame_buffer(stream.frame) {
            Ok(write) => break write,
            Err(super::FramePoolError::Busy) => {
                // Buffer reuse is driven directly by the compositor releasing
                // a non-front RGBA read lease after SURFLIVE. No decoder pixels
                // are copied, replaced, or published while ownership is busy.
                let (report, worker_online) = {
                    let mut state = VIDEO_CONVERSION_STATE.lock();
                    if !rgba_buffer_wait_counted {
                        state.rgba_buffer_wait_events =
                            state.rgba_buffer_wait_events.saturating_add(1);
                        rgba_buffer_wait_counted = true;
                    }
                    (state.report(), state.online)
                };
                log_video_conversion_backpressure(
                    "frame-buffer-busy",
                    playback_frame,
                    report,
                    worker_online,
                );
                wait_frame_buffer_release(stream.frame).await;
            }
            Err(error) => {
                crate::log_warn!(target: "ui4";
                    "ui4 video-frame acquire failed reason={} error={:?}\n", reason, error,
                );
                probe.rgba_acquire_us =
                    video_conversion_ticks_to_micros(rgba_acquire_started.elapsed().as_ticks());
                return probe.finish(false);
            }
        }
    };
    probe.rgba_acquire_us =
        video_conversion_ticks_to_micros(rgba_acquire_started.elapsed().as_ticks());

    let surface_prepare_started = Instant::now();
    let destination = match gpgpu_rgba_surface(write) {
        Ok(surface) => surface,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame destination unavailable frame={} buffer={} error={:?} reason={}\n",
                stream.frame.raw(), write.buffer_index, error, reason,
            );
            probe.surface_prepare_us =
                video_conversion_ticks_to_micros(surface_prepare_started.elapsed().as_ticks());
            return probe.finish(false);
        }
    };
    let pitch_bytes = match u32::try_from(source.pitch_bytes) {
        Ok(pitch) => pitch,
        Err(_) => {
            let _ = cancel_frame_buffer(write);
            probe.surface_prepare_us =
                video_conversion_ticks_to_micros(surface_prepare_started.elapsed().as_ticks());
            return probe.finish(false);
        }
    };
    let uv_offset = match u32::try_from(source.uv_offset) {
        Ok(offset) => offset,
        Err(_) => {
            let _ = cancel_frame_buffer(write);
            probe.surface_prepare_us =
                video_conversion_ticks_to_micros(surface_prepare_started.elapsed().as_ticks());
            return probe.finish(false);
        }
    };
    let Some(native_source) = crate::intel::gpgpu::GpgpuNv12Tile64Surface::new(
        source.phys,
        source.gpu,
        source.byte_len,
        source.width,
        source.height,
        pitch_bytes,
        uv_offset,
    ) else {
        let _ = cancel_frame_buffer(write);
        crate::log_warn!(target: "ui4";
            "ui4 video-frame native source rejected frame={} decode_seq={} media_gpu=0x{:X} reason={}\n",
            stream.frame.raw(), source.decode_sequence, source.gpu, reason,
        );
        probe.surface_prepare_us =
            video_conversion_ticks_to_micros(surface_prepare_started.elapsed().as_ticks());
        return probe.finish(false);
    };
    probe.surface_prepare_us =
        video_conversion_ticks_to_micros(surface_prepare_started.elapsed().as_ticks());

    let rcs_queue_started = Instant::now();
    let mut rcs_submit_wait_counted = false;
    let submission = loop {
        match crate::intel::gpgpu::queue_ui4_video_frame_nv12_tile64_to_rgba8(
            native_source,
            destination,
            layout.destination_x,
            layout.destination_y,
            layout.width,
            layout.height,
            layout.source_x,
            layout.source_y,
        ) {
            Ok(submission) => break submission,
            Err(crate::intel::gpgpu::Ui4CompositorSubmitError::Busy) => {
                // The dedicated Frame lease and decoder picture remain pinned
                // until this GuC runtime accepts their exact handoff.
                let (report, worker_online) = {
                    let mut state = VIDEO_CONVERSION_STATE.lock();
                    if !rcs_submit_wait_counted {
                        state.rcs_submit_wait_events =
                            state.rcs_submit_wait_events.saturating_add(1);
                        rcs_submit_wait_counted = true;
                    }
                    (state.report(), state.online)
                };
                log_video_conversion_backpressure(
                    "guc-rcs-submit-busy",
                    playback_frame,
                    report,
                    worker_online,
                );
                Timer::after(Duration::from_millis(1)).await;
            }
            Err(error) => {
                let _ = cancel_frame_buffer(write);
                crate::log_warn!(target: "ui4";
                    "ui4 video-frame GuC queue failed frame={} buffer={} decode_seq={} error={:?} reason={}\n",
                    stream.frame.raw(), write.buffer_index, source.decode_sequence, error, reason,
                );
                probe.rcs_queue_us =
                    video_conversion_ticks_to_micros(rcs_queue_started.elapsed().as_ticks());
                return probe.finish(false);
            }
        }
    };
    probe.rcs_queue_us = video_conversion_ticks_to_micros(rcs_queue_started.elapsed().as_ticks());

    let rcs_completion_started = Instant::now();
    let mut completion_failure_logged = false;
    let (release, stats) = loop {
        match crate::intel::gpgpu::poll_ui4_video_frame_submission(submission, destination) {
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Pending => {
                Timer::after(Duration::from_millis(1)).await;
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Complete { stats, release } => {
                break (release, stats);
            }
            crate::intel::gpgpu::Ui4VideoFrameCompletion::Failed => {
                // An accepted request has no cancellation proof. Keep the NV12
                // picture and write lease quarantined instead of permitting
                // either allocation to be reused under unfinished GPU work.
                if !completion_failure_logged {
                    completion_failure_logged = true;
                    crate::log_error!(target: "ui4";
                        "ui4 video-frame GuC completion failed frame={} buffer={} decode_seq={} reason={} action=retain-native-and-write-lease-no-publish log=once\n",
                        stream.frame.raw(), write.buffer_index, source.decode_sequence, reason,
                    );
                }
                Timer::after(Duration::from_millis(1)).await;
            }
        }
    };
    probe.rcs_completion_us =
        video_conversion_ticks_to_micros(rcs_completion_started.elapsed().as_ticks());
    probe.rcs = Some(stats.probe);
    let submit_ms = stats.submit_ms;

    // From this point onward the decoder source is no longer referenced by any
    // queued GPU command. Only the completed RGBA allocation crosses into UI4.
    let publish_started = Instant::now();
    let sequence = VIDEO_PUBLISH_SEQ.fetch_add(1, Ordering::AcqRel) + 1;
    let published = match publish_gpgpu_video_frame_buffer(write, release) {
        Ok(published) => published,
        Err(error) => {
            let _ = cancel_frame_buffer(write);
            crate::log_warn!(target: "ui4";
                "ui4 video-frame GPU publish failed frame={} buffer={} error={:?} reason={}\n",
                stream.frame.raw(), write.buffer_index, error, reason,
            );
            probe.publish_us =
                video_conversion_ticks_to_micros(publish_started.elapsed().as_ticks());
            return probe.finish(false);
        }
    };
    let window_serial = match publish_window_frame(VIDEO_OWNER, stream.window, DamageRect::FULL) {
        Ok(serial) => serial,
        Err(error) => {
            crate::log_warn!(target: "ui4";
                "ui4 video-frame window publish failed frame={} window={} error={:?} reason={} action=close-stream source_already_released_at=guc-completion\n",
                stream.frame.raw(), stream.window.raw(), error, reason,
            );
            let _ = stop_decoded_nv12_stream("window-publish-failed");
            probe.publish_us =
                video_conversion_ticks_to_micros(publish_started.elapsed().as_ticks());
            return probe.finish(false);
        }
    };
    probe.publish_us = video_conversion_ticks_to_micros(publish_started.elapsed().as_ticks());
    if sequence <= 8 || sequence.is_multiple_of(120) {
        crate::log_info!(target: "ui4";
            "ui4 video-frame published seq={} decode_seq={} frame={} window={} buffer={} frame_serial={} window_serial={} producer=guc-nv12-to-ui4-rgba8-frame producer_release={} submit_ms={} source=media-ytile-nv12 {}x{} visible={}x{} crop={}x{}@{},{} destination={},{} media_gpu=0x{:X} target_gpu=0x{:X} rgba_buffers={} rgba_ownership=producer-write+broker-pending+display-live plane_route=slot1-rgba8 decoder_source_release=guc-completion display_release=surflive native_attachment=0 linked_nv12_slots=0 producer_plane_mmio=0 cpu_pixel_copy=0\n",
            sequence,
            source.decode_sequence,
            stream.frame.raw(),
            stream.window.raw(),
            published.buffer_index,
            published.publish_serial,
            window_serial,
            release.sequence(),
            submit_ms,
            source.width,
            source.height,
            source.visible_width,
            source.visible_height,
            layout.width,
            layout.height,
            layout.source_x,
            layout.source_y,
            layout.destination_x,
            layout.destination_y,
            source.gpu,
            destination.gpu,
            VIDEO_RGBA_BUFFER_COUNT,
        );
    }
    probe.finish(true)
}

fn bind_decoded_source_stream(spec: DecodedVideoFrameSpec, reason: &str) -> Option<VideoStream> {
    if !spec.valid() || !VIDEO_LIFECYCLE_RESERVED.load(Ordering::Acquire) {
        return None;
    }
    {
        let mut slot = VIDEO_STREAM.lock();
        if let Some(stream) = slot.as_mut() {
            let source_changed = stream.source_width != spec.coded_width
                || stream.source_height != spec.coded_height
                || stream.visible_width != spec.visible_width
                || stream.visible_height != spec.visible_height;
            stream.source_width = spec.coded_width;
            stream.source_height = spec.coded_height;
            stream.visible_width = spec.visible_width;
            stream.visible_height = spec.visible_height;
            if source_changed {
                stream.pan_x = centered_crop_origin(spec.visible_width, stream.frame_width);
                stream.pan_y = centered_crop_origin(spec.visible_height, stream.frame_height);
                let layout = native_viewport_layout(
                    stream.visible_width,
                    stream.visible_height,
                    stream.frame_width,
                    stream.frame_height,
                    stream.pan_x,
                    stream.pan_y,
                )?;
                crate::log_info!(target: "ui4";
                    "ui4 video-player source-bound frame={} window={} source={}x{} visible={}x{} viewport={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 playback=playing producer=guc-nv12-to-ui4-rgba8-frame attachment=none rgba_buffers={} rgba_ownership=producer-write+broker-pending+display-live plane_slot={} reason={}\n",
                    stream.frame.raw(),
                    stream.window.raw(),
                    spec.coded_width,
                    spec.coded_height,
                    spec.visible_width,
                    spec.visible_height,
                    stream.frame_width,
                    stream.frame_height,
                    layout.width,
                    layout.height,
                    layout.source_x,
                    layout.source_y,
                    layout.destination_x,
                    layout.destination_y,
                    VIDEO_RGBA_BUFFER_COUNT,
                    VIDEO_PLANE_SLOT,
                    reason,
                );
            }
            return Some(*stream);
        }
    }
    let stream = create_stream(spec, spec.visible_width, spec.visible_height)?;
    let mut slot = VIDEO_STREAM.lock();
    if let Some(existing) = *slot {
        drop(slot);
        cleanup_uninstalled_stream(stream);
        return Some(existing);
    }
    *slot = Some(stream);
    Some(stream)
}

pub(crate) fn stop_decoded_nv12_stream(reason: &str) -> bool {
    let reserved = VIDEO_LIFECYCLE_RESERVED.swap(false, Ordering::AcqRel);
    let stream = VIDEO_STREAM.lock().take();
    if let Some(stream) = stream {
        let animated = finish_window_session_with_request(
            VIDEO_OWNER,
            stream.session,
            WindowSessionCloseRequest::default().direct_plane_animate_and_retire_frames(),
        )
        .is_ok();
        if !animated {
            let _ = finish_window_session(VIDEO_OWNER, stream.session);
            retire_video_frame(stream.frame);
        }
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame={} window={} teardown={} display_release=surflive plane_mutation=none\n",
            reason,
            stream.frame.raw(),
            stream.window.raw(),
            if animated { "direct-plane-shrink+fade" } else { "immediate-fallback" },
        );
        true
    } else if reserved {
        crate::log_info!(
            target: "ui4";
            "ui4 video-frame stopped reason={} frame=none window=none teardown=none-before-first-decoded-frame display_release=none plane_mutation=none\n",
            reason,
        );
        true
    } else {
        false
    }
}

fn create_stream(
    spec: DecodedVideoFrameSpec,
    frame_width: u32,
    frame_height: u32,
) -> Option<VideoStream> {
    if !spec.valid() || !super::video_frame_extent_admitted(frame_width, frame_height) {
        return None;
    }
    let frame = create_video_frame(frame_width, frame_height).ok()?;
    let session = match begin_window_session(VIDEO_OWNER) {
        Ok(session) => session,
        Err(_) => {
            let _ = destroy_frame(frame);
            return None;
        }
    };
    let (scanout_width, scanout_height) =
        crate::intel::active_scanout_dimensions().unwrap_or((frame_width, frame_height));
    let placement = WindowPlacement {
        x: (scanout_width.saturating_sub(frame_width) / 2) as i32,
        y: (scanout_height.saturating_sub(frame_height) / 2) as i32,
        width: frame_width,
        height: frame_height,
        z: 100,
        opacity: 0xFF,
        visible: true,
    };
    let window = match create_window(WindowCreate {
        owner: VIDEO_OWNER,
        session,
        frame,
        output: VIDEO_OUTPUT,
        plane: super::WindowPlane::Universal(VIDEO_PLANE_SLOT as u8),
        placement,
        interaction: super::WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = finish_window_session(VIDEO_OWNER, session);
            let _ = destroy_frame(frame);
            return None;
        }
    };
    // No placeholder is published. The first visible buffer is always a fully
    // converted, GuC-released RGBA picture; the producer never writes display
    // MMIO and decoder memory is never attached to the broker Frame.
    let visible_width = spec.visible_width;
    let visible_height = spec.visible_height;
    let pan_x = centered_crop_origin(visible_width, frame_width);
    let pan_y = centered_crop_origin(visible_height, frame_height);
    let layout = native_viewport_layout(
        spec.visible_width,
        spec.visible_height,
        frame_width,
        frame_height,
        centered_crop_origin(spec.visible_width, frame_width),
        centered_crop_origin(spec.visible_height, frame_height),
    )?;
    crate::log_info!(
        target: "ui4";
        "ui4 video-frame created owner={:?} frame={} window={} rgba_buffers={} rgba_ownership=producer-write+broker-pending+display-live cadence=streaming frame_format=rgba8-premultiplied native_format=media-ytile-nv12 attachment=none source={} source_size={}x{} frame_size={}x{} source_crop={}x{}@{},{} destination={},{} scaling=none-1to1 placement={},{} z={} plane_slot={} direct_import=after-compute-release plane_mutation=none\n",
        VIDEO_OWNER,
        frame.raw(),
        window.raw(),
        VIDEO_RGBA_BUFFER_COUNT,
        "media-ytile-nv12",
        spec.coded_width,
        spec.coded_height,
        frame_width,
        frame_height,
        layout.width,
        layout.height,
        layout.source_x,
        layout.source_y,
        layout.destination_x,
        layout.destination_y,
        placement.x,
        placement.y,
        placement.z,
        VIDEO_PLANE_SLOT,
    );
    Some(VideoStream {
        session,
        frame,
        window,
        source_width: spec.coded_width,
        source_height: spec.coded_height,
        visible_width,
        visible_height,
        frame_width,
        frame_height,
        pan_x,
        pan_y,
        active_pan_source: None,
    })
}

fn create_video_frame(width: u32, height: u32) -> Result<FrameHandle, super::FramePoolError> {
    create_frame(FrameSpec {
        output: VIDEO_OUTPUT,
        content: FrameContent::Video,
        cadence: FrameCadence::Streaming,
        buffering: VIDEO_RGBA_BUFFERING,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        // The SIMD16 producer overwrites every pixel, including opaque-black
        // letterbox regions, before this allocation can be published.
        base_color: None,
    })
}

const fn centered_crop_origin(source_extent: u32, viewport_extent: u32) -> u32 {
    source_extent.saturating_sub(viewport_extent) / 2
}

/// Map native pixels into an equally-sized destination rectangle. A smaller
/// viewport selects a movable source crop; a larger viewport centers the whole
/// native picture and leaves the surrounding initialized pixels as letterbox.
fn native_viewport_layout(
    source_width: u32,
    source_height: u32,
    destination_width: u32,
    destination_height: u32,
    pan_x: u32,
    pan_y: u32,
) -> Option<NativeViewportLayout> {
    if source_width == 0 || source_height == 0 || destination_width == 0 || destination_height == 0
    {
        return None;
    }
    Some(NativeViewportLayout {
        source_x: pan_x.min(source_width.saturating_sub(destination_width)),
        source_y: pan_y.min(source_height.saturating_sub(destination_height)),
        destination_x: destination_width.saturating_sub(source_width) / 2,
        destination_y: destination_height.saturating_sub(source_height) / 2,
        width: source_width.min(destination_width),
        height: source_height.min(destination_height),
    })
}

fn valid_source(source: DecodedNv12Source) -> bool {
    source.decode_sequence != 0
        && source.gpu != 0
        && source.phys != 0
        && source.virt != 0
        && source.byte_len != 0
        && source.width != 0
        && source.height != 0
        && source.visible_width != 0
        && source.visible_height != 0
        && source.visible_width <= source.width
        && source.visible_height <= source.height
        && source.pitch_bytes >= source.width as usize
        && source.pitch_bytes.is_multiple_of(128)
        && source.uv_offset < source.byte_len
        && source.uv_offset.is_multiple_of(source.pitch_bytes)
}
