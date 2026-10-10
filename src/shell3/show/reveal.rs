//! Cached strip pixels use the same retired BCS text-area backing as Matrix pan.
use super::super::{update::StripReveal, update::RenderedLine};
use crate::ui4::text_area::{CellRect, RasterBudget, RasterLayout};
use crate::ui4::text_area_raster::BcsTextArea;

pub(super) struct StripCache {
    raster: BcsTextArea<(char, Option<super::super::RgbaColor>)>,
    cells: RenderedLine,
    scale: u32,
    revision: u64,
}
impl StripCache {
    pub async fn prepare(slot: &mut Option<Self>, input: &StripReveal, scale: u32,
        budget: &RasterBudget, poisoned: &mut bool) -> Result<bool, &'static str> {
        if input.spans.is_empty() { return Ok(false); }
        let view = CellRect { x: 0, y: 0, columns: input.cells.len() as u32, rows: 1 };
        if slot.as_ref().is_some_and(|cache| cache.cells.len() != input.cells.len() || cache.scale != scale) {
            *slot = None;
        }
        if slot.is_none() {
            let Some(layout) = RasterLayout::fit(view,
                (microfont::FWIDTH as u32 * scale, microfont::FHEIGHT as u32 * scale),
                0, crate::allcaps::shell3::SH3_PANBUFFER_CAP_BYTES) else { return Ok(false); };
            let Some(raster) = BcsTextArea::new(layout, budget) else { return Ok(false); };
            *slot = Some(Self { raster, cells: RenderedLine::new(), scale, revision: 0 });
        }
        let cache = slot.as_mut().ok_or("shell3-strip-cache-missing")?;
        if cache.cells != input.cells {
            cache.revision = cache.revision.wrapping_add(1);
            cache.cells = input.cells.clone();
        }
        cache.raster.render(view, cache.revision, |x, _| input.cells[x as usize],
            |write, layout| super::copy::glyph_for_cell(write.x * layout.cell_width,
                0, layout.cell_width, layout.cell_height, write.cell, scale), poisoned).await?;
        Ok(true)
    }
    pub async fn copy(&mut self, input: &StripReveal, destination: crate::intel::GucBcs0RgbaSurface,
        poisoned: &mut bool) -> Result<(), &'static str> {
        let views: alloc::vec::Vec<_> = input.spans.iter().map(|span| {
            (CellRect { x: span.start as i64, y: 0, columns: span.len() as u32, rows: 1 },
             (((input.start + span.start) * microfont::FWIDTH * self.scale as usize) as u32,
              (input.row * microfont::FHEIGHT * self.scale as usize) as u32))
        }).collect();
        self.raster.copy_views(&views, destination, poisoned).await?;
        Ok(())
    }
}
