//! Shared producer state and playback clocks for network and analog outputs.
mod sink;

use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use sink::Sink;
pub(crate) use sink::{
    Clip, MAX_PRODUCERS, OutputBucket, ProducerOwner, RenderReport, RouteSource, VideoAudioOwner,
};
pub(super) use sink::{validate_append_size, validate_size};

pub(super) static SINK: Mutex<Sink> = Mutex::new(Sink::new());

const CLOCK_PERIOD: Duration = Duration::from_millis(10);
const CLOCK_BLOCK_BYTES: usize = 48_000 * 4 / 100;

/// Handle for a continuous 48 kHz stereo signed little-endian movie soundtrack.
/// Queued PCM is owned by the video session and expires with that session.
#[derive(Clone, Copy)]
pub(crate) struct VideoPcm {
    owner: VideoAudioOwner,
    id: u64,
}

impl VideoPcm {
    /// Append complete sample frames. Returns the number of bytes accepted
    /// so the demuxer can retry any unwritten tail when playback makes room.
    pub(crate) fn write_pcm(self, pcm: &[u8]) -> Result<usize, i32> {
        SINK.lock().write_video(self.owner, self.id, pcm)
    }

    /// Keep the source until the output clocks drain its queued PCM.
    pub(crate) fn finish(self) {
        SINK.lock().finish_video(self.owner, self.id);
    }

    pub(crate) fn abort(self) {
        SINK.lock().stop_video(self.owner, Some(self.id));
    }

    pub(crate) fn queued_frames(self) -> usize {
        SINK.lock().video_queued_frames(self.owner, self.id)
    }
}

pub(crate) fn begin_video_pcm(owner: VideoAudioOwner) -> Result<VideoPcm, i32> {
    let id = SINK.lock().begin_video(owner)?;
    Ok(VideoPcm { owner, id })
}

pub(crate) fn start_video(owner: VideoAudioOwner) {
    SINK.lock().start_video(owner);
}

pub(crate) fn pause_video(owner: VideoAudioOwner, paused: bool) {
    SINK.lock().pause_video(owner, paused);
}

pub(crate) fn stop_video(owner: VideoAudioOwner) {
    SINK.lock().stop_video(owner, None);
}

pub(crate) struct RoutingSnapshot {
    pub sources: [Option<RouteSource>; MAX_PRODUCERS],
    pub http_enabled: bool,
    pub analog_connected: bool,
}

pub(crate) fn routing_snapshot() -> RoutingSnapshot {
    let sink = SINK.lock();
    RoutingSnapshot {
        sources: sink.route_sources(),
        http_enabled: sink.http_enabled,
        analog_connected: sink.analog_connected,
    }
}

pub(crate) fn toggle_route(id: u64, bucket: OutputBucket) -> bool {
    SINK.lock()
        .toggle_route(id, bucket, crate::hv::vm_run_generation)
}

pub(crate) fn http_enabled() -> bool {
    SINK.lock().http_enabled
}

pub(crate) fn set_http_enabled(enabled: bool) {
    let mut sink = SINK.lock();
    sink.http_enabled = enabled;
    if !enabled {
        sink.set_http_rendering(false);
    }
}

pub(crate) fn set_http_rendering(rendering: bool) {
    SINK.lock().set_http_rendering(rendering);
}

pub(crate) fn set_local_playback(enabled: bool) {
    SINK.lock().set_local_playback(enabled);
}

pub(crate) fn set_analog_connected(connected: bool) {
    SINK.lock().analog_connected = connected;
}

pub(super) fn monotonic_millis() -> u64 {
    Instant::now().as_millis()
}

pub(crate) fn render_local(out: &mut [[i16; 2]]) {
    SINK.lock().render_local(out, monotonic_millis());
}

pub(crate) fn render_http(
    a: &mut [u8],
    b: &mut [u8],
    mixed: &mut [u8],
    now_ms: u64,
) -> ([Option<u64>; MAX_PRODUCERS], [RenderReport; MAX_PRODUCERS]) {
    let (expired, reports) = {
        let mut sink = SINK.lock();
        if !sink.http_rendering() {
            a.fill(0);
            b.fill(0);
            mixed.fill(0);
            return (
                [None; MAX_PRODUCERS],
                [RenderReport::default(), RenderReport::default()],
            );
        }
        (sink.expire(now_ms), sink.render_sources(a, b))
    };
    sink::mix_center(mixed, &a[..reports[0].bytes], &b[..reports[1].bytes]);
    (expired, reports)
}

pub(crate) fn http_snapshot() -> Option<Clip> {
    let sink = SINK.lock();
    if !sink.http_enabled {
        return None;
    }
    sink.snapshot().filter(Clip::http_forward_allowed)
}

pub(crate) fn http_status() -> (Option<u64>, [Option<u64>; MAX_PRODUCERS]) {
    let sink = SINK.lock();
    if !sink.http_enabled {
        return (None, [None; MAX_PRODUCERS]);
    }
    let clip = sink.snapshot().filter(Clip::http_forward_allowed);
    (clip.map(|clip| clip.id), sink.http_active_ids())
}

pub(crate) fn source_active(id: u64) -> bool {
    SINK.lock().active_ids().contains(&Some(id))
}

#[trueos_executor::task]
pub(crate) async fn audio_playback_clock_task() {
    let mut next_tick = Instant::now();
    loop {
        let now = Instant::now();
        SINK.lock()
            .advance_headless(CLOCK_BLOCK_BYTES, now.as_millis());
        next_tick += CLOCK_PERIOD;
        if now >= next_tick + Duration::from_millis(100) {
            next_tick = now + CLOCK_PERIOD;
        }
        Timer::at(next_tick).await;
    }
}
