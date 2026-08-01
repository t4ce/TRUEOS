const LAB256_SPIRIT_INITIAL_TRACE_FRAMES: u32 = 30;
const LAB256_SPIRIT_PERIODIC_TRACE_FRAMES: u32 = 60;

/// Opaque identity for one accepted Spirit Lab256 dispatch. The issuer may
/// retain this value across Embassy yields but cannot manufacture a producer
/// release from it.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct Lab256SpiritSubmission {
    tag: u64,
    frame: u32,
}

impl Lab256SpiritSubmission {
    pub(crate) const fn tag(self) -> u64 {
        self.tag
    }

    pub(crate) const fn frame(self) -> u32 {
        self.frame
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Lab256SpiritCompletion {
    Pending,
    Complete(GpgpuRgba8ReleaseFence),
    Failed,
    InvalidSubmission,
}

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
struct Lab256Submitted {
    state: DirectRcsState,
    state_in: Lab256Buffer,
    state_out: Lab256Buffer,
    report: Lab256Buffer,
    dst: GpgpuRgba8Surface,
    frame: u32,
    present_fps: u32,
    pointer_xy: Option<(u16, u16)>,
    started_tick: u64,
}

#[derive(Copy, Clone, Debug)]
struct Lab256SpiritPending {
    handle: Lab256SpiritSubmission,
    submitted: Lab256Submitted,
}

static LAB256_SPIRIT_NEXT_TAG: AtomicU64 = AtomicU64::new(1);
static LAB256_SPIRIT_PENDING: Mutex<Option<Lab256SpiritPending>> = Mutex::new(None);

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
            Some(last) => frame == last.wrapping_add(1),
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

fn lab256_write_fixed_control(
    control: Lab256Buffer,
    frame: u32,
    present_fps: u32,
    pointer_xy: Option<(u16, u16)>,
) {
    unsafe {
        core::ptr::write_bytes(control.virt, 0, control.bytes);
        let dwords = control.virt as *mut u32;
        core::ptr::write_volatile(dwords, LAB256_CONTROL_MAGIC);
        core::ptr::write_volatile(dwords.add(1), LAB256_CONTROL_VERSION);
        core::ptr::write_volatile(dwords.add(2), frame);
        let mut flags = LAB256_FLAG_WRAP;
        if pointer_xy.is_some() {
            // Pointer input belongs only to the independent reaction layer;
            // centered flare geometry never consumes this coordinate.
            flags |= LAB256_FLAG_INJECT;
        }
        if frame == 0 {
            flags |= LAB256_FLAG_RESET;
        }
        core::ptr::write_volatile(dwords.add(3), flags);
        core::ptr::write_volatile(dwords.add(4), (frame as f32 * 0.035).to_bits());
        let (pointer_x, pointer_y) = pointer_xy.unwrap_or((128, 128));
        core::ptr::write_volatile(
            dwords.add(5),
            (u32::from(pointer_y) << 16) | u32::from(pointer_x),
        );
        core::ptr::write_volatile(dwords.add(6), 18.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(7), 0.58f32.to_bits());
        core::ptr::write_volatile(dwords.add(8), 0.0367f32.to_bits());
        core::ptr::write_volatile(dwords.add(9), 0.0649f32.to_bits());
        core::ptr::write_volatile(dwords.add(10), 1.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(11), 0.72f32.to_bits());
        core::ptr::write_volatile(dwords.add(12), 0.12f32.to_bits());
        core::ptr::write_volatile(dwords.add(13), 0.68f32.to_bits());
        core::ptr::write_volatile(dwords.add(14), 1.0f32.to_bits());
        core::ptr::write_volatile(dwords.add(15), 0x53);
        core::ptr::write_volatile(dwords.add(16), 0);
        core::ptr::write_volatile(dwords.add(17), LAB256_BACKGROUND_ALPHA.to_bits());
        core::ptr::write_volatile(dwords.add(18), present_fps.min(1_000));
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
            let marker = core::ptr::read_volatile(dwords.add(16 + lane * 8 + 7));
            if marker != 0xD06E_0000 | lane as u32 {
                return false;
            }
        }
    }
    true
}

/// Admit one Lab256 frame for Spirit and return immediately. The returned tag
/// owns the execution direct-RCS lane until `poll_lab256_spirit_submission`
/// observes the post-sync marker; admission never waits for GPU execution.
pub(crate) fn submit_lab256_spirit_frame(
    dst: GpgpuRgba8Surface,
    present_fps: u32,
    pointer_xy: Option<(u16, u16)>,
) -> Option<Lab256SpiritSubmission> {
    let _submit_guard = EXECUTION_RCS_SUBMIT_LOCK.try_lock()?;
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != 0
        || LAB256_SPIRIT_PENDING.lock().is_some()
    {
        return None;
    }
    let submitted = submit_lab256_batch(dst, None, present_fps, pointer_xy)?;
    let tag = LAB256_SPIRIT_NEXT_TAG
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1)
        .max(1);
    let handle = Lab256SpiritSubmission {
        tag,
        frame: submitted.frame,
    };

    // Publish detached ownership before dropping the direct-submit lock. Any
    // later direct-RCS issuer will now defer before touching shared state.
    EXECUTION_RCS_DETACHED_TAG.store(tag, Ordering::Release);
    *LAB256_SPIRIT_PENDING.lock() = Some(Lab256SpiritPending { handle, submitted });

    if lab256_trace_spirit_frame(submitted.frame) {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: lab256 one-shot accepted tag={} frame={} present_fps={} pointer_active={} pointer_xy={:?} dst=0x{:X} owner=spirit-worker lane=execution admission=gpu-executor/vgpu/guc wait=detached\n",
            tag,
            submitted.frame,
            submitted.present_fps,
            submitted.pointer_xy.is_some() as u8,
            submitted.pointer_xy,
            dst.gpu,
        );
    }
    Some(handle)
}

/// Observe one exact Spirit completion tag once. Pending returns without
/// spinning; the Embassy owner decides when to poll again.
pub(crate) fn poll_lab256_spirit_submission(
    handle: Lab256SpiritSubmission,
) -> Lab256SpiritCompletion {
    if EXECUTION_RCS_DETACHED_TAG.load(Ordering::Acquire) != handle.tag {
        return Lab256SpiritCompletion::InvalidSubmission;
    }
    let mut pending_slot = LAB256_SPIRIT_PENDING.lock();
    let Some(pending) = *pending_slot else {
        return Lab256SpiritCompletion::InvalidSubmission;
    };
    if pending.handle != handle {
        return Lab256SpiritCompletion::InvalidSubmission;
    }

    let marker = direct_rcs_read_result_slot(pending.submitted.state, LAB256_POST_MARKER_SLOT);
    let elapsed_ms = direct_rcs_elapsed_ms_since(pending.submitted.started_tick);
    if marker != LAB256_POST_MARKER && elapsed_ms < LAB256_COMPLETION_TIMEOUT_MS {
        return Lab256SpiritCompletion::Pending;
    }

    // Claim the terminal transition so a copied/stale handle cannot retire the
    // executor token or mint the exact-allocation release twice.
    *pending_slot = None;
    drop(pending_slot);

    let ok = marker == LAB256_POST_MARKER;
    let report_ok = ok && lab256_report_audit(pending.submitted.report, pending.submitted.frame);
    if !ok {
        quarantine_execution_rcs_context("lab256-marker-timeout");
    }
    complete_execution_rcs_submission(ok);
    finish_lab256_runtime(pending.submitted.frame, ok);
    EXECUTION_RCS_DETACHED_TAG.store(0, Ordering::Release);
    log_lab256_completion(pending.submitted, marker, report_ok, "spirit-worker");

    if ok {
        Lab256SpiritCompletion::Complete(gpgpu_rgba8_release(pending.submitted.dst))
    } else {
        Lab256SpiritCompletion::Failed
    }
}

/// Produce the next persistent Lab256 frame for a live UI4 preview.
///
/// Spirit owns the deterministic continuous frame numbers. A Shell2 preview may
/// begin after any number of those frames, so it advances from the last
/// retired state while holding the shared direct-RCS submit lock instead of
/// guessing a frame number in the UI service.
pub(crate) fn lab256_preview_frame(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    lab256_frame(dst)
}

fn lab256_frame(dst: GpgpuRgba8Surface) -> GpgpuRgba8KernelResult {
    if !dst.is_valid()
        || dst.width != LAB256_SIZE
        || dst.height != LAB256_SIZE
        || dst.pitch_bytes < LAB256_SIZE * 4
        || dst.storage_order != GpgpuRgba8StorageOrder::Rgba
    {
        return GpgpuRgba8KernelResult::default();
    }

    let Some(_submit_guard) = EXECUTION_RCS_SUBMIT_LOCK.try_lock() else {
        return GpgpuRgba8KernelResult::default();
    };
    let Some(submitted) = submit_lab256_batch(dst, None, 0, None) else {
        return GpgpuRgba8KernelResult::default();
    };
    let marker = execution_rcs_poll_result_slot_timeout_ms(
        submitted.state,
        LAB256_POST_MARKER_SLOT,
        LAB256_POST_MARKER,
        LAB256_COMPLETION_TIMEOUT_MS,
    );
    let ok = marker == LAB256_POST_MARKER;
    let report_ok = ok && lab256_report_audit(submitted.report, submitted.frame);
    finish_lab256_runtime(submitted.frame, ok);
    if !ok {
        quarantine_execution_rcs_context("lab256-marker-timeout");
    }
    log_lab256_completion(submitted, marker, report_ok, "ui4-gpgpu-preview");
    GpgpuRgba8KernelResult {
        ok,
        submitted: true,
        marker,
        submit_ms: direct_rcs_elapsed_ms_since(submitted.started_tick),
        release: ok.then(|| gpgpu_rgba8_release(dst)),
    }
}

/// Encode and admit the fixed three-walker batch without waiting for it. The
/// caller must own `EXECUTION_RCS_SUBMIT_LOCK` and either poll synchronously or
/// publish a detached tag before releasing that lock.
fn submit_lab256_batch(
    dst: GpgpuRgba8Surface,
    requested_frame: Option<u32>,
    present_fps: u32,
    pointer_xy: Option<(u16, u16)>,
) -> Option<Lab256Submitted> {
    if !dst.is_valid()
        || dst.width != LAB256_SIZE
        || dst.height != LAB256_SIZE
        || dst.pitch_bytes < LAB256_SIZE * 4
        || dst.storage_order != GpgpuRgba8StorageOrder::Rgba
    {
        return None;
    }
    let started_tick = direct_rcs_now_tick();
    let Some(dev) = super::claimed_device() else {
        return None;
    };
    let Some(upload) = upload_lab256_multiphase_kernel() else {
        return None;
    };
    let Some(shared_state) = execution_rcs_state_once(dev) else {
        return None;
    };
    let Some(runtime_snapshot) = lab256_runtime_once() else {
        return None;
    };
    let frame = requested_frame.unwrap_or_else(|| {
        runtime_snapshot
            .last_complete_frame
            .map_or(0, |last| last.wrapping_add(1))
    });
    if runtime_snapshot.quarantined || !runtime_snapshot.accepts_frame(frame) {
        return None;
    }

    let (state_in, state_out) = runtime_snapshot.state_pair();
    let present_fps = present_fps.min(1_000);
    lab256_write_fixed_control(runtime_snapshot.control, frame, present_fps, pointer_xy);
    let forcewake_ok = direct_rcs_forcewake(dev);
    let mapped_ok = forcewake_ok && direct_rcs_map_state(dev, shared_state);
    let ppgtt_ok = mapped_ok && direct_rcs_init_ppgtt(shared_state);
    let kernel_ok = ppgtt_ok
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            upload.gpu,
            upload.phys,
            upload.mapped_bytes,
        );
    let resources_ok = kernel_ok
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            state_in.gpu,
            state_in.phys,
            state_in.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            state_out.gpu,
            state_out.phys,
            state_out.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            runtime_snapshot.control.gpu,
            runtime_snapshot.control.phys,
            runtime_snapshot.control.bytes,
        )
        && direct_rcs_map_ppgtt_kernel(
            shared_state,
            runtime_snapshot.report.gpu,
            runtime_snapshot.report.phys,
            runtime_snapshot.report.bytes,
        );
    let dst_ok = resources_ok
        && direct_rcs_map_ppgtt_scanout(shared_state, dst.gpu, dst.phys, dst.bytes);
    let state = execution_rcs_next_job_slot(shared_state)?;
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
    if !batch_ok || !execution_rcs_submit_batch(dev, state) {
        return None;
    }
    Some(Lab256Submitted {
        state,
        state_in,
        state_out,
        report: runtime_snapshot.report,
        dst,
        frame,
        present_fps,
        pointer_xy,
        started_tick,
    })
}

fn finish_lab256_runtime(frame: u32, completed: bool) {
    if completed {
        let mut runtime_slot = LAB256_RUNTIME.lock();
        if let Some(runtime) = runtime_slot.as_mut() {
            runtime.read_from_a = !runtime.read_from_a;
            runtime.last_complete_frame = Some(frame);
        }
    } else {
        if let Some(runtime) = LAB256_RUNTIME.lock().as_mut() {
            runtime.quarantined = true;
        }
    }
}

fn lab256_trace_spirit_frame(frame: u32) -> bool {
    frame < LAB256_SPIRIT_INITIAL_TRACE_FRAMES
        || frame
            .wrapping_add(1)
            .is_multiple_of(LAB256_SPIRIT_PERIODIC_TRACE_FRAMES)
}

fn log_lab256_completion(
    submitted: Lab256Submitted,
    marker: u32,
    report_ok: bool,
    producer_owner: &'static str,
) {
    let ok = marker == LAB256_POST_MARKER;
    if producer_owner != "spirit-worker"
        || lab256_trace_spirit_frame(submitted.frame)
        || !ok
        || !report_ok
    {
        crate::log_info!(
            target: "gpgpu";
            "intel/gpgpu: lab256 frame={} ok={} submitted={} marker=0x{:08X} report_audit={} control_alpha={:.2} present_fps={} pointer_active={} pointer_xy={:?} state_in=0x{:X} state_out=0x{:X} dst=0x{:X} submit_ms={} owner={} admission=gpu-executor/vgpu/guc direct_elsp=0\n",
            submitted.frame,
            ok as u8,
            1,
            marker,
            report_ok as u8,
            LAB256_BACKGROUND_ALPHA,
            submitted.present_fps,
            submitted.pointer_xy.is_some() as u8,
            submitted.pointer_xy,
            submitted.state_in.gpu,
            submitted.state_out.gpu,
            submitted.dst.gpu,
            direct_rcs_elapsed_ms_since(submitted.started_tick),
            producer_owner,
        );
    }
}
