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

    let mesh = crate::graphics::font::tessellate_default_text_mesh();
    let tessel = mesh.summary;
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
    match crate::intel::render::submit_font_mesh_once(
        &mesh.vertices,
        &mesh.indices,
        (tessel.min_x, tessel.min_y, tessel.max_x, tessel.max_y),
    ) {
        Ok(render) => print_shell_line(
            io,
            format!(
                "font-tessel-render: submit={} target={} completed={} vs={} ps_state={} raster={} clip={} ps={}",
                render.submit_name,
                render.target,
                render.completed as u8,
                render.vs_counter as u8,
                render.ps_state_marker as u8,
                render.raster_packet as u8,
                render.clip_counter as u8,
                render.ps_observed as u8,
            )
            .as_str(),
        ),
        Err(reason) => print_shell_line(
            io,
            format!("font-tessel-render: status=skipped reason={}", reason).as_str(),
        ),
    }
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "font: usage `font tessel`");
}
