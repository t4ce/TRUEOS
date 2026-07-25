use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::intel::gpgpu::{
    FONT_OUTLINE_MESH_ADLS_ARTIFACT, FONT_OUTLINE_STAGE_AUDIT, FONT_OUTLINE_STAGE_FLATTEN,
    FONT_OUTLINE_STAGE_STROKE_MESH, shell_font_outline_probe, upload_font_outline_mesh_kernel,
};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "gpgpu preview start [all|static|static30|mandelbrot|chart|plasma|lab256] [duration_ms] [cadence_ms] [publish_every]",
    );
    print_shell_line(io, "gpgpu preview status");
    print_shell_line(io, "gpgpu preview stop");
    print_shell_line(io, "gpgpu test lab256 [duration_ms] [cadence_ms] [publish_every]");
    print_shell_line(io, "gpgpu svg start [basic|curves|holes]");
    print_shell_line(io, "gpgpu svg status");
    print_shell_line(io, "gpgpu svg stop");
    print_shell_line(io, "gpgpu probe copy-rect");
    print_shell_line(io, "gpgpu probe font-tessel [artifact|audit|flatten|mesh|all]");
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn run_preview(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(action) = args.next() else {
        usage(io);
        return;
    };
    if action.eq_ignore_ascii_case("start") {
        let preset = match args.clone().next().and_then(parse_preview_preset) {
            Some(preset) => {
                let _ = args.next();
                preset
            }
            None => crate::ui4::GpgpuPreviewPreset::All,
        };
        let duration_ms = match args.next() {
            Some(raw) => match raw.parse::<u64>() {
                Ok(value) => value,
                Err(_) => {
                    usage(io);
                    return;
                }
            },
            None => crate::ui4::GPGPU_PREVIEW_DEFAULT_DURATION_MS,
        };
        let cadence_ms = match args.next() {
            Some(raw) => match raw.parse::<u64>() {
                Ok(value) => value,
                Err(_) => {
                    usage(io);
                    return;
                }
            },
            None => crate::ui4::GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
        };
        let publish_every = match args.next() {
            Some(raw) => match raw.parse::<u32>() {
                Ok(value) => value,
                Err(_) => {
                    usage(io);
                    return;
                }
            },
            None => crate::ui4::GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
        };
        if !expect_no_more(io, args) {
            return;
        }
        let config = crate::ui4::GpgpuPreviewConfig {
            preset,
            duration_ms,
            cadence_ms,
            publish_every,
        };
        match crate::ui4::request_gpgpu_preview_start(config) {
            Ok(serial) => {
                let status = crate::ui4::gpgpu_preview_status();
                print_shell_line(
                    io,
                    alloc::format!(
                        "gpgpu preview start: queued=1 request={} preset={} service_online={} duration_ms={} cadence_ms={} publish_every={} ui4_consumer=kernel-app-5 frames={} windows={} buffering={} plane_layout={} slot_policy=fixed-per-window/no-round-robin interaction={}",
                        serial,
                        preset.label(),
                        status.online as u8,
                        duration_ms,
                        cadence_ms,
                        publish_every,
                        preview_surface_count(preset),
                        preview_surface_count(preset),
                        preset.buffering_label(),
                        preset.plane_layout_label(),
                        if preset.is_cpp() {
                            "application-movable-maximize-resize"
                        } else {
                            "movable-fixed-size"
                        },
                    )
                    .as_str(),
                );
            }
            Err(reason) => print_shell_line(
                io,
                alloc::format!("gpgpu preview start: queued=0 reason={reason}").as_str(),
            ),
        }
    } else if action.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_preview_status(io);
        }
    } else if action.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, args) {
            let serial = crate::ui4::request_gpgpu_preview_stop();
            print_shell_line(
                io,
                alloc::format!("gpgpu preview stop: queued=1 request={serial}").as_str(),
            );
        }
    } else {
        usage(io);
    }
}

fn parse_preview_preset(raw: &str) -> Option<crate::ui4::GpgpuPreviewPreset> {
    if raw.eq_ignore_ascii_case("all") {
        Some(crate::ui4::GpgpuPreviewPreset::All)
    } else if raw.eq_ignore_ascii_case("static") {
        Some(crate::ui4::GpgpuPreviewPreset::Static)
    } else if raw.eq_ignore_ascii_case("static30") {
        Some(crate::ui4::GpgpuPreviewPreset::Static30)
    } else if raw.eq_ignore_ascii_case("mandelbrot") {
        Some(crate::ui4::GpgpuPreviewPreset::Mandelbrot)
    } else if raw.eq_ignore_ascii_case("chart") {
        Some(crate::ui4::GpgpuPreviewPreset::Chart)
    } else if raw.eq_ignore_ascii_case("plasma") {
        Some(crate::ui4::GpgpuPreviewPreset::Plasma)
    } else if raw.eq_ignore_ascii_case("lab256") {
        Some(crate::ui4::GpgpuPreviewPreset::Lab256)
    } else {
        None
    }
}

const fn preview_surface_count(preset: crate::ui4::GpgpuPreviewPreset) -> usize {
    match preset {
        crate::ui4::GpgpuPreviewPreset::All => 3,
        crate::ui4::GpgpuPreviewPreset::Static30 => 30,
        crate::ui4::GpgpuPreviewPreset::Static
        | crate::ui4::GpgpuPreviewPreset::Mandelbrot
        | crate::ui4::GpgpuPreviewPreset::Chart
        | crate::ui4::GpgpuPreviewPreset::Plasma
        | crate::ui4::GpgpuPreviewPreset::Lab256
        | crate::ui4::GpgpuPreviewPreset::CppGallery
        | crate::ui4::GpgpuPreviewPreset::CppAurora
        | crate::ui4::GpgpuPreviewPreset::CppJulia
        | crate::ui4::GpgpuPreviewPreset::CppSdf
        | crate::ui4::GpgpuPreviewPreset::CppVoronoi
        | crate::ui4::GpgpuPreviewPreset::CppRetroSun
        | crate::ui4::GpgpuPreviewPreset::CppAudio
        | crate::ui4::GpgpuPreviewPreset::CppParticle => 1,
    }
}

fn print_preview_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu preview status: online={} phase={} desired_running={} request={} applied={} preset={} duration_ms={} cadence_ms={} publish_every={} frame={} window={} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} buffering={} plane_layout={} interaction={} error={}",
            status.online as u8,
            status.phase.label(),
            status.desired_running as u8,
            status.request_serial,
            status.applied_serial,
            status.config.preset.label(),
            status.config.duration_ms,
            status.config.cadence_ms,
            status.config.publish_every,
            status.frame.map(|frame| frame.raw()).unwrap_or(0),
            status.window.map(|window| window.raw()).unwrap_or(0),
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.completed,
            status.metrics.published,
            status.metrics.dropped_busy,
            status.metrics.failed,
            status.metrics.late,
            status.metrics.elapsed_ms,
            status.config.preset.buffering_label(),
            status.config.preset.plane_layout_label(),
            if status.config.preset.is_cpp() {
                "application-movable-maximize-resize"
            } else {
                "movable-fixed-size"
            },
            status.last_error,
        )
        .as_str(),
    );
    for member in status.members {
        print_shell_line(
            io,
            alloc::format!(
                "gpgpu preview member: preset={} slot={} frame={} window={} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} iterations={} marker=0x{:08X} submit_ms={} engine_ready_boundary=surflive",
                member.preset.label(),
                member.plane_slot,
                member.frame.map(|frame| frame.raw()).unwrap_or(0),
                member.window.map(|window| window.raw()).unwrap_or(0),
                member.metrics.attempted,
                member.metrics.submitted,
                member.metrics.completed,
                member.metrics.published,
                member.metrics.dropped_busy,
                member.metrics.failed,
                member.metrics.late,
                member.metrics.elapsed_ms,
                member.metrics.last_iterations,
                member.metrics.last_marker,
                member.metrics.last_submit_ms,
            )
            .as_str(),
        );
    }
}

const GPGPU_LAB256_TEST_DEFAULT_DURATION_MS: u64 = 30_000;

fn run_test(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(shader) = args.next() else {
        usage(io);
        return;
    };
    if !shader.eq_ignore_ascii_case("lab256") {
        usage(io);
        return;
    }
    let duration_ms = match args.next() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => GPGPU_LAB256_TEST_DEFAULT_DURATION_MS,
    };
    let cadence_ms = match args.next() {
        Some(raw) => match raw.parse::<u64>() {
            Ok(value) => value,
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => crate::ui4::GPGPU_PREVIEW_DEFAULT_CADENCE_MS,
    };
    let publish_every = match args.next() {
        Some(raw) => match raw.parse::<u32>() {
            Ok(value) => value,
            Err(_) => {
                usage(io);
                return;
            }
        },
        None => crate::ui4::GPGPU_PREVIEW_DEFAULT_PUBLISH_EVERY,
    };
    if !expect_no_more(io, args) {
        return;
    }

    let config = crate::ui4::GpgpuPreviewConfig {
        preset: crate::ui4::GpgpuPreviewPreset::Lab256,
        duration_ms,
        cadence_ms,
        publish_every,
    };
    match crate::ui4::request_gpgpu_preview_start(config) {
        Ok(serial) => {
            let status = crate::ui4::gpgpu_preview_status();
            print_shell_line(
                io,
                alloc::format!(
                    "gpgpu test lab256: queued=1 request={} service_online={} extent=256x256 passes=3 alpha=premultiplied-native background_alpha=0.08 duration_ms={} cadence_ms={} publish_every={} buffering=double plane=slot1 stop=\"gpgpu preview stop\"",
                    serial,
                    status.online as u8,
                    duration_ms,
                    cadence_ms,
                    publish_every,
                )
                .as_str(),
            );
        }
        Err(reason) => print_shell_line(
            io,
            alloc::format!("gpgpu test lab256: queued=0 reason={reason}").as_str(),
        ),
    }
}

fn run_svg(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(action) = args.next() else {
        usage(io);
        return;
    };
    if action.eq_ignore_ascii_case("start") {
        let demo = match args.next() {
            Some(raw) => match parse_svg_demo(raw) {
                Some(demo) => demo,
                None => {
                    usage(io);
                    return;
                }
            },
            None => crate::intel::gpgpu::SvgOutlineProbeDemo::Basic,
        };
        if !expect_no_more(io, args) {
            return;
        }
        let config = crate::ui4::GpgpuSvgProbeConfig { demo };
        match crate::ui4::request_gpgpu_svg_probe_start(config) {
            Ok(serial) => {
                let status = crate::ui4::gpgpu_svg_probe_status();
                print_shell_line(
                    io,
                    alloc::format!(
                        "gpgpu svg start: queued=1 request={} demo={} service_online={} ui4_consumer=kernel-app-7 frames=1 windows=1 buffering=double plane=universal-1 interaction=movable-fixed-size",
                        serial,
                        demo.label(),
                        status.online as u8,
                    )
                    .as_str(),
                );
            }
            Err(reason) => print_shell_line(
                io,
                alloc::format!("gpgpu svg start: queued=0 reason={reason}").as_str(),
            ),
        }
    } else if action.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_svg_status(io);
        }
    } else if action.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, args) {
            let serial = crate::ui4::request_gpgpu_svg_probe_stop();
            print_shell_line(
                io,
                alloc::format!("gpgpu svg stop: queued=1 request={serial}").as_str(),
            );
        }
    } else {
        usage(io);
    }
}

fn parse_svg_demo(raw: &str) -> Option<crate::intel::gpgpu::SvgOutlineProbeDemo> {
    if raw.eq_ignore_ascii_case("basic") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Basic)
    } else if raw.eq_ignore_ascii_case("curves") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Curves)
    } else if raw.eq_ignore_ascii_case("holes") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Holes)
    } else {
        None
    }
}

fn print_svg_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_svg_probe_status();
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu svg status: online={} phase={} desired_running={} request={} applied={} demo={} frame={} window={} extent={}x{} attempted={} submitted={} published={} layers={} ops={} nonzero_pixels={} submit_ms={} buffering=double plane=universal-1 engine_ready_boundary=surflive error={}",
            status.online as u8,
            status.phase.label(),
            status.desired_running as u8,
            status.request_serial,
            status.applied_serial,
            status.config.demo.label(),
            status.frame.map(|frame| frame.raw()).unwrap_or(0),
            status.window.map(|window| window.raw()).unwrap_or(0),
            status.width,
            status.height,
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.published,
            status.metrics.layers,
            status.metrics.ops,
            status.metrics.nonzero_pixels,
            status.metrics.submit_ms,
            status.last_error,
        )
        .as_str(),
    );
}

fn outline_checksum(ops: &[[u32; 8]]) -> u32 {
    let mut hash = 0x811C_9DC5u32;
    for op in ops {
        for word in op {
            hash ^= *word;
            hash = hash.wrapping_mul(0x0100_0193);
        }
    }
    hash
}

fn run_font_tessel_artifact(io: &'static dyn ShellBackend2) -> bool {
    let artifact = FONT_OUTLINE_MESH_ADLS_ARTIFACT;
    let registry_report = crate::intel::opencl::trueos_cl_validate_known_aot_registry();
    let Some(known) = crate::intel::opencl::registry::known_aot_kernel(artifact.name) else {
        print_shell_line(
            io,
            "gpgpu probe font-tessel artifact: ok=0 reason=missing-known-aot-contract",
        );
        return false;
    };
    let Some(upload) = upload_font_outline_mesh_kernel() else {
        print_shell_line(io, "gpgpu probe font-tessel artifact: ok=0 reason=upload-unavailable");
        return false;
    };
    let hash_ok = upload.bin_sha256 == artifact.bin_sha256;
    let contract_ok = known.contract.name == artifact.name
        && known.contract.target == artifact.target
        && known.contract.cross_thread_bytes == 128
        && known.contract.per_thread_bytes == 96
        && known.contract.binding_count == 2
        && known.contract.args.len() == 11;
    let ok =
        registry_report.passed() && upload.verified && upload.bytes != 0 && hash_ok && contract_ok;
    let message = alloc::format!(
        "gpgpu probe font-tessel artifact: ok={} kernel={} role={:?} target={} source={} bin_bytes=0x{:X} spv_bytes=0x{:X} gpu=0x{:X} verified={} hash_allowlisted={} contract_ok={} registry_all_ok={} args={} bindings={} cross_thread={} per_thread={} sha256={}",
        ok as u8,
        artifact.name,
        known.role,
        artifact.target,
        upload.source,
        artifact.bin.len(),
        artifact.spv.len(),
        upload.gpu,
        upload.verified as u8,
        hash_ok as u8,
        contract_ok as u8,
        registry_report.passed() as u8,
        known.contract.args.len(),
        known.contract.binding_count,
        known.contract.cross_thread_bytes,
        known.contract.per_thread_bytes,
        digest_hex(&upload.bin_sha256),
    );
    print_shell_line(io, message.as_str());
    if ok {
        crate::log_info!(target: "gpgpu"; "{}\n", message.as_str());
    } else {
        crate::log_error!(target: "gpgpu"; "{}\n", message.as_str());
    }
    ok
}

fn run_font_tessel_stage(
    io: &'static dyn ShellBackend2,
    ops: &[[u32; 8]],
    units_per_em: u16,
    stage: u32,
) -> (bool, crate::intel::gpgpu::GpgpuFontOutlineProbeResult) {
    let checksum = outline_checksum(ops);
    let result = shell_font_outline_probe(ops, checksum, stage, units_per_em);
    let expected_segments = result
        .line_count
        .saturating_add(result.close_count)
        .saturating_add(result.quad_count.saturating_mul(8))
        .saturating_add(result.cubic_count.saturating_mul(8));
    let shape_ok = match stage {
        FONT_OUTLINE_STAGE_AUDIT => result.vertices == 0 && result.indices == 0,
        FONT_OUTLINE_STAGE_FLATTEN => {
            result.segments == expected_segments
                && result.vertices == result.move_count.saturating_add(expected_segments)
                && result.indices == 0
        }
        FONT_OUTLINE_STAGE_STROKE_MESH => {
            let emitted_segments = result.vertices / 4;
            result.segments == expected_segments
                && result.vertices != 0
                && result.vertices % 4 == 0
                && emitted_segments <= expected_segments
                && result.indices == emitted_segments.saturating_mul(6)
                && result.indices % 3 == 0
        }
        _ => false,
    };
    let ok = result.ok && shape_ok;
    let label = match stage {
        FONT_OUTLINE_STAGE_AUDIT => "audit",
        FONT_OUTLINE_STAGE_FLATTEN => "flatten",
        FONT_OUTLINE_STAGE_STROKE_MESH => "mesh",
        _ => "unknown",
    };
    let message = alloc::format!(
        "gpgpu probe font-tessel {}: ok={} hw_ok={} shape_ok={} setup=[{},{},{},{},{},{},{},{},{}] retired={} kernel_done={} ops={} move={} line={} quad={} cubic={} close={} segments={}/{} vertices={} indices={} checksum=0x{:08X} invalid={} truncated={} index_range={} markers=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] bounds=({:.2},{:.2})..({:.2},{:.2}) geometry={} retained=probe-scratch cpu_geometry_math=0",
        label,
        ok as u8,
        result.ok as u8,
        shape_ok as u8,
        result.available as u8,
        result.forcewake_ok as u8,
        result.mapped_ok as u8,
        result.ppgtt_ok as u8,
        result.kernel_ppgtt_ok as u8,
        result.src_ppgtt_ok as u8,
        result.dst_ppgtt_ok as u8,
        result.batch_ok as u8,
        result.submitted as u8,
        result.retired as u8,
        result.kernel_done as u8,
        result.op_count,
        result.move_count,
        result.line_count,
        result.quad_count,
        result.cubic_count,
        result.close_count,
        result.segments,
        expected_segments,
        result.vertices,
        result.indices,
        result.checksum,
        result.invalid,
        result.truncated as u8,
        result.indices_in_range as u8,
        result.pre_marker,
        result.post_marker,
        result.report_marker,
        result.done_marker,
        result.min_x,
        result.min_y,
        result.max_x,
        result.max_y,
        if stage == FONT_OUTLINE_STAGE_STROKE_MESH {
            "indexed-stroke-triangles"
        } else if stage == FONT_OUTLINE_STAGE_FLATTEN {
            "flat-points"
        } else {
            "none"
        },
    );
    print_shell_line(io, message.as_str());
    (ok, result)
}

fn run_font_tessel(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let mode = args.next().unwrap_or("all");
    if !expect_no_more(io, args) {
        return;
    }
    let wants_artifact = mode.eq_ignore_ascii_case("artifact") || mode.eq_ignore_ascii_case("all");
    let wants_audit = mode.eq_ignore_ascii_case("audit") || mode.eq_ignore_ascii_case("all");
    let wants_flatten = mode.eq_ignore_ascii_case("flatten") || mode.eq_ignore_ascii_case("all");
    let wants_mesh = mode.eq_ignore_ascii_case("mesh") || mode.eq_ignore_ascii_case("all");
    if !(wants_artifact || wants_audit || wants_flatten || wants_mesh) {
        usage(io);
        return;
    }
    let Ok(outline) = crate::graphics::font::default_gpu_outline() else {
        print_shell_line(io, "gpgpu probe font-tessel: ok=0 reason=outline-unavailable");
        return;
    };
    let intro = alloc::format!(
        "gpgpu probe font-tessel: text=\"{}\" font={} file={} units_per_em={} glyphs={} contours={} full_ops={} mesh_ops={} outline_checksum=0x{:08X} source=skrifa-warm-outline placement=full-text-stream orientation=upright fill_tessellation=0",
        outline.text,
        outline.font_name,
        outline.font_file,
        outline.units_per_em,
        outline.glyphs,
        outline.contours,
        outline.ops.len(),
        outline.ops.len(),
        outline.checksum,
    );
    print_shell_line(io, intro.as_str());
    crate::log_info!(target: "gpgpu"; "{}\n", intro.as_str());

    let mut ok = true;
    if wants_artifact {
        ok &= run_font_tessel_artifact(io);
    }
    if wants_audit {
        let (stage_ok, _) = run_font_tessel_stage(
            io,
            outline.ops.as_slice(),
            outline.units_per_em,
            FONT_OUTLINE_STAGE_AUDIT,
        );
        ok &= stage_ok;
    }
    if wants_flatten {
        let (stage_ok, _) = run_font_tessel_stage(
            io,
            outline.ops.as_slice(),
            outline.units_per_em,
            FONT_OUTLINE_STAGE_FLATTEN,
        );
        ok &= stage_ok;
    }
    if wants_mesh {
        let (stage_ok, mesh_result) = run_font_tessel_stage(
            io,
            outline.ops.as_slice(),
            outline.units_per_em,
            FONT_OUTLINE_STAGE_STROKE_MESH,
        );
        ok &= stage_ok;
        let (chain, chain_error) = if stage_ok {
            match mesh_result.generated_mesh {
                Some(mesh) => match crate::intel::render::submit_gpu_font_outline_mesh_once(mesh) {
                    Ok(render) => (Some(render), "none"),
                    Err(reason) => (None, reason),
                },
                None => (None, "mesh-descriptor-missing"),
            }
        } else {
            (None, "compute-stage-failed")
        };
        let chain_ok = chain.as_ref().is_some_and(|render| render.completed);
        ok &= chain_ok;
        print_shell_line(
            io,
            alloc::format!(
                "gpgpu probe font-tessel compute-to-3d: ok={} mesh_ready={} completed={} vs={} clip={} ps={} error={} cpu_geometry_copy=0 target=scratch-visible-overlay",
                chain_ok as u8,
                mesh_result.generated_mesh.is_some() as u8,
                chain.as_ref().is_some_and(|render| render.completed) as u8,
                chain.as_ref().is_some_and(|render| render.vs_counter) as u8,
                chain.as_ref().is_some_and(|render| render.clip_counter) as u8,
                chain.as_ref().is_some_and(|render| render.ps_observed) as u8,
                chain_error,
            )
            .as_str(),
        );
    }
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu probe font-tessel done: ok={} compute_to_3d={} scope=full-True-OS-section-sign presentation=native-512x512-1to1 next=hole-aware-fill",
            ok as u8,
            wants_mesh as u8
        )
        .as_str(),
    );
}

fn run_probe(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(probe) = args.next() else {
        usage(io);
        return;
    };
    if probe.eq_ignore_ascii_case("copy-rect") {
        if expect_no_more(io, args) {
            run_copy_rect_probe(io);
        }
    } else if probe.eq_ignore_ascii_case("font-tessel") {
        run_font_tessel(io, args);
    } else {
        usage(io);
    }
}

fn run_copy_rect_probe(io: &'static dyn ShellBackend2) {
    let result = crate::intel::gpgpu::shell_copy_rect_rgba8_probe();
    let hash = digest_hex(&result.artifact_sha256);
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu probe copy-rect: ok={} reboot_required={} frontend={} feature={} feature_enabled={} artifact={} artifact_source={} target={} verified={} device={:02X}:{:02X}.{}-0x{:04X}-r{:02X} hash={} cases={}/{} retired={} passed={} first_failure_case={} first_failure={}",
            result.ok as u8,
            result.reboot_required as u8,
            result.frontend,
            result.feature,
            result.feature_enabled as u8,
            result.artifact,
            result.artifact_source,
            result.artifact_target,
            result.artifact_verified as u8,
            result.pci_bus,
            result.pci_slot,
            result.pci_function,
            result.device_id,
            result.revision_id,
            hash,
            result.attempted_cases,
            result.case_count,
            result.retired_cases,
            result.passed_cases,
            result.first_failure_case,
            result.first_failure,
        )
        .as_str(),
    );
    for case in result.cases.iter().take(result.case_count) {
        print_shell_line(
            io,
            alloc::format!(
                "gpgpu probe copy-rect case={}: attempted={} submitted={} retired={} ok={} src={}x{} pitch={} origin=({}, {}) dst={}x{} pitch={} origin=({}, {}) copy={}x{} checked_copy={} checked_guards={} checked_source={} markers=[0x{:08X},0x{:08X}] retire_ms={} first_failure={} failure_has_offset={} failure_offset=0x{:X} expected=0x{:08X} observed=0x{:08X}",
                case.label,
                case.attempted as u8,
                case.submitted as u8,
                case.retired as u8,
                case.ok as u8,
                case.src_width,
                case.src_height,
                case.src_pitch_bytes,
                case.src_x,
                case.src_y,
                case.dst_width,
                case.dst_height,
                case.dst_pitch_bytes,
                case.dst_x,
                case.dst_y,
                case.width,
                case.height,
                case.copied_pixels_checked,
                case.guard_pixels_checked,
                case.source_pixels_checked,
                case.pre_marker,
                case.post_marker,
                case.retire_ms,
                case.first_failure,
                case.failure_byte_offset.is_some() as u8,
                case.failure_byte_offset.unwrap_or(0),
                case.expected,
                case.observed,
            )
            .as_str(),
        );
    }
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(cmd) = args.next() else {
        usage(io);
        return ParseOutcome::Handled;
    };

    if cmd.eq_ignore_ascii_case("preview") {
        run_preview(io, args);
    } else if cmd.eq_ignore_ascii_case("test") {
        run_test(io, args);
    } else if cmd.eq_ignore_ascii_case("svg") {
        run_svg(io, args);
    } else if cmd.eq_ignore_ascii_case("probe") {
        run_probe(io, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
