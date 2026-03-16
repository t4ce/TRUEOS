extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

pub const MANDELBROT_PIPELINE_FS_TAG_RAW: u32 = 0x4D44_4C42;

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

    // Map uv -> complex plane: x=-2..1, y=1..-1
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

    // White outside the set, black inside.
    push(
        "SUB TEMP[3].x, TEMP[0].xxxx, TEMP[2].zzzz",
        &mut shader,
        &mut line,
    );
    push("MOV OUT[0].xyz, TEMP[3].xxxx", &mut shader, &mut line);
    push("MOV OUT[0].w, TEMP[0].xxxx", &mut shader, &mut line);
    push("END", &mut shader, &mut line);
    shader
}

pub fn fullscreen_quad_rgba_bytes() -> Vec<u8> {
    let verts: [(f32, f32, f32, f32); 6] = [
        (-1.0, 1.0, 0.0, 0.0),
        (1.0, 1.0, 1.0, 0.0),
        (1.0, -1.0, 1.0, 1.0),
        (-1.0, 1.0, 0.0, 0.0),
        (1.0, -1.0, 1.0, 1.0),
        (-1.0, -1.0, 0.0, 1.0),
    ];

    let mut out = Vec::with_capacity(verts.len().saturating_mul(20));
    for (x, y, u, v) in verts {
        out.extend_from_slice(&x.to_le_bytes());
        out.extend_from_slice(&y.to_le_bytes());
        out.extend_from_slice(&u.to_le_bytes());
        out.extend_from_slice(&v.to_le_bytes());
        out.extend_from_slice(&[0xFF, 0xFF, 0xFF, 0xFF]);
    }
    out
}
