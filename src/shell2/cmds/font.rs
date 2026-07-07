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

    let mut clip_field_vf_vue_isolate_completed = None;
    let mut clip_field_vf_vue_two_completed = None;
    let mut clip_field_vf_vue_all_completed = None;
    let mut clip_accept_one = false;
    let mut clip_accept_two = false;
    let mut clip_accept_all = false;
    let mut clip_accept_one_route = "none";
    let mut clip_accept_two_route = "none";
    let mut clip_accept_all_route = "none";
    let mut candidate_accept_one = false;
    let mut candidate_accept_two = false;
    let mut candidate_accept_all = false;
    let mut candidate_accept_one_route = "none";
    let mut candidate_accept_two_route = "none";
    let mut candidate_accept_all_route = "none";
    let mut vs_candidate_accept_one = false;
    let mut vs_candidate_accept_two = false;
    let mut vs_candidate_accept_all = false;
    let mut vs_candidate_accept_one_route = "none";
    let mut vs_candidate_accept_two_route = "none";
    let mut vs_candidate_accept_all_route = "none";

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
            print_shell_line(
                io,
                "font-tessel-render-font-field-vf-vue-isolate: source=first-triangle upload_vertices=3 path=vf-synthesized-vue goal=clip+vs-frontier-acceptance",
            );
            match crate::intel::render::submit_render_font_clip_field_vf_vue_probe(isolated) {
                Ok(render) => {
                    clip_field_vf_vue_isolate_completed = Some(render.completed);
                    if render_clip_accepted(&render) {
                        clip_accept_one = true;
                        clip_accept_one_route = "vf-vue";
                    }
                    if render_fragment_candidate_ready(&render) {
                        candidate_accept_one = true;
                        candidate_accept_one_route = "vf-vue";
                    }
                    if render_vs_fragment_candidate_ready(&render) {
                        vs_candidate_accept_one = true;
                        vs_candidate_accept_one_route = "vf-vue";
                    }
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-vf-vue-isolate-result: variant={} submit={} target={} completed={}",
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
                            "font-tessel-render-font-field-vf-vue-isolate-result: status=skipped reason={}",
                            err
                        )
                        .as_str(),
                    );
                }
            }
            print_shell_line(
                io,
                "font-tessel-render-font-field-vf-vue-two: source=first-two-triangles upload_vertices=6 upload_triangles=2 path=vf-synthesized-vue goal=clip+vs-frontier-acceptance",
            );
            match crate::intel::render::submit_render_font_clip_field_vf_vue_probe(isolated_two) {
                Ok(render) => {
                    clip_field_vf_vue_two_completed = Some(render.completed);
                    if render_clip_accepted(&render) {
                        clip_accept_two = true;
                        clip_accept_two_route = "vf-vue";
                    }
                    if render_fragment_candidate_ready(&render) {
                        candidate_accept_two = true;
                        candidate_accept_two_route = "vf-vue";
                    }
                    if render_vs_fragment_candidate_ready(&render) {
                        vs_candidate_accept_two = true;
                        vs_candidate_accept_two_route = "vf-vue";
                    }
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-vf-vue-two-result: variant={} submit={} target={} completed={}",
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
                            "font-tessel-render-font-field-vf-vue-two-result: status=skipped reason={}",
                            err
                        )
                        .as_str(),
                    );
                }
            }
            print_shell_line(
                io,
                format!(
                    "font-tessel-render-font-field-vf-vue-all: source=full-field upload_vertices={} upload_triangles={} path=vf-synthesized-vue goal=clip+vs-frontier-acceptance",
                    isolated_all.len(),
                    isolated_all.len() / 3,
                )
                .as_str(),
            );
            match crate::intel::render::submit_render_font_clip_field_vf_vue_probe(isolated_all) {
                Ok(render) => {
                    clip_field_vf_vue_all_completed = Some(render.completed);
                    if render_clip_accepted(&render) {
                        clip_accept_all = true;
                        clip_accept_all_route = "vf-vue";
                    }
                    if render_fragment_candidate_ready(&render) {
                        candidate_accept_all = true;
                        candidate_accept_all_route = "vf-vue";
                    }
                    if render_vs_fragment_candidate_ready(&render) {
                        vs_candidate_accept_all = true;
                        vs_candidate_accept_all_route = "vf-vue";
                    }
                    print_shell_line(
                        io,
                        format!(
                            "font-tessel-render-font-field-vf-vue-all-result: variant={} submit={} target={} completed={}",
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
                            "font-tessel-render-font-field-vf-vue-all-result: status=skipped reason={}",
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

    let all_clip_counts_ready = clip_accept_one && clip_accept_two && clip_accept_all;
    print_shell_line(
        io,
        format!(
            "font-tessel-clip-acceptance: one={} route_one={} two={} route_two={} all={} route_all={} all_counts={} gate={}",
            clip_accept_one as u8,
            clip_accept_one_route,
            clip_accept_two as u8,
            clip_accept_two_route,
            clip_accept_all as u8,
            clip_accept_all_route,
            all_clip_counts_ready as u8,
            if all_clip_counts_ready { "ready" } else { "wait" },
        )
        .as_str(),
    );

    let all_candidate_counts_ready =
        candidate_accept_one && candidate_accept_two && candidate_accept_all;
    print_shell_line(
        io,
        format!(
            "font-tessel-final-candidate: one={} route_one={} two={} route_two={} all={} route_all={} all_counts={} gate={} markers=ps_state+raster_packet+clip_counter+no_ps",
            candidate_accept_one as u8,
            candidate_accept_one_route,
            candidate_accept_two as u8,
            candidate_accept_two_route,
            candidate_accept_all as u8,
            candidate_accept_all_route,
            all_candidate_counts_ready as u8,
            if all_candidate_counts_ready { "ready" } else { "wait" },
        )
        .as_str(),
    );
    let all_vs_candidate_counts_ready =
        vs_candidate_accept_one && vs_candidate_accept_two && vs_candidate_accept_all;
    print_shell_line(
        io,
        format!(
            "font-tessel-vs-final-candidate: one={} route_one={} two={} route_two={} all={} route_all={} all_counts={} gate={} markers=vs+ps_state+raster_packet+clip_counter+no_ps",
            vs_candidate_accept_one as u8,
            vs_candidate_accept_one_route,
            vs_candidate_accept_two as u8,
            vs_candidate_accept_two_route,
            vs_candidate_accept_all as u8,
            vs_candidate_accept_all_route,
            all_vs_candidate_counts_ready as u8,
            if all_vs_candidate_counts_ready { "ready" } else { "wait" },
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "font-tessel-gate: clip={} final_candidate={} vs_final_candidate={} next={}",
            all_clip_counts_ready as u8,
            all_candidate_counts_ready as u8,
            all_vs_candidate_counts_ready as u8,
            if all_vs_candidate_counts_ready {
                "ps-launch"
            } else if all_candidate_counts_ready {
                "fix-vs-to-clip"
            } else {
                "finish-frontier-candidate"
            },
        )
        .as_str(),
    );

    let frontier = crate::intel::render::latest_render_frontier_summary();
    print_shell_line(
        io,
        format!(
            "font-tessel-verdict: vf_vue_isolate={} vf_vue_two={} vf_vue_all={} sweep=vf-vue:one/two/all completed={} vs_counter={} ps_state_marker={} raster_packet={} clip_counter={} ps_observed={} fragment_candidate={} fragment_observed={} pixel_coverage=not-proven next={}",
            probe_status_word(clip_field_vf_vue_isolate_completed),
            probe_status_word(clip_field_vf_vue_two_completed),
            probe_status_word(clip_field_vf_vue_all_completed),
            frontier.completed as u8,
            frontier.vs_counter as u8,
            frontier.ps_state_marker as u8,
            frontier.raster_packet as u8,
            frontier.clip_counter as u8,
            frontier.ps_observed as u8,
            frontier.fragment_candidate_ready as u8,
            frontier.fragment_observed as u8,
            clip_field_next_step(
                clip_field_vf_vue_isolate_completed,
                clip_field_vf_vue_two_completed,
                clip_field_vf_vue_all_completed,
                frontier.vs_counter,
                frontier.clip_counter,
                frontier.ps_observed,
                frontier.fragment_observed,
            )
        )
        .as_str(),
    );

    print_shell_line(
        io,
        "font-tessel-boundary: input=graphics-font-outline-cache output=mirrored-clip-field-vf-vue-one/two/all intel=rcs-marker+3d-pipe-marker+clip-field-vf-vue-one/two/all raster_pixels=scratch-observational production-path=unchanged",
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

fn render_clip_accepted(render: &crate::intel::render::RenderJokerResult) -> bool {
    render.clip_counter && render.raster_packet
}

fn render_fragment_candidate_ready(render: &crate::intel::render::RenderJokerResult) -> bool {
    render.ps_state_marker && render.raster_packet && render.clip_counter && !render.ps_observed
}

fn render_vs_fragment_candidate_ready(render: &crate::intel::render::RenderJokerResult) -> bool {
    render.vs_counter
        && render.ps_state_marker
        && render.raster_packet
        && render.clip_counter
        && !render.ps_observed
}

fn clip_field_next_step(
    isolate_completed: Option<bool>,
    isolate_two_completed: Option<bool>,
    isolate_all_completed: Option<bool>,
    vs_counter: bool,
    clip_counter: bool,
    ps_observed: bool,
    fragment_observed: bool,
) -> &'static str {
    match (isolate_completed, isolate_two_completed, isolate_all_completed) {
        (Some(false), _, _) => "compare-font-isolate-state",
        (Some(true), Some(false), _) => "compare-two-triangle-state",
        (_, Some(true), Some(false)) => "compare-full-field-state",
        (_, _, Some(true)) if fragment_observed => "inspect-scratch-rt-write",
        (_, _, Some(true)) if ps_observed => "inspect-fragment-to-rt-boundary",
        (_, _, Some(true)) if clip_counter && !vs_counter => "fix-vs-to-clip",
        (_, _, Some(true)) if clip_counter => "ps-launch-frontier",
        (_, _, Some(true)) => "clip-counter-still-zero-vs-bypass-too",
        _ => "repeat-font-isolate",
    }
}
