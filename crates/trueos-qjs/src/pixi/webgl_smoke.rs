#![cfg(feature = "trueos")]

use core::sync::atomic::{AtomicBool, Ordering};

use crate::cmd_stream::{self, CmdStreamCommand};
use alloc::vec;
use alloc::vec::Vec;
use embassy_time::{Duration as EmbassyDuration, Timer};

unsafe extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
}

static WEBGL_SMOKE_TASK_STARTED: AtomicBool = AtomicBool::new(false);

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

#[derive(Clone, Copy)]
struct Pt {
    x: f32,
    y: f32,
}

#[inline]
fn px_to_ndc_x(x_px: f32, w: f32) -> f32 {
    (2.0 * (x_px / w)) - 1.0
}

#[inline]
fn px_to_ndc_y(y_px: f32, h: f32) -> f32 {
    1.0 - (2.0 * (y_px / h))
}

#[inline]
fn push_vertex_tex(dst: &mut Vec<u8>, p: Pt, uv: (f32, f32), w: f32, h: f32, rgba: u32) {
    let nx = px_to_ndc_x(p.x, w);
    let ny = px_to_ndc_y(p.y, h);
    dst.extend_from_slice(&nx.to_le_bytes());
    dst.extend_from_slice(&ny.to_le_bytes());
    dst.extend_from_slice(&uv.0.to_le_bytes());
    dst.extend_from_slice(&uv.1.to_le_bytes());
    dst.push(((rgba >> 24) & 0xff) as u8);
    dst.push(((rgba >> 16) & 0xff) as u8);
    dst.push(((rgba >> 8) & 0xff) as u8);
    dst.push((rgba & 0xff) as u8);
}

#[inline]
fn append_quad_tex(dst: &mut Vec<u8>, p0: Pt, p1: Pt, p2: Pt, p3: Pt, w: f32, h: f32, rgba: u32) {
    push_vertex_tex(dst, p0, (0.0, 0.0), w, h, rgba);
    push_vertex_tex(dst, p1, (1.0, 0.0), w, h, rgba);
    push_vertex_tex(dst, p2, (1.0, 1.0), w, h, rgba);

    push_vertex_tex(dst, p0, (0.0, 0.0), w, h, rgba);
    push_vertex_tex(dst, p2, (1.0, 1.0), w, h, rgba);
    push_vertex_tex(dst, p3, (0.0, 1.0), w, h, rgba);
}

fn build_text_texture_from_atlas(atlas: crate::FontAtlasView<'_>, text: &[u8]) -> (u32, u32, Vec<u8>) {
    let cell_w = (atlas.cell_w as usize).max(1);
    let cell_h = (atlas.cell_h as usize).max(1);
    let grid_w = (atlas.grid_w as usize).max(1);
    let grid_h = (atlas.grid_h as usize).max(1);
    let atlas_w = atlas.width as usize;

    let pad = 4usize;
    let mut total_w = pad * 2;
    for &ch in text {
        let advance = atlas
            .index
            .get(ch as usize)
            .copied()
            .and_then(|slot| {
                if slot == u16::MAX {
                    None
                } else {
                    let si = slot as usize;
                    atlas
                        .widths
                        .get(si)
                        .copied()
                        .map(|w| (w as usize).max(1))
                        .or(Some(cell_w))
                }
            })
            .unwrap_or(cell_w / 2);
        total_w = total_w.saturating_add(advance + 1);
    }
    if total_w > (pad * 2) {
        total_w = total_w.saturating_sub(1);
    }
    let width = total_w.max(16);
    let height = (pad * 2 + cell_h).max(16);

    let mut alpha = vec![0u8; width.saturating_mul(height)];
    let mut pen_x = pad;
    let baseline_y = pad;

    for &ch in text {
        let mut advance = (cell_w / 2).max(1);
        if let Some(&slot) = atlas.index.get(ch as usize) {
            if slot != u16::MAX {
                let si = slot as usize;
                let sx = (si % grid_w) * cell_w;
                let sy = (si / grid_w) * cell_h;
                if (si / grid_w) < grid_h {
                    for y in 0..cell_h {
                        let src_row = (sy + y).saturating_mul(atlas_w);
                        let dst_row = (baseline_y + y).saturating_mul(width);
                        for x in 0..cell_w {
                            let src = src_row + sx + x;
                            let dst = dst_row + pen_x + x;
                            if src < atlas.alpha.len() && dst < alpha.len() {
                                alpha[dst] = atlas.alpha[src];
                            }
                        }
                    }
                }
                advance = atlas
                    .widths
                    .get(si)
                    .copied()
                    .map(|w| (w as usize).max(1))
                    .unwrap_or(cell_w);
            }
        }
        pen_x = pen_x.saturating_add(advance + 1);
    }

    let mut rgba = vec![0u8; width.saturating_mul(height).saturating_mul(4)];
    let mut i = 0usize;
    while i < alpha.len() {
        let a = alpha[i];
        let o = i * 4;
        rgba[o] = 255;
        rgba[o + 1] = 255;
        rgba[o + 2] = 255;
        rgba[o + 3] = a;
        i += 1;
    }

    (width as u32, height as u32, rgba)
}

fn build_white_texture_4() -> Vec<u8> {
    let mut rgba = Vec::with_capacity(4 * 4 * 4);
    let mut i = 0usize;
    while i < 16 {
        rgba.push(255);
        rgba.push(255);
        rgba.push(255);
        rgba.push(255);
        i += 1;
    }
    rgba
}

fn build_centered_text_quads(w: f32, h: f32, tex_w: f32, tex_h: f32) -> (Vec<u8>, Vec<u8>) {
    let mut shadow = Vec::with_capacity(6 * 20);
    let mut text = Vec::with_capacity(6 * 20);

    let x0 = (w - tex_w) * 0.5;
    let y0 = (h - tex_h) * 0.5;
    let x1 = x0 + tex_w;
    let y1 = y0 + tex_h;

    // Subtle shadow to validate alpha edges.
    append_quad_tex(
        &mut shadow,
        Pt {
            x: x0 + 2.0,
            y: y0 + 2.0,
        },
        Pt {
            x: x1 + 2.0,
            y: y0 + 2.0,
        },
        Pt {
            x: x1 + 2.0,
            y: y1 + 2.0,
        },
        Pt {
            x: x0 + 2.0,
            y: y1 + 2.0,
        },
        w,
        h,
        0xff00_0070,
    );

    append_quad_tex(
        &mut text,
        Pt { x: x0, y: y0 },
        Pt { x: x1, y: y0 },
        Pt { x: x1, y: y1 },
        Pt { x: x0, y: y1 },
        w,
        h,
        0xffff_ffff,
    );

    (shadow, text)
}

#[embassy_executor::task]
pub async fn boot_webgl_smoke_task() {
    if WEBGL_SMOKE_TASK_STARTED.swap(true, Ordering::SeqCst) {
        log_str("qjs-webgl-smoke: already running\n");
        return;
    }

    let w = 1280i32;
    let h = 800i32;
    let wf = w.max(1) as f32;
    let hf = h.max(1) as f32;

    // 1x1-style white tiled tex for background clear validation through textured path.
    cmd_stream::enqueue(CmdStreamCommand::UploadTexture {
        tex_id: 1,
        width: 4,
        height: 4,
        rgba: build_white_texture_4(),
    });

    let Some(atlas) = crate::font_atlas_small_view() else {
        log_str("qjs-webgl-smoke: no rust font atlas provider registered\n");
        return;
    };

    // Text texture built from kernel font atlas (alpha mask -> RGBA upload).
    let (text_w, text_h, text_rgba) = build_text_texture_from_atlas(atlas, b"TRUEOS TEXT");
    cmd_stream::enqueue(CmdStreamCommand::UploadTexture {
        tex_id: 2,
        width: text_w,
        height: text_h,
        rgba: text_rgba,
    });

    let mut bg = Vec::with_capacity(6 * 20);
    append_quad_tex(
        &mut bg,
        Pt { x: 0.0, y: 0.0 },
        Pt { x: wf, y: 0.0 },
        Pt { x: wf, y: hf },
        Pt { x: 0.0, y: hf },
        wf,
        hf,
        0xffff_ffff,
    );

    let (shadow, text) = build_centered_text_quads(wf, hf, text_w as f32, text_h as f32);

    log_str("qjs-webgl-smoke: text test (kernel atlas alpha hotwire)\n");

    loop {
        cmd_stream::enqueue(CmdStreamCommand::SetViewport { w, h });
        cmd_stream::enqueue(CmdStreamCommand::SetBlendEnabled { enabled: true });
        cmd_stream::enqueue(CmdStreamCommand::SetBlendFunc {
            src_rgb: 0x0302,
            dst_rgb: 0x0303,
            src_alpha: 1,
            dst_alpha: 0x0303,
        });
        cmd_stream::enqueue(CmdStreamCommand::SetBlendEquation {
            rgb: 0x8006,
            alpha: 0x8006,
        });
        cmd_stream::enqueue(CmdStreamCommand::SetClearColor { clear_rgb: 0x00ff_ffff });
        cmd_stream::enqueue(CmdStreamCommand::BeginFrame);

        cmd_stream::enqueue(CmdStreamCommand::SetSampler {
            wrap_s: 0,
            wrap_t: 0,
            min_filter: 0,
            mag_filter: 0,
        });

        cmd_stream::enqueue(CmdStreamCommand::DrawTrianglesTex {
            tex_id: 1,
            vertices: bg.clone(),
        });
        cmd_stream::enqueue(CmdStreamCommand::DrawTrianglesTex {
            tex_id: 2,
            vertices: shadow.clone(),
        });
        cmd_stream::enqueue(CmdStreamCommand::DrawTrianglesTex {
            tex_id: 2,
            vertices: text.clone(),
        });

        cmd_stream::enqueue(CmdStreamCommand::EndFrame);

        Timer::after(EmbassyDuration::from_millis(50)).await;
    }
}
