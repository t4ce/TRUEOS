#![allow(dead_code)]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use trueos_gfx_core::{Rgba8, push_tex_quad_ndc, tex_vertices_byte_len};

pub const BEE_TILES_PIPELINE_FS_TAG_RAW: u32 = 0x4245_4531;

const PAN_X_PX_PER_SEC: f32 = 10.0;
const PAN_Y_PX_PER_SEC: f32 = 5.5;
const HEX_R: f32 = 28.0;
const BORDER_PX: f32 = 0.0;
const SQRT_3: f32 = 1.732_050_8;

const TEAL_PALETTE: [[f32; 3]; 21] = [
    [0.811765, 0.925490, 0.925490],
    [0.686275, 0.933333, 0.933333],
    [0.705882, 0.803922, 0.803922],
    [0.509804, 0.768627, 0.764706],
    [0.372549, 0.701961, 0.701961],
    [0.000000, 0.545098, 0.545098],
    [0.000000, 0.498039, 0.498039],
    [0.000000, 1.000000, 1.000000],
    [0.000000, 1.000000, 0.752941],
    [0.000000, 0.807843, 0.819608],
    [0.235294, 0.717647, 0.631373],
    [0.000000, 0.600000, 0.600000],
    [0.000000, 0.501961, 0.501961],
    [0.211765, 0.458824, 0.533333],
    [0.290196, 0.392157, 0.423529],
    [0.231373, 0.443137, 0.435294],
    [0.035294, 0.474510, 0.411765],
    [0.000000, 0.427451, 0.356863],
    [0.301961, 0.333333, 0.341176],
    [0.003922, 0.301961, 0.305882],
    [0.000000, 0.200000, 0.200000],
];

const MAGENTA_PALETTE: [[f32; 3]; 15] = [
    [1.000000, 0.941176, 1.000000],
    [1.000000, 0.839216, 1.000000],
    [1.000000, 0.721569, 1.000000],
    [1.000000, 0.549020, 1.000000],
    [1.000000, 0.388235, 1.000000],
    [1.000000, 0.215686, 1.000000],
    [1.000000, 0.000000, 0.968627],
    [0.941176, 0.000000, 0.901961],
    [0.839216, 0.000000, 0.839216],
    [0.749020, 0.000000, 0.749020],
    [0.627451, 0.000000, 0.784314],
    [0.541176, 0.000000, 0.721569],
    [0.439216, 0.000000, 0.560784],
    [0.321569, 0.000000, 0.400000],
    [0.200000, 0.000000, 0.200000],
];

#[derive(Clone, Copy)]
struct AxialHex {
    q: i32,
    r: i32,
}

fn cube_round(qf: f32, rf: f32) -> AxialHex {
    let sf = -qf - rf;
    let mut q = libm::roundf(qf) as i32;
    let mut r = libm::roundf(rf) as i32;
    let s = libm::roundf(sf) as i32;
    let q_diff = (q as f32 - qf).abs();
    let r_diff = (r as f32 - rf).abs();
    let s_diff = (s as f32 - sf).abs();
    if q_diff > r_diff && q_diff > s_diff {
        q = -r - s;
    } else if r_diff > s_diff {
        r = -q - s;
    }
    AxialHex { q, r }
}

fn contains_flat_hex(dx: f32, dy: f32, radius: f32) -> bool {
    let x = dx.abs();
    let y = dy.abs();
    x <= radius && y <= SQRT_3 * radius * 0.5 && SQRT_3 * x + y <= SQRT_3 * radius
}

fn shade_uv(u: f32, v: f32) -> [f32; 4] {
    let qf = (2.0 / 3.0 * u) / HEX_R;
    let rf = (-1.0 / 3.0 * u + SQRT_3 / 3.0 * v) / HEX_R;
    let hex = cube_round(qf, rf);
    let cx = HEX_R * 1.5 * hex.q as f32;
    let cy = HEX_R * SQRT_3 * (hex.r as f32 + hex.q as f32 * 0.5);
    let dx = u - cx;
    let dy = v - cy;
    if !contains_flat_hex(dx, dy, HEX_R) || !contains_flat_hex(dx, dy, HEX_R - BORDER_PX) {
        return [0.0, 0.0, 0.0, 0.0];
    }

    let seed = hex
        .q
        .wrapping_mul(73_856_093)
        .wrapping_add(hex.r.wrapping_mul(19_349_663))
        .wrapping_add((hex.q + hex.r).wrapping_mul(83_492_791));
    let index = seed.unsigned_abs() as usize;
    let rgb = if (hex.q ^ hex.r) & 1 == 0 {
        TEAL_PALETTE[index % TEAL_PALETTE.len()]
    } else {
        MAGENTA_PALETTE[index % MAGENTA_PALETTE.len()]
    };
    [rgb[0], rgb[1], rgb[2], 1.0]
}

pub fn shade_uv_simd16(us: [f32; 16], vs: [f32; 16], dispatch_mask: u16) -> [[f32; 4]; 16] {
    let mut out = [[0.0f32, 0.0, 0.0, 0.0]; 16];
    for lane in 0..16 {
        if (dispatch_mask & (1u16 << lane)) != 0 {
            out[lane] = shade_uv(us[lane], vs[lane]);
        }
    }
    out
}

pub const BEE_TILES_WGSL_FRAGMENT: &str = r#"struct FragmentOut {
    @location(0) color: vec4<f32>,
};

const HEX_R: f32 = 28.0;
const BORDER_PX: f32 = 0.0;
const SQRT_3: f32 = 1.7320508;

fn contains_flat_hex(d: vec2<f32>, r: f32) -> bool {
    let p = abs(d);
    return p.x <= r &&
        p.y <= SQRT_3 * r * 0.5 &&
        SQRT_3 * p.x + p.y <= SQRT_3 * r;
}

fn cube_round(qf: f32, rf: f32) -> vec2<i32> {
    let sf = -qf - rf;
    var q = i32(round(qf));
    var r = i32(round(rf));
    let s = i32(round(sf));
    let qd = abs(f32(q) - qf);
    let rd = abs(f32(r) - rf);
    let sd = abs(f32(s) - sf);
    if (qd > rd && qd > sd) {
        q = -r - s;
    } else if (rd > sd) {
        r = -q - s;
    }
    return vec2<i32>(q, r);
}

fn teal_palette(i: u32) -> vec3<f32> {
    var c = vec3<f32>(0.0, 0.2, 0.2);
    if (i == 0u) { c = vec3<f32>(0.811765, 0.925490, 0.925490); }
    if (i == 1u) { c = vec3<f32>(0.686275, 0.933333, 0.933333); }
    if (i == 2u) { c = vec3<f32>(0.705882, 0.803922, 0.803922); }
    if (i == 3u) { c = vec3<f32>(0.509804, 0.768627, 0.764706); }
    if (i == 4u) { c = vec3<f32>(0.372549, 0.701961, 0.701961); }
    if (i == 5u) { c = vec3<f32>(0.000000, 0.545098, 0.545098); }
    if (i == 6u) { c = vec3<f32>(0.000000, 0.498039, 0.498039); }
    if (i == 7u) { c = vec3<f32>(0.000000, 1.000000, 1.000000); }
    if (i == 8u) { c = vec3<f32>(0.000000, 1.000000, 0.752941); }
    if (i == 9u) { c = vec3<f32>(0.000000, 0.807843, 0.819608); }
    if (i == 10u) { c = vec3<f32>(0.235294, 0.717647, 0.631373); }
    if (i == 11u) { c = vec3<f32>(0.000000, 0.600000, 0.600000); }
    if (i == 12u) { c = vec3<f32>(0.000000, 0.501961, 0.501961); }
    if (i == 13u) { c = vec3<f32>(0.211765, 0.458824, 0.533333); }
    if (i == 14u) { c = vec3<f32>(0.290196, 0.392157, 0.423529); }
    if (i == 15u) { c = vec3<f32>(0.231373, 0.443137, 0.435294); }
    if (i == 16u) { c = vec3<f32>(0.035294, 0.474510, 0.411765); }
    if (i == 17u) { c = vec3<f32>(0.000000, 0.427451, 0.356863); }
    if (i == 18u) { c = vec3<f32>(0.301961, 0.333333, 0.341176); }
    if (i == 19u) { c = vec3<f32>(0.003922, 0.301961, 0.305882); }
    if (i == 20u) { c = vec3<f32>(0.000000, 0.200000, 0.200000); }
    return c;
}

fn magenta_palette(i: u32) -> vec3<f32> {
    var c = vec3<f32>(0.2, 0.0, 0.2);
    if (i == 0u) { c = vec3<f32>(1.000000, 0.941176, 1.000000); }
    if (i == 1u) { c = vec3<f32>(1.000000, 0.839216, 1.000000); }
    if (i == 2u) { c = vec3<f32>(1.000000, 0.721569, 1.000000); }
    if (i == 3u) { c = vec3<f32>(1.000000, 0.549020, 1.000000); }
    if (i == 4u) { c = vec3<f32>(1.000000, 0.388235, 1.000000); }
    if (i == 5u) { c = vec3<f32>(1.000000, 0.215686, 1.000000); }
    if (i == 6u) { c = vec3<f32>(1.000000, 0.000000, 0.968627); }
    if (i == 7u) { c = vec3<f32>(0.941176, 0.000000, 0.901961); }
    if (i == 8u) { c = vec3<f32>(0.839216, 0.000000, 0.839216); }
    if (i == 9u) { c = vec3<f32>(0.749020, 0.000000, 0.749020); }
    if (i == 10u) { c = vec3<f32>(0.627451, 0.000000, 0.784314); }
    if (i == 11u) { c = vec3<f32>(0.541176, 0.000000, 0.721569); }
    if (i == 12u) { c = vec3<f32>(0.439216, 0.000000, 0.560784); }
    if (i == 13u) { c = vec3<f32>(0.321569, 0.000000, 0.400000); }
    if (i == 14u) { c = vec3<f32>(0.200000, 0.000000, 0.200000); }
    return c;
}

@fragment
fn fs_main(
    @location(0) uv: vec2<f32>,
    @location(1) vertex_color: vec4<f32>,
) -> FragmentOut {
    let qf = (2.0 / 3.0 * uv.x) / HEX_R;
    let rf = (-1.0 / 3.0 * uv.x + SQRT_3 / 3.0 * uv.y) / HEX_R;
    let hex = cube_round(qf, rf);
    let center = vec2<f32>(
        HEX_R * 1.5 * f32(hex.x),
        HEX_R * SQRT_3 * (f32(hex.y) + f32(hex.x) * 0.5),
    );
    let d = uv - center;
    let inside = contains_flat_hex(d, HEX_R) && contains_flat_hex(d, HEX_R - BORDER_PX);
    let seed = u32(abs(hex.x * 73856093 + hex.y * 19349663 + (hex.x + hex.y) * 83492791));
    let color = select(
        magenta_palette(seed % 15u),
        teal_palette(seed % 21u),
        ((hex.x ^ hex.y) & 1) == 0,
    );

    var out: FragmentOut;
    out.color = select(vec4<f32>(0.0, 0.0, 0.0, 0.0), vec4<f32>(color, vertex_color.a), inside);
    return out;
}
"#;

pub fn fullscreen_quad_rgba_bytes_for_view(
    ticks: u64,
    tick_hz: u64,
    width: u32,
    height: u32,
) -> Vec<u8> {
    let seconds = ticks as f32 / tick_hz.max(1) as f32;
    let pan_x = seconds * PAN_X_PX_PER_SEC;
    let pan_y = seconds * PAN_Y_PX_PER_SEC;
    let mut out = Vec::with_capacity(tex_vertices_byte_len(6));
    push_tex_quad_ndc(
        &mut out,
        -1.0,
        1.0,
        1.0,
        -1.0,
        [pan_x, pan_y, pan_x + width as f32, pan_y + height as f32],
        Rgba8::new(0xFF, 0xFF, 0xFF, 0xFF),
    );
    out
}

pub fn build_fragment_shader_tgsi() -> String {
    let mut shader = String::new();
    shader.push_str("FRAG\n");
    shader.push_str("DCL IN[0], TEXCOORD[0], LINEAR\n");
    shader.push_str("DCL IN[1], COLOR, LINEAR\n");
    shader.push_str("DCL OUT[0], COLOR\n");
    for i in 0..9 {
        let _ = writeln!(shader, "DCL TEMP[{}]", i);
    }
    shader.push_str("IMM[0] FLT32 { 0.023810, -0.011905, 0.020619, 0.500000 }\n");
    shader.push_str("IMM[1] FLT32 { 1.000000, 0.000000, 0.250000, 0.750000 }\n");
    shader.push_str("IMM[2] FLT32 { 0.000000, 0.807843, 0.819608, 1.000000 }\n");
    shader.push_str("IMM[3] FLT32 { 1.000000, 0.000000, 0.968627, 1.000000 }\n");
    shader.push_str("IMM[4] FLT32 { 0.686275, 0.933333, 0.933333, 1.000000 }\n");
    shader.push_str("IMM[5] FLT32 { 0.627451, 0.000000, 0.784314, 1.000000 }\n");
    shader.push_str("IMM[6] FLT32 { 0.370000, 0.610000, 0.500000, 1024.000000 }\n");

    let mut line = 0u32;
    let push = |text: &str, out: &mut String, line_no: &mut u32| {
        let _ = writeln!(out, "    {}: {}", *line_no, text);
        *line_no = line_no.wrapping_add(1);
    };

    // Cube-round axial hex coordinates. This assigns every fragment to one hex,
    // so 0px gaps really tile the plane instead of leaving masked-out cell space.
    push("MUL TEMP[0].x, IN[0].xxxx, IMM[0].xxxx", &mut shader, &mut line);
    push("MUL TEMP[0].y, IN[0].xxxx, IMM[0].yyyy", &mut shader, &mut line);
    push("MUL TEMP[0].z, IN[0].yyyy, IMM[0].zzzz", &mut shader, &mut line);
    push("ADD TEMP[0].y, TEMP[0].yyyy, TEMP[0].zzzz", &mut shader, &mut line);
    push("ADD TEMP[1].x, TEMP[0].xxxx, IMM[0].wwww", &mut shader, &mut line);
    push("ADD TEMP[1].y, TEMP[0].yyyy, IMM[0].wwww", &mut shader, &mut line);
    push("ADD TEMP[1].z, TEMP[0].xxxx, TEMP[0].yyyy", &mut shader, &mut line);
    push("NEG TEMP[1].z, TEMP[1].zzzz", &mut shader, &mut line);
    push("ADD TEMP[1].w, TEMP[1].zzzz, IMM[0].wwww", &mut shader, &mut line);
    push("ADD TEMP[2].x, TEMP[1].xxxx, IMM[6].wwww", &mut shader, &mut line);
    push("FRC TEMP[8].x, TEMP[2].xxxx", &mut shader, &mut line);
    push("SUB TEMP[2].x, TEMP[2].xxxx, TEMP[8].xxxx", &mut shader, &mut line);
    push("SUB TEMP[2].x, TEMP[2].xxxx, IMM[6].wwww", &mut shader, &mut line);
    push("ADD TEMP[2].y, TEMP[1].yyyy, IMM[6].wwww", &mut shader, &mut line);
    push("FRC TEMP[8].y, TEMP[2].yyyy", &mut shader, &mut line);
    push("SUB TEMP[2].y, TEMP[2].yyyy, TEMP[8].yyyy", &mut shader, &mut line);
    push("SUB TEMP[2].y, TEMP[2].yyyy, IMM[6].wwww", &mut shader, &mut line);
    push("ADD TEMP[2].z, TEMP[1].wwww, IMM[6].wwww", &mut shader, &mut line);
    push("FRC TEMP[8].z, TEMP[2].zzzz", &mut shader, &mut line);
    push("SUB TEMP[2].z, TEMP[2].zzzz, TEMP[8].zzzz", &mut shader, &mut line);
    push("SUB TEMP[2].z, TEMP[2].zzzz, IMM[6].wwww", &mut shader, &mut line);
    push("SUB TEMP[3].x, TEMP[2].xxxx, TEMP[0].xxxx", &mut shader, &mut line);
    push("SUB TEMP[3].y, TEMP[2].yyyy, TEMP[0].yyyy", &mut shader, &mut line);
    push("SUB TEMP[3].z, TEMP[2].zzzz, TEMP[1].zzzz", &mut shader, &mut line);
    push("ABS TEMP[3].x, TEMP[3].xxxx", &mut shader, &mut line);
    push("ABS TEMP[3].y, TEMP[3].yyyy", &mut shader, &mut line);
    push("ABS TEMP[3].z, TEMP[3].zzzz", &mut shader, &mut line);
    push("SLT TEMP[4].x, TEMP[3].yyyy, TEMP[3].xxxx", &mut shader, &mut line);
    push("SLT TEMP[4].y, TEMP[3].zzzz, TEMP[3].xxxx", &mut shader, &mut line);
    push("MUL TEMP[4].x, TEMP[4].xxxx, TEMP[4].yyyy", &mut shader, &mut line);
    push("SLT TEMP[4].z, TEMP[3].zzzz, TEMP[3].yyyy", &mut shader, &mut line);
    push("SUB TEMP[4].w, IMM[1].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
    push("MUL TEMP[4].z, TEMP[4].zzzz, TEMP[4].wwww", &mut shader, &mut line);
    push("ADD TEMP[5].x, TEMP[2].yyyy, TEMP[2].zzzz", &mut shader, &mut line);
    push("NEG TEMP[5].x, TEMP[5].xxxx", &mut shader, &mut line);
    push("ADD TEMP[5].y, TEMP[2].xxxx, TEMP[2].zzzz", &mut shader, &mut line);
    push("NEG TEMP[5].y, TEMP[5].yyyy", &mut shader, &mut line);
    push("SUB TEMP[5].z, IMM[1].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
    push("MUL TEMP[6].x, TEMP[5].xxxx, TEMP[4].xxxx", &mut shader, &mut line);
    push("MUL TEMP[6].y, TEMP[2].xxxx, TEMP[5].zzzz", &mut shader, &mut line);
    push("ADD TEMP[2].x, TEMP[6].xxxx, TEMP[6].yyyy", &mut shader, &mut line);
    push("SUB TEMP[5].z, IMM[1].xxxx, TEMP[4].zzzz", &mut shader, &mut line);
    push("MUL TEMP[6].x, TEMP[5].yyyy, TEMP[4].zzzz", &mut shader, &mut line);
    push("MUL TEMP[6].y, TEMP[2].yyyy, TEMP[5].zzzz", &mut shader, &mut line);
    push("ADD TEMP[2].y, TEMP[6].xxxx, TEMP[6].yyyy", &mut shader, &mut line);
    push("MUL TEMP[7].x, TEMP[2].xxxx, IMM[6].xxxx", &mut shader, &mut line);
    push("MUL TEMP[7].y, TEMP[2].yyyy, IMM[6].yyyy", &mut shader, &mut line);
    push("ADD TEMP[7].x, TEMP[7].xxxx, TEMP[7].yyyy", &mut shader, &mut line);
    push("FRC TEMP[7].x, TEMP[7].xxxx", &mut shader, &mut line);
    push("SLT TEMP[7].y, TEMP[7].xxxx, IMM[1].zzzz", &mut shader, &mut line);
    push("SLT TEMP[7].z, TEMP[7].xxxx, IMM[0].wwww", &mut shader, &mut line);
    push("SLT TEMP[7].w, TEMP[7].xxxx, IMM[1].wwww", &mut shader, &mut line);
    push("SUB TEMP[8].x, IMM[1].xxxx, TEMP[7].yyyy", &mut shader, &mut line);
    push("MUL TEMP[5].xyz, IMM[2].xyzx, TEMP[7].yyyy", &mut shader, &mut line);
    push("MUL TEMP[6].xyz, IMM[3].xyzx, TEMP[8].xxxx", &mut shader, &mut line);
    push("ADD TEMP[5].xyz, TEMP[5].xyzx, TEMP[6].xyzx", &mut shader, &mut line);
    push("SUB TEMP[8].x, TEMP[7].zzzz, TEMP[7].yyyy", &mut shader, &mut line);
    push("SUB TEMP[8].y, IMM[1].xxxx, TEMP[8].xxxx", &mut shader, &mut line);
    push("MUL TEMP[5].xyz, TEMP[5].xyzx, TEMP[8].yyyy", &mut shader, &mut line);
    push("MUL TEMP[6].xyz, IMM[4].xyzx, TEMP[8].xxxx", &mut shader, &mut line);
    push("ADD TEMP[5].xyz, TEMP[5].xyzx, TEMP[6].xyzx", &mut shader, &mut line);
    push("SUB TEMP[8].x, TEMP[7].wwww, TEMP[7].zzzz", &mut shader, &mut line);
    push("SUB TEMP[8].y, IMM[1].xxxx, TEMP[8].xxxx", &mut shader, &mut line);
    push("MUL TEMP[5].xyz, TEMP[5].xyzx, TEMP[8].yyyy", &mut shader, &mut line);
    push("MUL TEMP[6].xyz, IMM[5].xyzx, TEMP[8].xxxx", &mut shader, &mut line);
    push("ADD OUT[0].xyz, TEMP[5].xyzx, TEMP[6].xyzx", &mut shader, &mut line);
    push("MOV OUT[0].w, IN[1].wwww", &mut shader, &mut line);
    push("END", &mut shader, &mut line);
    shader
}
