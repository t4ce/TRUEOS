const NATIVE_SCALAR_CODE_OFFSET: usize = 0x1000;
const NATIVE_SCALAR_IDD: usize = 0x800;
const NATIVE_SCALAR_BINDING: usize = 0x840;
const NATIVE_SCALAR_SURFACE: usize = 0x880;
const NATIVE_SCALAR_PAYLOAD: usize = 0x900;
const NATIVE_SCALAR_RESULT: usize = 0x100;
const NATIVE_SCALAR_COOKIE: u32 = 0x4f44_494e;

pub(crate) fn native_scalar_f32(code: &[u8], a: u32, b: u32) -> Result<u32, i32> {
    if !crate::gpu::native_xelp::validate(code) {
        return Err(-22);
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or(-19)?;
    // Encoding 12p1 alone does not prove stepping-specific command support.
    if !matches!((dev.device_id, dev.revision_id), (0x4680, 0x0c) | (0x9a49, 0x01)) {
        return Err(-95);
    }
    let state = direct_rcs_state_once(dev).ok_or(-19)?;
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
    {
        return Err(-5);
    }
    if !encode_native_scalar(state, code, a, b) {
        return Err(-22);
    }
    let submitted = direct_rcs_submit_batch_state(dev, state);
    if !submitted.may_have_submitted() {
        return Err(-16);
    }
    if !submitted.can_poll()
        || direct_rcs_poll_result_slot_timeout_ms(
            state,
            0,
            NATIVE_SCALAR_COOKIE,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        ) != NATIVE_SCALAR_COOKIE
    {
        quarantine_direct_rcs_context("odin-native-scalar-incomplete");
        return Err(-32);
    }
    super::dma_flush(unsafe { state.result_virt.add(NATIVE_SCALAR_RESULT) }, 4);
    Ok(unsafe {
        core::ptr::read_volatile(state.result_virt.add(NATIVE_SCALAR_RESULT).cast::<u32>())
    })
}

fn encode_native_scalar(state: DirectRcsState, code: &[u8], a: u32, b: u32) -> bool {
    if NATIVE_SCALAR_CODE_OFFSET + code.len() > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        core::ptr::copy_nonoverlapping(
            code.as_ptr(),
            state.batch_virt.add(NATIVE_SCALAR_CODE_OFFSET),
            code.len(),
        );
        let payload = state.batch_virt.add(NATIVE_SCALAR_PAYLOAD).cast::<u32>();
        for lane in 0..8 {
            payload.add(lane).write(a);
            payload.add(8 + lane).write(b);
            payload.add(16 + lane).write(0);
        }
        state
            .batch_virt
            .add(NATIVE_SCALAR_BINDING)
            .cast::<u32>()
            .write(NATIVE_SCALAR_SURFACE as u32);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        NATIVE_SCALAR_IDD,
        NATIVE_SCALAR_BINDING,
        NATIVE_SCALAR_CODE_OFFSET as u64,
        1,
        3,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        NATIVE_SCALAR_SURFACE,
        state.gpu_va.result + NATIVE_SCALAR_RESULT as u64,
        4,
    ) {
        return false;
    }
    unsafe {
        // This ABI has no local-ID/per-thread payload; the legacy helper uses three GRFs.
        state
            .batch_virt
            .add(NATIVE_SCALAR_IDD + 20)
            .cast::<u32>()
            .write(0);
        state
            .batch_virt
            .add(NATIVE_SCALAR_IDD + 8)
            .cast::<u32>()
            .write(IDD_THREAD_PREEMPTION_DISABLE | (1 << 19));
    }
    let batch = unsafe {
        core::slice::from_raw_parts_mut(state.batch_virt.cast::<u32>(), NATIVE_SCALAR_IDD / 4)
    };
    let mut cursor = 0;
    if !direct_rcs_push_gpgpu_prologue_with_instruction_base(
        batch,
        &mut cursor,
        state.gpu_va.batch,
        state.gpu_va.batch,
        GPGPU_VFE_DW5_UOS,
    ) {
        return false;
    }
    for word in [
        MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD,
        0,
        32,
        NATIVE_SCALAR_IDD as u32,
        GPGPU_WALKER_CMD,
        0,
        128,
        NATIVE_SCALAR_PAYLOAD as u32,
        0,
        0,
        0,
        1,
        0,
        0,
        1,
        0,
        1,
        1,
        1,
        MEDIA_STATE_FLUSH_CMD,
        0,
    ] {
        if !direct_rcs_push(batch, &mut cursor, word) {
            return false;
        }
    }
    if !direct_rcs_push_gpgpu_dispatch_epilogue(
        batch,
        &mut cursor,
        state.gpu_va.result,
        0,
        NATIVE_SCALAR_COOKIE,
    ) {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
