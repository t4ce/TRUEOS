use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;
use spin::Mutex;
use trueos_gfx_core::{Rgba8, TexVertex, push_tex_vertex_bytes};

use crate as qjs;

struct CmdStreamLyonIconTexRecord {
    icon_id: u32,
    color_id: u32,
    small_set: u32,
    tex_id: u32,
    side_px: u32,
}

struct CmdStreamLyonUnitQuadRecord {
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
    verts: Arc<[u8]>,
}

static CMD_STREAM_LYON_ICON_TEX_RECS: Mutex<Vec<CmdStreamLyonIconTexRecord>> =
    Mutex::new(Vec::new());
static CMD_STREAM_LYON_UNIT_QUAD_RECS: Mutex<Vec<CmdStreamLyonUnitQuadRecord>> =
    Mutex::new(Vec::new());

#[inline]
fn cmd_stream_lyon_palette_rgb(color_id: u32) -> (u8, u8, u8) {
    match (color_id % 5) as usize {
        0 => (0, 0, 0),
        1 => (217, 46, 46),
        2 => (31, 158, 56),
        3 => (31, 82, 217),
        _ => (242, 140, 31),
    }
}

#[inline]
fn cmd_stream_ensure_lyon_icon_tex(
    icon_id: u32,
    color_id: u32,
    small_set: u32,
) -> Option<(u32, u32)> {
    {
        let recs = CMD_STREAM_LYON_ICON_TEX_RECS.lock();
        if let Some(rec) = recs
            .iter()
            .find(|r| r.icon_id == icon_id && r.color_id == color_id && r.small_set == small_set)
        {
            return Some((rec.tex_id, rec.side_px));
        }
    }

    let need = unsafe {
        super::trueos_cabi_gfx_bake_lyon_icon_rgba(
            icon_id,
            color_id,
            small_set,
            core::ptr::null_mut(),
            0,
        )
    };
    if need <= 0 {
        return None;
    }
    let need = need as usize;
    if need % 4 != 0 {
        return None;
    }
    let px_count = need / 4;
    let side = if small_set != 0 { 16usize } else { 32usize };
    if side.saturating_mul(side) != px_count {
        return None;
    }

    let mut rgba = vec![0u8; need];
    let wrote = unsafe {
        super::trueos_cabi_gfx_bake_lyon_icon_rgba(
            icon_id,
            color_id,
            small_set,
            rgba.as_mut_ptr(),
            rgba.len(),
        )
    };
    if wrote != need as i32 {
        return None;
    }

    let tex_id = super::cmd_stream_alloc_tex_id();
    let rc = unsafe {
        super::trueos_cabi_gfx_upload_texture_rgba(
            tex_id,
            side as u32,
            side as u32,
            rgba.as_ptr(),
            rgba.len(),
        )
    };
    if rc != 0 {
        super::cmd_stream_release_tex_id(tex_id);
        return None;
    }

    CMD_STREAM_LYON_ICON_TEX_RECS
        .lock()
        .push(CmdStreamLyonIconTexRecord {
            icon_id,
            color_id,
            small_set,
            tex_id,
            side_px: side as u32,
        });
    Some((tex_id, side as u32))
}

#[inline]
fn cmd_stream_get_lyon_unit_quad_verts(
    view_w: u32,
    view_h: u32,
    side_px: u32,
    r: u8,
    g: u8,
    b: u8,
) -> Arc<[u8]> {
    {
        let recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
        if let Some(rec) = recs.iter().find(|rec| {
            rec.view_w == view_w
                && rec.view_h == view_h
                && rec.side_px == side_px
                && rec.r == r
                && rec.g == g
                && rec.b == b
        }) {
            return rec.verts.clone();
        }
    }

    let vw = view_w.max(1) as f32;
    let vh = view_h.max(1) as f32;
    let side = side_px.max(1) as f32;
    let dx = 2.0 * (side / vw);
    let dy = 2.0 * (side / vh);

    let mut verts = Vec::with_capacity(6 * 20);
    let color = Rgba8::new(r, g, b, 255);
    push_tex_vertex_bytes(&mut verts, TexVertex { x: 0.0, y: -dy, u: 0.0, v: 1.0, color });
    push_tex_vertex_bytes(&mut verts, TexVertex { x: dx, y: -dy, u: 1.0, v: 1.0, color });
    push_tex_vertex_bytes(&mut verts, TexVertex { x: dx, y: 0.0, u: 1.0, v: 0.0, color });
    push_tex_vertex_bytes(&mut verts, TexVertex { x: 0.0, y: -dy, u: 0.0, v: 1.0, color });
    push_tex_vertex_bytes(&mut verts, TexVertex { x: dx, y: 0.0, u: 1.0, v: 0.0, color });
    push_tex_vertex_bytes(&mut verts, TexVertex { x: 0.0, y: 0.0, u: 0.0, v: 0.0, color });

    let out: Arc<[u8]> = Arc::from(verts.into_boxed_slice());
    let mut recs = CMD_STREAM_LYON_UNIT_QUAD_RECS.lock();
    recs.push(CmdStreamLyonUnitQuadRecord {
        view_w,
        view_h,
        side_px,
        r,
        g,
        b,
        verts: out.clone(),
    });
    if recs.len() > 64 {
        let excess = recs.len() - 64;
        recs.drain(0..excess);
    }
    out
}

fn draw_lyon_in_frame(
    icon_id: u32,
    x: f32,
    y: f32,
    view_w: u32,
    view_h: u32,
    color_id: u32,
) -> bool {
    if !super::CMD_STREAM_FRAME_OPEN.load(Ordering::Relaxed) {
        return false;
    }
    super::CMD_STREAM_VIEW_W.store(view_w.max(1), Ordering::Relaxed);
    super::CMD_STREAM_VIEW_H.store(view_h.max(1), Ordering::Relaxed);

    let Some((tex_id, side_px)) = cmd_stream_ensure_lyon_icon_tex(icon_id, 0, 1) else {
        return false;
    };
    let (r, g, b) = cmd_stream_lyon_palette_rgb(color_id);

    let view_w_u = view_w.max(1);
    let view_h_u = view_h.max(1);
    let view_w_f = view_w_u as f32;
    let view_h_f = view_h_u as f32;
    let (origin_x, origin_y) = super::cmd_stream_origin_px();
    let origin_x_ndc = (2.0 * ((x + origin_x) / view_w_f)) - 1.0;
    let origin_y_ndc = 1.0 - (2.0 * ((y + origin_y) / view_h_f));

    let verts = cmd_stream_get_lyon_unit_quad_verts(view_w_u, view_h_u, side_px, r, g, b);
    true
}

pub(crate) fn release_tex_id(id: u32) {
    CMD_STREAM_LYON_ICON_TEX_RECS
        .lock()
        .retain(|rec| rec.tex_id != id);
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_draw_lyon_icon_in_frame(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 3) else {
        return qjs::JSValue::undefined();
    };
    let Some(icon_id_f) = super::cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(x_f) = super::cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(y_f) = super::cmd_stream_arg_f64(ctx, args, 2) else {
        return qjs::JSValue::undefined();
    };

    let color_id_f = super::cmd_stream_arg_f64(ctx, args, 3).unwrap_or(0.0);
    let icon_id = (icon_id_f as i64).max(0) as u32;
    let color_id = (color_id_f as i64).max(0) as u32;
    let view_w = super::CMD_STREAM_VIEW_W.load(Ordering::Relaxed).max(1);
    let view_h = super::CMD_STREAM_VIEW_H.load(Ordering::Relaxed).max(1);

    let _ = draw_lyon_in_frame(icon_id, x_f as f32, y_f as f32, view_w, view_h, color_id);
    qjs::JSValue::undefined()
}
