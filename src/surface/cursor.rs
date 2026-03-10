extern crate alloc;

use alloc::vec::Vec;

#[derive(Clone, Copy)]
struct RgbVtx {
    x: f32,
    y: f32,
    r: f32,
    g: f32,
    b: f32,
    a: f32,
}

#[inline]
fn clamp01(v: f32) -> f32 {
    if v <= 0.0 {
        0.0
    } else if v >= 1.0 {
        1.0
    } else {
        v
    }
}

#[inline]
fn push_rgb_vtx(out: &mut Vec<u8>, v: RgbVtx) {
    out.extend_from_slice(&v.x.to_le_bytes());
    out.extend_from_slice(&v.y.to_le_bytes());
    out.push((clamp01(v.r) * 255.0 + 0.5) as u8);
    out.push((clamp01(v.g) * 255.0 + 0.5) as u8);
    out.push((clamp01(v.b) * 255.0 + 0.5) as u8);
    out.push((clamp01(v.a) * 255.0 + 0.5) as u8);
}

#[inline]
fn push_rgb_quad(
    out: &mut Vec<u8>,
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    color: (f32, f32, f32, f32),
) {
    let (r, g, b, a) = color;
    let v0 = RgbVtx {
        x: x0,
        y: y0,
        r,
        g,
        b,
        a,
    };
    let v1 = RgbVtx {
        x: x1,
        y: y0,
        r,
        g,
        b,
        a,
    };
    let v2 = RgbVtx {
        x: x1,
        y: y1,
        r,
        g,
        b,
        a,
    };
    let v3 = RgbVtx {
        x: x0,
        y: y1,
        r,
        g,
        b,
        a,
    };
    push_rgb_vtx(out, v0);
    push_rgb_vtx(out, v1);
    push_rgb_vtx(out, v2);
    push_rgb_vtx(out, v0);
    push_rgb_vtx(out, v2);
    push_rgb_vtx(out, v3);
}

#[inline]
fn append_cursor_cross(
    out: &mut Vec<u8>,
    ndc_x: f32,
    ndc_y: f32,
    vp_w: u32,
    vp_h: u32,
    color: (f32, f32, f32, f32),
) {
    // Fixed marker geometry: 10x10 cross, 2px line thickness.
    let w = (vp_w as f32).max(1.0);
    let h = (vp_h as f32).max(1.0);
    let half_span_x = (5.0f32 * 2.0) / w;
    let half_span_y = (5.0f32 * 2.0) / h;
    let half_thickness_x = (1.0f32 * 2.0) / w;
    let half_thickness_y = (1.0f32 * 2.0) / h;

    // Horizontal bar.
    push_rgb_quad(
        out,
        ndc_x - half_span_x,
        ndc_y - half_thickness_y,
        ndc_x + half_span_x,
        ndc_y + half_thickness_y,
        color,
    );
    // Vertical bar.
    push_rgb_quad(
        out,
        ndc_x - half_thickness_x,
        ndc_y - half_span_y,
        ndc_x + half_thickness_x,
        ndc_y + half_span_y,
        color,
    );
}

// Debug cursor automode: synthesize 4 scout cursors when real inputs are absent.
const CURSOR_AUTOMODE_DEBUG: bool = false;
const CURSOR_AUTOMODE_SLOTS: usize = 4;

#[inline]
fn collect_real_cursor_norm(out: &mut Vec<(f32, f32)>) {
    out.clear();

    let mice = crate::usb::hid::mouse_cursor_snapshot();
    for (mx, my) in mice {
        let nx = if mx.is_finite() {
            mx.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let ny = if my.is_finite() {
            my.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        out.push((nx, ny));
    }

    let tablets = crate::usb::hid::tablet_cursor_snapshot();
    for (tx, ty) in tablets {
        let nx = if tx.is_finite() {
            tx.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        let ny = if ty.is_finite() {
            ty.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        out.push((nx, ny));
    }
}

#[inline]
fn automode_slot_norm(slot: usize, t_sec: f32) -> (f32, f32) {
    let (cx, cy) = match slot {
        0 => (0.25f32, 0.25f32),
        1 => (0.75f32, 0.25f32),
        2 => (0.25f32, 0.75f32),
        _ => (0.75f32, 0.75f32),
    };
    let speed = match slot {
        0 => 0.55f32,
        1 => 0.78f32,
        2 => 1.02f32,
        _ => 1.33f32,
    };
    let phase = (slot as f32) * 1.618f32;
    let rx = 0.16f32;
    let ry = 0.12f32;
    let ax = t_sec * speed + phase;
    let ay = t_sec * (speed * 1.37f32) + phase * 0.73f32;
    let x = (cx + rx * libm::cosf(ax)).clamp(0.02, 0.98);
    let y = (cy + ry * libm::sinf(ay)).clamp(0.02, 0.98);
    (x, y)
}

pub fn append_kernel_cursor_overlay_rgb(rgb_blob: &mut Vec<u8>, vp_w: u32, vp_h: u32) {
    if vp_w == 0 || vp_h == 0 {
        return;
    }

    let mut real: Vec<(f32, f32)> = Vec::new();
    collect_real_cursor_norm(&mut real);

    if CURSOR_AUTOMODE_DEBUG {
        let hz = embassy_time_driver::TICK_HZ.max(1);
        let t_sec = (embassy_time_driver::now() as f32) / (hz as f32);
        let slot_colors: [(f32, f32, f32, f32); CURSOR_AUTOMODE_SLOTS] = [
            (0.0, 1.0, 0.2, 0.95),
            (1.0, 0.9, 0.1, 0.95),
            (0.2, 0.8, 1.0, 0.95),
            (1.0, 0.4, 0.8, 0.95),
        ];

        for slot in 0..CURSOR_AUTOMODE_SLOTS {
            let (nx, ny) = if slot < real.len() {
                // Real cursor claims this automode slot and halts its orbit.
                real[slot]
            } else {
                automode_slot_norm(slot, t_sec)
            };
            let ndc_x = nx * 2.0 - 1.0;
            let ndc_y = 1.0 - ny * 2.0;
            append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, slot_colors[slot]);
        }

        // Keep support for N kernel cursors: render any extras beyond 4 as well.
        for &(nx, ny) in real.iter().skip(CURSOR_AUTOMODE_SLOTS) {
            let ndc_x = nx * 2.0 - 1.0;
            let ndc_y = 1.0 - ny * 2.0;
            append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, (1.0, 1.0, 1.0, 0.95));
        }
    } else {
        for (i, &(nx, ny)) in real.iter().enumerate() {
            let ndc_x = nx * 2.0 - 1.0;
            let ndc_y = 1.0 - ny * 2.0;
            let color = if (i & 1) == 0 {
                (0.0, 1.0, 0.2, 0.95)
            } else {
                (1.0, 0.9, 0.1, 0.95)
            };
            append_cursor_cross(rgb_blob, ndc_x, ndc_y, vp_w, vp_h, color);
        }
    }
}

pub unsafe fn input_cursor_buttons(cursor_id: u32, out_buttons_down: *mut u32) -> i32 {
    if out_buttons_down.is_null() {
        return -1;
    }
    if cursor_id == 0 {
        return -1;
    }

    let idx = (cursor_id - 1) as usize;
    let mice = crate::usb::hid::mouse_cursor_snapshot_with_buttons();
    let tablets = crate::usb::hid::tablet_cursor_snapshot();

    if idx < mice.len() {
        *out_buttons_down = mice[idx].2;
        return 0;
    }

    let tidx = idx - mice.len();
    if tidx < tablets.len() {
        *out_buttons_down = 0;
        return 0;
    }

    1
}

pub unsafe fn input_pop_cursor_event(out: *mut crate::usb::hid::TrueosHidCursorEvent) -> i32 {
    if out.is_null() {
        return -1;
    }
    let Some(ev) = crate::usb::hid::pop_cursor_event() else {
        return 0;
    };
    *out = ev;
    1
}

pub unsafe fn input_read_cursor_events_since(
    read_seq: u64,
    out: *mut crate::usb::hid::TrueosHidCursorEvent,
    out_cap: u32,
    out_next_seq: *mut u64,
    out_dropped: *mut u32,
) -> u32 {
    if out_next_seq.is_null() || out_dropped.is_null() {
        return 0;
    }

    let cap = out_cap as usize;
    if cap == 0 || out.is_null() {
        let mut none: [crate::usb::hid::TrueosHidCursorEvent; 0] = [];
        let (next_seq, dropped, _wrote) =
            crate::usb::hid::read_cursor_events_since(read_seq, &mut none);
        *out_next_seq = next_seq;
        *out_dropped = dropped;
        return 0;
    }

    let out_slice = core::slice::from_raw_parts_mut(out, cap);
    let (next_seq, dropped, wrote) = crate::usb::hid::read_cursor_events_since(read_seq, out_slice);
    *out_next_seq = next_seq;
    *out_dropped = dropped;
    wrote as u32
}
