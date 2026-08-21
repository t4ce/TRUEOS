//! Temporary, direct-to-Spirit GPU diagnostics.
//!
//! The logger is deliberately a lease rather than a persistent Spirit mode.
//! A producer has to keep renewing its bounded lease and only the exact most
//! recent generation may release it.  The Spirit worker consumes
//! [`active_snapshot`] and can therefore fall back to Lilly without a cleanup
//! message when a producer disappears.

use alloc::vec::Vec;

use trueos_time::Instant;
use spin::Mutex;

use super::SpiritSurfaceLayout;
use crate::intel::gpgpu::{GpgpuRect, GpgpuSolidRect};

const MIN_TTL_MS: u64 = 1_000;
const MAX_TTL_MS: u64 = 300_000;
const PANEL_DIM: u32 = 256;
const METRIC_ROW_Y: [u32; 11] = [58, 72, 86, 100, 114, 128, 142, 156, 170, 184, 198];
const BAR_X: u32 = 29;
const BAR_WIDTH: u32 = 211;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum GpuLoggerSource {
    Helio,
    #[cfg(test)]
    Other,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct GpuLoggerLease {
    pub(crate) generation: u64,
    pub(crate) source: GpuLoggerSource,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuLoggerSample {
    pub(crate) frame_index: u64,
    /// Time between consecutive successful producer publications. Unlike
    /// `frame_us`, this includes pacing, admission waits, and retry delay, so
    /// its reciprocal is the producer's measured delivered rate.
    pub(crate) cadence_us: u64,
    pub(crate) frame_us: u64,
    pub(crate) geometry_us: u64,
    pub(crate) prepare_us: u64,
    pub(crate) retire_wait_us: u64,
    pub(crate) poll_iters: u64,
    pub(crate) objects: u64,
    pub(crate) draws: u64,
    pub(crate) triangles: u64,
    pub(crate) busy_retries: u64,
    pub(crate) incomplete_retries: u64,
}

const EMPTY_SAMPLE: GpuLoggerSample = GpuLoggerSample {
    frame_index: 0,
    cadence_us: 0,
    frame_us: 0,
    geometry_us: 0,
    prepare_us: 0,
    retire_wait_us: 0,
    poll_iters: 0,
    objects: 0,
    draws: 0,
    triangles: 0,
    busy_retries: 0,
    incomplete_retries: 0,
};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct BusyStatus {
    pub(crate) active_source: GpuLoggerSource,
    pub(crate) active_generation: u64,
    pub(crate) remaining_ms: u64,
}

#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct GpuLoggerStatus {
    pub(crate) active: bool,
    pub(crate) source: Option<GpuLoggerSource>,
    pub(crate) generation: u64,
    pub(crate) remaining_ms: u64,
    pub(crate) sample: GpuLoggerSample,
}

/// Immutable worker-side view.  Obtaining one also performs expiry, so an
/// abandoned producer can never permanently replace the normal Spirit image.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct ActiveSnapshot {
    pub(crate) source: GpuLoggerSource,
    pub(crate) generation: u64,
    pub(crate) remaining_ms: u64,
    pub(crate) sample: GpuLoggerSample,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct ActiveLease {
    source: GpuLoggerSource,
    generation: u64,
    expires_at_ms: u64,
    sample: GpuLoggerSample,
}

struct GpuLoggerState {
    next_generation: u64,
    active: Option<ActiveLease>,
    latest_helio: GpuLoggerSample,
    #[cfg(test)]
    latest_other: GpuLoggerSample,
}

impl GpuLoggerState {
    const fn new() -> Self {
        Self {
            next_generation: 0,
            active: None,
            latest_helio: EMPTY_SAMPLE,
            #[cfg(test)]
            latest_other: EMPTY_SAMPLE,
        }
    }

    const fn latest_sample(&self, source: GpuLoggerSource) -> GpuLoggerSample {
        match source {
            GpuLoggerSource::Helio => self.latest_helio,
            #[cfg(test)]
            GpuLoggerSource::Other => self.latest_other,
        }
    }

    fn store_latest_sample(&mut self, source: GpuLoggerSource, sample: GpuLoggerSample) {
        match source {
            GpuLoggerSource::Helio => self.latest_helio = sample,
            #[cfg(test)]
            GpuLoggerSource::Other => self.latest_other = sample,
        }
    }

    fn expire_at(&mut self, now_ms: u64) {
        if self
            .active
            .is_some_and(|active| now_ms >= active.expires_at_ms)
        {
            self.active = None;
        }
    }

    fn next_generation(&mut self) -> u64 {
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        self.next_generation
    }

    fn request_at(
        &mut self,
        source: GpuLoggerSource,
        ttl_ms: u64,
        now_ms: u64,
    ) -> Result<GpuLoggerLease, BusyStatus> {
        self.expire_at(now_ms);
        if let Some(active) = self.active
            && active.source != source
        {
            return Err(BusyStatus {
                active_source: active.source,
                active_generation: active.generation,
                remaining_ms: active.expires_at_ms.saturating_sub(now_ms),
            });
        }

        let generation = self.next_generation();
        let expires_at_ms = now_ms.saturating_add(ttl_ms.clamp(MIN_TTL_MS, MAX_TTL_MS));
        let sample = self
            .active
            .filter(|active| active.source == source)
            .map(|active| active.sample)
            .unwrap_or_else(|| self.latest_sample(source));
        self.active = Some(ActiveLease {
            source,
            generation,
            expires_at_ms,
            sample,
        });
        Ok(GpuLoggerLease { generation, source })
    }

    fn release_at(&mut self, lease: GpuLoggerLease, now_ms: u64) -> bool {
        self.expire_at(now_ms);
        let matches = self.active.is_some_and(|active| {
            active.source == lease.source && active.generation == lease.generation
        });
        if matches {
            self.active = None;
        }
        matches
    }

    #[allow(dead_code)]
    fn stop_source_at(&mut self, source: GpuLoggerSource, now_ms: u64) -> bool {
        self.expire_at(now_ms);
        let matches = self.active.is_some_and(|active| active.source == source);
        if matches {
            self.active = None;
        }
        matches
    }

    fn publish_at(
        &mut self,
        source: GpuLoggerSource,
        sample: GpuLoggerSample,
        now_ms: u64,
    ) -> bool {
        // Preserve the latest complete sample even when the visual override is
        // inactive. A static scene can open the monitor after its only frame.
        self.store_latest_sample(source, sample);
        self.expire_at(now_ms);
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        if active.source != source {
            return false;
        }
        active.sample = sample;
        true
    }

    fn active_snapshot_at(&mut self, now_ms: u64) -> Option<ActiveSnapshot> {
        self.expire_at(now_ms);
        self.active.map(|active| ActiveSnapshot {
            source: active.source,
            generation: active.generation,
            remaining_ms: active.expires_at_ms.saturating_sub(now_ms),
            sample: active.sample,
        })
    }

    fn status_at(&mut self, now_ms: u64) -> GpuLoggerStatus {
        let Some(active) = self.active_snapshot_at(now_ms) else {
            return GpuLoggerStatus::default();
        };
        GpuLoggerStatus {
            active: true,
            source: Some(active.source),
            generation: active.generation,
            remaining_ms: active.remaining_ms,
            sample: active.sample,
        }
    }
}

static GPU_LOGGER: Mutex<GpuLoggerState> = Mutex::new(GpuLoggerState::new());

#[inline]
fn now_ms() -> u64 {
    Instant::now().as_millis()
}

/// Acquire or renew the temporary logger mode.  Milliseconds are clamped to a
/// one-second minimum and five-minute maximum.  Renewal issues a new token and
/// consequently makes an older release harmless.
pub(crate) fn request(source: GpuLoggerSource, ttl_ms: u64) -> Result<GpuLoggerLease, BusyStatus> {
    GPU_LOGGER.lock().request_at(source, ttl_ms, now_ms())
}

/// Release only the exact current generation.
pub(crate) fn release(lease: GpuLoggerLease) -> bool {
    GPU_LOGGER.lock().release_at(lease, now_ms())
}

/// Administrative source-scoped stop used when the requester no longer has
/// its most recent renewal token.
#[allow(dead_code)]
pub(crate) fn stop_source(source: GpuLoggerSource) -> bool {
    GPU_LOGGER.lock().stop_source_at(source, now_ms())
}

/// Publish without extending the lease.  This is intentionally a cheap copy
/// of counters rather than a queue: the debug monitor only needs the newest
/// complete frame.
pub(crate) fn publish(source: GpuLoggerSource, sample: GpuLoggerSample) -> bool {
    GPU_LOGGER.lock().publish_at(source, sample, now_ms())
}

pub(crate) fn status() -> GpuLoggerStatus {
    GPU_LOGGER.lock().status_at(now_ms())
}

/// Read the newest complete sample independently of the visual lease. This
/// lets Shell2 inspect baseline producer cadence without enabling the Spirit
/// override and perturbing the shared GPU lane it is measuring.
pub(crate) fn latest_sample(source: GpuLoggerSource) -> GpuLoggerSample {
    GPU_LOGGER.lock().latest_sample(source)
}

/// Spirit-worker-only read of the active override.  Expiry is enforced here,
/// at the actual presentation decision boundary.
pub(crate) fn active_snapshot() -> Option<ActiveSnapshot> {
    GPU_LOGGER.lock().active_snapshot_at(now_ms())
}

#[derive(Copy, Clone)]
struct Color {
    r: u8,
    g: u8,
    b: u8,
}

impl Color {
    const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    const fn gpu_bgra(self) -> u32 {
        // The fill kernel stores this word verbatim.  Spirit's Intel cursor
        // allocation is BGRA in increasing byte order, so swap logical R/B at
        // this packed-color boundary.
        u32::from_le_bytes([self.b, self.g, self.r, 0xFF])
    }
}

const BACKGROUND: Color = Color::new(5, 11, 18);
const PANEL: Color = Color::new(9, 21, 32);
const PANEL_ALT: Color = Color::new(11, 26, 39);
const RULE: Color = Color::new(29, 55, 72);
const CYAN: Color = Color::new(48, 224, 226);
const WHITE: Color = Color::new(225, 240, 244);
const MUTED: Color = Color::new(113, 148, 160);
const GREEN: Color = Color::new(66, 226, 151);
const AMBER: Color = Color::new(249, 184, 65);
const RED: Color = Color::new(255, 92, 111);

trait RectSink {
    fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color);
}

struct GpuRectSink<'a> {
    rects: &'a mut Vec<GpgpuSolidRect>,
}

impl RectSink for GpuRectSink<'_> {
    fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        if width == 0 || height == 0 || x >= PANEL_DIM || y >= PANEL_DIM {
            return;
        }
        self.rects.push(GpgpuSolidRect {
            rect: GpgpuRect::new(
                x as i32,
                y as i32,
                width.min(PANEL_DIM - x),
                height.min(PANEL_DIM - y),
            ),
            color_rgba: color.gpu_bgra(),
        });
    }
}

struct BgraSink<'a> {
    pixels: &'a mut [u8],
    layout: SpiritSurfaceLayout,
}

impl RectSink for BgraSink<'_> {
    fn rect(&mut self, x: u32, y: u32, width: u32, height: u32, color: Color) {
        let surface_width = self.layout.width.min(PANEL_DIM);
        let surface_height = self.layout.height.min(PANEL_DIM);
        let x_end = x.saturating_add(width).min(surface_width);
        let y_end = y.saturating_add(height).min(surface_height);
        if x >= x_end || y >= y_end {
            return;
        }
        let accessible = self.pixels.len().min(self.layout.byte_len);
        let pitch = self.layout.pitch_bytes as usize;
        for py in y..y_end {
            let Some(row) = (py as usize).checked_mul(pitch) else {
                break;
            };
            for px in x..x_end {
                let Some(offset) = row.checked_add((px as usize).saturating_mul(4)) else {
                    break;
                };
                if offset.saturating_add(4) > accessible {
                    break;
                }
                self.pixels[offset..offset + 4].copy_from_slice(&[color.b, color.g, color.r, 0xFF]);
            }
        }
    }
}

/// Ordered GPU submission layers for the complete opaque panel.
///
/// Rectangles inside one fill-worklist dispatch execute as unordered
/// workgroups.  These boundaries keep every intentional overlap in a later
/// dispatch: clear first, mutually-disjoint opaque bases second, and
/// foreground last.  Foreground rectangles are disjoint unless an overlap has
/// the same packed color (for example, two ends of the border rule).
pub(crate) struct GpuLoggerRectLayers {
    pub(crate) rects: Vec<GpgpuSolidRect>,
    pub(crate) clear_end: usize,
    pub(crate) base_end: usize,
}

impl GpuLoggerRectLayers {
    pub(crate) fn clear(&self) -> &[GpgpuSolidRect] {
        &self.rects[..self.clear_end]
    }

    pub(crate) fn bases(&self) -> &[GpgpuSolidRect] {
        &self.rects[self.clear_end..self.base_end]
    }

    pub(crate) fn foreground(&self) -> &[GpgpuSolidRect] {
        &self.rects[self.base_end..]
    }
}

pub(crate) fn build_gpu_rect_layers(snapshot: ActiveSnapshot) -> GpuLoggerRectLayers {
    let mut rects = Vec::with_capacity(768);
    let clear_end;
    let base_end;
    {
        let mut sink = GpuRectSink { rects: &mut rects };
        draw_panel_clear(&mut sink);
        clear_end = sink.rects.len();
        draw_panel_bases(&mut sink);
        base_end = sink.rects.len();
        draw_panel_foreground(&mut sink, snapshot);
    }
    GpuLoggerRectLayers {
        rects,
        clear_end,
        base_end,
    }
}

/// Flat compatibility view of the panel.  GPU producers must submit it using
/// the boundaries returned by [`build_gpu_rect_layers`], not as one unordered
/// worklist.
#[cfg(test)]
fn build_gpu_rects(snapshot: ActiveSnapshot) -> Vec<GpgpuSolidRect> {
    build_gpu_rect_layers(snapshot).rects
}

/// Allocation-free CPU reference/fallback rasterizer for the cursor's direct
/// BGRA-premultiplied storage.  The production worker can render the identical
/// rectangle stream with the GPU fill worklist.
pub(crate) fn render_bgra(
    pixels: &mut [u8],
    layout: SpiritSurfaceLayout,
    snapshot: ActiveSnapshot,
) {
    let mut sink = BgraSink { pixels, layout };
    draw_panel(&mut sink, snapshot);
}

fn draw_panel(sink: &mut impl RectSink, snapshot: ActiveSnapshot) {
    draw_panel_clear(sink);
    draw_panel_bases(sink);
    draw_panel_foreground(sink, snapshot);
}

fn draw_panel_clear(sink: &mut impl RectSink) {
    sink.rect(0, 0, PANEL_DIM, PANEL_DIM, BACKGROUND);
}

fn draw_panel_bases(sink: &mut impl RectSink) {
    // These opaque rectangles are mutually disjoint, allowing each to occupy
    // the same unordered worklist submission safely.
    sink.rect(0, 0, 4, PANEL_DIM, CYAN);
    sink.rect(8, 8, 240, 42, PANEL);
    for (index, y) in METRIC_ROW_Y.into_iter().enumerate() {
        if index & 1 == 0 {
            sink.rect(8, y - 3, 240, 12, PANEL_ALT);
        }
    }
    draw_meter_base(sink, 210);
    draw_meter_base(sink, 225);
    draw_meter_base(sink, 240);
}

fn draw_panel_foreground(sink: &mut impl RectSink, snapshot: ActiveSnapshot) {
    let sample = snapshot.sample;

    // A restrained instrument-panel frame.  Everything is opaque so cursor
    // blending cannot leak the normal Lilly sprite through logger mode.
    sink.rect(8, 8, 240, 1, RULE);
    sink.rect(8, 49, 240, 1, RULE);
    sink.rect(8, 8, 1, 42, RULE);
    sink.rect(247, 8, 1, 42, RULE);
    draw_text(sink, 16, 15, b"GPU LOGGER", 2, WHITE);
    draw_text(sink, 203, 16, source_label(snapshot.source), 1, CYAN);
    draw_text(sink, 16, 39, b"DIRECT CURSOR / NO UI4", 1, MUTED);
    draw_text(sink, 190, 39, b"FPS", 1, MUTED);
    let fps = fps_from_cadence_us(sample.cadence_us);
    draw_u64_right(sink, 240, 39, fps, 1, CYAN);

    let health = health_color(sample);
    sink.rect(238, 16, 4, 10, health);

    draw_metric(sink, METRIC_ROW_Y[0], b"FRAME", sample.frame_index);
    draw_metric(sink, METRIC_ROW_Y[1], b"CADENCE US", sample.cadence_us);
    draw_metric(sink, METRIC_ROW_Y[2], b"RENDER US", sample.frame_us);
    draw_metric(sink, METRIC_ROW_Y[3], b"GEOM US", sample.geometry_us);
    draw_metric(sink, METRIC_ROW_Y[4], b"PREP US", sample.prepare_us);
    draw_metric(sink, METRIC_ROW_Y[5], b"WAIT US", sample.retire_wait_us);
    draw_metric(sink, METRIC_ROW_Y[6], b"POLL", sample.poll_iters);
    draw_metric(sink, METRIC_ROW_Y[7], b"OBJECTS", sample.objects);
    draw_metric(sink, METRIC_ROW_Y[8], b"DRAWS", sample.draws);
    draw_metric(sink, METRIC_ROW_Y[9], b"TRIANGLES", sample.triangles);
    draw_text(sink, 14, METRIC_ROW_Y[10], b"RETRY B/I", 1, MUTED);
    draw_pair_right(
        sink,
        240,
        METRIC_ROW_Y[10],
        sample.busy_retries,
        sample.incomplete_retries,
        if sample.busy_retries == 0 && sample.incomplete_retries == 0 {
            GREEN
        } else {
            AMBER
        },
    );

    draw_meter_foreground(
        sink,
        210,
        b"C",
        sample.cadence_us,
        33_334,
        frame_color(sample.cadence_us),
    );
    draw_meter_foreground(sink, 225, b"R", sample.frame_us, sample.cadence_us.max(1), CYAN);
    draw_meter_foreground(
        sink,
        240,
        b"W",
        sample.retire_wait_us,
        sample.frame_us.max(1),
        if sample.retire_wait_us.saturating_mul(4) > sample.frame_us {
            RED
        } else {
            AMBER
        },
    );
}

fn draw_metric(sink: &mut impl RectSink, y: u32, label: &[u8], value: u64) {
    draw_text(sink, 14, y, label, 1, MUTED);
    draw_u64_right(sink, 240, y, value, 1, WHITE);
}

fn draw_meter_base(sink: &mut impl RectSink, y: u32) {
    sink.rect(BAR_X, y, BAR_WIDTH, 5, PANEL_ALT);
}

fn draw_meter_foreground(
    sink: &mut impl RectSink,
    y: u32,
    label: &[u8],
    value: u64,
    maximum: u64,
    color: Color,
) {
    draw_text(sink, 14, y, label, 1, MUTED);
    let fill = if maximum == 0 {
        0
    } else {
        ((value.min(maximum) as u128 * BAR_WIDTH as u128) / maximum as u128) as u32
    };
    sink.rect(BAR_X, y, fill, 5, color);
}

fn frame_color(frame_us: u64) -> Color {
    if frame_us <= 16_667 {
        GREEN
    } else if frame_us <= 25_000 {
        AMBER
    } else {
        RED
    }
}

pub(crate) const fn fps_from_cadence_us(cadence_us: u64) -> u64 {
    if cadence_us == 0 {
        0
    } else {
        1_000_000u64.saturating_add(cadence_us / 2) / cadence_us
    }
}

fn health_color(sample: GpuLoggerSample) -> Color {
    if sample.incomplete_retries != 0 || sample.cadence_us > 33_334 {
        RED
    } else if sample.busy_retries != 0 || sample.cadence_us > 16_667 {
        AMBER
    } else {
        GREEN
    }
}

fn source_label(source: GpuLoggerSource) -> &'static [u8] {
    match source {
        GpuLoggerSource::Helio => b"HELIO",
        #[cfg(test)]
        GpuLoggerSource::Other => b"OTHER",
    }
}

fn draw_u64_right(
    sink: &mut impl RectSink,
    right: u32,
    y: u32,
    value: u64,
    scale: u32,
    color: Color,
) {
    let mut buffer = [0u8; 20];
    let digits = decimal_bytes(value, &mut buffer);
    let x = right.saturating_sub(text_width(digits.len(), scale));
    draw_text(sink, x, y, digits, scale, color);
}

fn draw_pair_right(
    sink: &mut impl RectSink,
    right: u32,
    y: u32,
    left_value: u64,
    right_value: u64,
    color: Color,
) {
    let mut left_buffer = [0u8; 20];
    let mut right_buffer = [0u8; 20];
    let left = decimal_bytes(left_value, &mut left_buffer);
    let right_digits = decimal_bytes(right_value, &mut right_buffer);
    let advance = 4;
    let right_x = right.saturating_sub(text_width(right_digits.len(), 1));
    draw_text(sink, right_x, y, right_digits, 1, color);
    let slash_x = right_x.saturating_sub(advance);
    draw_text(sink, slash_x, y, b"/", 1, MUTED);
    let left_x = slash_x.saturating_sub(advance + text_width(left.len(), 1));
    draw_text(sink, left_x, y, left, 1, color);
}

fn decimal_bytes<'a>(mut value: u64, buffer: &'a mut [u8; 20]) -> &'a [u8] {
    let mut cursor = buffer.len();
    loop {
        cursor -= 1;
        buffer[cursor] = b'0' + (value % 10) as u8;
        value /= 10;
        if value == 0 {
            return &buffer[cursor..];
        }
    }
}

fn text_width(byte_len: usize, scale: u32) -> u32 {
    if byte_len == 0 {
        0
    } else {
        (byte_len as u32)
            .saturating_mul(4)
            .saturating_sub(1)
            .saturating_mul(scale)
    }
}

fn draw_text(sink: &mut impl RectSink, x: u32, y: u32, text: &[u8], scale: u32, color: Color) {
    if scale == 0 {
        return;
    }
    for (character_index, ch) in text.iter().copied().enumerate() {
        let glyph_x = x.saturating_add((character_index as u32).saturating_mul(4 * scale));
        for (row, bits) in tiny_glyph(ch).into_iter().enumerate() {
            // One rectangle per contiguous horizontal stroke, rather than one
            // descriptor per pixel, keeps the direct GPU worklist compact.
            let mut column = 0u32;
            while column < 3 {
                if bits & (1 << (2 - column)) == 0 {
                    column += 1;
                    continue;
                }
                let start = column;
                while column < 3 && bits & (1 << (2 - column)) != 0 {
                    column += 1;
                }
                sink.rect(
                    glyph_x.saturating_add(start.saturating_mul(scale)),
                    y.saturating_add((row as u32).saturating_mul(scale)),
                    column.saturating_sub(start).saturating_mul(scale),
                    scale,
                    color,
                );
            }
        }
    }
}

fn tiny_glyph(ch: u8) -> [u8; 5] {
    match ch.to_ascii_uppercase() {
        b'A' => [0b111, 0b101, 0b111, 0b101, 0b101],
        b'B' => [0b110, 0b101, 0b110, 0b101, 0b110],
        b'C' => [0b111, 0b100, 0b100, 0b100, 0b111],
        b'D' => [0b110, 0b101, 0b101, 0b101, 0b110],
        b'E' => [0b111, 0b100, 0b110, 0b100, 0b111],
        b'F' => [0b111, 0b100, 0b110, 0b100, 0b100],
        b'G' => [0b111, 0b100, 0b101, 0b101, 0b111],
        b'H' => [0b101, 0b101, 0b111, 0b101, 0b101],
        b'I' => [0b111, 0b010, 0b010, 0b010, 0b111],
        b'J' => [0b001, 0b001, 0b001, 0b101, 0b010],
        b'K' => [0b101, 0b101, 0b110, 0b101, 0b101],
        b'L' => [0b100, 0b100, 0b100, 0b100, 0b111],
        b'M' => [0b101, 0b111, 0b111, 0b101, 0b101],
        b'N' => [0b101, 0b111, 0b111, 0b111, 0b101],
        b'O' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'P' => [0b111, 0b101, 0b111, 0b100, 0b100],
        b'Q' => [0b111, 0b101, 0b101, 0b111, 0b001],
        b'R' => [0b111, 0b101, 0b111, 0b110, 0b101],
        b'S' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'T' => [0b111, 0b010, 0b010, 0b010, 0b010],
        b'U' => [0b101, 0b101, 0b101, 0b101, 0b111],
        b'V' => [0b101, 0b101, 0b101, 0b101, 0b010],
        b'W' => [0b101, 0b101, 0b111, 0b111, 0b101],
        b'X' => [0b101, 0b101, 0b010, 0b101, 0b101],
        b'Y' => [0b101, 0b101, 0b010, 0b010, 0b010],
        b'Z' => [0b111, 0b001, 0b010, 0b100, 0b111],
        b'0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        b'1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        b'2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        b'3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        b'4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        b'5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        b'6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        b'7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        b'8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        b'9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        b'-' => [0b000, 0b000, 0b111, 0b000, 0b000],
        b'_' => [0b000, 0b000, 0b000, 0b000, 0b111],
        b'.' => [0b000, 0b000, 0b000, 0b000, 0b010],
        b':' => [0b000, 0b010, 0b000, 0b010, 0b000],
        b'/' => [0b001, 0b001, 0b010, 0b100, 0b100],
        b'#' => [0b101, 0b111, 0b101, 0b111, 0b101],
        b' ' => [0; 5],
        _ => [0b111, 0b001, 0b010, 0b000, 0b010],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> GpuLoggerSample {
        GpuLoggerSample {
            frame_index: 73,
            cadence_us: 40_000,
            frame_us: 16_250,
            geometry_us: 8_000,
            prepare_us: 1_500,
            retire_wait_us: 2_000,
            poll_iters: 41,
            objects: 2_200,
            draws: 5,
            triangles: 26_400,
            busy_retries: 1,
            incomplete_retries: 0,
        }
    }

    #[test]
    fn fps_is_measured_from_successful_publish_cadence() {
        assert_eq!(fps_from_cadence_us(0), 0);
        assert_eq!(fps_from_cadence_us(16_667), 60);
        assert_eq!(fps_from_cadence_us(40_000), 25);
        assert_eq!(fps_from_cadence_us(142_857), 7);
    }

    #[test]
    fn latest_sample_survives_visual_lease_release() {
        let mut state = GpuLoggerState::new();
        let lease = state
            .request_at(GpuLoggerSource::Helio, 1_000, 100)
            .unwrap();
        assert!(state.publish_at(GpuLoggerSource::Helio, sample(), 101));
        assert!(state.release_at(lease, 102));
        assert!(!state.status_at(103).active);
        assert_eq!(state.latest_sample(GpuLoggerSource::Helio), sample());
    }

    #[test]
    fn grant_renew_and_exact_release_are_generation_safe() {
        let mut state = GpuLoggerState::new();
        let first = state
            .request_at(GpuLoggerSource::Helio, 8_000, 100)
            .unwrap();
        assert_eq!(first.generation, 1);
        assert!(state.publish_at(GpuLoggerSource::Helio, sample(), 101));

        let renewed = state
            .request_at(GpuLoggerSource::Helio, 12_000, 200)
            .unwrap();
        assert_eq!(renewed.generation, 2);
        assert_eq!(state.active_snapshot_at(201).unwrap().sample, sample());
        assert!(!state.release_at(first, 202));
        assert_eq!(state.status_at(203).generation, renewed.generation);
        assert!(state.release_at(renewed, 204));
        assert!(!state.status_at(205).active);
    }

    #[test]
    fn another_source_is_busy_until_expiry() {
        let mut state = GpuLoggerState::new();
        state
            .request_at(GpuLoggerSource::Helio, 1_000, 5_000)
            .unwrap();
        let busy = state
            .request_at(GpuLoggerSource::Other, 1_000, 5_250)
            .unwrap_err();
        assert_eq!(busy.active_source, GpuLoggerSource::Helio);
        assert_eq!(busy.remaining_ms, 750);

        assert!(state.active_snapshot_at(5_999).is_some());
        assert!(state.active_snapshot_at(6_000).is_none());
        assert!(
            state
                .request_at(GpuLoggerSource::Other, 1_000, 6_000)
                .is_ok()
        );
    }

    #[test]
    fn ttl_is_clamped_and_stop_is_source_scoped() {
        let mut state = GpuLoggerState::new();
        state.request_at(GpuLoggerSource::Helio, 0, 10).unwrap();
        assert_eq!(state.active_snapshot_at(11).unwrap().remaining_ms, 999);
        assert!(!state.stop_source_at(GpuLoggerSource::Other, 12));
        assert!(state.stop_source_at(GpuLoggerSource::Helio, 12));

        state
            .request_at(GpuLoggerSource::Helio, u64::MAX, 100)
            .unwrap();
        assert_eq!(state.active_snapshot_at(100).unwrap().remaining_ms, MAX_TTL_MS);
    }

    #[test]
    fn cpu_reference_is_opaque_bgra_and_honors_pitch() {
        let pitch = PANEL_DIM as usize * 4 + 16;
        let byte_len = pitch * PANEL_DIM as usize;
        let mut pixels = alloc::vec![0xA5; byte_len];
        render_bgra(
            &mut pixels,
            SpiritSurfaceLayout {
                width: PANEL_DIM,
                height: PANEL_DIM,
                pitch_bytes: pitch as u32,
                byte_len,
            },
            ActiveSnapshot {
                source: GpuLoggerSource::Helio,
                generation: 9,
                remaining_ms: 4_000,
                sample: sample(),
            },
        );
        assert_eq!(&pixels[0..4], &[BACKGROUND.b, BACKGROUND.g, BACKGROUND.r, 0xFF]);
        for y in 0..PANEL_DIM as usize {
            for x in 0..PANEL_DIM as usize {
                assert_eq!(pixels[y * pitch + x * 4 + 3], 0xFF);
            }
            assert!(
                pixels[y * pitch + PANEL_DIM as usize * 4..(y + 1) * pitch]
                    .iter()
                    .all(|byte| *byte == 0xA5)
            );
        }
    }

    #[test]
    fn gpu_rect_stream_starts_with_complete_bgra_clear() {
        let snapshot = ActiveSnapshot {
            source: GpuLoggerSource::Helio,
            generation: 1,
            remaining_ms: 1_000,
            sample: sample(),
        };
        let rects = build_gpu_rects(snapshot);
        assert!(!rects.is_empty());
        assert_eq!(rects[0].rect, GpgpuRect::new(0, 0, PANEL_DIM, PANEL_DIM));
        assert_eq!(
            rects[0].color_rgba.to_le_bytes(),
            [BACKGROUND.b, BACKGROUND.g, BACKGROUND.r, 0xFF]
        );
    }

    fn overlaps(left: GpgpuRect, right: GpgpuRect) -> bool {
        let left_right = i64::from(left.x) + i64::from(left.width);
        let left_bottom = i64::from(left.y) + i64::from(left.height);
        let right_right = i64::from(right.x) + i64::from(right.width);
        let right_bottom = i64::from(right.y) + i64::from(right.height);
        i64::from(left.x) < right_right
            && i64::from(right.x) < left_right
            && i64::from(left.y) < right_bottom
            && i64::from(right.y) < left_bottom
    }

    #[test]
    fn gpu_layers_isolate_every_order_dependent_overlap() {
        let layers = build_gpu_rect_layers(ActiveSnapshot {
            source: GpuLoggerSource::Helio,
            generation: 1,
            remaining_ms: 1_000,
            sample: sample(),
        });
        assert_eq!(layers.clear().len(), 1);
        assert!(!layers.bases().is_empty());
        assert!(!layers.foreground().is_empty());

        for (index, left) in layers.bases().iter().enumerate() {
            for right in &layers.bases()[index + 1..] {
                assert!(!overlaps(left.rect, right.rect));
            }
        }
        for (index, left) in layers.foreground().iter().enumerate() {
            for right in &layers.foreground()[index + 1..] {
                if overlaps(left.rect, right.rect) {
                    assert_eq!(left.color_rgba, right.color_rgba);
                }
            }
        }
    }
}
