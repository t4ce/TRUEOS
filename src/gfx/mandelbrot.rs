extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use trueos_gfx_core::{Rgba8, TEX_VERTEX_SIZE, TexVertex, push_tex_vertex_bytes};

pub const MANDELBROT_PIPELINE_FS_TAG_RAW: u32 = 0x4D44_4C42;
const FULL_CENTER_X: f32 = -0.5;
const FULL_CENTER_Y: f32 = 0.0;
const FULL_X_SPAN: f32 = 1.5;
const FULL_Y_SPAN: f32 = 1.0;
const SEA_HORSE_CENTER_X: f32 = -0.743_643_9;
const SEA_HORSE_CENTER_Y: f32 = 0.131_825_91;
const SEA_HORSE_X_SPAN_X8: f32 = 0.00075;
const SEA_HORSE_Y_SPAN_X8: f32 = SEA_HORSE_X_SPAN_X8 * (2.0 / 3.0);
const ZOOM_DURATION_SECS: u64 = 36;
const MANDELBROT_PALETTE: [([u8; 3], f32); 15] = [
    ([0xFF, 0xF0, 0xFF], 4.0),  // pink-ice
    ([0xFF, 0xD6, 0xFF], 8.0),  // pink-cloud
    ([0xFF, 0xB8, 0xFF], 12.0), // pink-cotton
    ([0xFF, 0x8C, 0xFF], 16.0), // bubblegum
    ([0xFF, 0x63, 0xFF], 20.0), // candy-pink
    ([0xFF, 0x37, 0xFF], 24.0), // neon-pink
    ([0xFF, 0x00, 0xF7], 28.0), // laser-magenta
    ([0xF0, 0x00, 0xE6], 32.0), // hot-fuchsia
    ([0xD6, 0x00, 0xD6], 36.0), // vivid-fuchsia
    ([0xBF, 0x00, 0xBF], 40.0), // orchid
    ([0xA0, 0x00, 0xC8], 44.0), // electric-purple
    ([0x8A, 0x00, 0xB8], 48.0), // violet-magenta
    ([0x70, 0x00, 0x8F], 52.0), // deep-violet
    ([0x52, 0x00, 0x66], 58.0), // dark-plum
    ([0x33, 0x00, 0x33], 64.0), // midnight-plum
];

fn emit_palette_immediates(shader: &mut String) {
    for (idx, (rgb, threshold)) in MANDELBROT_PALETTE.iter().enumerate() {
        let r = rgb[0] as f32 / 255.0;
        let g = rgb[1] as f32 / 255.0;
        let b = rgb[2] as f32 / 255.0;
        let _ = writeln!(
            shader,
            "IMM[{}] FLT32 {{ {:.6}, {:.6}, {:.6}, {:.6} }}",
            idx, r, g, b, threshold
        );
    }
}

pub fn build_fragment_shader_tgsi_unrolled(iterations: u32) -> String {
    let mut shader = String::new();
    shader.push_str("FRAG\n");
    shader.push_str("DCL IN[0], TEXCOORD[0], LINEAR\n");
    shader.push_str("DCL IN[1], COLOR, LINEAR\n");
    shader.push_str("DCL OUT[0], COLOR\n");
    shader.push_str("DCL TEMP[0]\n");
    shader.push_str("DCL TEMP[1]\n");
    shader.push_str("DCL TEMP[2]\n");
    shader.push_str("DCL TEMP[3]\n");
    shader.push_str("DCL TEMP[4]\n");
    shader.push_str("DCL TEMP[5]\n");
    emit_palette_immediates(&mut shader);

    let mut line = 0u32;
    let push = |text: &str, out: &mut String, line_no: &mut u32| {
        let _ = writeln!(out, "    {}: {}", *line_no, text);
        *line_no = line_no.wrapping_add(1);
    };

    // Derive constants from the incoming white vertex color so we do not need TGSI immediates.
    push("MOV TEMP[0].x, IN[1].xxxx", &mut shader, &mut line); // 1
    push(
        "ADD TEMP[0].y, TEMP[0].xxxx, TEMP[0].xxxx",
        &mut shader,
        &mut line,
    ); // 2
    push(
        "ADD TEMP[0].z, TEMP[0].yyyy, TEMP[0].xxxx",
        &mut shader,
        &mut line,
    ); // 3
    push(
        "ADD TEMP[0].w, TEMP[0].yyyy, TEMP[0].yyyy",
        &mut shader,
        &mut line,
    ); // 4
    push(
        "SUB TEMP[1].w, TEMP[0].xxxx, TEMP[0].xxxx",
        &mut shader,
        &mut line,
    ); // 0

    // Map uv -> complex plane: x=-2..1, y=1..-1.
    push(
        "MUL TEMP[1].x, IN[0].xxxx, TEMP[0].zzzz",
        &mut shader,
        &mut line,
    ); // u*3
    push(
        "SUB TEMP[1].x, TEMP[1].xxxx, TEMP[0].yyyy",
        &mut shader,
        &mut line,
    ); // -2 + u*3
    push(
        "MUL TEMP[1].y, IN[0].yyyy, TEMP[0].yyyy",
        &mut shader,
        &mut line,
    ); // v*2
    push(
        "SUB TEMP[1].y, TEMP[0].xxxx, TEMP[1].yyyy",
        &mut shader,
        &mut line,
    ); // 1 - v*2

    push("MOV TEMP[2].x, TEMP[1].wwww", &mut shader, &mut line); // zr = 0
    push("MOV TEMP[2].y, TEMP[1].wwww", &mut shader, &mut line); // zi = 0
    push("MOV TEMP[2].z, TEMP[0].xxxx", &mut shader, &mut line); // alive = 1
    push("MOV TEMP[2].w, TEMP[1].wwww", &mut shader, &mut line); // iter sum = 0

    for _ in 0..iterations {
        push(
            "MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[3].y, TEMP[2].yyyy, TEMP[2].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[3].z, TEMP[2].xxxx, TEMP[2].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[3].w, TEMP[3].xxxx, TEMP[3].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "SLT TEMP[4].x, TEMP[3].wwww, TEMP[0].wwww",
            &mut shader,
            &mut line,
        ); // mag2 < 4
        push(
            "MUL TEMP[4].x, TEMP[4].xxxx, TEMP[2].zzzz",
            &mut shader,
            &mut line,
        ); // alive mask
        push(
            "SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx",
            &mut shader,
            &mut line,
        ); // inverse mask
        push(
            "SUB TEMP[4].z, TEMP[3].xxxx, TEMP[3].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[4].z, TEMP[4].zzzz, TEMP[1].xxxx",
            &mut shader,
            &mut line,
        ); // new zr
        push(
            "ADD TEMP[4].w, TEMP[3].zzzz, TEMP[3].zzzz",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[4].w, TEMP[4].wwww, TEMP[1].yyyy",
            &mut shader,
            &mut line,
        ); // new zi
        push(
            "MUL TEMP[5].x, TEMP[4].zzzz, TEMP[4].xxxx",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[5].y, TEMP[4].wwww, TEMP[4].xxxx",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[5].z, TEMP[2].xxxx, TEMP[4].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[5].w, TEMP[2].yyyy, TEMP[4].yyyy",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[2].x, TEMP[5].xxxx, TEMP[5].zzzz",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[2].y, TEMP[5].yyyy, TEMP[5].wwww",
            &mut shader,
            &mut line,
        );
        push(
            "ADD TEMP[2].w, TEMP[2].wwww, TEMP[4].xxxx",
            &mut shader,
            &mut line,
        );
        push("MOV TEMP[2].z, TEMP[4].xxxx", &mut shader, &mut line);
    }

    // Map the escape count into the pink -> plum palette.
    push("MOV TEMP[3].xyz, IMM[14].xyzx", &mut shader, &mut line);
    for idx in (0..14).rev() {
        let line_a = format!("SLT TEMP[4].x, TEMP[2].wwww, IMM[{}].wwww", idx);
        push(&line_a, &mut shader, &mut line);
        push(
            "SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx",
            &mut shader,
            &mut line,
        );
        push(
            "MUL TEMP[5].xyz, TEMP[3].xyzx, TEMP[4].yyyy",
            &mut shader,
            &mut line,
        );
        let line_b = format!("MUL TEMP[3].xyz, IMM[{}].xyzx, TEMP[4].xxxx", idx);
        push(&line_b, &mut shader, &mut line);
        push(
            "ADD TEMP[3].xyz, TEMP[5].xyzx, TEMP[3].xyzx",
            &mut shader,
            &mut line,
        );
    }
    push("MOV OUT[0].xyz, TEMP[3].xyzx", &mut shader, &mut line);
    push("MOV OUT[0].w, TEMP[0].xxxx", &mut shader, &mut line);
    push("END", &mut shader, &mut line);
    shader
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn zoom_progress(ticks: u64, tick_hz: u64) -> f32 {
    let leg_ticks = tick_hz.saturating_mul(ZOOM_DURATION_SECS).max(1);
    let cycle_ticks = leg_ticks.saturating_mul(2).max(1);
    let phase = (ticks % cycle_ticks) as f32 / cycle_ticks as f32;
    let t = if phase <= 0.5 {
        phase * 2.0
    } else {
        (1.0 - phase) * 2.0
    };
    t * t * (3.0 - 2.0 * t)
}

fn animated_view_uvs(ticks: u64, tick_hz: u64) -> (f32, f32, f32, f32) {
    let t = zoom_progress(ticks, tick_hz);
    let center_x = lerp_f32(FULL_CENTER_X, SEA_HORSE_CENTER_X, t);
    let center_y = lerp_f32(FULL_CENTER_Y, SEA_HORSE_CENTER_Y, t);
    let x_span = lerp_f32(FULL_X_SPAN, SEA_HORSE_X_SPAN_X8, t);
    let y_span = lerp_f32(FULL_Y_SPAN, SEA_HORSE_Y_SPAN_X8, t);

    let x0 = center_x - x_span;
    let x1 = center_x + x_span;
    let y0 = center_y - y_span;
    let y1 = center_y + y_span;

    let u0 = (x0 + 2.0) / 3.0;
    let u1 = (x1 + 2.0) / 3.0;
    let v0 = (1.0 - y1) / 2.0;
    let v1 = (1.0 - y0) / 2.0;

    (u0, v0, u1, v1)
}

pub fn fullscreen_quad_rgba_bytes_for_view(ticks: u64, tick_hz: u64) -> Vec<u8> {
    let (u0, v0, u1, v1) = animated_view_uvs(ticks, tick_hz);
    let color = Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF);
    let verts = [
        TexVertex {
            x: -1.0,
            y: 1.0,
            u: u0,
            v: v0,
            color,
        },
        TexVertex {
            x: 1.0,
            y: 1.0,
            u: u1,
            v: v0,
            color,
        },
        TexVertex {
            x: 1.0,
            y: -1.0,
            u: u1,
            v: v1,
            color,
        },
        TexVertex {
            x: -1.0,
            y: 1.0,
            u: u0,
            v: v0,
            color,
        },
        TexVertex {
            x: 1.0,
            y: -1.0,
            u: u1,
            v: v1,
            color,
        },
        TexVertex {
            x: -1.0,
            y: -1.0,
            u: u0,
            v: v1,
            color,
        },
    ];

    let mut out = Vec::with_capacity(verts.len().saturating_mul(TEX_VERTEX_SIZE));
    for vertex in verts {
        push_tex_vertex_bytes(&mut out, vertex);
    }
    out
}
