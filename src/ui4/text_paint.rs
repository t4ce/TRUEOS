//! Ordered text paint work for retired, exclusively owned RGBA destinations.
//! Uniform cells become contiguous fills; mixed masks keep mono expansion.
use crate::intel::{GucBcs0MonoGlyph, GucBcs0RgbaFill, GucBcs0RgbaSurface};
use alloc::vec::Vec;
use trueos_time::{Duration, Timer};

pub(crate) enum PaintRun {
    Mono(Vec<GucBcs0MonoGlyph>),
    Fill(Vec<GucBcs0RgbaFill>),
}
pub(crate) struct PaintPlan {
    runs: Vec<PaintRun>,
    glyphs: usize,
    solid_cells: usize,
}
#[derive(Default)]
pub(crate) struct PaintStats {
    pub cpu_pixels: usize,
    pub gpu_pixels: usize,
    pub mono_batches: usize,
    pub fill_batches: usize,
}
impl PaintPlan {
    pub fn new(glyphs: Vec<GucBcs0MonoGlyph>) -> Self {
        let mut plan = Self {
            runs: Vec::new(),
            glyphs: glyphs.len(),
            solid_cells: 0,
        };
        for glyph in glyphs {
            if let Some(color) = solid_color(&glyph) {
                plan.solid_cells += 1;
                if !matches!(plan.runs.last(), Some(PaintRun::Fill(_))) {
                    plan.runs.push(PaintRun::Fill(Vec::new()));
                }
                let Some(PaintRun::Fill(fills)) = plan.runs.last_mut() else {
                    unreachable!()
                };
                if let Some(last) = fills.last_mut()
                    && last.color == color
                    && last.y == glyph.y
                    && last.height == glyph.height
                    && last.x.checked_add(last.width) == Some(glyph.x)
                    && let Some(width) = last.width.checked_add(glyph.width)
                {
                    last.width = width;
                } else {
                    fills.push(GucBcs0RgbaFill {
                        x: glyph.x,
                        y: glyph.y,
                        width: glyph.width,
                        height: glyph.height,
                        color,
                    });
                }
            } else {
                if !matches!(plan.runs.last(), Some(PaintRun::Mono(_))) {
                    plan.runs.push(PaintRun::Mono(Vec::new()));
                }
                let Some(PaintRun::Mono(cells)) = plan.runs.last_mut() else {
                    unreachable!()
                };
                cells.push(glyph);
            }
        }
        plan
    }
    /// Avoid turning fragmented text into more submissions than plain mono.
    /// The unoptimised path can consume the same plan without reconstructing masks.
    fn cpu_budget(&self, surface: GucBcs0RgbaSurface) -> usize {
        let height = self.runs.first().map_or(0, |run| match run {
            PaintRun::Mono(g) => g.first().map_or(0, |g| g.height),
            PaintRun::Fill(f) => f.first().map_or(0, |f| f.height),
        });
        // Splitting work cannot multiply the measured CPU allowance.
        crate::allcaps::text_blit::CPU_MONO_MAX_PIXELS
            .min(crate::allcaps::text_blit::CPU_COPY_MAX_PIXELS)
            .min(surface.width as usize * height as usize / 2)
    }
    fn batches(&self, surface: GucBcs0RgbaSurface) -> usize {
        let mut remaining = self.cpu_budget(surface);
        self.runs
            .iter()
            .map(|run| {
                if use_cpu(surface, run, &mut remaining) {
                    return 0;
                }
                match run {
                    PaintRun::Mono(g) => g.len().div_ceil(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS),
                    PaintRun::Fill(f) => f.len().div_ceil(128),
                }
            })
            .sum()
    }
    /// Caller retains exclusive ownership until all submitted writes retire.
    /// On an uncertain GPU failure, `poisoned` remains set, as in Shell3's
    /// existing path. A retry after safe rejection must repaint the frame.
    pub async fn paint(
        &self,
        surface: GucBcs0RgbaSurface,
        poisoned: &mut bool,
    ) -> Result<PaintStats, &'static str> {
        if *poisoned {
            return Err("text-paint-destination-pinned");
        }
        for run in &self.runs {
            let valid = match run {
                PaintRun::Mono(g) => super::text_blit::valid_mono(surface, g),
                PaintRun::Fill(f) => super::text_blit::valid_fills(surface, f),
            };
            if !valid {
                return Err("text-paint-bounds");
            }
        }
        let mut stats = PaintStats::default();
        // Retain the original single mono stream if splitting it would cost
        // more GPU submissions. Re-expanded fills have no font lookup cost.
        let baseline_batches = self.glyphs.div_ceil(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS);
        if self.solid_cells != 0 && self.batches(surface) > baseline_batches {
            let glyphs = self.as_mono();
            if !super::text_blit::mono_on_cpu(surface, &glyphs) {
                prepare_gpu(surface);
            }
            paint_mono(surface, &glyphs, true, poisoned, &mut stats).await?;
        } else {
            if self.batches(surface) != 0 {
                prepare_gpu(surface);
            }
            let mut remaining = self.cpu_budget(surface);
            for run in &self.runs {
                let cpu = use_cpu(surface, run, &mut remaining);
                match run {
                    PaintRun::Mono(g) => paint_mono(surface, g, cpu, poisoned, &mut stats).await?,
                    PaintRun::Fill(f) => {
                        let pixels = super::text_blit::fill_pixels(f);
                        if cpu && unsafe { super::text_blit::try_fill(surface, f) } {
                            stats.cpu_pixels += pixels;
                        } else {
                            for chunk in f.chunks(128) {
                                retire(
                                    crate::intel::queue_guc_bcs0_rgba_fills(surface, chunk),
                                    poisoned,
                                )
                                .await?;
                                stats.fill_batches += 1;
                            }
                            stats.gpu_pixels += pixels;
                        }
                    }
                }
            }
        }
        Ok(stats)
    }
    fn as_mono(&self) -> Vec<GucBcs0MonoGlyph> {
        let mut output = Vec::with_capacity(self.glyphs);
        for run in &self.runs {
            match run {
                PaintRun::Mono(g) => output.extend_from_slice(g),
                PaintRun::Fill(f) => {
                    for fill in f {
                        // Merged fills may exceed mono width; split into <=16px tiles.
                        let mut x = fill.x;
                        let end = fill.x + fill.width;
                        while x < end {
                            let width = (end - x).min(16);
                            output.push(GucBcs0MonoGlyph {
                                x,
                                y: fill.y,
                                width,
                                height: fill.height,
                                mask: [0; 64],
                                foreground: fill.color,
                                background: fill.color,
                            });
                            x += width;
                        }
                    }
                }
            }
        }
        output
    }
}
fn solid_color(g: &GucBcs0MonoGlyph) -> Option<u32> {
    if g.width == 0 || g.width > 16 || g.height == 0 || g.height > 32 {
        return None;
    }
    if g.foreground == g.background {
        return Some(g.background);
    }
    let mask = u16::MAX << (16 - g.width);
    let first = u16::from_be_bytes([g.mask[0], g.mask[1]]) & mask;
    if first != 0 && first != mask {
        return None;
    }
    if (1..g.height as usize)
        .any(|y| u16::from_be_bytes([g.mask[y * 2], g.mask[y * 2 + 1]]) & mask != first)
    {
        return None;
    }
    Some(if first == 0 {
        g.background
    } else {
        g.foreground
    })
}
async fn paint_mono(
    surface: GucBcs0RgbaSurface,
    glyphs: &[GucBcs0MonoGlyph],
    allow_cpu: bool,
    poisoned: &mut bool,
    stats: &mut PaintStats,
) -> Result<(), &'static str> {
    if glyphs.is_empty() {
        return Ok(());
    }
    let pixels = super::text_blit::mono_pixels(glyphs);
    if allow_cpu && unsafe { super::text_blit::try_mono(surface, glyphs) } {
        stats.cpu_pixels += pixels;
    } else {
        for chunk in glyphs.chunks(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS) {
            retire(crate::intel::queue_guc_bcs0_mono_glyphs(surface, chunk), poisoned).await?;
            stats.mono_batches += 1;
        }
        stats.gpu_pixels += pixels;
    }
    Ok(())
}
async fn retire(
    result: Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError>,
    poisoned: &mut bool,
) -> Result<(), &'static str> {
    let submission = match result {
        Ok(s) => s,
        Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
            *poisoned = true;
            return Err("text-paint-submit-uncertain");
        }
        Err(_) => return Err("text-paint-unavailable"),
    };
    *poisoned = true;
    loop {
        match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
            crate::intel::GucBcs0CopyCompletion::Pending => {
                Timer::after(Duration::from_millis(1)).await
            }
            crate::intel::GucBcs0CopyCompletion::Complete => {
                *poisoned = false;
                return Ok(());
            }
            _ => return Err("text-paint-retirement-uncertain"),
        }
    }
}

fn prepare_gpu(surface: GucBcs0RgbaSurface) {
    crate::intel::dma_cache_flush_range(
        crate::phys::phys_to_virt(surface.phys as usize) as *const u8,
        surface.bytes,
    );
}

fn use_cpu(surface: GucBcs0RgbaSurface, run: &PaintRun, remaining: &mut usize) -> bool {
    let (pixels, eligible) = match run {
        PaintRun::Mono(g) => {
            (super::text_blit::mono_pixels(g), super::text_blit::mono_on_cpu(surface, g))
        }
        PaintRun::Fill(f) => {
            (super::text_blit::fill_pixels(f), super::text_blit::fill_on_cpu(surface, f))
        }
    };
    if eligible && pixels <= *remaining {
        *remaining -= pixels;
        true
    } else {
        false
    }
}
