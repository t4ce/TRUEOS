extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use trueos_gfx_core::{Rgba8, push_tex_quad_ndc, tex_vertices_byte_len};

pub const MANDELBROT_PIPELINE_FS_TAG_RAW: u32 = 0x4D44_4C42;
pub const JULIA_PIPELINE_FS_TAG_RAW: u32 = 0x4A55_4C49;
pub const BURNING_SHIP_PIPELINE_FS_TAG_RAW: u32 = 0x4253_4850;
pub const MANDELBROT_ITERATIONS: u32 = 64;
pub const JULIA_ITERATIONS: u32 = 64;
pub const BURNING_SHIP_ITERATIONS: u32 = 64;
pub const MANDELBROT_WGSL_FRAGMENT: &str = r#"struct FragmentOut {
    @location(0) color: vec4<f32>,
};

fn mandelbrot_color(iter_sum: f32) -> vec3<f32> {
    var color = vec3<f32>(0.200000, 0.000000, 0.200000);
    if (iter_sum < 58.0) { color = vec3<f32>(0.321569, 0.000000, 0.400000); }
    if (iter_sum < 52.0) { color = vec3<f32>(0.439216, 0.000000, 0.560784); }
    if (iter_sum < 48.0) { color = vec3<f32>(0.541176, 0.000000, 0.721569); }
    if (iter_sum < 44.0) { color = vec3<f32>(0.627451, 0.000000, 0.784314); }
    if (iter_sum < 40.0) { color = vec3<f32>(0.749020, 0.000000, 0.749020); }
    if (iter_sum < 36.0) { color = vec3<f32>(0.839216, 0.000000, 0.839216); }
    if (iter_sum < 32.0) { color = vec3<f32>(0.941176, 0.000000, 0.901961); }
    if (iter_sum < 28.0) { color = vec3<f32>(1.000000, 0.000000, 0.968627); }
    if (iter_sum < 24.0) { color = vec3<f32>(1.000000, 0.215686, 1.000000); }
    if (iter_sum < 20.0) { color = vec3<f32>(1.000000, 0.388235, 1.000000); }
    if (iter_sum < 16.0) { color = vec3<f32>(1.000000, 0.549020, 1.000000); }
    if (iter_sum < 12.0) { color = vec3<f32>(1.000000, 0.721569, 1.000000); }
    if (iter_sum < 8.0) { color = vec3<f32>(1.000000, 0.839216, 1.000000); }
    if (iter_sum < 4.0) { color = vec3<f32>(1.000000, 0.941176, 1.000000); }
    return color;
}

@fragment
fn fs_main(
    @location(0) uv: vec2<f32>,
    @location(1) vertex_color: vec4<f32>,
) -> FragmentOut {
    var zr = 0.0;
    var zi = 0.0;
    var iter_sum = 0.0;
    let cx = uv.x * 3.0 - 2.0;
    let cy = 1.0 - uv.y * 2.0;
    var alive = 1.0;

    for (var i = 0u; i < 64u; i = i + 1u) {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        let zrzi = zr * zi;
        let inside = select(0.0, 1.0, zr2 + zi2 < 4.0) * alive;
        let outside = 1.0 - inside;
        let next_zr = zr2 - zi2 + cx;
        let next_zi = zrzi + zrzi + cy;
        zr = next_zr * inside + zr * outside;
        zi = next_zi * inside + zi * outside;
        iter_sum = iter_sum + inside;
        alive = inside;
    }

    var out: FragmentOut;
    out.color = vec4<f32>(mandelbrot_color(iter_sum), vertex_color.a);
    return out;
}
"#;
pub const JULIA_WGSL_FRAGMENT: &str = r#"struct FragmentOut {
    @location(0) color: vec4<f32>,
};

fn julia_color(iter_sum: f32) -> vec3<f32> {
    var color = vec3<f32>(0.200000, 0.000000, 0.200000);
    if (iter_sum < 58.0) { color = vec3<f32>(0.321569, 0.000000, 0.400000); }
    if (iter_sum < 52.0) { color = vec3<f32>(0.439216, 0.000000, 0.560784); }
    if (iter_sum < 48.0) { color = vec3<f32>(0.541176, 0.000000, 0.721569); }
    if (iter_sum < 44.0) { color = vec3<f32>(0.627451, 0.000000, 0.784314); }
    if (iter_sum < 40.0) { color = vec3<f32>(0.749020, 0.000000, 0.749020); }
    if (iter_sum < 36.0) { color = vec3<f32>(0.839216, 0.000000, 0.839216); }
    if (iter_sum < 32.0) { color = vec3<f32>(0.941176, 0.000000, 0.901961); }
    if (iter_sum < 28.0) { color = vec3<f32>(1.000000, 0.000000, 0.968627); }
    if (iter_sum < 24.0) { color = vec3<f32>(1.000000, 0.215686, 1.000000); }
    if (iter_sum < 20.0) { color = vec3<f32>(1.000000, 0.388235, 1.000000); }
    if (iter_sum < 16.0) { color = vec3<f32>(1.000000, 0.549020, 1.000000); }
    if (iter_sum < 12.0) { color = vec3<f32>(1.000000, 0.721569, 1.000000); }
    if (iter_sum < 8.0) { color = vec3<f32>(1.000000, 0.839216, 1.000000); }
    if (iter_sum < 4.0) { color = vec3<f32>(1.000000, 0.941176, 1.000000); }
    return color;
}

@fragment
fn fs_main(
    @location(0) uv: vec2<f32>,
    @location(1) vertex_color: vec4<f32>,
) -> FragmentOut {
    var zr = uv.x * 3.0 - 1.5;
    var zi = 1.5 - uv.y * 3.0;
    var iter_sum = 0.0;
    let cx = -0.800000;
    let cy = 0.156000;
    var alive = 1.0;

    for (var i = 0u; i < 64u; i = i + 1u) {
        let zr2 = zr * zr;
        let zi2 = zi * zi;
        let zrzi = zr * zi;
        let inside = select(0.0, 1.0, zr2 + zi2 < 4.0) * alive;
        let outside = 1.0 - inside;
        let next_zr = zr2 - zi2 + cx;
        let next_zi = zrzi + zrzi + cy;
        zr = next_zr * inside + zr * outside;
        zi = next_zi * inside + zi * outside;
        iter_sum = iter_sum + inside;
        alive = inside;
    }

    var out: FragmentOut;
    out.color = vec4<f32>(julia_color(iter_sum), vertex_color.a);
    return out;
}
"#;
pub const BURNING_SHIP_WGSL_FRAGMENT: &str = r#"struct FragmentOut {
    @location(0) color: vec4<f32>,
};

fn burning_ship_color(iter_sum: f32) -> vec3<f32> {
    var color = vec3<f32>(0.200000, 0.000000, 0.200000);
    if (iter_sum < 58.0) { color = vec3<f32>(0.321569, 0.000000, 0.400000); }
    if (iter_sum < 52.0) { color = vec3<f32>(0.439216, 0.000000, 0.560784); }
    if (iter_sum < 48.0) { color = vec3<f32>(0.541176, 0.000000, 0.721569); }
    if (iter_sum < 44.0) { color = vec3<f32>(0.627451, 0.000000, 0.784314); }
    if (iter_sum < 40.0) { color = vec3<f32>(0.749020, 0.000000, 0.749020); }
    if (iter_sum < 36.0) { color = vec3<f32>(0.839216, 0.000000, 0.839216); }
    if (iter_sum < 32.0) { color = vec3<f32>(0.941176, 0.000000, 0.901961); }
    if (iter_sum < 28.0) { color = vec3<f32>(1.000000, 0.000000, 0.968627); }
    if (iter_sum < 24.0) { color = vec3<f32>(1.000000, 0.215686, 1.000000); }
    if (iter_sum < 20.0) { color = vec3<f32>(1.000000, 0.388235, 1.000000); }
    if (iter_sum < 16.0) { color = vec3<f32>(1.000000, 0.549020, 1.000000); }
    if (iter_sum < 12.0) { color = vec3<f32>(1.000000, 0.721569, 1.000000); }
    if (iter_sum < 8.0) { color = vec3<f32>(1.000000, 0.839216, 1.000000); }
    if (iter_sum < 4.0) { color = vec3<f32>(1.000000, 0.941176, 1.000000); }
    return color;
}

@fragment
fn fs_main(
    @location(0) uv: vec2<f32>,
    @location(1) vertex_color: vec4<f32>,
) -> FragmentOut {
    var zr = 0.0;
    var zi = 0.0;
    var iter_sum = 0.0;
    let cx = uv.x * 3.4 - 2.2;
    let cy = 1.0 - uv.y * 3.0;
    var alive = 1.0;

    for (var i = 0u; i < 64u; i = i + 1u) {
        let ar = abs(zr);
        let ai = abs(zi);
        let zr2 = ar * ar;
        let zi2 = ai * ai;
        let inside = select(0.0, 1.0, zr2 + zi2 < 4.0) * alive;
        let outside = 1.0 - inside;
        let next_zr = zr2 - zi2 + cx;
        let next_zi = ar * ai * 2.0 + cy;
        zr = next_zr * inside + zr * outside;
        zi = next_zi * inside + zi * outside;
        iter_sum = iter_sum + inside;
        alive = inside;
    }

    var out: FragmentOut;
    out.color = vec4<f32>(burning_ship_color(iter_sum), vertex_color.a);
    return out;
}
"#;
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

fn color_for_iter_sum(iter_sum: f32) -> [f32; 4] {
    let mut rgb = MANDELBROT_PALETTE[14].0;
    for idx in (0..14).rev() {
        if iter_sum < MANDELBROT_PALETTE[idx].1 {
            rgb = MANDELBROT_PALETTE[idx].0;
        }
    }
    [
        rgb[0] as f32 / 255.0,
        rgb[1] as f32 / 255.0,
        rgb[2] as f32 / 255.0,
        1.0,
    ]
}

pub fn shade_uv_simd16(
    us: [f32; 16],
    vs: [f32; 16],
    dispatch_mask: u16,
    iterations: u32,
) -> [[f32; 4]; 16] {
    let mut zr = [0.0f32; 16];
    let mut zi = [0.0f32; 16];
    let mut iter_sum = [0.0f32; 16];
    let mut cx = [0.0f32; 16];
    let mut cy = [0.0f32; 16];
    let mut live_mask = dispatch_mask;

    for lane in 0..16 {
        cx[lane] = us[lane] * 3.0 - 2.0;
        cy[lane] = 1.0 - vs[lane] * 2.0;
    }

    for _ in 0..iterations {
        if live_mask == 0 {
            break;
        }
        for lane in 0..16 {
            let lane_bit = 1u16 << lane;
            if (live_mask & lane_bit) == 0 {
                continue;
            }

            let zr2 = zr[lane] * zr[lane];
            let zi2 = zi[lane] * zi[lane];
            if zr2 + zi2 < 4.0 {
                let zrzi = zr[lane] * zi[lane];
                zr[lane] = zr2 - zi2 + cx[lane];
                zi[lane] = zrzi + zrzi + cy[lane];
                iter_sum[lane] += 1.0;
            } else {
                live_mask &= !lane_bit;
            }
        }
    }

    let mut out = [[0.0f32, 0.0, 0.0, 0.0]; 16];
    for lane in 0..16 {
        if (dispatch_mask & (1u16 << lane)) != 0 {
            out[lane] = color_for_iter_sum(iter_sum[lane]);
        }
    }
    out
}

pub fn shade_julia_uv_simd16(
    us: [f32; 16],
    vs: [f32; 16],
    dispatch_mask: u16,
    iterations: u32,
) -> [[f32; 4]; 16] {
    let mut zr = [0.0f32; 16];
    let mut zi = [0.0f32; 16];
    let mut iter_sum = [0.0f32; 16];
    let mut live_mask = dispatch_mask;

    for lane in 0..16 {
        zr[lane] = us[lane] * 3.0 - 1.5;
        zi[lane] = 1.5 - vs[lane] * 3.0;
    }

    for _ in 0..iterations {
        if live_mask == 0 {
            break;
        }
        for lane in 0..16 {
            let lane_bit = 1u16 << lane;
            if (live_mask & lane_bit) == 0 {
                continue;
            }

            let zr2 = zr[lane] * zr[lane];
            let zi2 = zi[lane] * zi[lane];
            if zr2 + zi2 < 4.0 {
                let zrzi = zr[lane] * zi[lane];
                zr[lane] = zr2 - zi2 - 0.8;
                zi[lane] = zrzi + zrzi + 0.156;
                iter_sum[lane] += 1.0;
            } else {
                live_mask &= !lane_bit;
            }
        }
    }

    let mut out = [[0.0f32, 0.0, 0.0, 0.0]; 16];
    for lane in 0..16 {
        if (dispatch_mask & (1u16 << lane)) != 0 {
            out[lane] = color_for_iter_sum(iter_sum[lane]);
        }
    }
    out
}

pub fn shade_burning_ship_uv_simd16(
    us: [f32; 16],
    vs: [f32; 16],
    dispatch_mask: u16,
    iterations: u32,
) -> [[f32; 4]; 16] {
    let mut zr = [0.0f32; 16];
    let mut zi = [0.0f32; 16];
    let mut iter_sum = [0.0f32; 16];
    let mut cx = [0.0f32; 16];
    let mut cy = [0.0f32; 16];
    let mut live_mask = dispatch_mask;

    for lane in 0..16 {
        cx[lane] = us[lane] * 3.4 - 2.2;
        cy[lane] = 1.0 - vs[lane] * 3.0;
    }

    for _ in 0..iterations {
        if live_mask == 0 {
            break;
        }
        for lane in 0..16 {
            let lane_bit = 1u16 << lane;
            if (live_mask & lane_bit) == 0 {
                continue;
            }

            let ar = zr[lane].abs();
            let ai = zi[lane].abs();
            let zr2 = ar * ar;
            let zi2 = ai * ai;
            if zr2 + zi2 < 4.0 {
                zr[lane] = zr2 - zi2 + cx[lane];
                zi[lane] = ar * ai * 2.0 + cy[lane];
                iter_sum[lane] += 1.0;
            } else {
                live_mask &= !lane_bit;
            }
        }
    }

    let mut out = [[0.0f32, 0.0, 0.0, 0.0]; 16];
    for lane in 0..16 {
        if (dispatch_mask & (1u16 << lane)) != 0 {
            out[lane] = color_for_iter_sum(iter_sum[lane]);
        }
    }
    out
}

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
    push("ADD TEMP[0].y, TEMP[0].xxxx, TEMP[0].xxxx", &mut shader, &mut line); // 2
    push("ADD TEMP[0].z, TEMP[0].yyyy, TEMP[0].xxxx", &mut shader, &mut line); // 3
    push("ADD TEMP[0].w, TEMP[0].yyyy, TEMP[0].yyyy", &mut shader, &mut line); // 4
    push("SUB TEMP[1].w, TEMP[0].xxxx, TEMP[0].xxxx", &mut shader, &mut line); // 0

    // Map uv -> complex plane: x=-2..1, y=1..-1.
    push("MUL TEMP[1].x, IN[0].xxxx, TEMP[0].zzzz", &mut shader, &mut line); // u*3
    push("SUB TEMP[1].x, TEMP[1].xxxx, TEMP[0].yyyy", &mut shader, &mut line); // -2 + u*3
    push("MUL TEMP[1].y, IN[0].yyyy, TEMP[0].yyyy", &mut shader, &mut line); // v*2
    push("SUB TEMP[1].y, TEMP[0].xxxx, TEMP[1].yyyy", &mut shader, &mut line); // 1 - v*2

    push("MOV TEMP[2].x, TEMP[1].wwww", &mut shader, &mut line); // zr = 0
    push("MOV TEMP[2].y, TEMP[1].wwww", &mut shader, &mut line); // zi = 0
    push("MOV TEMP[2].z, TEMP[0].xxxx", &mut shader, &mut line); // alive = 1
    push("MOV TEMP[2].w, TEMP[1].wwww", &mut shader, &mut line); // iter sum = 0

    for _ in 0..iterations {
        push("MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx", &mut shader, &mut line);
        push("MUL TEMP[3].y, TEMP[2].yyyy, TEMP[2].yyyy", &mut shader, &mut line);
        push("MUL TEMP[3].z, TEMP[2].xxxx, TEMP[2].yyyy", &mut shader, &mut line);
        push("ADD TEMP[3].w, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("SLT TEMP[4].x, TEMP[3].wwww, TEMP[0].wwww", &mut shader, &mut line); // mag2 < 4
        push("MUL TEMP[4].x, TEMP[4].xxxx, TEMP[2].zzzz", &mut shader, &mut line); // alive mask
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line); // inverse mask
        push("SUB TEMP[4].z, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("ADD TEMP[4].z, TEMP[4].zzzz, TEMP[1].xxxx", &mut shader, &mut line); // new zr
        push("ADD TEMP[4].w, TEMP[3].zzzz, TEMP[3].zzzz", &mut shader, &mut line);
        push("ADD TEMP[4].w, TEMP[4].wwww, TEMP[1].yyyy", &mut shader, &mut line); // new zi
        push("MUL TEMP[5].x, TEMP[4].zzzz, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].y, TEMP[4].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].z, TEMP[2].xxxx, TEMP[4].yyyy", &mut shader, &mut line);
        push("MUL TEMP[5].w, TEMP[2].yyyy, TEMP[4].yyyy", &mut shader, &mut line);
        push("ADD TEMP[2].x, TEMP[5].xxxx, TEMP[5].zzzz", &mut shader, &mut line);
        push("ADD TEMP[2].y, TEMP[5].yyyy, TEMP[5].wwww", &mut shader, &mut line);
        push("ADD TEMP[2].w, TEMP[2].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MOV TEMP[2].z, TEMP[4].xxxx", &mut shader, &mut line);
    }

    // Map the escape count into the pink -> plum palette.
    push("MOV TEMP[3].xyz, IMM[14].xyzx", &mut shader, &mut line);
    for idx in (0..14).rev() {
        let line_a = format!("SLT TEMP[4].x, TEMP[2].wwww, IMM[{}].wwww", idx);
        push(&line_a, &mut shader, &mut line);
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].xyz, TEMP[3].xyzx, TEMP[4].yyyy", &mut shader, &mut line);
        let line_b = format!("MUL TEMP[3].xyz, IMM[{}].xyzx, TEMP[4].xxxx", idx);
        push(&line_b, &mut shader, &mut line);
        push("ADD TEMP[3].xyz, TEMP[5].xyzx, TEMP[3].xyzx", &mut shader, &mut line);
    }
    push("MOV OUT[0].xyz, TEMP[3].xyzx", &mut shader, &mut line);
    push("MOV OUT[0].w, TEMP[0].xxxx", &mut shader, &mut line);
    push("END", &mut shader, &mut line);
    shader
}

pub fn build_julia_fragment_shader_tgsi_unrolled(iterations: u32) -> String {
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
    shader.push_str("IMM[15] FLT32 { -0.800000, 0.156000, 1.500000, 0.0 }\n");

    let mut line = 0u32;
    let push = |text: &str, out: &mut String, line_no: &mut u32| {
        let _ = writeln!(out, "    {}: {}", *line_no, text);
        *line_no = line_no.wrapping_add(1);
    };

    push("MOV TEMP[0].x, IN[1].xxxx", &mut shader, &mut line); // 1
    push("ADD TEMP[0].y, TEMP[0].xxxx, TEMP[0].xxxx", &mut shader, &mut line); // 2
    push("ADD TEMP[0].z, TEMP[0].yyyy, TEMP[0].xxxx", &mut shader, &mut line); // 3
    push("ADD TEMP[0].w, TEMP[0].yyyy, TEMP[0].yyyy", &mut shader, &mut line); // 4

    // Julia: z starts at the mapped point, c is fixed.
    push("MUL TEMP[2].x, IN[0].xxxx, TEMP[0].zzzz", &mut shader, &mut line); // u*3
    push("SUB TEMP[2].x, TEMP[2].xxxx, IMM[15].zzzz", &mut shader, &mut line); // u*3 - 1.5
    push("MUL TEMP[2].y, IN[0].yyyy, TEMP[0].zzzz", &mut shader, &mut line); // v*3
    push("SUB TEMP[2].y, IMM[15].zzzz, TEMP[2].yyyy", &mut shader, &mut line); // 1.5 - v*3
    push("MOV TEMP[2].z, TEMP[0].xxxx", &mut shader, &mut line); // alive = 1
    push("MOV TEMP[2].w, IMM[15].wwww", &mut shader, &mut line); // iter sum = 0
    push("MOV TEMP[1].x, IMM[15].xxxx", &mut shader, &mut line); // cx
    push("MOV TEMP[1].y, IMM[15].yyyy", &mut shader, &mut line); // cy

    for _ in 0..iterations {
        push("MUL TEMP[3].x, TEMP[2].xxxx, TEMP[2].xxxx", &mut shader, &mut line);
        push("MUL TEMP[3].y, TEMP[2].yyyy, TEMP[2].yyyy", &mut shader, &mut line);
        push("MUL TEMP[3].z, TEMP[2].xxxx, TEMP[2].yyyy", &mut shader, &mut line);
        push("ADD TEMP[3].w, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("SLT TEMP[4].x, TEMP[3].wwww, TEMP[0].wwww", &mut shader, &mut line);
        push("MUL TEMP[4].x, TEMP[4].xxxx, TEMP[2].zzzz", &mut shader, &mut line);
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
        push("SUB TEMP[4].z, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("ADD TEMP[4].z, TEMP[4].zzzz, TEMP[1].xxxx", &mut shader, &mut line);
        push("ADD TEMP[4].w, TEMP[3].zzzz, TEMP[3].zzzz", &mut shader, &mut line);
        push("ADD TEMP[4].w, TEMP[4].wwww, TEMP[1].yyyy", &mut shader, &mut line);
        push("MUL TEMP[5].x, TEMP[4].zzzz, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].y, TEMP[4].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].z, TEMP[2].xxxx, TEMP[4].yyyy", &mut shader, &mut line);
        push("MUL TEMP[5].w, TEMP[2].yyyy, TEMP[4].yyyy", &mut shader, &mut line);
        push("ADD TEMP[2].x, TEMP[5].xxxx, TEMP[5].zzzz", &mut shader, &mut line);
        push("ADD TEMP[2].y, TEMP[5].yyyy, TEMP[5].wwww", &mut shader, &mut line);
        push("ADD TEMP[2].w, TEMP[2].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MOV TEMP[2].z, TEMP[4].xxxx", &mut shader, &mut line);
    }

    push("MOV TEMP[3].xyz, IMM[14].xyzx", &mut shader, &mut line);
    for idx in (0..14).rev() {
        let line_a = format!("SLT TEMP[4].x, TEMP[2].wwww, IMM[{}].wwww", idx);
        push(&line_a, &mut shader, &mut line);
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].xyz, TEMP[3].xyzx, TEMP[4].yyyy", &mut shader, &mut line);
        let line_b = format!("MUL TEMP[3].xyz, IMM[{}].xyzx, TEMP[4].xxxx", idx);
        push(&line_b, &mut shader, &mut line);
        push("ADD TEMP[3].xyz, TEMP[5].xyzx, TEMP[3].xyzx", &mut shader, &mut line);
    }
    push("MOV OUT[0].xyz, TEMP[3].xyzx", &mut shader, &mut line);
    push("MOV OUT[0].w, TEMP[0].xxxx", &mut shader, &mut line);
    push("END", &mut shader, &mut line);
    shader
}

pub fn build_burning_ship_fragment_shader_tgsi_unrolled(iterations: u32) -> String {
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
    shader.push_str("DCL TEMP[6]\n");
    emit_palette_immediates(&mut shader);
    shader.push_str("IMM[15] FLT32 { 2.200000, 3.400000, 3.000000, 0.0 }\n");

    let mut line = 0u32;
    let push = |text: &str, out: &mut String, line_no: &mut u32| {
        let _ = writeln!(out, "    {}: {}", *line_no, text);
        *line_no = line_no.wrapping_add(1);
    };

    push("MOV TEMP[0].x, IN[1].xxxx", &mut shader, &mut line); // 1
    push("ADD TEMP[0].y, TEMP[0].xxxx, TEMP[0].xxxx", &mut shader, &mut line); // 2
    push("ADD TEMP[0].z, TEMP[0].yyyy, TEMP[0].xxxx", &mut shader, &mut line); // 3
    push("ADD TEMP[0].w, TEMP[0].yyyy, TEMP[0].yyyy", &mut shader, &mut line); // 4

    // Burning Ship: z starts at zero, c is mapped into x=-2.2..1.2, y=1..-2.
    push("MUL TEMP[1].x, IN[0].xxxx, IMM[15].yyyy", &mut shader, &mut line); // u*3.4
    push("SUB TEMP[1].x, TEMP[1].xxxx, IMM[15].xxxx", &mut shader, &mut line); // u*3.4 - 2.2
    push("MUL TEMP[1].y, IN[0].yyyy, IMM[15].zzzz", &mut shader, &mut line); // v*3
    push("SUB TEMP[1].y, TEMP[0].xxxx, TEMP[1].yyyy", &mut shader, &mut line); // 1 - v*3
    push("MOV TEMP[2].x, IMM[15].wwww", &mut shader, &mut line); // zr = 0
    push("MOV TEMP[2].y, IMM[15].wwww", &mut shader, &mut line); // zi = 0
    push("MOV TEMP[2].z, TEMP[0].xxxx", &mut shader, &mut line); // alive = 1
    push("MOV TEMP[2].w, IMM[15].wwww", &mut shader, &mut line); // iter sum = 0

    for _ in 0..iterations {
        push("ABS TEMP[6].x, TEMP[2].xxxx", &mut shader, &mut line);
        push("ABS TEMP[6].y, TEMP[2].yyyy", &mut shader, &mut line);
        push("MUL TEMP[3].x, TEMP[6].xxxx, TEMP[6].xxxx", &mut shader, &mut line);
        push("MUL TEMP[3].y, TEMP[6].yyyy, TEMP[6].yyyy", &mut shader, &mut line);
        push("MUL TEMP[3].z, TEMP[6].xxxx, TEMP[6].yyyy", &mut shader, &mut line);
        push("ADD TEMP[3].w, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("SLT TEMP[4].x, TEMP[3].wwww, TEMP[0].wwww", &mut shader, &mut line);
        push("MUL TEMP[4].x, TEMP[4].xxxx, TEMP[2].zzzz", &mut shader, &mut line);
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
        push("SUB TEMP[4].z, TEMP[3].xxxx, TEMP[3].yyyy", &mut shader, &mut line);
        push("ADD TEMP[4].z, TEMP[4].zzzz, TEMP[1].xxxx", &mut shader, &mut line);
        push("ADD TEMP[4].w, TEMP[3].zzzz, TEMP[3].zzzz", &mut shader, &mut line);
        push("ADD TEMP[4].w, TEMP[4].wwww, TEMP[1].yyyy", &mut shader, &mut line);
        push("MUL TEMP[5].x, TEMP[4].zzzz, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].y, TEMP[4].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].z, TEMP[2].xxxx, TEMP[4].yyyy", &mut shader, &mut line);
        push("MUL TEMP[5].w, TEMP[2].yyyy, TEMP[4].yyyy", &mut shader, &mut line);
        push("ADD TEMP[2].x, TEMP[5].xxxx, TEMP[5].zzzz", &mut shader, &mut line);
        push("ADD TEMP[2].y, TEMP[5].yyyy, TEMP[5].wwww", &mut shader, &mut line);
        push("ADD TEMP[2].w, TEMP[2].wwww, TEMP[4].xxxx", &mut shader, &mut line);
        push("MOV TEMP[2].z, TEMP[4].xxxx", &mut shader, &mut line);
    }

    push("MOV TEMP[3].xyz, IMM[14].xyzx", &mut shader, &mut line);
    for idx in (0..14).rev() {
        let line_a = format!("SLT TEMP[4].x, TEMP[2].wwww, IMM[{}].wwww", idx);
        push(&line_a, &mut shader, &mut line);
        push("SUB TEMP[4].y, TEMP[0].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
        push("MUL TEMP[5].xyz, TEMP[3].xyzx, TEMP[4].yyyy", &mut shader, &mut line);
        let line_b = format!("MUL TEMP[3].xyz, IMM[{}].xyzx, TEMP[4].xxxx", idx);
        push(&line_b, &mut shader, &mut line);
        push("ADD TEMP[3].xyz, TEMP[5].xyzx, TEMP[3].xyzx", &mut shader, &mut line);
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
    let mut out = Vec::with_capacity(tex_vertices_byte_len(6));
    push_tex_quad_ndc(
        &mut out,
        -1.0,
        1.0,
        1.0,
        -1.0,
        [u0, v0, u1, v1],
        Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF),
    );
    out
}

pub fn fullscreen_quad_rgba_bytes_for_julia_view() -> Vec<u8> {
    let mut out = Vec::with_capacity(tex_vertices_byte_len(6));
    push_tex_quad_ndc(
        &mut out,
        -1.0,
        1.0,
        1.0,
        -1.0,
        [0.0, 0.0, 1.0, 1.0],
        Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF),
    );
    out
}

pub fn fullscreen_quad_rgba_bytes_for_burning_ship_view() -> Vec<u8> {
    let mut out = Vec::with_capacity(tex_vertices_byte_len(6));
    push_tex_quad_ndc(
        &mut out,
        -1.0,
        1.0,
        1.0,
        -1.0,
        [0.0, 0.0, 1.0, 1.0],
        Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF),
    );
    out
}
