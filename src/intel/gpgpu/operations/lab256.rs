#[derive(Copy, Clone, Debug)]
struct Lab256Buffer {
    phys: u64,
    gpu: u64,
    virt: *mut u8,
    bytes: usize,
}

unsafe impl Send for Lab256Buffer {}
unsafe impl Sync for Lab256Buffer {}

#[derive(Copy, Clone, Debug)]
struct Lab256Runtime {
    state_a: Lab256Buffer,
    state_b: Lab256Buffer,
    control: Lab256Buffer,
    report: Lab256Buffer,
    read_from_a: bool,
    last_complete_frame: Option<u32>,
    quarantined: bool,
}

unsafe impl Send for Lab256Runtime {}
unsafe impl Sync for Lab256Runtime {}

impl Lab256Runtime {
    fn state_pair(self) -> (Lab256Buffer, Lab256Buffer) {
        if self.read_from_a {
            (self.state_a, self.state_b)
        } else {
            (self.state_b, self.state_a)
        }
    }

    fn accepts_frame(self, frame: u32) -> bool {
        match self.last_complete_frame {
            None => frame == 0,
            Some(last) => frame == last.saturating_add(1),
        }
    }
}

fn lab256_runtime_once() -> Option<Lab256Runtime> {
    if let Some(runtime) = *LAB256_RUNTIME.lock() {
        return Some(runtime);
    }

    let total_bytes = LAB256_STATE_BYTES
        .checked_mul(2)?
        .checked_add(LAB256_PAGE_BYTES.checked_mul(2)?)?;
    let (allocation_phys, allocation_virt) = crate::dma::alloc(total_bytes, super::WARM_ALIGN)?;
    unsafe {
        core::ptr::write_bytes(allocation_virt, 0, total_bytes);
    }
    super::dma_flush(allocation_virt, total_bytes);

    let state_b_offset = LAB256_STATE_BYTES;
    let control_offset = state_b_offset + LAB256_STATE_BYTES;
    let report_offset = control_offset + LAB256_PAGE_BYTES;
    let runtime = Lab256Runtime {
        state_a: Lab256Buffer {
            phys: allocation_phys,
            gpu: LAB256_STATE_A_GPU,
            virt: allocation_virt,
            bytes: LAB256_STATE_BYTES,
        },
        state_b: Lab256Buffer {
            phys: allocation_phys + state_b_offset as u64,
            gpu: LAB256_STATE_B_GPU,
            virt: unsafe { allocation_virt.add(state_b_offset) },
            bytes: LAB256_STATE_BYTES,
        },
        control: Lab256Buffer {
            phys: allocation_phys + control_offset as u64,
            gpu: LAB256_CONTROL_GPU,
            virt: unsafe { allocation_virt.add(control_offset) },
            bytes: LAB256_PAGE_BYTES,
        },
        report: Lab256Buffer {
            phys: allocation_phys + report_offset as u64,
            gpu: LAB256_REPORT_GPU,
            virt: unsafe { allocation_virt.add(report_offset) },
            bytes: LAB256_PAGE_BYTES,
        },
        read_from_a: true,
        last_complete_frame: None,
        quarantined: false,
    };
    *LAB256_RUNTIME.lock() = Some(runtime);
    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: lab256 persistent storage ready state=[0x{:X},0x{:X}] control=0x{:X} report=0x{:X} bytes=0x{:X} ppgtt=[0x{:X},0x{:X},0x{:X},0x{:X}]\n",
        runtime.state_a.phys,
        runtime.state_b.phys,
        runtime.control.phys,
        runtime.report.phys,
        total_bytes,
        runtime.state_a.gpu,
        runtime.state_b.gpu,
        runtime.control.gpu,
        runtime.report.gpu,
    );
    Some(runtime)
}

fn lab256_write_fixed_control(control: Lab256Buffer, frame: u32) {
    unsafe {
        core::ptr::write_bytes(control.virt, 0, control.bytes);
        let dwords = control.virt as *mut u32;
        core::ptr::write_volatile(dwords, LAB256_CONTROL_MAGIC);
        core::ptr::write_volatile(dwords.add(1), LAB256_CONTROL_VERSION);
        core::ptr::write_volatile(dwords.add(2), frame);
        let mut flags =
            LAB256_FLAG_WRAP | LAB256_FLAG_MANDELBROT | LAB256_FLAG_CHART | LAB256_FLAG_FLOW_WARP;
        if frame == 0 {
            flags |= LAB256_FLAG_RESET;
        }
        core::ptr::write_volatile(dwords.add(3), flags);
        core::ptr::write_volatile(dwords.add(4), (frame as f32 * 0.12).to_bits());
        core::ptr::write_volatile(dwords.add(5), (128u32 << 16) | 128);
        core::ptr::write_volatile(dwords.add(6), 12.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(7), 0.8f32.to_bits());
        core::ptr::write_volatile(dwords.add(8), 0.0367f32.to_bits());
        core::ptr::write_volatile(dwords.add(9), 0.0649f32.to_bits());
        core::ptr::write_volatile(dwords.add(10), 1.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(11), (-0.62f32).to_bits());
        core::ptr::write_volatile(dwords.add(12), 0.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(13), 1.55f32.to_bits());
        core::ptr::write_volatile(dwords.add(14), 48);
        core::ptr::write_volatile(dwords.add(15), 0x53u32.wrapping_add(frame * 7));
        core::ptr::write_volatile(dwords.add(16), frame & 255);
        core::ptr::write_volatile(dwords.add(17), LAB256_BACKGROUND_ALPHA.to_bits());
        for index in 0..256usize {
            let sample = (((index as u32).wrapping_add(frame * 9)) & 255) * 257;
            core::ptr::write_volatile(dwords.add(32 + index), sample);
        }
    }
    super::dma_flush(control.virt, control.bytes);
}

fn lab256_report_audit(report: Lab256Buffer, frame: u32) -> bool {
    super::dma_flush(report.virt, LAB256_REPORT_BYTES);
    let dwords = report.virt as *const u32;
    unsafe {
        if core::ptr::read_volatile(dwords) != LAB256_REPORT_MAGIC
            || core::ptr::read_volatile(dwords.add(1)) != LAB256_CONTROL_VERSION
            || core::ptr::read_volatile(dwords.add(2)) != frame
        {
            return false;
        }
        for lane in 0..16usize {
            let marker = core::ptr::read_volatile(dwords.add(16 + lane * 24 + 23));
            if marker != 0xD06E_0000 | lane as u32 {
                return false;
            }
        }
    }
    true
}

/// Produce one deterministic Lab256 frame for Spirit. This function does not
/// present or touch cursor MMIO: it only submits the fixed three-walker batch
/// through gpu::executor/vGPU/GuC and returns an exact producer-release proof.
pub(crate) fn lab256_spirit_frame(dst: GpgpuRgba8Surface, frame: u32) -> GpgpuRgba8KernelResult {
    lab256_frame(dst, Some(frame))
}

/// Produce the next persistent Lab256 frame for a live UI4 preview.
///
/// Spirit owns the deterministic startup frame numbers. A Shell2 preview may
/// begin after any number of those frames, so it advances from the last
/// retired state while holding the shared direct-RCS submit lock instead of
/// guessing a frame number in the UI service.
pub(crate) fn lab256_preview_frame(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    lab256_frame(dst, None)
}

fn lab256_frame(dst: GpgpuRgba8Surface, requested_frame: Option<u32>) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    let producer_owner = if requested_frame.is_some() {
        "spirit-worker"
    } else {
        "ui4-gpgpu-preview"
    };
    if !dst.is_valid()
        || dst.width != LAB256_SIZE
        || dst.height != LAB256_SIZE
        || dst.pitch_bytes < LAB256_SIZE * 4
        || dst.storage_order != GpgpuRgba8StorageOrder::Rgba
    {
        return GpgpuRgba8KernelResult::default();
    }

    let Some(_submit_guard) = DIRECT_RCS_SUBMIT_LOCK.try_lock() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(upload) = upload_lab256_multiphase_kernel() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(runtime_snapshot) = lab256_runtime_once() else {
        return GpgpuRgba8KernelResult::default();
    };
    let frame = requested_frame.unwrap_or_else(|| {
        runtime_snapshot
            .last_complete_frame
            .map_or(0, |last| last.saturating_add(1))
    });
    if runtime_snapshot.quarantined || !runtime_snapshot.accepts_frame(frame) {
        return GpgpuRgba8KernelResult::default();
    }

    let (state_in, state_out) = runtime_snapshot.state_pair();
    lab256_write_fixed_control(runtime_snapshot.control, frame);
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(state);
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(state, upload.gpu, upload.phys, upload.mapped_bytes);
    let resources_ok = kernel_ok
        && direct_rcs_map_ppgtt_kernel(state, state_in.gpu, state_in.phys, state_in.bytes)
        && direct_rcs_map_ppgtt_kernel(state, state_out.gpu, state_out.phys, state_out.bytes)
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime_snapshot.control.gpu,
            runtime_snapshot.control.phys,
            runtime_snapshot.control.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            state,
            runtime_snapshot.report.gpu,
            runtime_snapshot.report.phys,
            runtime_snapshot.report.bytes,
        );
    let dst_ok = resources_ok && direct_rcs_map_ppgtt_scanout(state, dst.gpu, dst.phys, dst.bytes);
    let batch_ok = dst_ok
        && direct_rcs_encode_lab256_batch(
            state,
            upload,
            state_in,
            state_out,
            runtime_snapshot.control,
            runtime_snapshot.report,
            dst,
        );
    let submitted = batch_ok && direct_rcs_submit_batch(dev, state);
    let marker = if submitted {
        direct_rcs_poll_result_slot_timeout_ms(
            state,
            LAB256_POST_MARKER_SLOT,
            LAB256_POST_MARKER,
            LAB256_COMPLETION_TIMEOUT_MS,
        )
    } else {
        0
    };
    let ok = marker == LAB256_POST_MARKER;
    // Telemetry integrity is deliberately observational. Spirit's producer bit
    // depends only on the GuC-retired post-sync marker, never on this CPU read.
    let report_ok = ok && lab256_report_audit(runtime_snapshot.report, frame);

    if ok {
        let mut runtime_slot = LAB256_RUNTIME.lock();
        if let Some(runtime) = runtime_slot.as_mut() {
            runtime.read_from_a = !runtime.read_from_a;
            runtime.last_complete_frame = Some(frame);
        }
    } else if submitted {
        quarantine_direct_rcs_context("lab256-marker-timeout");
        if let Some(runtime) = LAB256_RUNTIME.lock().as_mut() {
            runtime.quarantined = true;
        }
    }

    crate::log_info!(
        target: "gpgpu";
        "intel/gpgpu: lab256 frame={} ok={} submitted={} marker=0x{:08X} report_audit={} control_alpha={:.2} state_in=0x{:X} state_out=0x{:X} dst=0x{:X} submit_ms={} owner={} admission=gpu-executor/vgpu/guc direct_elsp=0\n",
        frame,
        ok as u8,
        submitted as u8,
        marker,
        report_ok as u8,
        LAB256_BACKGROUND_ALPHA,
        state_in.gpu,
        state_out.gpu,
        dst.gpu,
        direct_rcs_elapsed_ms_since(started),
        producer_owner,
    );
    GpgpuRgba8KernelResult {
        ok,
        submitted,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}
