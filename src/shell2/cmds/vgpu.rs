use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::gpu::vgpu::{self, KernelClient};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vgpu status");
    print_shell_line(io, "vgpu test broker|abi|guc|compute|blit|font|all");
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
                test if test.eq_ignore_ascii_case("font") => test_font(io),
                test if test.eq_ignore_ascii_case("all") => {
                    let broker = test_broker(io);
                    let abi = test_abi(io);
                    let guc = test_guc(io);
                    let compute = test_compute(io);
                    let blit = test_blit(io);
                    let font = test_font(io);
                    broker && abi && guc && compute && blit && font
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
            "vgpu: physical_ready={} adapter={} pci=8086:{:04X} rev={:02X} guc={} epoch={} devices={} contexts={}/{} submissions={} failures={}",
            status.physical_ready as u8,
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
        )
        .as_str(),
    );
    print_shell_line(
        io,
        format!(
            "vgpu: executor submissions={} completions={} failures={} admitting={} inflight={} waiters={}",
            executor.submissions,
            executor.completions,
            executor.failures,
            executor.admitting,
            executor.inflight,
            executor.waiters,
        )
        .as_str(),
    );
    for device in status.devices {
        print_shell_line(
            io,
            format!(
                "vgpu: device=0x{:016X} principal={:?} caps=0x{:016X} epoch={} lost={} memory={}/{} buffers={} queues={} contexts={}",
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
            )
            .as_str(),
        );
    }
    print_kernel_timeline(io, "render/font", KernelClient::Render);
    print_kernel_timeline(io, "gpgpu", KernelClient::Gpgpu);
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
    let passed = status.physical_ready
        && status.guc_submission
        && status.scheduler.context_capacity >= 2
        && status.scheduler.failures == 0;
    print_shell_line(
        io,
        format!(
            "vgpu guc: ready={} guc={} registered={} enabled={} capacity={} submissions={} registrations={} failures={}",
            status.physical_ready as u8,
            status.guc_submission as u8,
            status.scheduler.registered_contexts,
            status.scheduler.enabled_contexts,
            status.scheduler.context_capacity,
            status.scheduler.submissions,
            status.scheduler.registrations,
            status.scheduler.failures,
        )
        .as_str(),
    );
    passed
}

fn test_compute(io: &'static dyn ShellBackend2) -> bool {
    let before = vgpu::kernel_timeline(KernelClient::Gpgpu).unwrap_or_default();
    let dispatch = crate::intel::gpgpu::submit_fill_rect_worklist_rgba8_probe_now();
    let after = vgpu::kernel_timeline(KernelClient::Gpgpu).unwrap_or_default();
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
            "vgpu blit: engine=bcs0 path=guc forcewake={} ggtt={} ppgtt={} batch={} submitted={} pending={} retired={} timeline_retired={} copy_ok={} src_preserved={} marker=0x{:08X} retire_ms={} timeline={} points={}->{} completed={} physical_serial={} legacy_fallback=0",
            probe.forcewake as u8,
            probe.ggtt as u8,
            probe.ppgtt as u8,
            probe.batch as u8,
            probe.submitted as u8,
            probe.pending as u8,
            probe.retired as u8,
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

fn test_font(io: &'static dyn ShellBackend2) -> bool {
    use crate::intel::gpu_font::{GPU_FONT_LEGACY_BLUE, GpuFontFace, GpuFontTextRequest};

    let before = vgpu::kernel_timeline(KernelClient::Render).unwrap_or_default();
    let render = crate::intel::gpu_font::stamp_text_once_with_font_centered(
        GpuFontTextRequest::SingleLine("hello"),
        GpuFontFace::Default,
        100,
        GPU_FONT_LEGACY_BLUE,
    );
    let after = vgpu::kernel_timeline(KernelClient::Render).unwrap_or_default();
    let rendered = render
        .as_ref()
        .is_ok_and(|result| result.stamped && result.render.completed);
    let timeline = after.submitted > before.submitted && after.completed == after.submitted;
    print_shell_line(
        io,
        format!(
            "vgpu font: text=hello configured=legacy-blue/default/100percent rendered={} timeline={} submitted={}->{} completed={} error={}",
            rendered as u8,
            timeline as u8,
            before.submitted,
            after.submitted,
            after.completed,
            render.err().unwrap_or("none"),
        )
        .as_str(),
    );
    rendered && timeline
}
