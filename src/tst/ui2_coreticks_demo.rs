extern crate alloc;

use alloc::vec::Vec;
use core::hint::spin_loop;
use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, Ordering};


const UI2_CORETICKS_TEX_ID: u32 = crate::tst_ui2_ids::Ui2DemoTexId::Coreticks.get();
const UI2_CORETICKS_WINDOW_TITLE: &str = "Coreticks";
const UI2_CORETICKS_WINDOW_X: f32 = 720.0;
const UI2_CORETICKS_WINDOW_Y: f32 = 140.0;
const UI2_CORETICKS_WINDOW_Z: i16 = 36;
const UI2_CORETICKS_WINDOW_ALPHA: u8 = 236;
const UI2_CORETICKS_CANVAS_W: u32 = 256;
const UI2_CORETICKS_CANVAS_H: u32 = 256;
const UI2_CORETICKS_TILE_W: usize = 16;
const UI2_CORETICKS_TILE_H: usize = 16;
const UI2_CORETICKS_TILE_COLS: usize = 16;
const UI2_CORETICKS_TILE_ROWS: usize = 16;
const UI2_CORETICKS_TILE_COUNT: usize = UI2_CORETICKS_TILE_COLS * UI2_CORETICKS_TILE_ROWS;
const UI2_CORETICKS_TILE_PIXELS: u16 = (UI2_CORETICKS_TILE_W * UI2_CORETICKS_TILE_H) as u16;
const UI2_CORETICKS_PIXEL_COUNT: usize =
    (UI2_CORETICKS_CANVAS_W as usize) * (UI2_CORETICKS_CANVAS_H as usize);
const UI2_CORETICKS_FRAME_MS: u64 = 16;
const UI2_CORETICKS_BG_RGBA: [u8; 4] = [0xFF, 0xFF, 0xFF, 0xFF];
const UI2_CORETICKS_FG_RGBA: [u8; 4] = [0x00, 0x00, 0x00, 0xFF];
const UI2_CORETICKS_TILE_1_RGBA: [u8; 4] = [0xFF, 0x4F, 0xB3, 0xFF];
const UI2_CORETICKS_TILE_2_RGBA: [u8; 4] = [0x2A, 0xE8, 0xD3, 0xFF];

static UI2_CORETICKS_DIRTY: AtomicBool = AtomicBool::new(true);
static UI2_CORETICKS_SNAP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static UI2_CORETICKS_ACTIVE_TICKS: AtomicU32 = AtomicU32::new(0);
static UI2_CORETICKS_TILE_CURSOR: [AtomicU16; UI2_CORETICKS_TILE_COUNT] =
    [const { AtomicU16::new(0) }; UI2_CORETICKS_TILE_COUNT];
static UI2_CORETICKS_PIXEL_STATE: [AtomicU8; UI2_CORETICKS_PIXEL_COUNT] =
    [const { AtomicU8::new(0) }; UI2_CORETICKS_PIXEL_COUNT];

#[inline]
const fn ui2_coreticks_tile_index(tile_x: usize, tile_y: usize) -> usize {
    tile_y * UI2_CORETICKS_TILE_COLS + tile_x
}

#[inline]
const fn ui2_coreticks_canvas_pixel_index(x: usize, y: usize) -> usize {
    y * (UI2_CORETICKS_CANVAS_W as usize) + x
}

const fn ui2_coreticks_spiral_local_xy(mut step: usize) -> (usize, usize) {
    let mut ring = 0usize;
    let mut side = UI2_CORETICKS_TILE_W;

    loop {
        if side == 0 {
            return (0, 0);
        }
        if side == 1 {
            return (ring, ring);
        }

        let top_len = side;
        if step < top_len {
            return (ring + step, ring);
        }
        step -= top_len;

        let right_len = side - 1;
        if step < right_len {
            return (ring + side - 1, ring + 1 + step);
        }
        step -= right_len;

        let bottom_len = side - 1;
        if step < bottom_len {
            return (ring + side - 2 - step, ring + side - 1);
        }
        step -= bottom_len;

        let left_len = side - 2;
        if step < left_len {
            return (ring, ring + side - 2 - step);
        }
        step -= left_len;

        ring += 1;
        side -= 2;
    }
}

#[inline]
const fn ui2_coreticks_tile_pixel_index(tile_index: usize, local_pixel_index: usize) -> usize {
    let tile_x = tile_index % UI2_CORETICKS_TILE_COLS;
    let tile_y = tile_index / UI2_CORETICKS_TILE_COLS;
    let (local_x, local_y) = ui2_coreticks_spiral_local_xy(local_pixel_index);
    let x = tile_x * UI2_CORETICKS_TILE_W + local_x;
    let y = tile_y * UI2_CORETICKS_TILE_H + local_y;
    ui2_coreticks_canvas_pixel_index(x, y)
}

fn ui2_coreticks_clear_tile_pixels(tile_index: usize) {
    for local_pixel_index in 0..UI2_CORETICKS_TILE_PIXELS as usize {
        let pixel_index = ui2_coreticks_tile_pixel_index(tile_index, local_pixel_index);
        UI2_CORETICKS_PIXEL_STATE[pixel_index].store(0, Ordering::Release);
    }
}

fn ui2_coreticks_tick_tile_by_index(index: usize) -> bool {
    if index >= UI2_CORETICKS_TILE_COUNT {
        return false;
    }
    if UI2_CORETICKS_SNAP_IN_PROGRESS.load(Ordering::Acquire) {
        return false;
    }

    UI2_CORETICKS_ACTIVE_TICKS.fetch_add(1, Ordering::AcqRel);
    if UI2_CORETICKS_SNAP_IN_PROGRESS.load(Ordering::Acquire) {
        UI2_CORETICKS_ACTIVE_TICKS.fetch_sub(1, Ordering::AcqRel);
        return false;
    }

    let slot = &UI2_CORETICKS_TILE_CURSOR[index];
    let mut current = slot.load(Ordering::Acquire);
    let changed = loop {
        if current == UI2_CORETICKS_TILE_PIXELS {
            match slot.compare_exchange_weak(current, 0, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => {
                    ui2_coreticks_clear_tile_pixels(index);
                    UI2_CORETICKS_DIRTY.store(true, Ordering::Release);
                    break true;
                }
                Err(next) => {
                    current = next;
                    continue;
                }
            }
        }
        match slot.compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
        {
            Ok(_) => {
                let pixel_index = ui2_coreticks_tile_pixel_index(index, current as usize);
                UI2_CORETICKS_PIXEL_STATE[pixel_index].store(1, Ordering::Release);
                UI2_CORETICKS_DIRTY.store(true, Ordering::Release);
                break true;
            }
            Err(next) => current = next,
        }
    };

    UI2_CORETICKS_ACTIVE_TICKS.fetch_sub(1, Ordering::AcqRel);
    changed
}

#[inline]
fn ui2_coreticks_tick_tile<const TILE_X: usize, const TILE_Y: usize>() -> bool {
    ui2_coreticks_tick_tile_by_index(ui2_coreticks_tile_index(TILE_X, TILE_Y))
}

pub fn ui2_coreticks_snap_all() {
    UI2_CORETICKS_SNAP_IN_PROGRESS.store(true, Ordering::Release);
    while UI2_CORETICKS_ACTIVE_TICKS.load(Ordering::Acquire) != 0 {
        spin_loop();
    }

    for slot in &UI2_CORETICKS_TILE_CURSOR {
        slot.store(0, Ordering::Release);
    }
    for pixel in &UI2_CORETICKS_PIXEL_STATE {
        pixel.store(0, Ordering::Release);
    }

    UI2_CORETICKS_SNAP_IN_PROGRESS.store(false, Ordering::Release);
    UI2_CORETICKS_DIRTY.store(true, Ordering::Release);
}

#[inline]
fn ui2_coreticks_tile_fill_rgba(tile_index: usize) -> [u8; 4] {
    match tile_index {
        0 => UI2_CORETICKS_TILE_1_RGBA,
        1 => UI2_CORETICKS_TILE_2_RGBA,
        _ => UI2_CORETICKS_FG_RGBA,
    }
}

fn ui2_coreticks_fill_rgba(pixels: &mut [u8], rgba: [u8; 4]) {
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

fn ui2_coreticks_render_pixels_rgba() -> Vec<u8> {
    let mut pixels = vec![
        0u8;
        (UI2_CORETICKS_CANVAS_W as usize)
            .saturating_mul(UI2_CORETICKS_CANVAS_H as usize)
            .saturating_mul(4)
    ];
    ui2_coreticks_fill_rgba(pixels.as_mut_slice(), UI2_CORETICKS_BG_RGBA);

    for tile_y in 0..UI2_CORETICKS_TILE_ROWS {
        for tile_x in 0..UI2_CORETICKS_TILE_COLS {
            let tile_index = ui2_coreticks_tile_index(tile_x, tile_y);
            let fill_rgba = ui2_coreticks_tile_fill_rgba(tile_index);
            let base_x = tile_x * UI2_CORETICKS_TILE_W;
            let base_y = tile_y * UI2_CORETICKS_TILE_H;

            for local_y in 0..UI2_CORETICKS_TILE_H {
                for local_x in 0..UI2_CORETICKS_TILE_W {
                    let x = base_x + local_x;
                    let y = base_y + local_y;
                    let pixel_index = ui2_coreticks_canvas_pixel_index(x, y);
                    if UI2_CORETICKS_PIXEL_STATE[pixel_index].load(Ordering::Acquire) == 0 {
                        continue;
                    }
                    let offset = pixel_index * 4;
                    pixels[offset..offset + 4].copy_from_slice(&fill_rgba);
                }
            }
        }
    }

    pixels
}

pub fn ui2_coreticks_demo_current_rgba() -> Vec<u8> {
    ui2_coreticks_render_pixels_rgba()
}

pub fn ui2_coreticks_tick_tile_xy(tile_x: usize, tile_y: usize) -> bool {
    if tile_x >= UI2_CORETICKS_TILE_COLS || tile_y >= UI2_CORETICKS_TILE_ROWS {
        return false;
    }
    ui2_coreticks_tick_tile_by_index(ui2_coreticks_tile_index(tile_x, tile_y))
}

pub fn ui2_coreticks_tick_tile_index(tile_index: usize) -> bool {
    ui2_coreticks_tick_tile_by_index(tile_index)
}

#[embassy_executor::task]
pub async fn ui2_coreticks_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard("ui2-coreticks-demo");
    let Some(surface) = crate::r::ui2::Ui2SurfaceWindow::new(
        UI2_CORETICKS_WINDOW_TITLE,
        crate::r::ui2::Ui2Rect {
            x: UI2_CORETICKS_WINDOW_X,
            y: UI2_CORETICKS_WINDOW_Y,
            w: UI2_CORETICKS_CANVAS_W as f32,
            h: UI2_CORETICKS_CANVAS_H as f32,
        },
        UI2_CORETICKS_WINDOW_Z,
        UI2_CORETICKS_WINDOW_ALPHA,
        UI2_CORETICKS_TEX_ID,
        false,
        UI2_CORETICKS_BG_RGBA,
    ) else {
        return;
    };
    let _ = surface.bind_spawn_task("ui2-coreticks-demo");

    let window_id = surface.window_id();
    let _ = crate::r::ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let _ = crate::r::ui2::set_window_resize_maintain_aspect(window_id, true);
    let _ = crate::r::ui2::set_window_content_preserve_scale(window_id, true);

    loop {
        if crate::r::spawn_service::task_stop_requested("ui2-coreticks-demo") {
            break;
        }
        if UI2_CORETICKS_DIRTY.swap(false, Ordering::AcqRel) {
            let pixels = ui2_coreticks_demo_current_rgba();
            if !surface.upload_rgba(pixels.as_slice(), "ui2-coreticks-demo-present") {
                break;
            }
            let _ = crate::r::ui2::request_window_content_present(
                surface.window_id(),
                "ui2-coreticks-demo-present",
            );
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(
            "ui2-coreticks-demo",
            UI2_CORETICKS_FRAME_MS,
        )
        .await
        {
            break;
        }
    }
}
