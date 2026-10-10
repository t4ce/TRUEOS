//! BCS0 raster adapter for text_area. All transfers have separate allocations;
//! backing is retained through retirement and quarantined on uncertain work.
use super::text_area::{
    CellCopy, CellRect, CellWrite, RasterBudget, RasterLayout, RasterReservation, TextArea,
};
use crate::intel::gpgpu::GpgpuOwnedRgba8Surface;
use crate::intel::{GucBcs0RgbaCopy, GucBcs0RgbaSurface};
use alloc::vec::Vec;
use trueos_time::{Duration, Timer};

pub(crate) struct BcsTextArea<C> {
    cells: TextArea<C>,
    backing: GpgpuOwnedRgba8Surface,
    reservation: Option<RasterReservation>,
    unretired: bool,
    resize_copies: usize,
}

impl<C> Drop for BcsTextArea<C> {
    fn drop(&mut self) {
        if self.unretired {
            self.backing.quarantine_backing();
            if let Some(reservation) = self.reservation.take() {
                core::mem::forget(reservation);
            }
        }
    }
}

pub(crate) struct RasterWork {
    pub produced: usize,
    pub reused: usize,
    pub painted: usize,
    pub mono_batches: usize,
    pub cpu_glyphs: usize,
}

impl<C: Clone + PartialEq> BcsTextArea<C> {
    pub fn new(layout: RasterLayout, budget: &RasterBudget) -> Option<Self> {
        let reservation = budget.reserve(layout.bytes)?;
        let backing =
            crate::intel::gpgpu::allocate_font_instance_rgba8_surface(layout.width, layout.height)?;
        if backing.surface().bytes != layout.bytes {
            return None;
        }
        Some(Self {
            cells: TextArea::new(layout),
            backing,
            reservation: Some(reservation),
            unretired: false,
            resize_copies: 0,
        })
    }
    pub fn layout(&self) -> RasterLayout {
        self.cells.layout()
    }
    pub fn last_resize_copies(&self) -> usize {
        self.resize_copies
    }
    pub fn invalidate(&mut self) {
        self.cells.invalidate();
    }
    fn quarantine(&mut self) {
        self.backing.quarantine_backing();
        // Quarantine deliberately keeps the physical allocation charged.
        if let Some(reservation) = self.reservation.take() {
            core::mem::forget(reservation);
        }
    }
    fn surface(&self) -> GucBcs0RgbaSurface {
        let s = self.backing.surface();
        GucBcs0RgbaSurface {
            phys: s.phys,
            gpu: s.gpu,
            bytes: s.bytes,
            width: s.width,
            height: s.height,
            pitch_bytes: s.pitch_bytes,
        }
    }

    /// False means two allocations would exceed the shared cap (or allocation
    /// failed). The owner may drop this retired cache and rebuild under cap.
    pub async fn resize(
        &mut self,
        layout: RasterLayout,
        view: CellRect,
        budget: &RasterBudget,
        poisoned: &mut bool,
    ) -> Result<bool, &'static str> {
        if self.unretired || *poisoned {
            return Err("text-area-backing-pinned");
        }
        if !self
            .reservation
            .as_ref()
            .is_some_and(|r| r.belongs_to(budget))
        {
            return Err("text-area-budget-owner");
        }
        self.resize_copies = 0;
        if self.layout() == layout {
            return Ok(true);
        }
        let Some(mut next) = Self::new(layout, budget) else {
            return Ok(false);
        };
        let (cells, copies) = self
            .cells
            .resized(layout, view)
            .ok_or("text-area-resize-view")?;
        next.cells = cells;
        let old = self.layout();
        let copies = copies
            .iter()
            .map(|copy| rgba_copy(*copy, self.surface(), old, (0, 0)))
            .collect::<Option<Vec<_>>>()
            .ok_or("text-area-copy-origin")?;
        // Destination ring cell sizes equal the source unless there is no
        // overlap (font change), so rgba_copy's pixel conversion is shared.
        if !copies.is_empty() {
            let queued = crate::intel::queue_guc_bcs0_rgba_copies(next.surface(), &copies);
            // Persist both owners' state before an await. Cancellation can
            // drop next locally and later drop self through its Show owner.
            self.unretired = submission_may_be_live(&queued);
            if let Err(error) = retire(queued, poisoned, &mut next.unretired).await {
                if *poisoned {
                    self.quarantine();
                    next.quarantine();
                } else {
                    self.unretired = false;
                }
                return Err(error);
            }
            self.unretired = false;
        }
        next.resize_copies = copies.len();
        *self = next;
        Ok(true)
    }

    pub async fn render(
        &mut self,
        view: CellRect,
        revision: u64,
        produce: impl FnMut(i64, i64) -> C,
        mut glyph: impl FnMut(CellWrite<C>, RasterLayout) -> crate::intel::GucBcs0MonoGlyph,
        poisoned: &mut bool,
    ) -> Result<RasterWork, &'static str> {
        if self.unretired || *poisoned {
            return Err("text-area-backing-pinned");
        }
        let update = self
            .cells
            .update(view, revision, produce)
            .ok_or("text-area-view")?;
        let layout = self.layout();
        let glyphs: Vec<_> = update
            .writes
            .into_iter()
            .map(|write| glyph(write, layout))
            .collect();
        let paint_started = super::text_blit::stamp();
        // Both the persistent backing and its destination are retired here.
        let cpu_paint = unsafe { super::text_blit::try_mono(self.surface(), &glyphs) };
        let work = RasterWork {
            produced: update.produced,
            reused: update.reused,
            painted: glyphs.len(),
            cpu_glyphs: if cpu_paint {glyphs.len()} else {0},
            mono_batches: if cpu_paint {0} else {glyphs
                .len()
                .div_ceil(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS)},
        };
        for chunk in glyphs.chunks(crate::intel::GUC_BCS0_MONO_MAX_GLYPHS).filter(|_| !cpu_paint) {
            let queued = crate::intel::queue_guc_bcs0_mono_glyphs(self.surface(), chunk);
            if let Err(error) = retire(queued, poisoned, &mut self.unretired).await {
                self.invalidate();
                if *poisoned {
                    self.quarantine();
                }
                return Err(error);
            }
        }
        super::text_blit::report("cache-mono", super::text_blit::mono_pixels(&glyphs),cpu_paint,work.mono_batches,paint_started);
        Ok(work)
    }

    pub async fn copy_view(
        &mut self,
        view: CellRect,
        destination: GucBcs0RgbaSurface,
        origin: (u32, u32),
        poisoned: &mut bool,
    ) -> Result<usize, &'static str> {
        self.copy_views(&[(view, origin)], destination, poisoned).await
    }

    /// Copy disjoint pieces of one cached area in bounded batches.
    pub async fn copy_views(
        &mut self,
        views: &[(CellRect, (u32, u32))],
        destination: GucBcs0RgbaSurface,
        poisoned: &mut bool,
    ) -> Result<usize, &'static str> {
        if self.unretired || *poisoned {
            return Err("text-area-backing-pinned");
        }
        let layout = self.layout();
        let mut converted = Vec::new();
        for &(view, origin) in views {
            let pieces = self.cells.view_copies(view).ok_or("text-area-copy-view")?;
            for piece in pieces {
                converted.push(rgba_copy(piece, self.surface(), layout, origin).ok_or("text-area-copy-origin")?);
            }
        }
        let copies: Vec<_> = converted
            .into_iter()
            .filter_map(|mut copy| {
                if copy.destination_x >= destination.width
                    || copy.destination_y >= destination.height
                {
                    return None;
                }
                copy.width = copy.width.min(destination.width - copy.destination_x);
                copy.height = copy.height.min(destination.height - copy.destination_y);
                Some(copy)
            })
            .collect();
        if copies.is_empty() {
            return Ok(0);
        }
        let count = copies.len();
        let started = super::text_blit::stamp();
        // Caller holds the destination's writable frame lease; this cache has
        // no unretired work and supplies stable pixels through the sync copy.
        if unsafe { super::text_blit::try_copy(destination, &copies, layout.cell_height) } {
            super::text_blit::report("viewport-copy",super::text_blit::copy_pixels(&copies),true,0,started);
            return Ok(count);
        }
        // Stay below BCS0's 160-copy admission limit, including ring wraps.
        for chunk in copies.chunks(128) {
            let queued = crate::intel::queue_guc_bcs0_rgba_copies(destination, chunk);
            if let Err(error) = retire(queued, poisoned, &mut self.unretired).await {
                if *poisoned { self.quarantine(); }
                return Err(error);
            }
        }
        super::text_blit::report("viewport-copy",super::text_blit::copy_pixels(&copies),false,copies.len().div_ceil(128),started);
        Ok(count)
    }
}

fn rgba_copy(
    copy: CellCopy,
    source: GucBcs0RgbaSurface,
    layout: RasterLayout,
    origin: (u32, u32),
) -> Option<GucBcs0RgbaCopy> {
    Some(GucBcs0RgbaCopy {
        source,
        source_x: copy.source_x * layout.cell_width,
        source_y: copy.source_y * layout.cell_height,
        destination_x: origin
            .0
            .checked_add(copy.destination_x * layout.cell_width)?,
        destination_y: origin
            .1
            .checked_add(copy.destination_y * layout.cell_height)?,
        width: copy.columns * layout.cell_width,
        height: copy.rows * layout.cell_height,
    })
}

fn submission_may_be_live(
    queued: &Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError>,
) -> bool {
    queued.is_ok() || matches!(queued, Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed))
}

async fn retire(
    queued: Result<crate::intel::GucBcs0CopySubmission, crate::intel::GucBcs0CopySubmitError>,
    poisoned: &mut bool,
    unretired: &mut bool,
) -> Result<(), &'static str> {
    *unretired = submission_may_be_live(&queued);
    let submission = match queued {
        Ok(submission) => submission,
        Err(crate::intel::GucBcs0CopySubmitError::SubmitFailed) => {
            *poisoned = true;
            return Err("text-area-submit-uncertain");
        }
        Err(_) => return Err("text-area-admission"),
    };
    *poisoned = true;
    loop {
        match crate::intel::poll_guc_bcs0_rgba_copies(submission) {
            crate::intel::GucBcs0CopyCompletion::Pending => {
                Timer::after(Duration::from_millis(1)).await
            }
            crate::intel::GucBcs0CopyCompletion::Complete => {
                *poisoned = false;
                *unretired = false;
                return Ok(());
            }
            _ => return Err("text-area-retirement-uncertain"),
        }
    }
}
