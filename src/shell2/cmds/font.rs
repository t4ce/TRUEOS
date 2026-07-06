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

    print_shell_line(
        io,
        "font-tessel-intel-note: marker-preflight=skipped render-helper-recovers-and-submits",
    );

    let mut clip_field_isolate_completed = None;
    let mut clip_field_isolate_two_completed = None;
    let mut clip_field_all_completed = None;
    let mut clip_counter_sweep_completed = None;
    let mut clip_counter_vf_vue_completed = None;

    match crate::graphics::font::font_tessellated_scratch_triangle() {
        Some(triangle) => {
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font: probe=font-triangle-scratch source_vertices={} source_indices={} source_triangles={} selected_indices=[{},{},{}] source_area2={:.3} scratch_area2={:.3}",
                    triangle.source_vertex_count,
                    triangle.source_index_count,
                    triangle.source_triangle_count,
                    triangle.source_indices[0],
                    triangle.source_indices[1],
                    triangle.source_indices[2],
                    triangle.source_area2,
                    triangle.scratch_area2
                )
                .as_str(),
            );
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-vertices: v0=({:.2},{:.2})->({:.2},{:.2}) v1=({:.2},{:.2})->({:.2},{:.2}) v2=({:.2},{:.2})->({:.2},{:.2})",
                    triangle.source_vertices[0][0],
                    triangle.source_vertices[0][1],
                    triangle.vertices[0][0],
                    triangle.vertices[0][1],
                    triangle.source_vertices[1][0],
                    triangle.source_vertices[1][1],
                    triangle.vertices[1][0],
                    triangle.vertices[1][1],
                    triangle.source_vertices[2][0],
                    triangle.source_vertices[2][1],
                    triangle.vertices[2][0],
                    triangle.vertices[2][1],
                )
                .as_str(),
            );

            let field = triangle.mirrored_clip_field();
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-field: vertices={} triangles={} axes={} rings={} radii=({:.0},{:.0},{:.0}) sizes=({:.0},{:.0},{:.0}) rot_deg=({},{},{}) bounds=({:.2},{:.2},{:.2})->({:.2},{:.2},{:.2})",
                    field.vertex_count,
                    field.triangle_count,
                    field.axes,
                    field.rings,
                    field.radii[0],
                    field.radii[1],
                    field.radii[2],
                    field.sizes[0],
                    field.sizes[1],
                    field.sizes[2],
                    field.rotations_deg[0],
                    field.rotations_deg[1],
                    field.rotations_deg[2],
                    field.min_x,
                    field.min_y,
                    field.min_z,
                    field.max_x,
                    field.max_y,
                    field.max_z,
                )
                .as_str(),
            );
            let isolated = field.isolated_scratch_triangle();
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-field-isolate: source=first-triangle upload_vertices=3 vertices=({:.2},{:.2},{:.2})/({:.2},{:.2},{:.2})/({:.2},{:.2},{:.2})",
                    isolated[0][0],
                    isolated[0][1],
                    isolated[0][2],
                    isolated[1][0],
                    isolated[1][1],
                    isolated[1][2],
                    isolated[2][0],
                    isolated[2][1],
                    isolated[2][2],
                )
                .as_str(),
            );
            match crate::intel::render::submit_render_font_clip_field_isolate_probe(isolated) {
                Ok(render) => {
                    clip_field_isolate_completed = Some(render.completed);
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-result: variant={} submit={} target={} completed={}",
                            render.variant,
                            render.submit_name,
                            render.target,
                            render.completed as u8,
                        )
                        .as_str(),
                    );
                }
                Err(err) => {
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-result: status=skipped reason={}",
                            err
                        )
                        .as_str(),
                    );
                }
            }
            let isolated_two = field.isolated_scratch_two_triangles();
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-field-isolate-two: source=first-two-triangles upload_vertices={} upload_triangles={}",
                    isolated_two.len(),
                    isolated_two.len() / 3,
                )
                .as_str(),
            );
            match crate::intel::render::submit_render_font_clip_field_isolate_probe(isolated_two) {
                Ok(render) => {
                    clip_field_isolate_two_completed = Some(render.completed);
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-two-result: variant={} submit={} target={} completed={}",
                            render.variant,
                            render.submit_name,
                            render.target,
                            render.completed as u8,
                        )
                        .as_str(),
                    );
                }
                Err(err) => {
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-two-result: status=skipped reason={}",
                            err
                        )
                        .as_str(),
                    );
                }
            }
            let isolated_all = field.isolated_scratch_all_triangles();
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-field-isolate-all: source=full-field upload_vertices={} upload_triangles={}",
                    isolated_all.len(),
                    isolated_all.len() / 3,
                )
                .as_str(),
            );
            match crate::intel::render::submit_render_font_clip_field_isolate_probe(isolated_all) {
                Ok(render) => {
                    clip_field_all_completed = Some(render.completed);
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-all-result: variant={} submit={} target={} completed={}",
                            render.variant,
                            render.submit_name,
                            render.target,
                            render.completed as u8,
                        )
                        .as_str(),
                    );
                }
                Err(err) => {
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-isolate-all-result: status=skipped reason={}",
                            err
                        )
                        .as_str(),
                    );
                }
            }
        }
        None => {
            print_shell_line(
                io,
                "font-tessel-render-font: probe=font-triangle-scratch status=skipped reason=no-font-triangle",
            );
        }
    }

    print_shell_line(
        io,
        "font-tessel-render-clip-counter-sweep: source=big-inbounds-screen-space upload_vertices=3 target=scratch path=known-vs viewport=default-scratch-8x8 scissor=full-scratch sample_mask=normal cull=none goal=clip-counter-or-pixel-proof",
    );
    match crate::intel::render::submit_render_font_clip_counter_sweep_probe() {
        Ok(render) => {
            clip_counter_sweep_completed = Some(render.completed);
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-clip-counter-sweep-result: variant={} submit={} target={} completed={}",
                    render.variant,
                    render.submit_name,
                    render.target,
                    render.completed as u8,
                )
                .as_str(),
            );
        }
        Err(err) => {
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-clip-counter-sweep-result: status=skipped reason={}",
                    err
                )
                .as_str(),
            );
        }
    }
    print_shell_line(
        io,
        "font-tessel-render-clip-counter-vf-vue: source=big-inbounds-screen-space upload_vertices=3 target=scratch path=vf-synthesized-vue vs=disabled viewport=default-scratch-8x8 goal=separate-vs-export-from-clip",
    );
    match crate::intel::render::submit_render_font_clip_counter_vf_vue_probe() {
        Ok(render) => {
            clip_counter_vf_vue_completed = Some(render.completed);
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-clip-counter-vf-vue-result: variant={} submit={} target={} completed={}",
                    render.variant,
                    render.submit_name,
                    render.target,
                    render.completed as u8,
                )
                .as_str(),
            );
        }
        Err(err) => {
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-clip-counter-vf-vue-result: status=skipped reason={}",
                    err
                )
                .as_str(),
            );
        }
    }

    let frontier = crate::intel::render::latest_render_frontier_summary();
    print_shell_line(
        io,
        format!(
            "font-tessel-verdict: clip_field_isolate={} clip_field_isolate_two={} clip_field_all={} clip_counter_sweep={} clip_counter_vf_vue={} sweep=one:screen-space-default,two:clip-normal,all:clip-sf-sync,clip-counter:known-vs-big-inbounds,vf-vue-big-inbounds completed={} ps_state_marker={} raster_packet={} clip_counter={} ps_observed={} fragment_candidate={} fragment_observed={} pixel_coverage=not-proven next={}",
            probe_status_word(clip_field_isolate_completed),
            probe_status_word(clip_field_isolate_two_completed),
            probe_status_word(clip_field_all_completed),
            probe_status_word(clip_counter_sweep_completed),
            probe_status_word(clip_counter_vf_vue_completed),
            frontier.completed as u8,
            frontier.ps_state_marker as u8,
            frontier.raster_packet as u8,
            frontier.clip_counter as u8,
            frontier.ps_observed as u8,
            frontier.fragment_candidate_ready as u8,
            frontier.fragment_observed as u8,
            clip_field_next_step(
                clip_field_isolate_completed,
                clip_field_isolate_two_completed,
                clip_field_all_completed,
                clip_counter_sweep_completed,
                clip_counter_vf_vue_completed,
                frontier.clip_counter,
                frontier.ps_observed,
                frontier.fragment_observed,
            )
        )
        .as_str(),
    );

    print_shell_line(
        io,
        "font-tessel-boundary: input=graphics-font-outline-cache output=mirrored-clip-field-isolate-all+big-inbounds-known-vs+big-inbounds-vf-vue intel=rcs-marker+3d-pipe-marker+clip-field-screen-space-default+clip-field-two-clip-normal+clip-field-all-clip-sf-sync+clip-counter-sweep raster_pixels=scratch-observational production-path=unchanged",
    );
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "font: usage `font tessel`");
}

fn probe_status_word(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "completed",
        Some(false) => "stalled",
        None => "skipped",
    }
}

fn clip_field_next_step(
    isolate_completed: Option<bool>,
    isolate_two_completed: Option<bool>,
    isolate_all_completed: Option<bool>,
    clip_counter_sweep_completed: Option<bool>,
    clip_counter_vf_vue_completed: Option<bool>,
    clip_counter: bool,
    ps_observed: bool,
    fragment_observed: bool,
) -> &'static str {
    match (
        isolate_completed,
        isolate_two_completed,
        isolate_all_completed,
        clip_counter_sweep_completed,
        clip_counter_vf_vue_completed,
    ) {
        (Some(false), _, _, _, _) => "compare-font-isolate-state",
        (Some(true), Some(false), _, _, _) => "compare-two-triangle-state",
        (_, Some(true), Some(false), _, _) => "compare-full-field-state",
        (_, _, Some(true), Some(false), _) => "compare-known-vs-big-inbounds-submit-state",
        (_, _, Some(true), _, Some(false)) => "compare-vf-vue-submit-state",
        (_, _, Some(true), Some(true), Some(true)) if fragment_observed => {
            "inspect-scratch-rt-write"
        }
        (_, _, Some(true), Some(true), Some(true)) if ps_observed => {
            "inspect-fragment-to-rt-boundary"
        }
        (_, _, Some(true), Some(true), Some(true)) if clip_counter => "ps-launch-frontier",
        (_, _, Some(true), Some(true), Some(true)) => "clip-counter-still-zero-vs-bypass-too",
        (_, _, Some(true), Some(true), None) => "compare-vf-vue-clip-counter",
        (_, _, Some(true), None, _) => "compare-known-vs-clip-counter",
        _ => "repeat-font-isolate",
    }
}
