extern crate alloc;

use alloc::{collections::VecDeque, string::String, vec, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::r::ui2::{self, Ui2FontTier};

const UI2_RENDER_ALBUM_TASK_NAME: &str = "ui2-render-album-demo";
const UI2_RENDER_ALBUM_TEX_ID: u32 = crate::tst::ui2::ids::Ui2DemoTexId::RenderAlbum.get();
const UI2_RENDER_ALBUM_WINDOW_TITLE: &str = "Render Album";
const UI2_RENDER_ALBUM_WINDOW_W: u32 = 2560;
const UI2_RENDER_ALBUM_WINDOW_H: u32 = 1440;
const UI2_RENDER_ALBUM_WINDOW_X: f32 = 0.0;
const UI2_RENDER_ALBUM_WINDOW_Y: f32 = 0.0;
const UI2_RENDER_ALBUM_WINDOW_Z: i16 = 42;
const UI2_RENDER_ALBUM_WINDOW_ALPHA: u8 = 0xFF;
const UI2_RENDER_ALBUM_FRAME_MS: u64 = 33;
const UI2_RENDER_ALBUM_QUEUE_LIMIT: usize = 64;
const UI2_RENDER_ALBUM_TILE: usize = 256;
const UI2_RENDER_ALBUM_TILE_GAP: usize = 16;
const UI2_RENDER_ALBUM_PAD_X: usize = 24;
const UI2_RENDER_ALBUM_PAD_Y: usize = 72;
const UI2_RENDER_ALBUM_FONT_TIER: Ui2FontTier = Ui2FontTier::Third;
const UI2_RENDER_ALBUM_FONT_SIZE_CASE: usize = UI2_RENDER_ALBUM_FONT_TIER.size_case();

const BG: [u8; 4] = [0x10, 0x12, 0x17, 0xFF];
const TILE_BG: [u8; 4] = [0x18, 0x1B, 0x22, 0xFF];
const TILE_PANEL: [u8; 4] = [0x22, 0x27, 0x30, 0xFF];
const TEXT: [u8; 4] = [0xEA, 0xF0, 0xF6, 0xFF];
const DIM: [u8; 4] = [0x9A, 0xA5, 0xB2, 0xFF];
const OK: [u8; 4] = [0x3B, 0xCF, 0x86, 0xFF];
const BAD: [u8; 4] = [0xEC, 0x5B, 0x5B, 0xFF];
const WARN: [u8; 4] = [0xF3, 0xBD, 0x58, 0xFF];
const IDLE: [u8; 4] = [0x66, 0x6E, 0x7B, 0xFF];
const BLUE: [u8; 4] = [0x48, 0xB7, 0xFF, 0xFF];

#[derive(Clone)]
#[allow(dead_code)]
enum RenderAlbumPayload {
    Metrics([u32; 4]),
    Argb {
        width: u32,
        height: u32,
        pixels: Vec<u32>,
    },
}

#[derive(Clone)]
struct RenderAlbumCommand {
    slot: usize,
    label: String,
    status: u32,
    payload: RenderAlbumPayload,
}

static UI2_RENDER_ALBUM_DIRTY: AtomicBool = AtomicBool::new(true);
static UI2_RENDER_ALBUM_QUEUE: Mutex<VecDeque<RenderAlbumCommand>> = Mutex::new(VecDeque::new());

pub fn queue_render_album_metrics_tile(
    slot: usize,
    label: &str,
    status: u32,
    metrics: [u32; 4],
) -> bool {
    queue_render_album_command(RenderAlbumCommand {
        slot,
        label: clipped_label(label),
        status,
        payload: RenderAlbumPayload::Metrics(metrics),
    })
}

#[allow(dead_code)]
pub fn queue_render_album_argb_tile(
    slot: usize,
    label: &str,
    status: u32,
    width: u32,
    height: u32,
    argb: &[u32],
) -> bool {
    if width == 0 || height == 0 {
        return false;
    }
    let Some(pixel_count) = (width as usize).checked_mul(height as usize) else {
        return false;
    };
    if pixel_count == 0 || pixel_count > argb.len() {
        return false;
    }

    let mut pixels = Vec::with_capacity(pixel_count);
    pixels.extend_from_slice(&argb[..pixel_count]);
    queue_render_album_command(RenderAlbumCommand {
        slot,
        label: clipped_label(label),
        status,
        payload: RenderAlbumPayload::Argb {
            width,
            height,
            pixels,
        },
    })
}

fn clipped_label(label: &str) -> String {
    let mut out = String::new();
    for ch in label.chars().take(18) {
        out.push(ch);
    }
    out
}

fn queue_render_album_command(command: RenderAlbumCommand) -> bool {
    let mut queue = UI2_RENDER_ALBUM_QUEUE.lock();
    while queue.len() >= UI2_RENDER_ALBUM_QUEUE_LIMIT {
        let _ = queue.pop_front();
    }
    queue.push_back(command);
    UI2_RENDER_ALBUM_DIRTY.store(true, Ordering::Release);
    true
}

fn drain_album_commands() -> Vec<RenderAlbumCommand> {
    let mut drained = Vec::new();
    let mut queue = UI2_RENDER_ALBUM_QUEUE.lock();
    while let Some(command) = queue.pop_front() {
        drained.push(command);
    }
    drained
}

fn fill_rgba(pixels: &mut [u8], rgba: [u8; 4]) {
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&rgba);
    }
}

fn fill_rect(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    let x_end = x.saturating_add(w).min(width);
    let y_end = y.saturating_add(h).min(height);
    for row in y.min(height)..y_end {
        for col in x.min(width)..x_end {
            let idx = (row * width + col) * 4;
            pixels[idx..idx + 4].copy_from_slice(&rgba);
        }
    }
}

fn stroke_rect(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    rgba: [u8; 4],
) {
    fill_rect(pixels, width, height, x, y, w, 3, rgba);
    fill_rect(pixels, width, height, x, y.saturating_add(h).saturating_sub(3), w, 3, rgba);
    fill_rect(pixels, width, height, x, y, 3, h, rgba);
    fill_rect(pixels, width, height, x.saturating_add(w).saturating_sub(3), y, 3, h, rgba);
}

fn render_text(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    atlases: &ui2::Ui2FontCpuAtlases,
    x: usize,
    y: usize,
    max_width_px: usize,
    text: &str,
    rgba: [u8; 4],
) {
    let _ = ui2::ui2_font_blit_text_rgba(
        pixels,
        width,
        height,
        atlases,
        UI2_RENDER_ALBUM_FONT_TIER,
        x,
        y,
        max_width_px,
        text,
        rgba,
    );
}

fn status_color(status: u32) -> [u8; 4] {
    match status {
        1 => OK,
        2 => BAD,
        3 => WARN,
        _ => IDLE,
    }
}

fn status_label(status: u32) -> &'static str {
    match status {
        1 => "observed",
        2 => "missing",
        3 => "setup",
        _ => "idle",
    }
}

fn tile_origin(slot: usize) -> Option<(usize, usize)> {
    let width = UI2_RENDER_ALBUM_WINDOW_W as usize;
    let usable_w = width
        .saturating_sub(UI2_RENDER_ALBUM_PAD_X.saturating_mul(2))
        .saturating_add(UI2_RENDER_ALBUM_TILE_GAP);
    let stride = UI2_RENDER_ALBUM_TILE + UI2_RENDER_ALBUM_TILE_GAP;
    let cols = usable_w.checked_div(stride).unwrap_or(1).max(1);
    let col = slot % cols;
    let row = slot / cols;
    let x = UI2_RENDER_ALBUM_PAD_X + col.saturating_mul(stride);
    let y = UI2_RENDER_ALBUM_PAD_Y + row.saturating_mul(stride);
    if x.saturating_add(UI2_RENDER_ALBUM_TILE) > UI2_RENDER_ALBUM_WINDOW_W as usize
        || y.saturating_add(UI2_RENDER_ALBUM_TILE) > UI2_RENDER_ALBUM_WINDOW_H as usize
    {
        return None;
    }
    Some((x, y))
}

fn render_album_shell(
    pixels: &mut [u8],
    atlases: &ui2::Ui2FontCpuAtlases,
    width: usize,
    height: usize,
) {
    fill_rgba(pixels, BG);
    fill_rect(pixels, width, height, 0, 0, width, 56, [0x15, 0x18, 0x20, 0xFF]);
    fill_rect(pixels, width, height, 0, 56, width, 2, BLUE);
    render_text(pixels, width, height, atlases, 24, 18, 360, UI2_RENDER_ALBUM_WINDOW_TITLE, TEXT);
    render_text(
        pixels,
        width,
        height,
        atlases,
        420,
        18,
        700,
        "XeLP fixed-function proof tiles",
        DIM,
    );
}

fn render_metric_bars(
    pixels: &mut [u8],
    width: usize,
    height: usize,
    x: usize,
    y: usize,
    metrics: [u32; 4],
    accent: [u8; 4],
) {
    let max_metric = metrics
        .iter()
        .copied()
        .fold(1u32, |acc, v| acc.max(v.min(4096)));
    let bar_w = 42usize;
    let bar_h = 106usize;
    for (idx, metric) in metrics.iter().copied().enumerate() {
        let bx = x + idx * 54;
        fill_rect(pixels, width, height, bx, y, bar_w, bar_h, TILE_PANEL);
        let filled = ((metric.min(4096) as usize) * (bar_h - 10) / max_metric as usize).max(2);
        let color = if metric == 0 { IDLE } else { accent };
        fill_rect(
            pixels,
            width,
            height,
            bx + 5,
            y + bar_h.saturating_sub(5 + filled),
            bar_w - 10,
            filled,
            color,
        );
    }
}

fn render_argb_fit(
    pixels: &mut [u8],
    dst_w: usize,
    dst_h: usize,
    area_x: usize,
    area_y: usize,
    area_w: usize,
    area_h: usize,
    src_w: u32,
    src_h: u32,
    argb: &[u32],
) {
    if src_w == 0 || src_h == 0 || argb.is_empty() {
        return;
    }
    fill_rect(pixels, dst_w, dst_h, area_x, area_y, area_w, area_h, TILE_PANEL);
    let src_w_usize = src_w as usize;
    let src_h_usize = src_h as usize;
    let scale_w = area_w as u64 * 1024 / src_w_usize.max(1) as u64;
    let scale_h = area_h as u64 * 1024 / src_h_usize.max(1) as u64;
    let scale = scale_w.min(scale_h).max(1);
    let fit_w = ((src_w_usize as u64 * scale) / 1024) as usize;
    let fit_h = ((src_h_usize as u64 * scale) / 1024) as usize;
    let dst_x0 = area_x + area_w.saturating_sub(fit_w) / 2;
    let dst_y0 = area_y + area_h.saturating_sub(fit_h) / 2;
    for dy in 0..fit_h {
        let sy = (dy as u64 * src_h_usize as u64 / fit_h.max(1) as u64) as usize;
        for dx in 0..fit_w {
            let sx = (dx as u64 * src_w_usize as u64 / fit_w.max(1) as u64) as usize;
            let src_idx = sy
                .min(src_h_usize.saturating_sub(1))
                .saturating_mul(src_w_usize)
                .saturating_add(sx.min(src_w_usize.saturating_sub(1)));
            let Some(&px) = argb.get(src_idx) else {
                continue;
            };
            let dst_x = dst_x0 + dx;
            let dst_y = dst_y0 + dy;
            if dst_x >= dst_w || dst_y >= dst_h {
                continue;
            }
            let a = ((px >> 24) & 0xFF) as u8;
            let rgba = [
                ((px >> 16) & 0xFF) as u8,
                ((px >> 8) & 0xFF) as u8,
                (px & 0xFF) as u8,
                if a == 0 { 0xFF } else { a },
            ];
            let dst_idx = (dst_y * dst_w + dst_x) * 4;
            pixels[dst_idx..dst_idx + 4].copy_from_slice(&rgba);
        }
    }
}

fn render_album_tile(
    pixels: &mut [u8],
    atlases: &ui2::Ui2FontCpuAtlases,
    command: &RenderAlbumCommand,
) {
    let Some((x, y)) = tile_origin(command.slot) else {
        return;
    };
    let width = UI2_RENDER_ALBUM_WINDOW_W as usize;
    let height = UI2_RENDER_ALBUM_WINDOW_H as usize;
    let accent = status_color(command.status);
    fill_rect(pixels, width, height, x, y, UI2_RENDER_ALBUM_TILE, UI2_RENDER_ALBUM_TILE, TILE_BG);
    stroke_rect(pixels, width, height, x, y, UI2_RENDER_ALBUM_TILE, UI2_RENDER_ALBUM_TILE, accent);
    fill_rect(pixels, width, height, x + 10, y + 10, UI2_RENDER_ALBUM_TILE - 20, 8, accent);
    render_text(pixels, width, height, atlases, x + 14, y + 28, 92, command.label.as_str(), TEXT);
    render_text(
        pixels,
        width,
        height,
        atlases,
        x + 112,
        y + 28,
        124,
        status_label(command.status),
        DIM,
    );

    match &command.payload {
        RenderAlbumPayload::Metrics(metrics) => {
            render_metric_bars(pixels, width, height, x + 18, y + 86, *metrics, accent);
        }
        RenderAlbumPayload::Argb {
            width: src_w,
            height: src_h,
            pixels: argb,
        } => render_argb_fit(
            pixels,
            width,
            height,
            x + 14,
            y + 62,
            UI2_RENDER_ALBUM_TILE - 28,
            UI2_RENDER_ALBUM_TILE - 80,
            *src_w,
            *src_h,
            argb.as_slice(),
        ),
    }
}

#[embassy_executor::task]
pub async fn ui2_render_album_demo_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(UI2_RENDER_ALBUM_TASK_NAME);
    let Some(atlases) = ui2::ui2_font_decode_cpu_atlases(UI2_RENDER_ALBUM_FONT_SIZE_CASE) else {
        crate::log!("ui2-render-album-demo: font atlas decode failed\n");
        return;
    };
    let Some(surface) = ui2::Ui2SurfaceWindow::new(
        UI2_RENDER_ALBUM_WINDOW_TITLE,
        ui2::Ui2Rect {
            x: UI2_RENDER_ALBUM_WINDOW_X,
            y: UI2_RENDER_ALBUM_WINDOW_Y,
            w: UI2_RENDER_ALBUM_WINDOW_W as f32,
            h: UI2_RENDER_ALBUM_WINDOW_H as f32,
        },
        UI2_RENDER_ALBUM_WINDOW_Z,
        UI2_RENDER_ALBUM_WINDOW_ALPHA,
        UI2_RENDER_ALBUM_TEX_ID,
        false,
        BG,
    ) else {
        crate::log!("ui2-render-album-demo: window creation failed\n");
        return;
    };
    let _ = surface.bind_spawn_task(UI2_RENDER_ALBUM_TASK_NAME);
    let window_id = surface.window_id();
    let _ = ui2::set_window_left_scrollbar_visible(window_id, false);
    let _ = ui2::set_window_bottom_scrollbar_visible(window_id, false);
    let _ = ui2::set_window_resize_maintain_aspect(window_id, true);
    let _ = ui2::set_window_content_preserve_scale(window_id, true);

    let width = UI2_RENDER_ALBUM_WINDOW_W as usize;
    let height = UI2_RENDER_ALBUM_WINDOW_H as usize;
    let mut pixels = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    render_album_shell(pixels.as_mut_slice(), &atlases, width, height);
    UI2_RENDER_ALBUM_DIRTY.store(true, Ordering::Release);

    loop {
        if crate::r::spawn_service::task_stop_requested(UI2_RENDER_ALBUM_TASK_NAME) {
            break;
        }

        let commands = drain_album_commands();
        let mut changed = !commands.is_empty();
        for command in commands.iter() {
            render_album_tile(pixels.as_mut_slice(), &atlases, command);
        }

        if UI2_RENDER_ALBUM_DIRTY.swap(false, Ordering::AcqRel) {
            changed = true;
        }
        if changed {
            if !surface.upload_rgba_owned(pixels.clone(), "ui2-render-album-present") {
                crate::log!("ui2-render-album-demo: upload failed tex={}\n", surface.tex_id());
                break;
            }
            let _ = ui2::request_window_content_present(window_id, "ui2-render-album-present");
        }

        if crate::r::spawn_service::wait_task_or_timeout_ms(
            UI2_RENDER_ALBUM_TASK_NAME,
            UI2_RENDER_ALBUM_FRAME_MS,
        )
        .await
        {
            break;
        }
    }
}
