//! Shell3's one-time adapter to UI4's reusable character-area/raster API.
use super::super::{RgbaColor, update::MatrixAreaSnapshot};
use crate::ui4::text_area::{CellRect, RasterBudget, RasterLayout};
use crate::ui4::text_area_raster::BcsTextArea;
use alloc::string::String;

type Cell = (char, Option<RgbaColor>);
pub(super) struct PanBuffer {
    raster: BcsTextArea<Cell>,
    identity: (Option<String>, Option<u64>),
    view: CellRect,
    source_revision: Option<(u64, Option<bool>)>,
    cache_revision: u64,
}

impl PanBuffer {
    pub async fn prepare(
        slot: &mut Option<Self>,
        budget: &RasterBudget,
        input: Option<&MatrixAreaSnapshot>,
        scale: u32,
        poisoned: &mut bool,
    ) -> Result<bool, &'static str> {
        let Some(input) = input else {
            *slot = None;
            return Ok(false);
        };
        let view = CellRect {
            x: 0,
            y: input.offset as i64,
            columns: u32::try_from(input.columns).map_err(|_| "shell3-pan-columns")?,
            rows: u32::try_from(input.rows).map_err(|_| "shell3-pan-rows")?,
        };
        let Some(layout) = RasterLayout::fit(
            view,
            (microfont::FWIDTH as u32 * scale, microfont::FHEIGHT as u32 * scale),
            crate::allcaps::shell3::PANBUFFER_GUARD_CELLS,
            crate::allcaps::shell3::SH3_PANBUFFER_CAP_BYTES,
        ) else {
            *slot = None;
            crate::log_info!(target: "apps";
                "shell3/pan: fallback=viewport-cap view={}x{} cap={}\n",
                view.columns, view.rows, crate::allcaps::shell3::SH3_PANBUFFER_CAP_BYTES,
            );
            return Ok(false);
        };
        let mut resize_copies = 0;
        if let Some(cache) = slot {
            if cache.identity != input.identity {
                cache.raster.invalidate();
                cache.identity = input.identity.clone();
                cache.source_revision = None;
            }
            if !cache.raster.resize(layout, view, budget, poisoned).await? {
                // Both old and new allocations count during overlap transfer.
                // Under pressure, release the retired old tile and rebuild.
                *slot = None;
            } else {
                resize_copies = cache.raster.last_resize_copies();
            }
        }
        if slot.is_none() {
            let Some(raster) = BcsTextArea::new(layout, budget) else {
                crate::log_info!(target: "apps";
                    "shell3/pan: fallback=allocation bytes={} budget_used={}\n", layout.bytes, budget.used(),
                );
                return Ok(false);
            };
            *slot = Some(Self {
                raster,
                identity: input.identity.clone(),
                view,
                source_revision: None,
                cache_revision: 0,
            });
        }
        let cache = slot.as_mut().ok_or("shell3-pan-missing")?;
        let source_revision = (input.revision, input.blink_phase);
        if cache.source_revision != Some(source_revision) {
            cache.cache_revision = cache.cache_revision.wrapping_add(1);
            cache.source_revision = Some(source_revision);
        }
        let work = cache
            .raster
            .render(
                view,
                cache.cache_revision,
                |x, y| input.cell(x, y),
                |write, layout| {
                    super::copy::glyph_for_cell(
                        write.x * layout.cell_width,
                        write.y * layout.cell_height,
                        layout.cell_width,
                        layout.cell_height,
                        write.cell,
                        scale,
                    )
                },
                poisoned,
            )
            .await?;
        cache.view = view;
        crate::log_info!(target: "apps";
            "shell3/pan: view={}x{}@{},{} tile={}x{} bytes={} budget_used={} produced={} reused={} painted={} mono_batches={} resize_copies_retired={} cpu_glyphs={}\n",
            view.columns, view.rows, view.x, view.y, layout.columns, layout.rows,
            layout.bytes, budget.used(), work.produced, work.reused, work.painted, work.mono_batches, resize_copies, work.cpu_glyphs,
        );
        Ok(true)
    }

    pub async fn copy_view(
        &mut self,
        destination: crate::intel::GucBcs0RgbaSurface,
        scale: u32,
        poisoned: &mut bool,
    ) -> Result<(), &'static str> {
        let copies = self
            .raster
            .copy_view(self.view, destination, (0, 3 * microfont::FHEIGHT as u32 * scale), poisoned)
            .await?;
        crate::log_info!(target: "apps";
            "shell3/pan: copy_retired rectangles={} destination={}x{}\n",
            copies, destination.width, destination.height,
        );
        Ok(())
    }
}
