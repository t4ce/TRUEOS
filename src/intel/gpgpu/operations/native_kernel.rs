pub(crate) struct NativeKernelProgram {
    code: Vec<u8>,
    digest: [u8; 32],
}
impl NativeKernelProgram {
    pub(crate) fn code_len(&self) -> usize {
        self.code.len()
    }
    pub(crate) fn digest(&self) -> [u8; 32] {
        self.digest
    }
}
pub(crate) fn create_native_kernel_program(code: &[u8]) -> Result<NativeKernelProgram, i32> {
    if !crate::gpu::native_xelp::validate_simd8(code) {
        return Err(-22);
    }
    Ok(NativeKernelProgram {
        code: code.to_vec(),
        digest: sha256_digest(code),
    })
}
const NATIVE_KERNEL_COOKIE: u32 = 0x4f44_4b38;

pub(crate) fn dispatch_native_kernel(
    program: &NativeKernelProgram,
    inputs: &[[u32; 8]],
) -> Result<[u32; 8], (i32, bool)> {
    if !(1..=8).contains(&inputs.len()) {
        return Err((-22, false));
    }
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or((-19, false))?;
    if !matches!(
        (dev.device_id, dev.revision_id),
        (0x4680, 0x0c) | (0x9a49, 0x01)
    ) {
        return Err((-95, false));
    }
    let state = direct_rcs_state_once(dev).ok_or((-19, false))?;
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
    {
        return Err((-5, false));
    }
    if !encode_native_kernel(state, &program.code, inputs) {
        return Err((-22, false));
    }
    let output = unsafe { state.result_virt.add(NATIVE_SCALAR_RESULT) };
    unsafe {
        core::ptr::write_bytes(output.sub(32), 0xa5, 96);
    }
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    let submitted = direct_rcs_submit_batch_state(dev, state);
    if !submitted.may_have_submitted() {
        return Err((-16, false));
    }
    if !submitted.can_poll()
        || direct_rcs_poll_result_slot_timeout_ms(
            state,
            0,
            NATIVE_KERNEL_COOKIE,
            UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        ) != NATIVE_KERNEL_COOKIE
    {
        quarantine_direct_rcs_context("odin-native-kernel-incomplete");
        return Err((-32, true));
    }
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    let guards_valid = unsafe {
        (0..32).all(|i| {
            output.sub(32).add(i).read_volatile() == 0xa5
                && output.add(32 + i).read_volatile() == 0xa5
        })
    };
    if !guards_valid {
        quarantine_direct_rcs_context("odin-native-kernel-canary");
        return Err((-75, true));
    }
    Ok(core::array::from_fn(|i| unsafe {
        output.cast::<u32>().add(i).read_volatile()
    }))
}

unsafe fn write_native_kernel_surface(state: DirectRcsState) {
    let surface = unsafe { state.batch_virt.add(NATIVE_SCALAR_SURFACE).cast::<u32>() };
    let address = state.gpu_va.result + NATIVE_SCALAR_RESULT as u64;
    // SURFTYPE_2D/R8_UINT, width/pitch 32 and height 1, for the compiler's BT0 media-block store.
    let words = [
        1 << 29 | 0x140 << 18 | 1 << 16 | 1 << 14 | 1 << 8,
        3 << 25,
        31,
        31,
        0,
        0,
        0,
        4 << 25 | 5 << 22 | 6 << 19 | 7 << 16,
        address as u32,
        (address >> 32) as u32,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    for (i, word) in words.into_iter().enumerate() {
        unsafe {
            surface.add(i).write(word);
        }
    }
}

fn encode_native_kernel(state: DirectRcsState, code: &[u8], inputs: &[[u32; 8]]) -> bool {
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
        for (i, register) in inputs.iter().enumerate() {
            for (lane, bits) in register.iter().enumerate() {
                payload.add(i * 8 + lane).write(*bits);
            }
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
        0,
    ) {
        return false;
    }
    unsafe {
        write_native_kernel_surface(state);
        // A single hardware thread receives the caller's registers at r1 onward.
        state
            .batch_virt
            .add(NATIVE_SCALAR_IDD + 20)
            .cast::<u32>()
            .write((inputs.len() as u32) << 16);
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
        (inputs.len() as u32 * 32).next_multiple_of(64),
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
        0xff,
        0xffff_ffff,
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
        NATIVE_KERNEL_COOKIE,
    ) {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}
