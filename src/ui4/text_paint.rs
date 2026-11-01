//! Ordered text paint work for retired, exclusively owned RGBA destinations.
//! CPU work keeps its measured allowance; consecutive GPU work shares BCS plans.
use crate::intel::bcs_plan::{BCS_PLAN_ENVELOPE_DWORDS, BcsOp, BcsPlan, CopyBackend, SourceCache};
use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaFill, GucBcs0RgbaSurface};
use alloc::vec::Vec;
use trueos_time::{Duration, Timer};

#[derive(Default)]
pub(crate) struct PaintStats {
    pub cpu_pixels: usize,
    pub gpu_pixels: usize,
    /// A mixed batch contributes to both family counts, but only one bcs_batch.
    pub copy_batches: usize,
    pub fill_batches: usize,
    pub bcs_batches: usize,
    pub explicit_flushes: usize,
    pub source_mappings: usize,
    pub encoded_bytes: usize,
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
    // This assignment precedes the first possible suspension. Cancellation
    // therefore retains the frame/cache backing while hardware may own it.
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

/// A CPU step is an ownership boundary: preceding GPU writes must retire
/// before its stores, and those stores must be published before later GPU work.
enum PaintStep<'a> {
    Cpu(&'a RgbaPaintRun),
    SourceCopies(&'a [GucBcs0RgbaCopy]),
    Gpu(BcsPlan),
}
enum RgbaPaintRun {
    Copy(Vec<GucBcs0RgbaCopy>),
    Fill(Vec<GucBcs0RgbaFill>),
}
impl RgbaPaintRun {
    fn pixels(&self) -> usize {
        match self {
            Self::Copy(copies) => super::text_blit::copy_pixels(copies),
            Self::Fill(fills) => super::text_blit::fill_pixels(fills),
        }
    }
    fn append_gpu(&self, operations: &mut Vec<BcsOp>) {
        match self {
            Self::Copy(copies) => append_glyph_copies(operations, copies),
            Self::Fill(fills) => operations.extend(fills.iter().copied().map(BcsOp::Fill)),
        }
    }
    unsafe fn try_cpu(&self, surface: GucBcs0RgbaSurface) -> bool {
        match self {
            Self::Copy(copies) => unsafe { super::text_blit::try_glyph_copy(surface, copies) },
            Self::Fill(fills) => unsafe { super::text_blit::try_fill(surface, fills) },
        }
    }
}

/// Warm glyph pixels and solid cells retain their original painter order.
/// Coverage and color have already been baked into immutable source cells.
pub(crate) struct RgbaPaintPlan {
    runs: Vec<RgbaPaintRun>,
    copies: Vec<GucBcs0RgbaCopy>,
}
impl RgbaPaintPlan {
    pub fn new(
        copies: Vec<GucBcs0RgbaCopy>,
        mut solid: impl FnMut(&GucBcs0RgbaCopy) -> Option<u32>,
    ) -> Self {
        let mut runs = Vec::new();
        for &copy in &copies {
            if let Some(color) = solid(&copy) {
                if !matches!(runs.last(), Some(RgbaPaintRun::Fill(_))) {
                    runs.push(RgbaPaintRun::Fill(Vec::new()));
                }
                let Some(RgbaPaintRun::Fill(fills)) = runs.last_mut() else {
                    unreachable!()
                };
                if let Some(last) = fills.last_mut()
                    && last.color == color
                    && last.y == copy.destination_y
                    && last.height == copy.height
                    && last.x.checked_add(last.width) == Some(copy.destination_x)
                    && let Some(width) = last.width.checked_add(copy.width)
                {
                    last.width = width;
                } else {
                    fills.push(GucBcs0RgbaFill {
                        x: copy.destination_x,
                        y: copy.destination_y,
                        width: copy.width,
                        height: copy.height,
                        color,
                    });
                }
            } else {
                if !matches!(runs.last(), Some(RgbaPaintRun::Copy(_))) {
                    runs.push(RgbaPaintRun::Copy(Vec::new()));
                }
                let Some(RgbaPaintRun::Copy(glyphs)) = runs.last_mut() else {
                    unreachable!()
                };
                glyphs.push(copy);
            }
        }
        Self { runs, copies }
    }
    fn cpu_budget(&self, surface: GucBcs0RgbaSurface) -> usize {
        let height = self.copies.first().map_or(0, |copy| copy.height);
        crate::allcaps::text_blit::CPU_MONO_MAX_PIXELS
            .min(crate::allcaps::text_blit::CPU_COPY_MAX_PIXELS)
            .min(surface.width as usize * height as usize / 2)
    }
    fn fragmented_fills(&self, surface: GucBcs0RgbaSurface) -> bool {
        // Retain the existing workload policy: converting solid cells must not
        // cost more GPU run chunks than copying the original glyph stream.
        // A mixed hardware batch still pays an implied flush at each family
        // transition, even after the repeated host submissions are removed.
        let mut remaining = self.cpu_budget(surface);
        let gpu_chunks: usize = self
            .runs
            .iter()
            .map(|run| {
                if use_rgba_cpu(surface, run, &mut remaining) {
                    0
                } else {
                    match run {
                        RgbaPaintRun::Copy(copies) => copies.len().div_ceil(128),
                        RgbaPaintRun::Fill(fills) => fills.len().div_ceil(128),
                    }
                }
            })
            .sum();
        self.runs
            .iter()
            .any(|run| matches!(run, RgbaPaintRun::Fill(_)))
            && gpu_chunks > self.copies.len().div_ceil(128)
    }
    fn steps(
        &self,
        surface: GucBcs0RgbaSurface,
        initial_clear: Option<u32>,
    ) -> Result<Vec<PaintStep<'_>>, &'static str> {
        let mut steps = Vec::new();
        let mut operations = Vec::new();
        if let Some(color) = initial_clear {
            operations.push(BcsOp::Fill(full_clear(surface, color)));
        }
        if self.fragmented_fills(surface) {
            if super::text_blit::glyph_copy_on_cpu(surface, &self.copies) {
                push_gpu_step(&mut steps, surface, &mut operations)?;
                steps.push(PaintStep::SourceCopies(&self.copies));
            } else {
                append_glyph_copies(&mut operations, &self.copies);
                push_gpu_step(&mut steps, surface, &mut operations)?;
            }
            return Ok(steps);
        }
        let mut remaining = self.cpu_budget(surface);
        for run in &self.runs {
            if use_rgba_cpu(surface, run, &mut remaining) {
                push_gpu_step(&mut steps, surface, &mut operations)?;
                steps.push(PaintStep::Cpu(run));
            } else {
                run.append_gpu(&mut operations);
            }
        }
        push_gpu_step(&mut steps, surface, &mut operations)?;
        Ok(steps)
    }
    pub async fn paint(
        &self,
        surface: GucBcs0RgbaSurface,
        initial_clear: Option<u32>,
        poisoned: &mut bool,
    ) -> Result<PaintStats, &'static str> {
        if *poisoned {
            return Err("text-paint-destination-pinned");
        }
        if !super::text_blit::valid_copies(surface, &self.copies) {
            return Err("text-paint-bounds");
        }
        let mut stats = PaintStats::default();
        // Emulator frames stay CPU-local until UI4's eventual window upload.
        if crate::virtio_gpu_logo::output_dimensions().is_some() {
            if let Some(color) = initial_clear {
                if !unsafe { super::text_blit::fill(surface, &[full_clear(surface, color)]) } {
                    return Err("text-paint-emulator-clear");
                }
                stats.cpu_pixels += surface.width as usize * surface.height as usize;
            }
            if !self.copies.is_empty() && !unsafe { super::text_blit::copy(surface, &self.copies) }
            {
                return Err("text-paint-emulator-copy");
            }
            stats.cpu_pixels += super::text_blit::copy_pixels(&self.copies);
            return Ok(stats);
        }
        // Validate and size every GPU group before the first pixel mutation.
        // A fill/copy family change no longer introduces a host submission.
        for step in self.steps(surface, initial_clear)? {
            match step {
                PaintStep::Gpu(plan) => execute_gpu(surface, &plan, poisoned, &mut stats).await?,
                PaintStep::SourceCopies(copies) => {
                    if unsafe { super::text_blit::try_glyph_copy(surface, copies) } {
                        stats.cpu_pixels += super::text_blit::copy_pixels(copies);
                    } else {
                        let mut operations = Vec::new();
                        append_glyph_copies(&mut operations, copies);
                        let plan = BcsPlan::new(surface, &operations)
                            .map_err(|_| "text-paint-plan-invalid")?;
                        execute_gpu(surface, &plan, poisoned, &mut stats).await?;
                    }
                }
                PaintStep::Cpu(run) => {
                    if unsafe { run.try_cpu(surface) } {
                        stats.cpu_pixels += run.pixels();
                    } else {
                        let mut operations = Vec::new();
                        run.append_gpu(&mut operations);
                        let plan = BcsPlan::new(surface, &operations)
                            .map_err(|_| "text-paint-plan-invalid")?;
                        execute_gpu(surface, &plan, poisoned, &mut stats).await?;
                    }
                }
            }
        }
        Ok(stats)
    }
}

fn append_glyph_copies(operations: &mut Vec<BcsOp>, copies: &[GucBcs0RgbaCopy]) {
    operations.extend(copies.iter().map(|&copy| BcsOp::Copy {
        copy,
        backend: CopyBackend::Legacy,
        source_cache: SourceCache::WriteBack,
    }));
}

fn full_clear(surface: GucBcs0RgbaSurface, color: u32) -> GucBcs0RgbaFill {
    GucBcs0RgbaFill {
        x: 0,
        y: 0,
        width: surface.width,
        height: surface.height,
        color,
    }
}
fn push_gpu_step<'a>(
    steps: &mut Vec<PaintStep<'a>>,
    surface: GucBcs0RgbaSurface,
    operations: &mut Vec<BcsOp>,
) -> Result<(), &'static str> {
    if !operations.is_empty() {
        let plan = BcsPlan::new(surface, operations).map_err(|_| "text-paint-plan-invalid")?;
        steps.push(PaintStep::Gpu(plan));
        operations.clear();
    }
    Ok(())
}
fn use_rgba_cpu(surface: GucBcs0RgbaSurface, run: &RgbaPaintRun, remaining: &mut usize) -> bool {
    let eligible = match run {
        RgbaPaintRun::Copy(copies) => super::text_blit::glyph_copy_on_cpu(surface, copies),
        RgbaPaintRun::Fill(fills) => super::text_blit::fill_on_cpu(surface, fills),
    };
    let pixels = run.pixels();
    if eligible && pixels <= *remaining {
        *remaining -= pixels;
        true
    } else {
        false
    }
}
async fn execute_gpu(
    surface: GucBcs0RgbaSurface,
    plan: &BcsPlan,
    poisoned: &mut bool,
    stats: &mut PaintStats,
) -> Result<(), &'static str> {
    // Each group follows only retired GPU work or synchronous CPU operations.
    // Publish the latter before BCS acquires the destination again.
    prepare_gpu(surface);
    for batch in plan.batches() {
        retire(crate::intel::queue_guc_bcs0_plan_batch(batch), poisoned).await?;
        stats.bcs_batches += 1;
        stats.copy_batches += usize::from(batch.copy_count() != 0);
        stats.fill_batches += usize::from(batch.fill_count() != 0);
        stats.explicit_flushes += batch.explicit_flushes();
        stats.source_mappings += batch.source_mappings().len();
        stats.encoded_bytes += (BCS_PLAN_ENVELOPE_DWORDS + batch.encoded_dwords()) * 4;
        stats.gpu_pixels += (batch.copied_bytes() / 4) as usize;
    }
    Ok(())
}

/// Shell3's direct cell path shares the planner without changing glyph pixels
/// or the existing small-update CPU threshold.
pub(crate) async fn paint_glyph_copies(
    surface: GucBcs0RgbaSurface,
    copies: Vec<GucBcs0RgbaCopy>,
    initial_clear: Option<u32>,
    poisoned: &mut bool,
) -> Result<PaintStats, &'static str> {
    RgbaPaintPlan::new(copies, |_| None)
        .paint(surface, initial_clear, poisoned)
        .await
}
