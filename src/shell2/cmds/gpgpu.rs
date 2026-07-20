use alloc::string::String;
use core::fmt::Write;
use core::str::SplitWhitespace;

use embassy_executor::{Spawner, task};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line,
};
use crate::intel::gpgpu::{
    CHART_SINE_RGBA8_ADLS_ARTIFACT, FONT_OUTLINE_MESH_ADLS_ARTIFACT, FONT_OUTLINE_STAGE_AUDIT,
    FONT_OUTLINE_STAGE_FLATTEN, FONT_OUTLINE_STAGE_STROKE_MESH, PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT,
    UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME, probe_runtime_artifact_async,
    reload_all_known_kernel_artifacts, reload_known_kernel_artifact, shell_font_outline_probe,
    upload_chart_sine_rgba8_kernel, upload_font_outline_mesh_kernel,
    upload_pixel_plasma_rgba8_kernel,
};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu preview start [duration_ms] [cadence_ms] [publish_every]");
    print_shell_line(io, "gpgpu preview status");
    print_shell_line(io, "gpgpu preview stop");
    print_shell_line(io, "gpgpu chart artifact");
    print_shell_line(io, "gpgpu pixel artifact");
    print_shell_line(io, "gpgpu probe font-tessel [artifact|audit|flatten|mesh|all]");
    print_shell_line(io, "gpgpu artifacts probe <video|kernel>");
    print_shell_line(io, "gpgpu artifacts reload <kernel|all>");
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
        let preset = crate::ui4::GpgpuPreviewPreset::All;
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
                        "gpgpu preview start: queued=1 request={} demos=mandelbrot+chart+plasma service_online={} duration_ms={} cadence_ms={} publish_every={} ui4_consumer=kernel-app-5 frames=3 windows=3 buffering={} plane_layout={} slot_policy=fixed-per-window/no-round-robin interaction=movable-fixed-size",
                        serial,
                        status.online as u8,
                        duration_ms,
                        cadence_ms,
                        publish_every,
                        preset.buffering_label(),
                        preset.plane_layout_label(),
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

fn print_preview_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_preview_status();
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu preview status: online={} phase={} desired_running={} request={} applied={} preset={} duration_ms={} cadence_ms={} publish_every={} frame={} window={} attempted={} submitted={} completed={} published={} dropped_busy={} failed={} late={} elapsed_ms={} buffering={} plane_layout={} interaction=movable-fixed-size error={}",
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

fn run_chart(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(probe) = args.next() else {
        usage(io);
        return;
    };
    if probe.eq_ignore_ascii_case("artifact") {
        if expect_no_more(io, args) {
            run_chart_artifact(io);
        }
    } else {
        usage(io);
    }
}

fn run_chart_artifact(io: &'static dyn ShellBackend2) {
    let artifact = CHART_SINE_RGBA8_ADLS_ARTIFACT;
    let registry_report = crate::intel::opencl::trueos_cl_validate_known_aot_registry();
    let Some(known) = crate::intel::opencl::registry::known_aot_kernel(artifact.name) else {
        print_shell_line(
            io,
            "gpgpu chart artifact: ok=0 stage=registry reason=missing-known-aot-contract",
        );
        crate::log_error!(
            target: "gpgpu";
            "gpgpu chart artifact failed stage=registry kernel={}\n",
            artifact.name
        );
        return;
    };
    let Some(upload) = upload_chart_sine_rgba8_kernel() else {
        print_shell_line(io, "gpgpu chart artifact: ok=0 stage=upload reason=unavailable");
        return;
    };
    let hash_ok = upload.bin_sha256 == artifact.bin_sha256;
    let contract_ok = known.contract.name == artifact.name
        && known.contract.target == artifact.target
        && known.contract.cross_thread_bytes == 128
        && known.contract.per_thread_bytes == 96
        && known.contract.binding_count == 1;
    let ok = upload.verified && upload.bytes != 0 && hash_ok && contract_ok;
    let message = alloc::format!(
        "gpgpu chart artifact: ok={} stage=artifact kernel={} role={:?} target={} producer={:?} source={} source_path={} bin_bytes=0x{:X} spv_bytes=0x{:X} gpu=0x{:X} mapped=0x{:X} verified={} hash_allowlisted={} contract_ok={} registry_ok={} registry_kernels={} registry_issues={} args={} bindings={} cross_thread={} per_thread={} sha256={}",
        ok as u8,
        artifact.name,
        known.role,
        artifact.target,
        known.contract.producer,
        upload.source,
        known.contract.source_path,
        artifact.bin.len(),
        artifact.spv.len(),
        upload.gpu,
        upload.mapped_bytes,
        upload.verified as u8,
        hash_ok as u8,
        contract_ok as u8,
        registry_report.passed() as u8,
        registry_report.registry_kernels,
        registry_report.issues,
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
}

fn run_pixel(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(probe) = args.next() else {
        usage(io);
        return;
    };
    if probe.eq_ignore_ascii_case("artifact") {
        if expect_no_more(io, args) {
            run_pixel_artifact(io);
        }
    } else {
        usage(io);
    }
}

fn run_pixel_artifact(io: &'static dyn ShellBackend2) {
    let artifact = PIXEL_PLASMA_RGBA8_ADLS_ARTIFACT;
    let registry_report = crate::intel::opencl::trueos_cl_validate_known_aot_registry();
    let Some(known) = crate::intel::opencl::registry::known_aot_kernel(artifact.name) else {
        print_shell_line(
            io,
            "gpgpu pixel artifact: ok=0 stage=registry reason=missing-known-aot-contract",
        );
        crate::log_error!(
            target: "gpgpu";
            "gpgpu pixel artifact failed stage=registry kernel={}\n",
            artifact.name
        );
        return;
    };
    let Some(upload) = upload_pixel_plasma_rgba8_kernel() else {
        print_shell_line(io, "gpgpu pixel artifact: ok=0 stage=upload reason=unavailable");
        return;
    };
    let hash_ok = upload.bin_sha256 == artifact.bin_sha256;
    let contract_ok = known.contract.name == artifact.name
        && known.contract.target == artifact.target
        && known.contract.cross_thread_bytes == 128
        && known.contract.per_thread_bytes == 96
        && known.contract.binding_count == 1;
    let ok =
        registry_report.passed() && upload.verified && upload.bytes != 0 && hash_ok && contract_ok;
    let message = alloc::format!(
        "gpgpu pixel artifact: ok={} stage=artifact kernel={} role={:?} target={} producer={:?} source={} source_path={} bin_bytes=0x{:X} spv_bytes=0x{:X} gpu=0x{:X} mapped=0x{:X} verified={} hash_allowlisted={} contract_ok={} registry_ok={} registry_kernels={} registry_issues={} args={} bindings={} cross_thread={} per_thread={} sha256={}",
        ok as u8,
        artifact.name,
        known.role,
        artifact.target,
        known.contract.producer,
        upload.source,
        known.contract.source_path,
        artifact.bin.len(),
        artifact.spv.len(),
        upload.gpu,
        upload.mapped_bytes,
        upload.verified as u8,
        hash_ok as u8,
        contract_ok as u8,
        registry_report.passed() as u8,
        registry_report.registry_kernels,
        registry_report.issues,
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
    if probe.eq_ignore_ascii_case("font-tessel") {
        run_font_tessel(io, args);
    } else {
        usage(io);
    }
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[task(pool_size = 1)]
async fn artifact_async_probe_task(target: MatrixTarget, name: String) {
    match probe_runtime_artifact_async(name.as_str()).await {
        Ok(probe) => print_matrix_target_line(
            &target,
            alloc::format!(
                "gpgpu artifacts probe {}: ok carrier=managed-trueosfs-async bytes={} matches_embedded={} sha256={} gpu_state=untouched",
                probe.name,
                probe.bytes,
                probe.matches_embedded as u8,
                digest_hex(&probe.sha256),
            )
            .as_str(),
        ),
        Err(error) => print_matrix_target_line(
            &target,
            alloc::format!(
                "gpgpu artifacts probe {}: failed reason={} detail={error:?}",
                name,
                error.label(),
            )
            .as_str(),
        ),
    }
}

fn run_artifacts(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) {
    let Some(cmd) = args.next() else {
        usage(io);
        return;
    };
    if cmd.eq_ignore_ascii_case("probe") {
        let Some(raw_name) = args.next() else {
            usage(io);
            return;
        };
        if !expect_no_more(io, args) {
            return;
        }
        let name = if raw_name.eq_ignore_ascii_case("video") {
            String::from(UI4_NV12_TILE64_TO_RGBA8_FRAME_KERNEL_NAME)
        } else {
            String::from(raw_name)
        };
        let target = matrix_target_for_backend(io);
        match artifact_async_probe_task(target.clone(), name) {
            Ok(token) => {
                spawner.spawn(token);
                print_matrix_target_line(
                    &target,
                    "gpgpu artifacts probe: queued carrier=managed-trueosfs-async gpu_state=untouched",
                );
            }
            Err(_) => print_matrix_target_line(
                &target,
                "gpgpu artifacts probe: failed reason=task-unavailable",
            ),
        }
        return;
    }
    if !cmd.eq_ignore_ascii_case("reload") {
        usage(io);
        return;
    }
    let Some(name) = args.next() else {
        usage(io);
        return;
    };
    if !expect_no_more(io, args) {
        return;
    }

    if name.eq_ignore_ascii_case("all") {
        let summary = reload_all_known_kernel_artifacts();
        print_shell_line(
            io,
            alloc::format!(
                "gpgpu artifacts reload all: attempted={} reloaded={} failed={}",
                summary.attempted,
                summary.reloaded,
                summary.failed
            )
            .as_str(),
        );
        return;
    }

    match reload_known_kernel_artifact(name) {
        Ok(upload) => print_shell_line(
            io,
            alloc::format!(
                "gpgpu artifacts reload {}: ok source={} gpu=0x{:X} bytes=0x{:X} sha256={}",
                upload.name,
                upload.source,
                upload.gpu,
                upload.bytes,
                digest_hex(&upload.bin_sha256)
            )
            .as_str(),
        ),
        Err(err) => print_shell_line(
            io,
            alloc::format!("gpgpu artifacts reload {}: failed reason={}", name, err.label())
                .as_str(),
        ),
    }
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
    } else if cmd.eq_ignore_ascii_case("chart") {
        run_chart(io, args);
    } else if cmd.eq_ignore_ascii_case("pixel") {
        run_pixel(io, args);
    } else if cmd.eq_ignore_ascii_case("probe") {
        run_probe(io, args);
    } else if cmd.eq_ignore_ascii_case("artifacts") {
        run_artifacts(_spawner, io, args);
    } else {
        usage(io);
    }

    ParseOutcome::Handled
}
