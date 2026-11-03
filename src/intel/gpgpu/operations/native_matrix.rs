pub(crate) const NATIVE_MATRIX_MAX_N: usize = 128;
const NATIVE_MATRIX_BYTES: usize = NATIVE_MATRIX_MAX_N * NATIVE_MATRIX_MAX_N * 4;
const NATIVE_MATRIX_STRIDE: usize = NATIVE_MATRIX_BYTES + 8192;
const NATIVE_MATRIX_GPU: u64 = 0x0920_0000;
const NATIVE_MATRIX_COOKIE: u32 = 0x4d41_5458;
const _: () = assert!(NATIVE_MATRIX_GPU >= GENERATED_FIELD_GPU + GENERATED_FIELD_BYTES as u64);
const _: () = assert!(NATIVE_MATRIX_GPU + (2 * NATIVE_MATRIX_STRIDE) as u64 <= DIRECT_RCS_GPU_VA_FONT_COVERAGE_BASE);
static NATIVE_MATRIX_DMA: Mutex<Option<(u64, usize)>> = Mutex::new(None);

/// The caller supplies arithmetic only. The driver bounds every matrix read,
/// owns both buffers until retirement, and never exposes a guest GPU address.
pub(crate) fn dispatch_native_matrix(
    code: &[u8], input: &[u8], n: usize, saturate: u32, rounds: u32,
) -> Result<Vec<u8>, i32> {
    if !cfg!(feature = "odin-native-gpgpu") { return Err(-95); }
    if !(1..=NATIVE_MATRIX_MAX_N).contains(&n) || saturate > 1
        || !(1..=4).contains(&rounds) || input.len() != n*n*4 { return Err(-22); }
    let body = crate::gpu::native_xelp::matrix_body_simd8(code).ok_or(-22)?;
    let kernel = native_matrix_envelope(body)?;
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let dev = super::claimed_device().ok_or(-19)?;
    if !matches!((dev.device_id, dev.revision_id), (0x4680, 0x0c) | (0x9a49, 0x01)) { return Err(-95); }
    let state = direct_rcs_state_once(dev).ok_or(-19)?;
    if !direct_rcs_forcewake(dev) || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state) { return Err(-5); }
    let mut storage = NATIVE_MATRIX_DMA.lock();
    if storage.is_none() {
        let (phys, virt) = crate::dma::alloc(2*NATIVE_MATRIX_STRIDE, 4096).ok_or(-12)?;
        *storage = Some((phys, virt as usize));
    }
    let (phys, address) = storage.unwrap();
    let memory = address as *mut u8;
    if !direct_rcs_map_ppgtt_kernel(state, NATIVE_MATRIX_GPU, phys, 2*NATIVE_MATRIX_STRIDE) { return Err(-5); }
    // The allocation remains permanently pinned, including after quarantine.
    unsafe {
        core::ptr::write_bytes(memory, 0xa5, 2*NATIVE_MATRIX_STRIDE);
        core::ptr::copy_nonoverlapping(input.as_ptr(), memory.add(4096), input.len());
    }
    super::dma_flush(memory, 2*NATIVE_MATRIX_STRIDE);
    let mut source = 0;
    for _ in 0..rounds {
        let destination = 1-source;
        if !encode_native_matrix(state, &kernel, n, saturate, source, destination) { return Err(-22); }
        let submitted = direct_rcs_submit_batch_state(dev, state);
        if !submitted.may_have_submitted() { return Err(-16); }
        if !submitted.can_poll() || direct_rcs_poll_result_slot_timeout_ms(
            state, 0, NATIVE_MATRIX_COOKIE, UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
        ) != NATIVE_MATRIX_COOKIE {
            quarantine_direct_rcs_context("odin-matrix-incomplete");
            return Err(-32);
        }
        super::dma_flush(memory, 2*NATIVE_MATRIX_STRIDE);
        let guards_valid = (0..2).all(|slot| {
            let start = slot*NATIVE_MATRIX_STRIDE;
            (0..4096).chain(4096+input.len()..NATIVE_MATRIX_STRIDE).all(|offset| unsafe {
                memory.add(start+offset).read_volatile() == 0xa5
            })
        });
        if !guards_valid {
            quarantine_direct_rcs_context("odin-matrix-canary");
            return Err(-75);
        }
        source = destination;
    }
    let mut output = Vec::new();
    output.try_reserve_exact(input.len()).map_err(|_| -12)?;
    unsafe { output.extend_from_slice(core::slice::from_raw_parts(memory.add(source*NATIVE_MATRIX_STRIDE+4096), input.len())); }
    Ok(output)
}

fn encode_native_matrix(
    state: DirectRcsState, kernel: &[u8], n: usize, saturate: u32,
    source: usize, destination: usize,
) -> bool {
    if GENERATED_PIXEL_CODE_OFFSET+kernel.len() > DIRECT_RCS_BATCH_BYTES { return false; }
    unsafe {
        core::ptr::copy_nonoverlapping(kernel.as_ptr(), state.batch_virt.add(GENERATED_PIXEL_CODE_OFFSET), kernel.len());
        let payload = state.batch_virt.add(GENERATED_PIXEL_PAYLOAD).cast::<u32>();
        core::ptr::write_bytes(payload, 0, 32);
        payload.write(n as u32);
        payload.add(1).write(saturate);
        for lane in 0..8 { payload.add(8+lane).write(lane as u32); }
        state.batch_virt.add(GENERATED_PIXEL_BINDING).cast::<u32>().write(GENERATED_PIXEL_SURFACE as u32);
        state.batch_virt.add(GENERATED_PIXEL_BINDING+4).cast::<u32>().write(0x8c0);
    }
    if !direct_rcs_write_interface_descriptor_at(state, GENERATED_PIXEL_IDD, GENERATED_PIXEL_BINDING,
        GENERATED_PIXEL_CODE_OFFSET as u64, 2, 0)
        || !direct_rcs_write_buffer_surface_state(state, GENERATED_PIXEL_SURFACE,
            NATIVE_MATRIX_GPU+(source*NATIVE_MATRIX_STRIDE+4096) as u64, n*n*4)
        || !direct_rcs_write_buffer_surface_state(state, 0x8c0,
            NATIVE_MATRIX_GPU+(destination*NATIVE_MATRIX_STRIDE+4096) as u64, n*n*4)
    { return false; }
    unsafe {
        state.batch_virt.add(GENERATED_PIXEL_IDD+8).cast::<u32>().write(IDD_THREAD_PREEMPTION_DISABLE | (1<<19));
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
    }
    let batch = unsafe { core::slice::from_raw_parts_mut(state.batch_virt.cast::<u32>(), GENERATED_PIXEL_IDD/4) };
    let mut cursor = 0;
    if !direct_rcs_push_gpgpu_prologue_with_instruction_base(batch, &mut cursor,
        state.gpu_va.batch, state.gpu_va.batch, GPGPU_VFE_DW5_UOS) { return false; }
    for word in [
        MEDIA_INTERFACE_DESCRIPTOR_LOAD_CMD, 0, 32, GENERATED_PIXEL_IDD as u32,
        GPGPU_WALKER_CMD, 0, 128, GENERATED_PIXEL_PAYLOAD as u32,
        0, 0, 0, (n as u32).div_ceil(8), 0, 0, n as u32, 0, 1, 0xff, 0xff,
        MEDIA_STATE_FLUSH_CMD, 0,
    ] {
        if !direct_rcs_push(batch, &mut cursor, word) { return false; }
    }
    if !direct_rcs_push_gpgpu_dispatch_epilogue(batch, &mut cursor, state.gpu_va.result, 0, NATIVE_MATRIX_COOKIE) { return false; }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

fn native_matrix_envelope(body: &[u8]) -> Result<Vec<u8>, i32> {
    let mut code = Vec::new();
    code.try_reserve_exact(body.len()+1024).map_err(|_| -12)?;
    let mut emit = |w: [u32; 4]| { for word in w { code.extend_from_slice(&word.to_le_bytes()); } };
    emit(crate::gpu::native_xelp::PREFIX);
    // mov (8) r121:ud r1:ud {@1}; preserve n and mode outside body allocation.
    emit([0x00030161, 0x79050220, 0x00460105, 0]);
    // mov (8) r122:ud r2:ud {@1}; lane identifiers.
    emit([0x00030161, 0x7a050220, 0x00460205, 0]);
    // (W) mov (8) r112:ud r0.1<0;1,0>:ud {@1}; group column.
    emit([0x80030161, 0x70050220, 0x24, 0]);
    emit(generated_pixel_binary(0x69, 112, 112, 3, true, false));
    emit(generated_pixel_binary(0x40, 112, 112, 122, false, false));
    // (W) mov (8) r113:ud r0.6<0;1,0>:ud {@1}; row.
    emit([0x80030161, 0x71050220, 0xc4, 0]);
    // mov (8) r115:ud r121.0<0;1,0>:ud {@1}; N.
    emit([0x00030161, 0x73050220, 0x00007904, 0]);
    // mul (8) r122:ud r113:ud r121.0<0;1,0>:uw {@1}; row*N.
    emit([0x00030141, 0x7a050220, 0x01467105, 0x00007904]);
    emit(generated_pixel_binary(0x69, 122, 122, 2, true, false));
    // cmp.l (8) null:ud r112:ud r115:ud {@1}; padded columns read column zero.
    emit([0x00030170, 0x00010220, 0x52467005, 0x00467305]);
    // (+f0.0) sel (8) r123:ud r112:ud 0:ud {@1}.
    emit([0x01030162, 0x7b058220, 0x02467005, 0]);
    emit(generated_pixel_binary(0x69, 123, 123, 2, true, false));
    emit(generated_pixel_binary(0x69, 119, 112, 2, true, false));
    emit(generated_pixel_binary(0x40, 119, 119, 122, false, false));
    // mov (8) r114:ud 0:ud {@1}; K. mov (8) r116:ud 0:ud {@1}; accumulator.
    emit([0x00030161, 0x72054220, 0, 0]);
    emit([0x00030161, 0x74054220, 0, 0]);
    drop(emit);
    let loop_start = code.len();
    let mut emit = |w: [u32; 4]| { for word in w { code.extend_from_slice(&word.to_le_bytes()); } };
    emit(generated_pixel_binary(0x69, 127, 114, 2, true, false));
    emit(generated_pixel_binary(0x40, 127, 127, 122, false, false));
    // send.dc1 (8) r1 r127 null: BT0 SIMD8 untyped X read {$3}; sync.nop {$3.dst}.
    let desc = 0x0210_6e00u32;
    let read = |argument: u32| [0x00039331,
        argument<<24 | 0x00040000 | ((desc>>20)&31)<<19,
        0xc0007f04 | ((desc>>25)&31)<<3 | (desc&0x7ff)<<17,
        ((desc>>11)&0x1ff)<<17];
    emit(read(1));
    emit([0x00002301, 0x00010000, 0, 0]);
    // mul (8) r127:ud r114:ud r121.0<0;1,0>:uw {@1}; K*N.
    emit([0x00030141, 0x7f050220, 0x01467205, 0x00007904]);
    emit(generated_pixel_binary(0x69, 127, 127, 2, true, false));
    emit(generated_pixel_binary(0x40, 127, 127, 123, false, false));
    // send.dc1 (8) r2 r127 null: BT0 SIMD8 untyped X read {$3}; sync.nop {$3.dst}.
    emit(read(2));
    emit([0x00002301, 0x00010000, 0, 0]);
    // mov (8) r3:ud r116:ud {@1}; mov (8) r4:ud r121.1<0;1,0>:ud {@1}.
    emit([0x00030161, 0x03050220, 0x00467405, 0]);
    emit([0x00030161, 0x04050220, 0x00007924, 0]);
    drop(emit);
    code.extend_from_slice(body);
    let mut emit = |w: [u32; 4]| { for word in w { code.extend_from_slice(&word.to_le_bytes()); } };
    // mov (8) r116:ud r120:ud {@1}; retain result across the next MAC body.
    emit([0x00030161, 0x74050220, 0x00467805, 0]);
    emit(generated_pixel_binary(0x40, 114, 114, 1, true, false));
    // cmp.l (8) null:ud r114:ud r115:ud {@1}.
    emit([0x00030170, 0x00010220, 0x52467205, 0x00467305]);
    drop(emit);
    let displacement = (loop_start as i32-code.len() as i32) as u32;
    let mut emit = |w: [u32; 4]| { for word in w { code.extend_from_slice(&word.to_le_bytes()); } };
    // (+f0.0) while (8) loop_start {@1}; uniform K bound.
    emit([0x01030127, 0x00004000, 0, displacement]);
    // mov (8) r3:ud r119:ud {@1}; output byte offsets.
    emit([0x00030161, 0x03050220, 0x00467705, 0]);
    // cmp.l (8) null:ud r112:ud r115:ud {@1}; mask tail writes.
    emit([0x00030170, 0x00010220, 0x52467005, 0x00467305]);
    // sync.nop; (+f0.0) send.dc1 (8) null r3 r116: BT1 untyped X write {$0}.
    emit([0x00000101, 0x00010000, 0, 0]);
    emit([0x01034031, 0, 0xcc02030c, 0x009a740c]);
    for instruction in crate::gpu::native_xelp::SUFFIX.iter().skip(2).take(6) { emit(*instruction); }
    // (W) send.ts (1) null r127 null 0x0 0x02000000 {EOT,@1}.
    emit([0x80000131, 4, 0x70207f0c, 0]);
    Ok(code)
}
