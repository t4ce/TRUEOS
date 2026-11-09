//! Bounded, shared stroke variants made by the existing native line renderer.
//! Jobs run off the UI/VM-exit path; only requested R8 cells remain cached.
use super::*;
use alloc::{boxed::Box, collections::BTreeMap, sync::Arc};

type Key = (u32, u32, u32); // scalar, output height, added width in 1/128 px
enum Cell {
    Pending,
    Ready(Arc<[u8]>),
    Failed(&'static str),
}
static CELLS: spin::Mutex<BTreeMap<Key, Cell>> = spin::Mutex::new(BTreeMap::new());
const MAX_CELLS: usize = 128;

pub(crate) fn glyph(
    character: char,
    height: u32,
    extra_px: f32,
) -> Result<Arc<[u8]>, &'static str> {
    if !(11..=128).contains(&height) || !extra_px.is_finite() || !(0.0..=8.0).contains(&extra_px) {
        return Err("microfont-style-shape");
    }
    let key = (
        character as u32,
        height,
        libm::roundf(extra_px * 128.0) as u32,
    );
    {
        let mut cells = CELLS.lock();
        match cells.get(&key) {
            Some(Cell::Ready(mask)) => return Ok(Arc::clone(mask)),
            Some(Cell::Pending) => return Err("microfont-style-pending"),
            Some(Cell::Failed(error)) => return Err(error),
            None => {}
        }
        if cells.len() >= MAX_CELLS {
            return Err("microfont-style-quota");
        }
        cells.insert(key, Cell::Pending);
    }
    let job = Box::new(move || {
        let result = build(character, height, key.2 as f32 / 128.0);
        let mut cells = CELLS.lock();
        match result {
            Ok(mask) => {
                cells.insert(key, Cell::Ready(Arc::from(mask)));
            }
            Err(
                "render-busy"
                | "in-flight"
                | "render-storage-busy"
                | "microfont-atlas-busy"
                | "virgl-offscreen-busy"
                | "microfont-not-ready",
            ) => {
                cells.remove(&key);
            }
            Err(error) => {
                cells.insert(key, Cell::Failed(error));
            }
        }
    });
    if let Err(job) =
        crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microfont-style")
    {
        drop(job);
        CELLS.lock().remove(&key);
    }
    Err("microfont-style-pending")
}

fn build(character: char, height: u32, extra_px: f32) -> Result<Vec<u8>, &'static str> {
    let base = ready_atlas().ok_or("microfont-not-ready")?;
    // Express the original stroke at the requested output size, then add the
    // requested texture-pixel width. Never change Shell3's immutable masks.
    let config = AtlasConfig {
        glyph_height_px: Some(height),
        line_width_px: base.line_width_px * height as f32 / base.em_height_px as f32 + extra_px,
        ..AtlasConfig::default()
    }
    .normalized()?;
    let character = if base.glyph(character).is_some() {
        character
    } else {
        '?'
    };
    let slot = glyph_slot(character).ok_or("microfont-style-glyph")?;
    let width = (height * 6 + 5) / 11;
    with_warmed_atlas(config, |atlas| {
        let surface = atlas.surface();
        let (cell_width, cell_height) = config.cell_extent();
        let left = (slot as u32 % ATLAS_COLUMNS) * cell_width + config.gutter();
        let top = (slot as u32 / ATLAS_COLUMNS) * cell_height + config.gutter();
        let source = crate::phys::phys_to_virt(surface.phys as usize) as *mut u8;
        let first =
            unsafe { source.add(top as usize * surface.pitch_bytes as usize + left as usize) };
        if !crate::intel::dma_flush_strided_row_spans(&[crate::intel::DmaFlushRows::new(
            first,
            width as usize,
            surface.pitch_bytes as usize,
            height as usize,
        )]) {
            return Err("microfont-style-readback");
        }
        let mut pixels = alloc::vec![0; (width * height) as usize];
        for y in 0..height as usize {
            for x in 0..width as usize {
                pixels[y * width as usize + x] = unsafe {
                    core::ptr::read_volatile(first.add(y * surface.pitch_bytes as usize + x))
                };
            }
        }
        Ok(pixels)
    })
}
