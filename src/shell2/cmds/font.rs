use alloc::format;

use super::super::{ShellBackend2, print_native_line, print_shell_line, term_style};
use crate::shell2::shell2_cmd::ParseOutcome;

const FONT_CMD_RGB: (u8, u8, u8) = (255, 190, 90);

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let rest = rest.trim();
    if rest.is_empty() || rest.eq_ignore_ascii_case("probe") {
        print_probe(io);
        return ParseOutcome::Handled;
    }

    let mut args = rest.split_whitespace();
    match args.next().unwrap_or("").to_ascii_lowercase().as_str() {
        "stack" if args.next().is_none() => print_stack(io),
        "warm" if args.next().is_none() => print_warm(io),
        "bench" => match args.next().unwrap_or("").to_ascii_lowercase().as_str() {
            "athlas" if args.next().is_none() => print_bench_athlas(io),
            "vector" if args.next().is_none() => print_bench_vector(io),
            "skrifa" if args.next().is_none() => print_bench_skrifa(io),
            _ => print_usage(io),
        },
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}

fn print_probe(io: &'static dyn ShellBackend2) {
    match crate::font_probe::boot_font_probe_summary() {
        Ok(summary) => {
            print_native_line(
                io,
                format!(
                    "{}",
                    term_style::paint("font: skrifa probe ok")
                        .bold()
                        .color(FONT_CMD_RGB)
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font: L_10646.TTF bytes={} tables={} glyphs={} units_per_em={} cmap={} glyph_A={} glyph_space={}",
                    summary.bytes,
                    summary.tables,
                    summary.glyphs,
                    summary.units_per_em,
                    summary.cmap_status,
                    summary.glyph_a,
                    summary.glyph_space
                )
                .as_str(),
            );
        }
        Err(err) => {
            print_native_line(
                io,
                format!(
                    "{}",
                    term_style::paint("font: skrifa probe failed")
                        .bold()
                        .color(FONT_CMD_RGB)
                )
                .as_str(),
            );
            print_shell_line(io, format!("font: L_10646.TTF err={:?}", err).as_str());
        }
    }
}

fn print_stack(io: &'static dyn ShellBackend2) {
    let stack = crate::font_probe::font_stack_summary();
    print_native_line(
        io,
        format!(
            "{}",
            term_style::paint("font: stack map")
                .bold()
                .color(FONT_CMD_RGB)
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-stack: athlas_faces={} athlas_slots={} twemoji_slots={} sprite64_slots={} sprite64_kernel={} sprite64_atlas={} glyph_mask_kernel={} svg_lyon={}",
            stack.athlas_faces.len(),
            stack.athlas_slots,
            stack.twemoji_slots,
            stack.sprite64_slots,
            stack.sprite64_kernel,
            stack.sprite64_atlas,
            stack.glyph_mask_kernel,
            stack.svg_lyon
        )
        .as_str(),
    );
    for face in stack.athlas_faces {
        print_shell_line(
            io,
            format!(
                "font-stack-athlas: face={}/{} slots={} line_height_px={}",
                face.family, face.tier, face.slots, face.line_height_px
            )
            .as_str(),
        );
    }
    match stack.skrifa {
        Some(summary) => print_shell_line(
            io,
            format!(
                "font-stack-skrifa: status=ok font=L_10646.TTF bytes={} tables={} glyphs={} units_per_em={} cmap={} glyph_A={} glyph_space={}",
                summary.bytes,
                summary.tables,
                summary.glyphs,
                summary.units_per_em,
                summary.cmap_status,
                summary.glyph_a,
                summary.glyph_space
            )
            .as_str(),
        ),
        None => print_shell_line(io, "font-stack-skrifa: status=failed font=L_10646.TTF"),
    }
    print_shell_line(
        io,
        "font-stack-doorways: athlas->sprite64=production real-font=probe-only vector=cpu-tessellate/cpu-paint gpu-mask=possible-middle-path dispatch=observational",
    );
}

fn print_warm(io: &'static dyn ShellBackend2) {
    print_native_line(
        io,
        format!(
            "{}",
            term_style::paint("font: warm skrifa outlines")
                .bold()
                .color(FONT_CMD_RGB)
        )
        .as_str(),
    );
    match crate::font_probe::warm_skrifa_outline_cache() {
        Ok(warm) => {
            print_shell_line(
                io,
                format!(
                    "font-warm: status={} font=L_10646.TTF bytes={} tables={} glyphs={} units_per_em={}",
                    warm.status, warm.bytes, warm.tables, warm.glyphs, warm.units_per_em
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font-warm-cache: resident_bytes={} outline_cache_bytes={} range_bytes={} op_bytes={} commands={} first_gid={} last_gid={} max_ops_per_glyph={}",
                    warm.resident_bytes,
                    warm.cache_bytes,
                    warm.range_bytes,
                    warm.op_bytes,
                    warm.commands,
                    warm.range_first_glyph,
                    warm.range_last_glyph,
                    warm.range_max_ops
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font-warm-outlines: outline_glyphs={} success={} failures={} empty={} move={} line={} quad={} curve={} close={}",
                    warm.outline_glyphs,
                    warm.outline_success,
                    warm.outline_failures,
                    warm.empty_outlines,
                    warm.move_to,
                    warm.line_to,
                    warm.quad_to,
                    warm.curve_to,
                    warm.close
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font-warm-bounds: min_x={} min_y={} max_x={} max_y={} parse_ms={} outline_ms={} total_ms={}",
                    warm.min_x as i32,
                    warm.min_y as i32,
                    warm.max_x as i32,
                    warm.max_y as i32,
                    warm.parse_ms,
                    warm.outline_ms,
                    warm.total_ms
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "font-warm-boundary: cached=font-units-outline-commands tessellation=not-run raster_pixels=not-rendered production-path=unchanged",
            );
        }
        Err(err) => {
            print_shell_line(io, format!("font-warm: status=failed err={:?}", err).as_str());
        }
    }
}

fn print_bench_athlas(io: &'static dyn ShellBackend2) {
    print_native_line(
        io,
        format!(
            "{}",
            term_style::paint("font: bench athlas")
                .bold()
                .color(FONT_CMD_RGB)
        )
        .as_str(),
    );
    for bench in crate::font_probe::bench_athlas_samples() {
        print_shell_line(
            io,
            format!(
                "font-bench-athlas: sample={} repeats={} face={}/{} chars={} whitespace={} glyph_hits={} glyph_misses={} clipped={} placements={} slot_misses={} glyph_lookup_ms={} slot_ms={} placement_ms={} total_ms={}",
                bench.sample,
                bench.repeats,
                bench.face_family,
                bench.face_tier,
                bench.chars,
                bench.whitespace,
                bench.glyph_hits,
                bench.glyph_misses,
                bench.clipped,
                bench.placements,
                bench.slot_misses,
                bench.glyph_lookup_ms,
                bench.slot_ms,
                bench.placement_ms,
                bench.total_ms
            )
            .as_str(),
        );
    }
    print_shell_line(
        io,
        "font-bench-athlas-boundary: decision=observe small_text=current-athlas-sprite64 gpu_mask=not-dispatched",
    );
}

fn print_bench_vector(io: &'static dyn ShellBackend2) {
    print_native_line(
        io,
        format!(
            "{}",
            term_style::paint("font: bench vector")
                .bold()
                .color(FONT_CMD_RGB)
        )
        .as_str(),
    );
    match crate::font_probe::bench_vector_svg() {
        Ok(bench) => {
            print_shell_line(
                io,
                format!(
                    "font-bench-vector: sample=builtin-svg width={} height={} primitives={} vertices={} indices={} triangles={} pixels={} rgba_bytes={} parse_ms={} tessellate_ms={} paint_ms={} upload_ms={} upload_status={} total_ms={}",
                    bench.width,
                    bench.height,
                    bench.primitives,
                    bench.vertices,
                    bench.indices,
                    bench.triangles,
                    bench.pixels,
                    bench.rgba_bytes,
                    bench.parse_ms,
                    bench.tessellate_ms,
                    bench.paint_ms,
                    bench.upload_ms,
                    bench.upload_status,
                    bench.total_ms
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "font-bench-vector-boundary: decision=observe real-font-outline/tessellation=separate gpu-upload=not-run-no-texture-id",
            );
        }
        Err(err) => {
            print_shell_line(io, format!("font-bench-vector: status=failed err={}", err).as_str());
        }
    }
}

fn print_bench_skrifa(io: &'static dyn ShellBackend2) {
    print_native_line(
        io,
        format!(
            "{}",
            term_style::paint("font: bench skrifa")
                .bold()
                .color(FONT_CMD_RGB)
        )
        .as_str(),
    );
    match crate::font_probe::bench_skrifa() {
        Ok(bench) => {
            print_shell_line(
                io,
                format!(
                    "font-bench-skrifa: font=L_10646.TTF repeats={} bytes={} tables={} glyphs={} units_per_em={} sample_chars={} charmap_hits={} charmap_misses={} parse_ms={} charmap_ms={} outline_ms={} outline_tessellate_ms={} outline_attempts={} outline_success={} outline_failures={} outline_commands={} tessellate_success={} tessellate_failures={} outline_vertices={} outline_indices={} outline_triangles={} outline_status={} total_ms={}",
                    bench.repeats,
                    bench.bytes,
                    bench.tables,
                    bench.glyphs,
                    bench.units_per_em,
                    bench.sample_chars,
                    bench.charmap_hits,
                    bench.charmap_misses,
                    bench.parse_ms,
                    bench.charmap_ms,
                    bench.outline_ms,
                    bench.outline_tessellate_ms,
                    bench.outline_attempts,
                    bench.outline_success,
                    bench.outline_failures,
                    bench.outline_commands,
                    bench.outline_tessellate_success,
                    bench.outline_tessellate_failures,
                    bench.outline_vertices,
                    bench.outline_indices,
                    bench.outline_triangles,
                    bench.outline_status,
                    bench.total_ms
                )
                .as_str(),
            );
            print_shell_line(
                io,
                "font-bench-skrifa-boundary: decision=observe outline=skrifa-pen lyon=cpu-tessellated raster_pixels=not-rendered production-path=unchanged",
            );
        }
        Err(err) => {
            print_shell_line(
                io,
                format!("font-bench-skrifa: status=failed err={:?}", err).as_str(),
            );
        }
    }
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "font: usage `font` | `font probe` | `font stack` | `font warm` | `font bench athlas|vector|skrifa`",
    );
}
