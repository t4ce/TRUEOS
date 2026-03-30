use alloc::vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

const UI2_ATHLAS_GRID_DEMO_TEX_ID: u32 = 4_708;
const UI2_ATHLAS_GRID_DEMO_WINDOW_X: f32 = 560.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Y: f32 = 96.0;
const UI2_ATHLAS_GRID_DEMO_WINDOW_Z: i16 = 32;
const UI2_ATHLAS_GRID_DEMO_COLS: usize = 30;
const UI2_ATHLAS_GRID_DEMO_ROWS: usize = 15;
const UI2_ATHLAS_GRID_DEMO_SIZE_CASE: usize = 0;
const UI2_ATHLAS_GRID_DEMO_GLYPH_BYTE: u8 = 0xA7;
const UI2_ATHLAS_GRID_DEMO_BG_RGBA: [u8; 4] = [0x09, 0x0C, 0x12, 0xFF];
const UI2_ATHLAS_GRID_DEMO_FG_RGBA: (u8, u8, u8, u8) = (0xE8, 0xEC, 0xF2, 0xFF);

fn build_athlas_grid_rgba(width: u32, height: u32, cell_h: u32) -> Option<alloc::vec::Vec<u8>> {
    let mut rgba = vec![
        0u8;
        (width as usize)
            .saturating_mul(height as usize)
            .saturating_mul(4)
    ];
    for chunk in rgba.chunks_exact_mut(4) {
        chunk.copy_from_slice(&UI2_ATHLAS_GRID_DEMO_BG_RGBA);
    }

    let row_bytes = [UI2_ATHLAS_GRID_DEMO_GLYPH_BYTE; UI2_ATHLAS_GRID_DEMO_COLS];
    let px_h = cell_h as f32;
    for row in 0..UI2_ATHLAS_GRID_DEMO_ROWS {
        let y = (row as i32).saturating_mul(cell_h as i32);
        if !crate::gfx::imba_athlas::blit_imba_athlas_text_rgba_nearest_px(
            rgba.as_mut_slice(),
            width,
            height,
            &row_bytes,
            0,
            y,
            px_h,
            UI2_ATHLAS_GRID_DEMO_FG_RGBA,
        ) {
            return None;
        }
    }

    Some(rgba)
}

#[embassy_executor::task]
pub async fn ui2_athlas_grid_demo_task() {
    Timer::after(EmbassyDuration::from_millis(250)).await;

    let Some(glyph) =
        crate::gfx::imba_athlas::imba_athlas_lookup_codepoint(UI2_ATHLAS_GRID_DEMO_GLYPH_BYTE as u32)
    else {
        crate::log!("ui2-athlas-grid-demo: glyph lookup failed byte=0xA7\n");
        return;
    };

    let Some((cell_w, cell_h)) = crate::gfx::imba_athlas::imba_athlas_bucket_cell_px(
        UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
        glyph.bucket as usize,
    ) else {
        crate::log!(
            "ui2-athlas-grid-demo: missing bucket cell size size_case={} bucket={}\n",
            UI2_ATHLAS_GRID_DEMO_SIZE_CASE,
            glyph.bucket
        );
        return;
    };

    let width = cell_w.saturating_mul(UI2_ATHLAS_GRID_DEMO_COLS as u32);
    let height = cell_h.saturating_mul(UI2_ATHLAS_GRID_DEMO_ROWS as u32);
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        "Athlas Grid",
        crate::r::ui2::Ui2Rect {
            x: UI2_ATHLAS_GRID_DEMO_WINDOW_X,
            y: UI2_ATHLAS_GRID_DEMO_WINDOW_Y,
            w: width as f32,
            h: height as f32,
        },
        UI2_ATHLAS_GRID_DEMO_WINDOW_Z,
        220,
        UI2_ATHLAS_GRID_DEMO_TEX_ID,
        false,
        UI2_ATHLAS_GRID_DEMO_BG_RGBA,
    ) else {
        crate::log!(
            "ui2-athlas-grid-demo: window creation failed tex={}\n",
            UI2_ATHLAS_GRID_DEMO_TEX_ID
        );
        return;
    };

    if !crate::gfx::imba_athlas::ensure_imba_athlas_png_buckets_uploaded() {
        crate::log!("ui2-athlas-grid-demo: athlas bucket upload failed\n");
        loop {
            Timer::after(EmbassyDuration::from_secs(3600)).await;
        }
    }

    let Some(rgba) = build_athlas_grid_rgba(width, height, cell_h) else {
        crate::log!(
            "ui2-athlas-grid-demo: rgba build failed glyph=0x{:02X} bucket={} cell={}x{}\n",
            UI2_ATHLAS_GRID_DEMO_GLYPH_BYTE,
            glyph.bucket,
            cell_w,
            cell_h
        );
        return;
    };

    if !surface.upload_rgba(rgba.as_slice(), "ui2-athlas-grid-demo-upload") {
        crate::log!(
            "ui2-athlas-grid-demo: upload failed window={} tex={} size={}x{}\n",
            surface.window_id(),
            surface.tex_id(),
            width,
            height
        );
        return;
    }

    crate::log!(
        "ui2-athlas-grid-demo: window={} tex={} glyph=0x{:02X} bucket={} grid={}x{} cell={}x{} size={}x{}\n",
        surface.window_id(),
        surface.tex_id(),
        UI2_ATHLAS_GRID_DEMO_GLYPH_BYTE,
        glyph.bucket,
        UI2_ATHLAS_GRID_DEMO_COLS,
        UI2_ATHLAS_GRID_DEMO_ROWS,
        cell_w,
        cell_h,
        width,
        height
    );

    loop {
        Timer::after(EmbassyDuration::from_secs(3600)).await;
    }
}
