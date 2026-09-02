use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::gpu::vgpu::{self, KernelClient};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vgpu status");
    print_shell_line(io, "vgpu test broker|abi|guc|compute|blit|all");
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next(), args.next()) {
        (Some(cmd), None, None) if cmd.eq_ignore_ascii_case("status") => print_status(io),
        (Some(cmd), Some(test), None) if cmd.eq_ignore_ascii_case("test") => {
            let passed = match test {
                test if test.eq_ignore_ascii_case("broker") => test_broker(io),
                test if test.eq_ignore_ascii_case("abi") => test_abi(io),
                test if test.eq_ignore_ascii_case("guc") => test_guc(io),
                test if test.eq_ignore_ascii_case("compute") => test_compute(io),
                test if test.eq_ignore_ascii_case("blit") => test_blit(io),
                test if test.eq_ignore_ascii_case("all") => {
                    let broker = test_broker(io);
                    let abi = test_abi(io);
                    let guc = test_guc(io);
                    let compute = test_compute(io);
                    let blit = test_blit(io);
                    broker && abi && guc && compute && blit
                }
                _ => {
                    usage(io);
                    return ParseOutcome::Handled;
                }
            };
            print_shell_line(io, format!("vgpu test {test}: pass={}", passed as u8).as_str());
        }
        _ => usage(io),
    }
    ParseOutcome::Handled
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = vgpu::broker_status();
    let executor = crate::gpu::executor::status();
    print_shell_line(
        io,
        format!(
            "vgpu: physical_ready={} physical_lost={} adapter={} pci=8086:{:04X} rev={:02X} guc={} epoch={} devices={} contexts={}/{} submissions={} failures={} faulted_contexts={} quarantined_engine_lanes=0x{:08X} owner_handoffs_pending={} memory_cat_faults={} unattributed_faults={} gt_faulted={}",
            status.physical_ready as u8,
            status.physical_lost as u8,
            status.physical_name,
            status.physical_device_id,
            status.physical_revision_id,
            status.guc_submission as u8,
            status.epoch,
            status.devices.len(),
            status.scheduler.registered_contexts,
            status.scheduler.context_capacity,
            status.scheduler.submissions,
            status.scheduler.failures,
            status.scheduler.faulted_contexts,
            status.scheduler.quarantined_engine_lanes,
            status.scheduler.owner_handoffs_pending,
            status.scheduler.memory_cat_faults,
            status.scheduler.unattributed_faults,
            status.scheduler.gt_faulted as u8,
        )
        .as_str(),
    );
    if let Some(crumbs) = crate::intel::render::latest_helio_retained_transform_breadcrumbs() {
        print_shell_line(
            io,
            format!(
                "vgpu: helio-retained entry=0x{:08X} opening=0x{:08X} vf=0x{:08X} vs=0x{:08X} ps=0x{:08X} clip=0x{:08X} raster=0x{:08X} pre3d=0x{:08X} postdraw=0x{:08X} final=0x{:08X} transform=0x{:08X}/0x{:08X}/0x{:08X}/0x{:08X} returned=0x{:08X} release=0x{:08X}:0x{:08X}",
                crumbs.secondary_entry,
                crumbs.post_opening_pipe_controls,
                crumbs.post_vf_state,
                crumbs.post_vs_state,
                crumbs.post_ps_state,
                crumbs.post_clip_state,
                crumbs.post_raster_state,
                crumbs.pre_3dprimitive,
                crumbs.pre_postdraw_sync,
                crumbs.final_marker,
                crumbs.transform_prologue,
                crumbs.transform_prepare,
                crumbs.transform_rows,
                crumbs.transform_handoff_3d,
                crumbs.secondary_return,
                crumbs.scene_release_hi,
                crumbs.scene_release_lo,
            )
            .as_str(),
        );
    }
    let boundaries = status.kernel_context_boundaries;
    print_shell_line(
        io,
        format!(
            "vgpu: context-boundaries valid={} coherent={} unique_hwlrcas={} unique_ppgtt_roots={} bound={} active={} lost_bound={} helio_render_live={} spirit_execution_live={} font_engine_live={} helio_spirit_hwlrca_distinct={} helio_spirit_ppgtt_distinct={} helio_spirit_coexistence={} font_helio_hwlrca_distinct={} font_helio_ppgtt_distinct={} font_spirit_hwlrca_distinct={} font_spirit_ppgtt_distinct={} font_helio_spirit_coexistence={} render_principals_declared={}",
            boundaries.valid() as u8,
            boundaries.coherent as u8,
            boundaries.unique_hwlrcas as u8,
            boundaries.unique_ppgtt_roots as u8,
            boundaries.bound,
            boundaries.active,
            boundaries.lost_bound,
            boundaries.helio_render_live as u8,
            boundaries.spirit_execution_live as u8,
            boundaries.font_engine_live as u8,
            boundaries.helio_spirit_distinct_hwlrca as u8,
            boundaries.helio_spirit_distinct_ppgtt_root as u8,
            boundaries.helio_spirit_valid() as u8,
            boundaries.font_helio_distinct_hwlrca as u8,
            boundaries.font_helio_distinct_ppgtt_root as u8,
            boundaries.font_spirit_distinct_hwlrca as u8,
            boundaries.font_spirit_distinct_ppgtt_root as u8,
            boundaries.font_helio_spirit_valid() as u8,
            KernelClient::RENDER_CARRIERS.len(),
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "vgpu: executor submissions={} completions={} failures={} preparing={} admitting={} inflight={} waiters={} lost_clients={}",
            executor.submissions,
            executor.completions,
            executor.failures,
            executor.preparing,
            executor.admitting,
            executor.inflight,
            executor.waiters,
            executor.lost_clients,
        )
        .as_str(),
    );
    for device in status.devices {
        print_shell_line(
            io,
            format!(
                "vgpu: device=0x{:016X} principal={:?} caps=0x{:016X} epoch={} lost={} memory={}/{} buffers={} queues={} contexts={} vvideo={} identity={} digest=0x{:016X} copied_upload_bytes={} flushed_vvideo_bytes={}",
                device.handle.raw(),
                device.principal,
                device.capabilities.bits(),
                device.epoch,
                device.lost as u8,
                device.memory_used,
                device.memory_quota,
                device.buffers,
                device.queues,
                device.contexts,
                device.vvideo_buffers,
                device.vvideo_mapping_identity as u8,
                device.vvideo_mapping_digest,
                device.copied_upload_bytes,
                device.flushed_vvideo_bytes,
            )
            .as_str(),
        );
        if let Some(identity) = device.kernel_context_capability {
            print_shell_line(
                io,
                format!(
                    "vgpu: context-capability principal={} engine={:?}:{} hwlrca=0x{:08X}:0x{:08X} ppgtt-root=0x{:016X} immutable=1 registered={}",
                    device.principal.name(),
                    identity.engine.class,
                    identity.engine.instance,
                    identity.hwlrca_hi,
                    identity.hwlrca_lo,
                    identity.gpuvm_root_phys,
                    device.contexts,
                )
                .as_str(),
            );
        }
    }
    print_kernel_timeline(io, "render-graphics-0", KernelClient::Render);
    print_kernel_timeline(io, "render-graphics-1", KernelClient::Render1);
    print_kernel_timeline(io, "render-graphics-2", KernelClient::Render2);
    print_kernel_timeline(io, "gpgpu-system", KernelClient::GpgpuSystem);
    print_kernel_timeline(io, "gpgpu-font", KernelClient::GpgpuFont);
    print_kernel_timeline(io, "gpgpu-execution", KernelClient::GpgpuExecution);
    print_kernel_timeline(io, "ui4-compositor", KernelClient::Ui4Compositor);
    print_kernel_timeline(io, "ui4-blitter", KernelClient::Ui4Blitter);
}

fn print_kernel_timeline(io: &'static dyn ShellBackend2, name: &str, client: KernelClient) {
    if let Some(timeline) = vgpu::kernel_timeline(client) {
        print_shell_line(
            io,
            format!(
                "vgpu: timeline={} submitted={} completed={} failures={} physical_serial={}",
                name,
                timeline.submitted,
                timeline.completed,
                timeline.failures,
                timeline.last_physical_serial,
            )
            .as_str(),
        );
    }
}

fn test_broker(io: &'static dyn ShellBackend2) -> bool {
    let report = vgpu::run_broker_self_test();
    print_shell_line(
        io,
        format!(
            "vgpu broker: opened={} separate_gpuvms={} buffer={} isolation={} quota={} timeline={} device_loss={} stale={} cleanup={}",
            report.opened as u8,
            report.separate_gpuvms as u8,
            report.buffer_lifecycle as u8,
            report.cross_principal_rejected as u8,
            report.quota_rejected as u8,
            report.timeline_monotonic as u8,
            report.device_loss_propagated as u8,
            report.stale_handle_rejected as u8,
            report.cleanup as u8,
        )
        .as_str(),
    );
    report.passed()
}

fn test_abi(io: &'static dyn ShellBackend2) -> bool {
    let Ok(device) = v::vgpu::Device::open(v::vgpu::Capabilities::DEFAULT) else {
        print_shell_line(io, "vgpu abi: open=0");
        return false;
    };
    let mut buffer = None;
    let mut queue = None;
    let result = (|| {
        let info = device.info().ok()?;
        let created_buffer = device
            .create_buffer(4096, v::vgpu::BUFFER_USAGE_MAP_READ | v::vgpu::BUFFER_USAGE_MAP_WRITE)
            .ok()?;
        buffer = Some(created_buffer);
        let buffer_info = device.buffer_info(created_buffer).ok()?;
        let payload = b"vgpu-shared-bulk";
        device.write_buffer(created_buffer, 17, payload).ok()?;
        let mut readback = [0u8; 16];
        device.read_buffer(created_buffer, 17, &mut readback).ok()?;
        let created_queue = device.create_queue(v::vgpu::QueueClass::Compute).ok()?;
        queue = Some(created_queue);
        let first = device.submit_control_nop(created_queue).ok()?;
        let second = device.submit_control_nop(created_queue).ok()?;
        let timeline = device.timeline(created_queue).ok()?;
        device.wait(created_queue, second.value).ok()?;
        Some(
            info.capabilities & v::vgpu::Capabilities::DEFAULT.bits()
                == v::vgpu::Capabilities::DEFAULT.bits()
                && buffer_info.bytes == 4096
                && readback == *payload
                && first.value == 1
                && second.value == 2
                && timeline.submitted == 2
                && timeline.completed == 2,
        )
    })()
    .unwrap_or(false);
    if let Some(queue) = queue {
        let _ = device.destroy_queue(queue);
    }
    if let Some(buffer) = buffer {
        let _ = device.destroy_buffer(buffer);
    }
    let closed = device.close().is_ok();
    print_shell_line(
        io,
        format!("vgpu abi: lifecycle={} cleanup={}", result as u8, closed as u8).as_str(),
    );
    result && closed
}

fn test_guc(io: &'static dyn ShellBackend2) -> bool {
    let status = vgpu::broker_status();
    let boundaries = status.kernel_context_boundaries;
    let passed = status.physical_ready
        && !status.physical_lost
        && status.guc_submission
        && status.scheduler.context_capacity >= 2
        && status.scheduler.registered_contexts >= boundaries.active
        && status.scheduler.enabled_contexts >= boundaries.active
        && status.scheduler.failures == 0
        && status.scheduler.faulted_contexts == 0
        && status.scheduler.owner_handoffs_pending == 0
        && !status.scheduler.gt_faulted
        && boundaries.valid()
        && boundaries.bound == boundaries.active
        && boundaries.helio_spirit_valid()
        && boundaries.lost_bound == 0;
    print_shell_line(
        io,
        format!(
            "vgpu guc: ready={} physical_lost={} guc={} registered={} enabled={} capacity={} submissions={} registrations={} failures={} faulted_contexts={} quarantined_engine_lanes=0x{:08X} owner_handoffs_pending={} gt_faulted={} context_boundaries={} coherent={} unique_hwlrcas={} unique_ppgtt_roots={} bound={} active={} lost_bound={} helio_render_live={} spirit_execution_live={} font_engine_live={} helio_spirit_hwlrca_distinct={} helio_spirit_ppgtt_distinct={} helio_spirit_coexistence={} font_helio_hwlrca_distinct={} font_helio_ppgtt_distinct={} font_spirit_hwlrca_distinct={} font_spirit_ppgtt_distinct={} font_helio_spirit_coexistence={}",
            status.physical_ready as u8,
            status.physical_lost as u8,
            status.guc_submission as u8,
            status.scheduler.registered_contexts,
            status.scheduler.enabled_contexts,
            status.scheduler.context_capacity,
            status.scheduler.submissions,
            status.scheduler.registrations,
            status.scheduler.failures,
            status.scheduler.faulted_contexts,
            status.scheduler.quarantined_engine_lanes,
            status.scheduler.owner_handoffs_pending,
            status.scheduler.gt_faulted as u8,
            boundaries.valid() as u8,
            boundaries.coherent as u8,
            boundaries.unique_hwlrcas as u8,
            boundaries.unique_ppgtt_roots as u8,
            boundaries.bound,
            boundaries.active,
            boundaries.lost_bound,
            boundaries.helio_render_live as u8,
            boundaries.spirit_execution_live as u8,
            boundaries.font_engine_live as u8,
            boundaries.helio_spirit_distinct_hwlrca as u8,
            boundaries.helio_spirit_distinct_ppgtt_root as u8,
            boundaries.helio_spirit_valid() as u8,
            boundaries.font_helio_distinct_hwlrca as u8,
            boundaries.font_helio_distinct_ppgtt_root as u8,
            boundaries.font_spirit_distinct_hwlrca as u8,
            boundaries.font_spirit_distinct_ppgtt_root as u8,
            boundaries.font_helio_spirit_valid() as u8,
        )
        .as_str(),
    );
    passed
}

fn test_compute(io: &'static dyn ShellBackend2) -> bool {
    let before = vgpu::kernel_timeline(KernelClient::GpgpuSystem).unwrap_or_default();
    let dispatch = crate::intel::gpgpu::submit_solid_composite_worklist_rgba8_probe_now();
    let after = vgpu::kernel_timeline(KernelClient::GpgpuSystem).unwrap_or_default();
    let timeline = after.submitted > before.submitted && after.completed == after.submitted;
    print_shell_line(
        io,
        format!(
            "vgpu compute: dispatch={} timeline={} submitted={}->{} completed={} physical_serial={}",
            dispatch as u8,
            timeline as u8,
            before.submitted,
            after.submitted,
            after.completed,
            after.last_physical_serial,
        )
        .as_str(),
    );
    dispatch && timeline
}

fn test_blit(io: &'static dyn ShellBackend2) -> bool {
    let before = vgpu::kernel_timeline(KernelClient::Ui4Blitter).unwrap_or_default();
    let probe = crate::intel::submit_guc_bcs0_fast_copy_probe_now();
    let after = vgpu::kernel_timeline(KernelClient::Ui4Blitter).unwrap_or_default();
    let timeline = after.submitted > 0
        && after.submitted >= before.submitted
        && after.completed == after.submitted
        && after.failures == 0;
    print_shell_line(
        io,
        format!(
            "vgpu blit: engine=bcs0 path=guc forcewake={} ggtt={} ppgtt={} batch={} submitted={} pending={} retired={} context_saved={} saved_head={} published_tail={} timeline_retired={} copy_ok={} src_preserved={} marker=0x{:08X} retire_ms={} timeline={} points={}->{} completed={} physical_serial={} legacy_fallback=0",
            probe.forcewake as u8,
            probe.ggtt as u8,
            probe.ppgtt as u8,
            probe.batch as u8,
            probe.submitted as u8,
            probe.pending as u8,
            probe.retired as u8,
            probe.context_saved as u8,
            probe.saved_head & (4096 - 1),
            probe.published_tail,
            probe.timeline_retired as u8,
            probe.copy_ok as u8,
            probe.src_preserved as u8,
            probe.marker,
            probe.retire_ms,
            timeline as u8,
            before.submitted,
            after.submitted,
            after.completed,
            after.last_physical_serial,
        )
        .as_str(),
    );
    probe.passed() && timeline
}
