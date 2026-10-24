//! Hull's shared, colour-free microfont raster cache.
//!
//! Rasterization uses the crate's line/quad geometry, never its HEX bitmaps.
//! Hull calls `prepare_blocking` on its existing render worker, before taking
//! a writable UI4 surface view. Only cache lookup/publication holds the lock;
//! simultaneous cold requests may rasterize the same glyph and then share the
//! winner. This avoids waiting on another Hull while holding a UI or cache lock.
//! Immutable Arc masks remain valid after eviction. No GPU source is recycled
//! while an asynchronous copy might still reference it: composition is CPU RGBA8
//! into the caller's already leased frame, followed by its normal publication.
use alloc::{
    collections::{BTreeMap, BTreeSet},
    string::String,
    sync::Arc,
    vec::Vec,
};

const MAX_EM_HEIGHT: u32 = 4096;
const MAX_RUN_GLYPHS: usize = 65_536;
const MIN_CACHE_BYTES: usize = 256 * 1024;
const MAX_CACHE_BYTES: usize = 128 * 1024 * 1024;
const MAX_PREPARED_MASK_BYTES: usize = 32 * 1024 * 1024;
const ENTRY_BYTES: usize = 128;

/// Pixel geometry from the SVG. `advance_px` is the complete run's textLength;
/// zero selects microfont's natural 6/11-em advance. Colour is little-endian
/// straight RGBA. An alphabetic baseline is nine source rows into the 11-row em.
pub(crate) struct HullTextRun {
    pub(crate) text: String,
    pub(crate) x: f32,
    pub(crate) baseline_y: f32,
    pub(crate) size_px: f32,
    pub(crate) advance_px: f32,
    pub(crate) rgba: u32,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct CacheStats {
    pub(crate) requests: u64,
    pub(crate) hits: u64,
    pub(crate) rasterized: u64,
    pub(crate) concurrent_duplicates: u64,
    pub(crate) evictions: u64,
    pub(crate) raster_pixels: u64,
    pub(crate) raster_us: u64,
    pub(crate) prepare_us: u64,
    pub(crate) compose_us: u64,
    pub(crate) composed_pixels: u64,
    pub(crate) cache_bytes: usize,
    pub(crate) budget_bytes: usize,
}

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd)]
struct Key {
    height: u32,
    character: char,
}
struct Mask {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}
struct Entry {
    mask: Arc<Mask>,
    used: u64,
}
struct Cache {
    entries: BTreeMap<Key, Entry>,
    warmed: BTreeSet<u32>,
    clock: u64,
    stats: CacheStats,
}
impl Cache {
    const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            warmed: BTreeSet::new(),
            clock: 0,
            stats: CacheStats {
                requests: 0,
                hits: 0,
                rasterized: 0,
                concurrent_duplicates: 0,
                evictions: 0,
                raster_pixels: 0,
                raster_us: 0,
                prepare_us: 0,
                compose_us: 0,
                composed_pixels: 0,
                cache_bytes: 0,
                budget_bytes: MIN_CACHE_BYTES,
            },
        }
    }
    fn touch(&mut self, key: Key) -> Option<Arc<Mask>> {
        self.clock = self.clock.wrapping_add(1);
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(Arc::clone(&entry.mask))
    }
    fn trim(&mut self, incoming: usize) {
        while self.stats.cache_bytes.saturating_add(incoming) > self.stats.budget_bytes {
            let Some(key) = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(&key, _)| key)
            else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.stats.cache_bytes -= mask_cost(&entry.mask);
                self.stats.evictions += 1;
            }
        }
    }
    fn publish(&mut self, key: Key, mask: Arc<Mask>) -> (Arc<Mask>, bool) {
        if let Some(existing) = self.touch(key) {
            return (existing, true);
        }
        let cost = mask_cost(&mask);
        // A single exceptional large glyph is usable but must not empty the
        // entire cache or permanently exceed its byte budget.
        if cost <= self.stats.budget_bytes {
            self.trim(cost);
            self.entries.insert(
                key,
                Entry {
                    mask: Arc::clone(&mask),
                    used: self.clock,
                },
            );
            self.stats.cache_bytes += cost;
        }
        (mask, false)
    }
}
static CACHE: spin::Mutex<Cache> = spin::Mutex::new(Cache::new());

fn mask_cost(mask: &Mask) -> usize {
    mask.pixels.len().saturating_add(ENTRY_BYTES)
}
fn now_us() -> u64 {
    crate::ui4::text_blit::nanos(crate::ui4::text_blit::stamp()) / 1000
}

pub(crate) fn stats() -> CacheStats {
    CACHE.lock().stats
}

#[derive(Default)]
struct RequestStats {
    hits: u64,
    rasterized: u64,
    duplicates: u64,
    raster_us: u64,
    raster_pixels: u64,
}

fn cached_mask(key: Key, request: &mut RequestStats) -> Result<Arc<Mask>, &'static str> {
    if let Some(mask) = CACHE.lock().touch(key) {
        request.hits += 1;
        return Ok(mask);
    }
    let started = now_us();
    let mask = Arc::new(raster_glyph(key.character, key.height)?);
    request.raster_us += now_us().saturating_sub(started);
    request.rasterized += 1;
    request.raster_pixels += mask.pixels.len() as u64;
    let (mask, duplicate) = CACHE.lock().publish(key, mask);
    request.duplicates += u64::from(duplicate);
    Ok(mask)
}

/// Physical tiers use exactly the preview's 5/2-mm conversion, rounding only
/// the final pixel result. Below the first tier, and without EDID size data,
/// whole-pixel sizes round down so small SVG labels are never enlarged.
fn snapped_height(requested: f32) -> u32 {
    lower_step_height(requested, |step| {
        crate::intel::physical_extent_pixels_fraction(10, step * 5, 2).map(|(_, height)| height)
    })
}

/// Shared with the ABI's bounded long-run batching so natural-advance chunks
/// begin at precisely the same position as one unsplit SVG text run.
pub(crate) fn natural_advance_px(size_px: f32) -> f32 {
    snapped_height(size_px) as f32 * 6.0 / 11.0
}
fn lower_step_height(requested: f32, mut resolve: impl FnMut(u32) -> Option<u32>) -> u32 {
    let lower_pixel = (requested as u32).clamp(1, MAX_EM_HEIGHT);
    let Some(first) = resolve(1).filter(|&height| height != 0) else {
        return lower_pixel;
    };
    if first > lower_pixel {
        return lower_pixel;
    }
    // Integer pixel rounding can make neighbouring tiers equal. Search for
    // the last tier no larger than the request rather than multiplying the
    // already-rounded first tier (which drifts on fractional display ratios).
    let mut low = 1u32;
    let mut high = lower_pixel.saturating_mul(2).saturating_add(2);
    while low + 1 < high {
        let mid = low + (high - low) / 2;
        if resolve(mid).is_some_and(|height| height <= lower_pixel) {
            low = mid;
        } else {
            high = mid;
        }
    }
    resolve(low).unwrap_or(lower_pixel).min(lower_pixel).max(1)
}

struct PaintedGlyph {
    mask: Arc<Mask>,
    x: f32,
    y: f32,
    width: f32,
    rgba: [u8; 4],
}
pub(crate) struct PreparedText {
    glyphs: Vec<PaintedGlyph>,
    missing: usize,
}

/// This synchronous entry point belongs on Hull's existing blocking worker.
/// Several workers can prepare simultaneously. The caller retains its write
/// lease but takes no surface view or SURFACES lock while masks are produced.
/// No rasterization occurs under the shared cache lock.
pub(crate) fn prepare_blocking(
    runs: Vec<HullTextRun>,
    frame_width: u32,
    frame_height: u32,
) -> Result<PreparedText, &'static str> {
    let started = now_us();
    let run_count = runs.len();
    let glyph_count = runs
        .iter()
        .map(|run| run.text.chars().count())
        .sum::<usize>();
    if glyph_count > MAX_RUN_GLYPHS {
        return Err("hull-font-too-many-glyphs");
    }
    if runs.iter().any(|run| {
        !run.x.is_finite()
            || !run.baseline_y.is_finite()
            || !run.size_px.is_finite()
            || run.size_px <= 0.0
            || run.size_px > MAX_EM_HEIGHT as f32
            || !run.advance_px.is_finite()
            || run.advance_px < 0.0
    }) {
        return Err("hull-font-run-geometry");
    }
    let output = crate::ui4::output_dimensions();
    let display = output.unwrap_or((frame_width, frame_height));
    let budget = (display.0 as usize)
        .saturating_mul(display.1 as usize)
        .saturating_mul(4 * 4)
        .clamp(MIN_CACHE_BYTES, MAX_CACHE_BYTES);
    let initial_evictions;
    let prepared_budget;
    {
        let mut cache = CACHE.lock();
        initial_evictions = cache.stats.evictions;
        // Display dimensions are the stable cross-Hull authority. With no
        // scanout yet, retain the largest frame budget observed this boot.
        cache.stats.budget_bytes = if output.is_some() {
            budget
        } else {
            budget.max(cache.stats.budget_bytes)
        };
        cache.trim(0);
        prepared_budget = cache.stats.budget_bytes.min(MAX_PREPARED_MASK_BYTES);
    }
    let mut request = RequestStats::default();
    let mut glyphs = Vec::with_capacity(glyph_count);
    let mut missing = 0;
    let mut sizes = BTreeSet::new();
    let mut snapped_sizes = BTreeMap::new();
    let mut retained = BTreeMap::new();
    let mut retained_bytes = 0usize;
    for run in runs {
        let count = run.text.chars().count();
        let color = run.rgba.to_le_bytes();
        if count == 0 || color[3] == 0 {
            continue;
        }
        // Many SVG runs repeat the same CSS size. Resolve its physical ladder
        // once per call; never retain display-dependent tiers across modes.
        let height = *snapped_sizes
            .entry(run.size_px.to_bits())
            .or_insert_with(|| snapped_height(run.size_px));
        let advance = if run.advance_px > 0.0 {
            run.advance_px / count as f32
        } else {
            height as f32 * 6.0 / 11.0
        };
        let top = run.baseline_y - height as f32 * 9.0 / 11.0;
        if top >= frame_height as f32 || top + height as f32 <= 0.0 {
            continue;
        }
        for (column, character) in run.text.chars().enumerate() {
            let x = run.x + column as f32 * advance;
            if x >= frame_width as f32 || x + advance <= 0.0 || advance <= 0.0 {
                continue;
            }
            let present = supported(character);
            let character = if present {
                character
            } else {
                missing += 1;
                '?'
            };
            let key = Key { height, character };
            let (mask, local_hit) = retain_prepared_mask(
                &mut retained,
                &mut retained_bytes,
                key,
                prepared_budget,
                || cached_mask(key, &mut request),
            )?;
            request.hits += u64::from(local_hit);
            sizes.insert(height);
            glyphs.push(PaintedGlyph {
                mask,
                x,
                y: top,
                width: advance,
                rgba: if present {
                    color
                } else {
                    [255, 0, 0, color[3]]
                },
            });
        }
    }
    // Warm the cheap small alphabet only after fulfilling this Hull's demand.
    // Larger glyphs stay demand-only. Each size gets one lookahead attempt;
    // concurrent Hulls do not all warm it, and warming is capped at 1/8 budget.
    for height in sizes {
        warm_small_size(height, &mut request)?;
    }
    let elapsed = now_us().saturating_sub(started);
    let (bytes, budget, evictions) = {
        let mut cache = CACHE.lock();
        cache.stats.requests += 1;
        cache.stats.hits += request.hits;
        cache.stats.rasterized += request.rasterized;
        cache.stats.concurrent_duplicates += request.duplicates;
        cache.stats.raster_pixels += request.raster_pixels;
        cache.stats.raster_us += request.raster_us;
        cache.stats.prepare_us += elapsed;
        (
            cache.stats.cache_bytes,
            cache.stats.budget_bytes,
            cache.stats.evictions - initial_evictions,
        )
    };
    crate::log_important!(target: "hull-font";
        "hull-font: prepare runs={} glyphs={} rasterized={} hits={} concurrent_duplicates={} cache_bytes={} budget_bytes={} evictions={} prepare_us={} raster_us={} raster_pixels={} format=r8 quantum_mm=2.5\n",
        run_count, glyph_count, request.rasterized, request.hits, request.duplicates,
        bytes, budget, evictions, elapsed, request.raster_us, request.raster_pixels);
    Ok(PreparedText { glyphs, missing })
}

fn retain_prepared_mask(
    retained: &mut BTreeMap<Key, Arc<Mask>>,
    bytes: &mut usize,
    key: Key,
    budget: usize,
    load: impl FnOnce() -> Result<Arc<Mask>, &'static str>,
) -> Result<(Arc<Mask>, bool), &'static str> {
    if let Some(mask) = retained.get(&key) {
        return Ok((Arc::clone(mask), true));
    }
    let cost = (key.height as usize)
        .saturating_mul((key.height as usize * 6).div_ceil(11))
        .saturating_add(ENTRY_BYTES);
    let next = bytes.saturating_add(cost);
    if next > budget {
        return Err("hull-font-prepared-mask-budget");
    }
    // Keep exactly one owned mask per key for the complete request. A second
    // Hull may evict this key globally between repeated letters; consulting
    // the global cache again would retain multiple allocations but charge once.
    let mask = load()?;
    retained.insert(key, Arc::clone(&mask));
    *bytes = next;
    Ok((mask, false))
}

fn warm_small_size(height: u32, request: &mut RequestStats) -> Result<(), &'static str> {
    if height > 64 {
        return Ok(());
    }
    let estimated = (height as usize)
        .saturating_mul((height as usize * 6).div_ceil(11))
        .saturating_add(ENTRY_BYTES)
        .saturating_mul(if height <= 32 { 95 } else { 63 });
    {
        let mut cache = CACHE.lock();
        if estimated > cache.stats.budget_bytes / 8 || !cache.warmed.insert(height) {
            return Ok(());
        }
    }
    for character in ' '..='~' {
        if height > 32 && !character.is_ascii_alphanumeric() && character != '?' {
            continue;
        }
        if supported(character) {
            if let Err(error) = cached_mask(Key { height, character }, request) {
                CACHE.lock().warmed.remove(&height);
                return Err(error);
            }
        }
    }
    Ok(())
}

impl PreparedText {
    /// Conservative clipped union of all pixels `paint` can read or write,
    /// expressed as (x, y, width, height). Padding outside these rows/columns
    /// is never accessed. Empty/transparent/offscreen text returns None.
    pub(crate) fn paint_bounds(&self, width: u32, height: u32) -> Option<(u32, u32, u32, u32)> {
        let mut union: Option<(u32, u32, u32, u32)> = None;
        for glyph in &self.glyphs {
            let Some((left, top, right, bottom)) = glyph_paint_bounds(glyph, width, height) else {
                continue;
            };
            union = Some(match union {
                Some((l, t, r, b)) => (l.min(left), t.min(top), r.max(right), b.max(bottom)),
                None => (left, top, right, bottom),
            });
        }
        union.map(|(left, top, right, bottom)| (left, top, right - left, bottom - top))
    }

    /// Alpha-over into premultiplied RGBA8. No font work or cache lookup occurs
    /// under the frame lease. Fractional SVG positions and explicit textLength
    /// use bilinear mask sampling, and every write is clipped to this frame.
    pub(crate) fn paint(
        &self,
        pixels: &mut [u8],
        width: u32,
        height: u32,
        pitch_bytes: u32,
    ) -> Result<(), &'static str> {
        let row = (width as usize)
            .checked_mul(4)
            .ok_or("hull-font-frame-size")?;
        let required = (height as usize)
            .checked_sub(1)
            .and_then(|rows| rows.checked_mul(pitch_bytes as usize))
            .and_then(|bytes| bytes.checked_add(row));
        if width == 0
            || height == 0
            || (pitch_bytes as usize) < row
            || required.is_none_or(|required| required > pixels.len())
        {
            return Err("hull-font-frame-size");
        }
        let started = now_us();
        let mut composed = 0u64;
        for glyph in &self.glyphs {
            let Some((left, top, right, bottom)) = glyph_paint_bounds(glyph, width, height) else {
                continue;
            };
            for y in top..bottom {
                let sy = y as f32 + 0.5 - glyph.y - 0.5;
                for x in left..right {
                    let sx =
                        (x as f32 + 0.5 - glyph.x) * glyph.mask.width as f32 / glyph.width - 0.5;
                    let coverage = sample(&glyph.mask, sx, sy);
                    if coverage == 0 {
                        continue;
                    }
                    let offset = y as usize * pitch_bytes as usize + x as usize * 4;
                    blend(&mut pixels[offset..offset + 4], glyph.rgba, coverage);
                    composed += 1;
                }
            }
        }
        let elapsed = now_us().saturating_sub(started);
        {
            let mut cache = CACHE.lock();
            cache.stats.compose_us += elapsed;
            cache.stats.composed_pixels += composed;
        }
        crate::log_important!(target: "hull-font";
            "hull-font: paint glyphs={} missing={} pixels={} compose_us={}\n",
            self.glyphs.len(), self.missing, composed, elapsed);
        Ok(())
    }
}

// Exclusive right/bottom edges are shared by bounds reporting and composition,
// so fractional placement and clipping cannot drift between the two paths.
fn glyph_paint_bounds(
    glyph: &PaintedGlyph,
    width: u32,
    height: u32,
) -> Option<(u32, u32, u32, u32)> {
    if glyph.width <= 0.0 || glyph.rgba[3] == 0 {
        return None;
    }
    let left = libm::floorf(glyph.x).max(0.0).min(width as f32) as u32;
    let top = libm::floorf(glyph.y).max(0.0).min(height as f32) as u32;
    let right = libm::ceilf(glyph.x + glyph.width)
        .max(0.0)
        .min(width as f32) as u32;
    let bottom = libm::ceilf(glyph.y + glyph.mask.height as f32)
        .max(0.0)
        .min(height as f32) as u32;
    (left < right && top < bottom).then_some((left, top, right, bottom))
}

fn sample(mask: &Mask, x: f32, y: f32) -> u8 {
    let ix = libm::floorf(x) as i32;
    let iy = libm::floorf(y) as i32;
    let fx = x - ix as f32;
    let fy = y - iy as f32;
    let at = |x: i32, y: i32| -> f32 {
        if x < 0 || y < 0 || x >= mask.width as i32 || y >= mask.height as i32 {
            0.0
        } else {
            mask.pixels[y as usize * mask.width as usize + x as usize] as f32
        }
    };
    let upper = at(ix, iy) * (1.0 - fx) + at(ix + 1, iy) * fx;
    let lower = at(ix, iy + 1) * (1.0 - fx) + at(ix + 1, iy + 1) * fx;
    (upper * (1.0 - fy) + lower * fy + 0.5) as u8
}

fn blend(destination: &mut [u8], color: [u8; 4], coverage: u8) {
    let alpha = (u32::from(coverage) * u32::from(color[3]) + 127) / 255;
    for channel in 0..3 {
        destination[channel] = ((u32::from(color[channel]) * alpha
            + u32::from(destination[channel]) * (255 - alpha)
            + 127)
            / 255) as u8;
    }
    destination[3] = (alpha + (u32::from(destination[3]) * (255 - alpha) + 127) / 255) as u8;
}

fn supported(character: char) -> bool {
    character == ' '
        || microfont::raster_line::glyph_strips(character).is_some()
        || microfont::raster_quad::glyph_quads(character).is_some()
}

fn raster_glyph(character: char, height: u32) -> Result<Mask, &'static str> {
    if height == 0 || height > MAX_EM_HEIGHT {
        return Err("hull-font-glyph-size");
    }
    let width = (height * 6).div_ceil(11).max(1);
    let mut mask = Mask {
        width,
        height,
        pixels: alloc::vec![0; width as usize * height as usize],
    };
    if character == ' ' {
        return Ok(mask);
    }
    let scale = height as f32 / 11.0;
    if let Some(strips) = microfont::raster_line::glyph_strips(character) {
        // Relative source-cell weight: native round-ended line geometry stays
        // legible at small sizes without accumulating bitmap stair steps.
        let radius = (scale * 0.40).max(0.375);
        for strip in strips {
            for segment in 0..strip.segments() {
                let a = strip.point(segment).unwrap();
                let b = strip.point(segment + 1).unwrap();
                raster_segment(
                    &mut mask,
                    [(a.x() as f32 + 0.5) * scale, (a.y() as f32 + 0.5) * scale],
                    [(b.x() as f32 + 0.5) * scale, (b.y() as f32 + 0.5) * scale],
                    radius,
                );
            }
        }
        return Ok(mask);
    }
    if let Some(quads) = microfont::raster_quad::glyph_quads_at_size(character, height,
        crate::ui4::physical_extent_pixels(10, 10).map(|(_, h)| h)) {
        for quad in quads {
            raster_quad(
                &mut mask,
                quad.vertices
                    .map(|point| [point[0] * scale, point[1] * scale]),
            );
        }
        return Ok(mask);
    }
    Err("hull-font-missing-glyph")
}

fn raster_segment(mask: &mut Mask, a: [f32; 2], b: [f32; 2], radius: f32) {
    let left = libm::floorf(a[0].min(b[0]) - radius - 0.5).max(0.0) as u32;
    let top = libm::floorf(a[1].min(b[1]) - radius - 0.5).max(0.0) as u32;
    let right = libm::ceilf(a[0].max(b[0]) + radius + 0.5).min(mask.width as f32) as u32;
    let bottom = libm::ceilf(a[1].max(b[1]) + radius + 0.5).min(mask.height as f32) as u32;
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let length_squared = dx * dx + dy * dy;
    for y in top..bottom {
        for x in left..right {
            let px = x as f32 + 0.5 - a[0];
            let py = y as f32 + 0.5 - a[1];
            let t = if length_squared > 0.0 {
                ((px * dx + py * dy) / length_squared).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let distance =
                libm::sqrtf((px - t * dx) * (px - t * dx) + (py - t * dy) * (py - t * dy));
            let coverage = ((radius + 0.5 - distance).clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            let at = y as usize * mask.width as usize + x as usize;
            mask.pixels[at] = mask.pixels[at].max(coverage);
        }
    }
}

fn raster_quad(mask: &mut Mask, quad: [[f32; 2]; 4]) {
    let left =
        libm::floorf(quad.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min)).max(0.0) as u32;
    let top = libm::floorf(quad.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min)).max(0.0) as u32;
    let right = libm::ceilf(quad.iter().map(|p| p[0]).fold(f32::NEG_INFINITY, f32::max))
        .min(mask.width as f32) as u32;
    let bottom = libm::ceilf(quad.iter().map(|p| p[1]).fold(f32::NEG_INFINITY, f32::max))
        .min(mask.height as f32) as u32;
    // Four-by-four area samples preserve continuous diagonal ribbons and
    // fractional block edges without deriving any pixels from HEX masks.
    for y in top..bottom {
        for x in left..right {
            let mut count = 0u32;
            for sy in 0..4 {
                for sx in 0..4 {
                    let at = [
                        x as f32 + (sx as f32 + 0.5) * 0.25,
                        y as f32 + (sy as f32 + 0.5) * 0.25,
                    ];
                    let mut positive = false;
                    let mut negative = false;
                    for i in 0..4 {
                        let a = quad[i];
                        let b = quad[(i + 1) % 4];
                        let cross = (b[0] - a[0]) * (at[1] - a[1]) - (b[1] - a[1]) * (at[0] - a[0]);
                        positive |= cross > 0.0;
                        negative |= cross < 0.0;
                    }
                    count += u32::from(!(positive && negative));
                }
            }
            let coverage = ((count * 255 + 8) / 16) as u8;
            let at = y as usize * mask.width as usize + x as usize;
            mask.pixels[at] = mask.pixels[at].max(coverage);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rounded_box_corners_render_from_quads_at_small_and_large_sizes() {
        for ch in ['╭','╮','╯','╰'] {
            assert!(super::supported(ch));
            for height in [19,38,152] {
                let mask=super::raster_glyph(ch,height).unwrap();
                assert_eq!(mask.width,(height*6).div_ceil(11));
                assert_eq!(mask.height,height);
                assert!(mask.pixels.iter().any(|&p|p!=0));
                assert!(mask.pixels.iter().any(|&p|p==0));
            }
        }
    }
    use super::*;
    #[test]
    fn physical_steps_round_only_the_final_extent_and_never_enlarge() {
        let resolve = |step| Some((step * 25 + 3) / 6);
        assert_eq!(lower_step_height(12.9, resolve), 8);
        assert_eq!(lower_step_height(13.0, resolve), 13);
        assert_eq!(lower_step_height(3.9, resolve), 3);
        assert_eq!(lower_step_height(17.9, |_| None), 17);
        for size in 1..400 {
            assert!(lower_step_height(size as f32, resolve) <= size);
        }
    }
    #[test]
    fn geometry_masks_have_coverage_and_missing_characters_are_absent() {
        let line = raster_glyph('A', 23).unwrap();
        assert!(line.pixels.iter().any(|&value| value == 255));
        assert!(line.pixels.iter().any(|&value| value > 0 && value < 255));
        assert!(
            raster_glyph(' ', 23)
                .unwrap()
                .pixels
                .iter()
                .all(|&value| value == 0)
        );
        assert!(raster_glyph('\u{10ffff}', 23).is_err());
        assert!(!supported('\u{10ffff}'));
        assert!(supported('é'));
        assert!(supported('█'));
        assert!(
            raster_glyph('█', 22)
                .unwrap()
                .pixels
                .iter()
                .all(|&value| value == 255)
        );
    }
    #[test]
    fn standalone_quad_symbols_render_without_missing_glyph_fallback() {
        for (character, top, bottom) in [('▬', 4, 7), ('■', 2, 7)] {
            assert!(supported(character));
            assert!(microfont::raster_line::glyph_strips(character).is_none());
            assert_eq!(microfont::raster_quad::glyph_quads(character).unwrap().len(), 1);
            let mask = raster_glyph(character, 11).unwrap();
            assert_eq!((mask.width, mask.height), (6, 11));
            for (y, row) in mask.pixels.chunks_exact(6).enumerate() {
                let expected = if (top..bottom).contains(&y) { 255 } else { 0 };
                assert!(row.iter().all(|&coverage| coverage == expected));
            }
        }
    }
    #[test]
    fn byte_budget_evicts_old_masks_without_invalidating_prepared_readers() {
        let mut cache = Cache::new();
        cache.stats.budget_bytes = 3 * (16 + ENTRY_BYTES);
        let mask = || {
            Arc::new(Mask {
                width: 4,
                height: 4,
                pixels: alloc::vec![91; 16],
            })
        };
        let first = cache
            .publish(
                Key {
                    height: 4,
                    character: 'a',
                },
                mask(),
            )
            .0;
        cache.publish(
            Key {
                height: 4,
                character: 'b',
            },
            mask(),
        );
        cache.publish(
            Key {
                height: 4,
                character: 'c',
            },
            mask(),
        );
        cache.touch(Key {
            height: 4,
            character: 'a',
        });
        cache.publish(
            Key {
                height: 4,
                character: 'd',
            },
            mask(),
        );
        assert!(!cache.entries.contains_key(&Key {
            height: 4,
            character: 'b'
        }));
        assert_eq!(cache.stats.evictions, 1);
        assert!(cache.stats.cache_bytes <= cache.stats.budget_bytes);
        cache.entries.clear();
        assert_eq!(first.pixels, alloc::vec![91; 16]);
    }
    #[test]
    fn duplicate_publication_reuses_immutable_mask() {
        let mut cache = Cache::new();
        let key = Key {
            height: 4,
            character: '?',
        };
        let first = Arc::new(Mask {
            width: 1,
            height: 1,
            pixels: alloc::vec![17],
        });
        let second = Arc::new(Mask {
            width: 1,
            height: 1,
            pixels: alloc::vec![99],
        });
        cache.publish(key, Arc::clone(&first));
        let (winner, duplicate) = cache.publish(key, second);
        assert!(duplicate);
        assert!(Arc::ptr_eq(&winner, &first));
        assert_eq!(cache.stats.cache_bytes, 1 + ENTRY_BYTES);
    }
    #[test]
    fn prepared_masks_are_bounded_even_when_evicted_arcs_stay_alive() {
        let mut retained = BTreeMap::new();
        let mut bytes = 0;
        let budget = 10_000;
        let key = Key {
            height: 100,
            character: 'A',
        };
        retain_prepared_mask(&mut retained, &mut bytes, key, budget, || {
            Ok(Arc::new(raster_glyph('A', 100).unwrap()))
        })
        .unwrap();
        let first = bytes;
        retain_prepared_mask(&mut retained, &mut bytes, key, budget, || {
            panic!("repeated key must use its retained mask")
        })
        .unwrap();
        assert_eq!(bytes, first);
        assert!(
            retain_prepared_mask(
                &mut retained,
                &mut bytes,
                Key {
                    height: 100,
                    character: 'B'
                },
                budget,
                || panic!("budget must be checked before loading")
            )
            .is_err()
        );
        assert_eq!(bytes, first);
        assert_eq!(retained.len(), 1);
    }
    #[test]
    fn repeated_glyph_reuses_charged_arc_after_global_eviction_and_replacement() {
        let key = Key {
            height: 11,
            character: 'A',
        };
        let other_key = Key {
            height: 11,
            character: 'B',
        };
        let mask = |value| {
            Arc::new(Mask {
                width: 6,
                height: 11,
                pixels: alloc::vec![value; 66],
            })
        };
        let mut shared = Cache::new();
        let old = mask(17);
        shared.stats.budget_bytes = mask_cost(&old);
        shared.publish(key, Arc::clone(&old));
        let mut retained = BTreeMap::new();
        let mut bytes = 0;
        let budget = shared.stats.budget_bytes;
        let (first, local_hit) =
            retain_prepared_mask(&mut retained, &mut bytes, key, budget, || {
                Ok(shared.touch(key).unwrap())
            })
            .unwrap();
        assert!(!local_hit);
        let charged = bytes;
        // The competing request evicts A and then republishes a different
        // allocation for A. The first request must never retain that second
        // allocation while charging only its original key once.
        shared.publish(other_key, mask(33));
        assert!(!shared.entries.contains_key(&key));
        shared.publish(key, mask(99));
        let replacement = shared.touch(key).unwrap();
        assert!(!Arc::ptr_eq(&old, &replacement));
        let mut consulted_shared = false;
        let (repeated, local_hit) =
            retain_prepared_mask(&mut retained, &mut bytes, key, budget, || {
                consulted_shared = true;
                Ok(shared.touch(key).unwrap())
            })
            .unwrap();
        assert!(local_hit);
        assert!(!consulted_shared);
        assert!(Arc::ptr_eq(&first, &repeated));
        assert_eq!(repeated.pixels[0], 17);
        assert_eq!(bytes, charged);
        assert_eq!(retained.len(), 1);
    }
    #[test]
    fn invisible_text_does_not_prepare_any_glyphs() {
        let run = |x, baseline_y, rgba| HullTextRun {
            text: String::from("ABC"),
            x,
            baseline_y,
            size_px: 100.0,
            advance_px: 120.0,
            rgba,
        };
        let invisible = prepare_blocking(
            alloc::vec![
                run(10.0, 100.0, 0x00ffffff),
                run(500.0, 100.0, 0xffffffff),
                run(-200.0, 100.0, 0xffffffff),
                run(10.0, -200.0, 0xffffffff),
                run(10.0, 800.0, 0xffffffff),
            ],
            200,
            200,
        )
        .unwrap();
        assert!(invisible.glyphs.is_empty());
        assert_eq!(invisible.missing, 0);
    }
    #[test]
    fn concurrent_hulls_share_masks_and_missing_glyphs_render_red() {
        let barrier = Arc::new(std::sync::Barrier::new(4));
        let threads = (0..4)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    barrier.wait();
                    let prepared = prepare_blocking(
                        alloc::vec![HullTextRun {
                            text: String::from("A?\u{10ffff}"),
                            x: 10.0,
                            baseline_y: 95.0,
                            size_px: 80.0,
                            advance_px: 120.0,
                            rgba: u32::from_le_bytes([0, 255, 0, 255]),
                        }],
                        200,
                        120,
                    )
                    .unwrap();
                    assert_eq!(prepared.glyphs.len(), 3);
                    assert_eq!(prepared.missing, 1);
                    assert!(Arc::ptr_eq(&prepared.glyphs[1].mask, &prepared.glyphs[2].mask));
                    let mut pixels = alloc::vec![0; 200 * 120 * 4];
                    prepared.paint(&mut pixels, 200, 120, 200 * 4).unwrap();
                    assert!(
                        pixels
                            .chunks_exact(4)
                            .any(|pixel| pixel[0] > 0 && pixel[1] == 0)
                    );
                    assert!(
                        pixels
                            .chunks_exact(4)
                            .any(|pixel| pixel[1] > 0 && pixel[0] == 0)
                    );
                    (prepared, pixels)
                })
            })
            .collect::<Vec<_>>();
        let prepared = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        for result in &prepared[1..] {
            assert!(Arc::ptr_eq(&prepared[0].0.glyphs[0].mask, &result.0.glyphs[0].mask));
            assert_eq!(prepared[0].1, result.1);
        }
        let before = stats();
        let _ = prepare_blocking(
            alloc::vec![HullTextRun {
                text: String::from("A?"),
                x: 10.0,
                baseline_y: 95.0,
                size_px: 80.0,
                advance_px: 80.0,
                rgba: 0xffffffff,
            }],
            200,
            120,
        )
        .unwrap();
        let after = stats();
        assert_eq!(after.rasterized, before.rasterized);
        assert!(after.hits >= before.hits + 2);
    }
    #[test]
    fn painting_clips_and_preserves_padding_and_background() {
        let mask = Arc::new(Mask {
            width: 2,
            height: 2,
            pixels: alloc::vec![255; 4],
        });
        let text = PreparedText {
            glyphs: alloc::vec![PaintedGlyph {
                mask,
                x: -1.0,
                y: 1.0,
                width: 2.0,
                rgba: [255, 0, 0, 255]
            }],
            missing: 0,
        };
        assert_eq!(text.paint_bounds(3, 3), Some((0, 1, 1, 2)));
        let mut frame = alloc::vec![25; 3 * 16];
        text.paint(&mut frame, 3, 3, 16).unwrap();
        assert_eq!(&frame[16..20], &[255, 0, 0, 255]);
        assert_eq!(&frame[..16], &[25; 16]);
        assert_eq!(&frame[28..32], &[25; 4]);
        assert_eq!(&frame[44..48], &[25; 4]);
        assert!(text.paint(&mut frame[..20], 3, 3, 16).is_err());
    }
    #[test]
    fn fractional_clipped_paint_bounds_contain_every_modified_pixel_and_no_padding() {
        let mask = Arc::new(Mask {
            width: 2,
            height: 2,
            pixels: alloc::vec![255; 4],
        });
        let glyph = |x, y, width, alpha| PaintedGlyph {
            mask: Arc::clone(&mask),
            x,
            y,
            width,
            rgba: [255, 0, 0, alpha],
        };
        let text = PreparedText {
            glyphs: alloc::vec![
                glyph(-1.25, 2.5, 2.5, 255),
                glyph(6.25, 4.5, 2.5, 255),
                glyph(9.0, 7.0, 1.0, 0),
            ],
            missing: 0,
        };
        let bounds = text.paint_bounds(10, 8).unwrap();
        assert_eq!(bounds, (0, 2, 9, 5));
        let pitch = 10 * 4 + 9;
        let mut frame = alloc::vec![25; pitch * 8];
        text.paint(&mut frame, 10, 8, pitch as u32).unwrap();
        let (left, top, width, height) = bounds;
        let mut modified = 0;
        for y in 0..8 {
            for x in 0..10 {
                let at = y * pitch + x * 4;
                if frame[at..at + 4] != [25; 4] {
                    modified += 1;
                    assert!(x as u32 >= left && (x as u32) < left + width);
                    assert!(y as u32 >= top && (y as u32) < top + height);
                }
            }
            assert_eq!(&frame[y * pitch + 40..(y + 1) * pitch], &[25; 9]);
        }
        assert!(modified > 0);
    }
    #[test]
    fn paint_bounds_are_empty_for_empty_transparent_and_offscreen_text() {
        let mut text = PreparedText {
            glyphs: Vec::new(),
            missing: 0,
        };
        assert_eq!(text.paint_bounds(10, 8), None);
        let mask = Arc::new(Mask {
            width: 2,
            height: 2,
            pixels: alloc::vec![255; 4],
        });
        for (x, y, width, alpha) in [
            (1.5, 2.5, 2.0, 0),
            (10.0, 2.0, 2.0, 255),
            (-3.0, 2.0, 2.0, 255),
            (1.0, 8.0, 2.0, 255),
            (1.0, -2.0, 2.0, 255),
            (1.0, 2.0, 0.0, 255),
        ] {
            text.glyphs.push(PaintedGlyph {
                mask: Arc::clone(&mask),
                x,
                y,
                width,
                rgba: [255, 0, 0, alpha],
            });
        }
        assert_eq!(text.paint_bounds(10, 8), None);
        let mut frame = alloc::vec![25; 40 * 8];
        text.paint(&mut frame, 10, 8, 40).unwrap();
        assert!(frame.iter().all(|&byte| byte == 25));
        text.glyphs.push(PaintedGlyph {
            mask,
            x: 1.0,
            y: 1.0,
            width: 2.0,
            rgba: [255, 0, 0, 255],
        });
        assert_eq!(text.paint_bounds(0, 8), None);
        assert_eq!(text.paint_bounds(10, 0), None);
    }
    #[test]
    fn source_over_keeps_rgba_premultiplied() {
        let mut pixel = [0, 0, 0, 0];
        blend(&mut pixel, [255, 80, 0, 128], 255);
        assert_eq!(pixel, [128, 40, 0, 128]);
        blend(&mut pixel, [0, 0, 255, 255], 255);
        assert_eq!(pixel, [0, 0, 255, 255]);
    }
}
