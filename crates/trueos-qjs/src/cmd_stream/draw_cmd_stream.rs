use parry2d::math::Isometry;
use parry2d::query;
use parry2d::shape::Ball;

use crate as qjs;

pub(crate) unsafe extern "C" fn qjs_cmd_stream_draw_triangles_u8(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 1) else {
        return qjs::JSValue::undefined();
    };
    super::atlas_cmd_stream::flush_text_batches();
    let _ = super::cmd_stream_with_u8_buffer(ctx, args[0], |ptr, len| {
        if len > 0 {
            let _ = super::trueos_cabi_gfx_draw_rgb_triangles_no_present(ptr, len);
        }
    });
    qjs::JSValue::undefined()
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_fill_rect(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 5) else {
        return qjs::JSValue::undefined();
    };
    let Some(x_f) = super::cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(y_f) = super::cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = super::cmd_stream_arg_f64(ctx, args, 2) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = super::cmd_stream_arg_f64(ctx, args, 3) else {
        return qjs::JSValue::undefined();
    };
    let Some(rgba_f) = super::cmd_stream_arg_f64(ctx, args, 4) else {
        return qjs::JSValue::undefined();
    };

    super::atlas_cmd_stream::flush_text_batches();
    let rgba = (rgba_f as i64).max(0) as u32;
    let outline = super::cmd_stream_arg_f64(ctx, args, 5).unwrap_or(0.0) != 0.0;
    let chamfer = super::cmd_stream_arg_f64(ctx, args, 6).unwrap_or(0.0) != 0.0;
    let _ = super::cmd_stream_fill_rect(
        x_f as f32,
        y_f as f32,
        w_f as f32,
        h_f as f32,
        rgba,
        outline,
        chamfer,
    );
    qjs::JSValue::undefined()
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_draw_line(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 5) else {
        return qjs::JSValue::undefined();
    };
    let Some(x1_f) = super::cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(y1_f) = super::cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(x2_f) = super::cmd_stream_arg_f64(ctx, args, 2) else {
        return qjs::JSValue::undefined();
    };
    let Some(y2_f) = super::cmd_stream_arg_f64(ctx, args, 3) else {
        return qjs::JSValue::undefined();
    };
    let Some(rgba_f) = super::cmd_stream_arg_f64(ctx, args, 4) else {
        return qjs::JSValue::undefined();
    };

    super::atlas_cmd_stream::flush_text_batches();
    let rgba = (rgba_f as i64).max(0) as u32;
    let thickness = super::cmd_stream_arg_f64(ctx, args, 5).unwrap_or(1.0).max(0.5) as f32;
    let _ = super::cmd_stream_draw_line(
        x1_f as f32,
        y1_f as f32,
        x2_f as f32,
        y2_f as f32,
        rgba,
        thickness,
    );
    qjs::JSValue::undefined()
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_draw_textured_triangles_u8(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 2) else {
        return qjs::JSValue::undefined();
    };
    super::atlas_cmd_stream::flush_text_batches();
    let Some(tex_id_f) = super::cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if tex_id == 0 {
        return qjs::JSValue::undefined();
    }
    let _ = super::cmd_stream_with_u8_buffer(ctx, args[1], |ptr, len| {
        if len > 0 {
            let _ = super::trueos_cabi_gfx_draw_tex_triangles_no_present(tex_id, ptr, len);
        }
    });
    qjs::JSValue::undefined()
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_draw_texture_rect(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 5) else {
        return qjs::JSValue::undefined();
    };
    let Some(tex_id_f) = super::cmd_stream_arg_f64(ctx, args, 0) else {
        return qjs::JSValue::undefined();
    };
    let Some(x_f) = super::cmd_stream_arg_f64(ctx, args, 1) else {
        return qjs::JSValue::undefined();
    };
    let Some(y_f) = super::cmd_stream_arg_f64(ctx, args, 2) else {
        return qjs::JSValue::undefined();
    };
    let Some(w_f) = super::cmd_stream_arg_f64(ctx, args, 3) else {
        return qjs::JSValue::undefined();
    };
    let Some(h_f) = super::cmd_stream_arg_f64(ctx, args, 4) else {
        return qjs::JSValue::undefined();
    };
    let tex_id = (tex_id_f as i64).max(0) as u32;
    if tex_id == 0 {
        return qjs::JSValue::undefined();
    }

    let u0 = super::cmd_stream_arg_f64(ctx, args, 5).unwrap_or(0.0) as f32;
    let v0 = super::cmd_stream_arg_f64(ctx, args, 6).unwrap_or(0.0) as f32;
    let u1 = super::cmd_stream_arg_f64(ctx, args, 7).unwrap_or(1.0) as f32;
    let v1 = super::cmd_stream_arg_f64(ctx, args, 8).unwrap_or(1.0) as f32;
    let rgba =
        (super::cmd_stream_arg_f64(ctx, args, 9).unwrap_or(0xFFFF_FFFFu32 as f64) as i64).max(0)
            as u32;

    super::atlas_cmd_stream::flush_text_batches();
    let _ = super::cmd_stream_draw_texture_rect(
        tex_id,
        x_f as f32,
        y_f as f32,
        w_f as f32,
        h_f as f32,
        u0,
        v0,
        u1,
        v1,
        rgba,
    );
    qjs::JSValue::undefined()
}

pub(crate) unsafe extern "C" fn qjs_cmd_stream_step_icon_collisions(
    ctx: *mut qjs::JSContext,
    _this_val: qjs::JSValueConst,
    argc: i32,
    argv: *const qjs::JSValueConst,
) -> qjs::JSValue {
    let Some(args) = super::cmd_stream_args(argv, argc, 5) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let Some((pos_ptr, pos_len, pos_ab)) = super::cmd_stream_read_f32_slice_from_value(ctx, args[0]) else {
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some((vel_ptr, vel_len, vel_ab)) = super::cmd_stream_read_f32_slice_from_value(ctx, args[1]) else {
        qjs::js_free_value(ctx, pos_ab);
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let Some(dt_ms_f) = super::cmd_stream_arg_f64(ctx, args, 2) else {
        qjs::js_free_value(ctx, vel_ab);
        qjs::js_free_value(ctx, pos_ab);
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some(icon_size_f) = super::cmd_stream_arg_f64(ctx, args, 3) else {
        qjs::js_free_value(ctx, vel_ab);
        qjs::js_free_value(ctx, pos_ab);
        return qjs::JS_NewFloat64(ctx, 0.0);
    };
    let Some(restitution_f) = super::cmd_stream_arg_f64(ctx, args, 4) else {
        qjs::js_free_value(ctx, vel_ab);
        qjs::js_free_value(ctx, pos_ab);
        return qjs::JS_NewFloat64(ctx, 0.0);
    };

    let icon_size = (icon_size_f as f32).max(1.0);
    let radius = (icon_size * 0.5).max(0.5);
    let dt = ((dt_ms_f as f32) / 1000.0).clamp(0.0, 0.05);
    let restitution = (restitution_f as f32).clamp(0.0, 1.0);
    let n = core::cmp::min(pos_len, vel_len) / 2;
    if n < 2 {
        qjs::js_free_value(ctx, vel_ab);
        qjs::js_free_value(ctx, pos_ab);
        return qjs::JS_NewFloat64(ctx, 0.0);
    }

    let pos = core::slice::from_raw_parts_mut(pos_ptr, n * 2);
    let vel = core::slice::from_raw_parts_mut(vel_ptr, n * 2);
    let view_w = super::CMD_STREAM_VIEW_W.load(core::sync::atomic::Ordering::Relaxed).max(1) as f32;
    let view_h = super::CMD_STREAM_VIEW_H.load(core::sync::atomic::Ordering::Relaxed).max(1) as f32;

    for i in 0..n {
        let b = i * 2;
        pos[b] += vel[b] * dt;
        pos[b + 1] += vel[b + 1] * dt;

        if pos[b] < 0.0 {
            pos[b] = 0.0;
            vel[b] = vel[b].abs() * restitution;
        } else if pos[b] + icon_size > view_w {
            pos[b] = (view_w - icon_size).max(0.0);
            vel[b] = -vel[b].abs() * restitution;
        }

        if pos[b + 1] < 0.0 {
            pos[b + 1] = 0.0;
            vel[b + 1] = vel[b + 1].abs() * restitution;
        } else if pos[b + 1] + icon_size > view_h {
            pos[b + 1] = (view_h - icon_size).max(0.0);
            vel[b + 1] = -vel[b + 1].abs() * restitution;
        }
    }

    let shape = Ball::new(radius);
    let mut contacts = 0u32;
    for i in 0..n {
        let ib = i * 2;
        let ci_x = pos[ib] + radius;
        let ci_y = pos[ib + 1] + radius;
        let pi = Isometry::translation(ci_x, ci_y);

        for j in (i + 1)..n {
            let jb = j * 2;
            let cj_x = pos[jb] + radius;
            let cj_y = pos[jb + 1] + radius;
            let pj = Isometry::translation(cj_x, cj_y);

            let Ok(Some(c)) = query::contact(&pi, &shape, &pj, &shape, 0.0) else {
                continue;
            };

            if c.dist >= 0.0 {
                continue;
            }
            contacts = contacts.saturating_add(1);

            let nrm = c.normal1.into_inner();
            let nx = nrm.x;
            let ny = nrm.y;

            let rvx = vel[jb] - vel[ib];
            let rvy = vel[jb + 1] - vel[ib + 1];
            let rel = (rvx * nx) + (rvy * ny);
            if rel < 0.0 {
                let impulse = -((1.0 + restitution) * rel) * 0.5;
                vel[ib] -= impulse * nx;
                vel[ib + 1] -= impulse * ny;
                vel[jb] += impulse * nx;
                vel[jb + 1] += impulse * ny;
            }

            let penetration = (-c.dist).max(0.0);
            if penetration > 0.0 {
                let corr = (penetration * 0.5) + 0.01;
                pos[ib] -= corr * nx;
                pos[ib + 1] -= corr * ny;
                pos[jb] += corr * nx;
                pos[jb + 1] += corr * ny;
            }
        }
    }

    qjs::js_free_value(ctx, vel_ab);
    qjs::js_free_value(ctx, pos_ab);
    qjs::JS_NewFloat64(ctx, contacts as f64)
}