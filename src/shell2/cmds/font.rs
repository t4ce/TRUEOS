use alloc::format;

use super::super::{ShellBackend2, print_native_line, print_shell_line, term_style};
use crate::shell2::shell2_cmd::ParseOutcome;

const FONT_CMD_RGB: (u8, u8, u8) = (255, 190, 90);

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.trim().split_whitespace();
    match args.next().unwrap_or("").to_ascii_lowercase().as_str() {
        "tessel" if args.next().is_none() => print_tessel(io),
        _ => print_usage(io),
    }

    ParseOutcome::Handled
}

fn print_tessel(io: &'static dyn ShellBackend2) {
    print_native_line(
        io,
        format!("{}", term_style::paint("font: tessel").bold().color(FONT_CMD_RGB)).as_str(),
    );

    let tessel = crate::graphics::font::tessellate_default_text();
    print_shell_line(
        io,
        format!(
            "font-tessel: status={} reason={} text=\"{}\" font={} file={} px={}",
            tessel.status,
            tessel.reason,
            tessel.text,
            tessel.font_name,
            tessel.font_file,
            tessel.px_size as u32
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-glyphs: source={} glyphs={} hits={} misses={} outline_glyphs={} empty_glyphs={} commands={}",
            tessel.outline_source,
            tessel.glyphs,
            tessel.glyph_hits,
            tessel.glyph_misses,
            tessel.outline_glyphs,
            tessel.empty_glyphs,
            tessel.path_commands
        )
        .as_str(),
    );

    print_shell_line(
        io,
        format!(
            "font-tessel-geometry: vertices={} indices={} triangles={} vertex_bytes={} index_bytes={} geometry_bytes={}",
            tessel.vertices,
            tessel.indices,
            tessel.triangles,
            tessel.vertex_bytes,
            tessel.index_bytes,
            tessel.geometry_bytes
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-bounds: min=({}, {}) max=({}, {}) failures={}",
            tessel.min_x as i32,
            tessel.min_y as i32,
            tessel.max_x as i32,
            tessel.max_y as i32,
            tessel.tessellate_failures
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-time: charmap_ms={} path_ms={} tessellate_ms={} total_ms={}",
            tessel.charmap_ms, tessel.path_ms, tessel.tessellate_ms, tessel.total_ms
        )
        .as_str(),
    );

    let marker = crate::intel::gpgpu::submit_direct_rcs_marker_probe_now();
    print_shell_line(
        io,
        format!(
            "font-tessel-intel: probe=rcs-marker available={} forcewake={} mapped={} ppgtt={} batch={} submitted={} retired={}",
            marker.available as u8,
            marker.forcewake_ok as u8,
            marker.mapped_ok as u8,
            marker.ppgtt_ok as u8,
            marker.batch_ok as u8,
            marker.submitted as u8,
            marker.retired as u8
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-intel-marker: observed=0x{:08X} expected=0x{:08X} retire_ms={} submit_seq={}",
            marker.observed,
            marker.expected,
            marker.retire_ms,
            marker.submit_seq
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-intel-regs: head=0x{:08X} tail=0x{:08X} acthd=0x{:08X} ipeir=0x{:08X} ipehr=0x{:08X} eir=0x{:08X}",
            marker.head,
            marker.tail,
            marker.acthd,
            marker.ipeir,
            marker.ipehr,
            marker.eir
        )
        .as_str(),
    );
    print_shell_line(io, "font-tessel-intel-note: triangle-rasterizer-not-wired");

    print_shell_line(
        io,
        "font-tessel-boundary: input=graphics-font-outline-cache output=lyon-triangles intel=rcs-marker rasterizer=not-wired raster_pixels=not-rendered production-path=unchanged",
    );
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "font: usage `font tessel`");
}
