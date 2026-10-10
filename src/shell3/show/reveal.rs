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
        if input.hidden == input.cells.len() { return Ok(false); }
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
        let view = CellRect { x: input.hidden as i64, y: 0,
            columns: (input.cells.len() - input.hidden) as u32, rows: 1 };
        self.raster.copy_view(view, destination,
            (((input.start + input.hidden) * microfont::FWIDTH * self.scale as usize) as u32,
             (input.row * microfont::FHEIGHT * self.scale as usize) as u32), poisoned).await?;
        Ok(())
    }
}
