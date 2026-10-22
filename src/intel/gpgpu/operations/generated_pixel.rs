//! Experimental native Odin pixel programs. The compiler supplies only a
//! bounded SIMD8 arithmetic body; this driver owns coordinates, the only
//! memory SEND, cache release and EOT. No catalog entry or static hash admits
//! the code. An owned immutable copy is staged in the system compute carrier
//! while its submission lock is held, exactly as for native scalar calls.

pub(crate) const GENERATED_PIXEL_MAX_BODY_BYTES: usize = 24 * 1024;
const GENERATED_PIXEL_CODE_OFFSET: usize = 0x1000;
const GENERATED_PIXEL_IDD: usize = 0x800;
const GENERATED_PIXEL_BINDING: usize = 0x840;
const GENERATED_PIXEL_SURFACE: usize = 0x880;
const GENERATED_PIXEL_PAYLOAD: usize = 0x900;
const GENERATED_PIXEL_COOKIE: u32 = 0x4f44_5049;
const GENERATED_PIXEL_MAX_BATCH_PIXELS: u32 = 64 * 1024;

pub(crate) struct GeneratedPixelProgram {
    kernel: Vec<u8>,
    body_bytes: usize,
    digest: [u8; 32],
    reported: AtomicBool,
}

impl GeneratedPixelProgram {
    pub(crate) fn code_len(&self) -> usize {
        self.body_bytes
    }
    pub(crate) fn code_hash(&self) -> [u8; 32] {
        self.digest
    }
}

#[derive(Copy, Clone, Debug)]
pub(crate) struct GeneratedPixelParamsV1 {
    pub(crate) origin_x: f32,
    pub(crate) origin_y: f32,
    pub(crate) step_x: f32,
    pub(crate) step_y: f32,
}

impl GeneratedPixelParamsV1 {
    fn is_valid(self) -> bool {
        [self.origin_x, self.origin_y, self.step_x, self.step_y]
            .iter()
            .all(|value| value.is_finite())
    }
}

pub(crate) fn create_generated_pixel_program(code: &[u8]) -> Result<GeneratedPixelProgram, i32> {
    if !generated_pixel_body_valid(code) {
        return Err(-22);
    }
    let digest = sha256_digest(code);
    let kernel = generated_pixel_envelope(code)?;
    Ok(GeneratedPixelProgram {
        kernel,
        body_bytes: code.len(),
        digest,
        reported: AtomicBool::new(false),
    })
}

/// Admit exactly the compiler's straight-line SIMD8 ALU profile. Arithmetic
/// never accesses the hardware header, byte offsets, driver temporaries or a
/// memory message. Both source registers must have been initialized on every
/// lane; a predicated write cannot initialize a previously undefined value.
fn generated_pixel_body_valid(code: &[u8]) -> bool {
    if code.is_empty() || code.len() > GENERATED_PIXEL_MAX_BODY_BYTES || code.len() % 16 != 0 {
        return false;
    }
    let mut initialized = [false; 128];
    initialized[1] = true;
    initialized[2] = true;
    let mut flags_initialized = false;
    for bytes in code.chunks_exact(16) {
        let w: [u32; 4] = core::array::from_fn(|i| {
            u32::from_le_bytes(bytes[4 * i..4 * i + 4].try_into().unwrap())
        });
        let op = w[0] & 0x7f;
        let pred = w[0] & 0x1100_0000;
        if w[0] & !0x1100_0000 != 0x0003_0100 | op
            || pred == 0x1000_0000
            || (pred != 0 && !flags_initialized)
        {
            return false;
        }
        let dst = (w[1] >> 24) as usize;
        let ty = (w[1] >> 4) & 15;
        let null_cmp = op == 0x70 && dst == 0;
        if (!null_cmp && !(4..112).contains(&dst)) || !matches!(ty, 2 | 10) {
            return false;
        }
        let base =
            (dst as u32) << 24 | if null_cmp { 0x10000 } else { 0x50000 } | ty << 4 | ty << 8;
        let src0 = ((w[2] >> 8) & 255) as usize;
        let src1 = ((w[3] >> 8) & 255) as usize;
        let readable = |r: usize| r < initialized.len() && initialized[r];
        if op == 0x61 {
            if w[1] == base | 0x4000 {
                if w[2] != 0 {
                    return false;
                }
            } else if w[1] != base
                || w[3] != 0
                || !readable(src0)
                || w[2] != 0x00460005 | (src0 as u32) << 8
            {
                return false;
            }
        } else {
            if !matches!(op, 0x40 | 0x41 | 0x62 | 0x65 | 0x66 | 0x67 | 0x68 | 0x69 | 0x70)
                || (matches!(op, 0x65..=0x69) && ty != 2)
                || (op == 0x62 && pred == 0)
                || !readable(src0)
            {
                return false;
            }
            let condition = if op == 0x70 { w[2] >> 28 } else { 0 };
            if (op == 0x70 && (!(1..=6).contains(&condition) || pred != 0))
                || w[2] != condition << 28 | ty << 24 | 0x00460005 | (src0 as u32) << 8
                || (w[1] != base | 0x8000
                    && (w[1] != base || !readable(src1) || w[3] != 0x00460005 | (src1 as u32) << 8))
            {
                return false;
            }
        }
        if op == 0x70 {
            flags_initialized = true;
        }
        if !null_cmp {
            if pred != 0 && op != 0x62 && !initialized[dst] {
                return false;
            }
            initialized[dst] = true;
        }
    }
    initialized[4]
}

pub(crate) fn generated_pixel_rgba8_surface_full(
    program: &GeneratedPixelProgram,
    dst: GpgpuRgba8Surface,
    params: GeneratedPixelParamsV1,
) -> GpgpuRgba8KernelResult {
    let started = direct_rcs_now_tick();
    if !dst.is_valid()
        || dst.pitch_bytes > 65535
        || dst.bytes > u32::MAX as usize
        || !params.is_valid()
        || direct_rcs_context_is_quarantined()
    {
        return GpgpuRgba8KernelResult::default();
    }
    let Some(launched_width) = dst.width.checked_add(7).map(|w| w / 8 * 8) else {
        return GpgpuRgba8KernelResult::default();
    };
    let rows_per_batch = (GENERATED_PIXEL_MAX_BATCH_PIXELS / launched_width).max(1);
    let _guard = DIRECT_RCS_SUBMIT_LOCK.lock();
    let Some(dev) = super::claimed_device() else {
        return GpgpuRgba8KernelResult::default();
    };
    if !matches!((dev.device_id, dev.revision_id), (0x4680, 0x0c) | (0x9a49, 0x01)) {
        return GpgpuRgba8KernelResult::default();
    }
    let Some(state) = direct_rcs_state_once(dev) else {
        return GpgpuRgba8KernelResult::default();
    };
    if !direct_rcs_forcewake(dev)
        || !direct_rcs_map_state(dev, state)
        || !direct_rcs_init_ppgtt(state)
        || !direct_rcs_map_ppgtt_destination(state, dst.gpu, dst.phys, dst.bytes, true)
    {
        return GpgpuRgba8KernelResult::default();
    }
    let mut first_row = 0;
    let mut any_submitted = false;
    while first_row < dst.height {
        let rows = rows_per_batch.min(dst.height - first_row);
        if !encode_generated_pixel(state, program, dst, params, first_row, rows) {
            return GpgpuRgba8KernelResult {
                submitted: any_submitted,
                ..Default::default()
            };
        }
        let submitted = direct_rcs_submit_batch_state(dev, state);
        any_submitted |= submitted.may_have_submitted();
        if !submitted.may_have_submitted() {
            return GpgpuRgba8KernelResult {
                submitted: any_submitted,
                ..Default::default()
            };
        }
        let observed = if submitted.can_poll() {
            direct_rcs_poll_result_slot_timeout_ms(
                state,
                0,
                GENERATED_PIXEL_COOKIE,
                UI4_COMPUTE_PRODUCER_RETIRE_TIMEOUT_MS,
            )
        } else {
            0
        };
        if observed != GENERATED_PIXEL_COOKIE {
            quarantine_direct_rcs_context("odin-generated-pixel-incomplete");
            return GpgpuRgba8KernelResult {
                submitted: true,
                marker: observed,
                submit_ms: direct_rcs_elapsed_ms_since(started),
                ..Default::default()
            };
        }
        first_row += rows;
    }
    if !program.reported.swap(true, Ordering::AcqRel) {
        let checksum = generated_pixel_surface_fnv(dst);
        crate::log_info!(target: "gpgpu";
            "odin-native-pixel: completed marker=0x{:08X} extent={}x{} pitch={} body_bytes={} body_sha256={} framebuffer_fnv1a64=0x{:016X} source=native-eu gpu_retired=1 cpu_pixel_writes=0\n",
            GENERATED_PIXEL_COOKIE, dst.width, dst.height, dst.pitch_bytes,
            program.body_bytes, digest_hex(&program.digest).as_str(), checksum);
    }
    GpgpuRgba8KernelResult {
        ok: true,
        submitted: any_submitted,
        marker: GENERATED_PIXEL_COOKIE,
        submit_ms: direct_rcs_elapsed_ms_since(started),
        release: Some(gpgpu_rgba8_release(dst)),
    }
}

fn encode_generated_pixel(
    state: DirectRcsState,
    program: &GeneratedPixelProgram,
    dst: GpgpuRgba8Surface,
    params: GeneratedPixelParamsV1,
    first_row: u32,
    rows: u32,
) -> bool {
    if rows == 0 || first_row >= dst.height || rows > dst.height - first_row {
        return false;
    }
    let code = &program.kernel;
    if GENERATED_PIXEL_CODE_OFFSET + code.len() > DIRECT_RCS_BATCH_BYTES {
        return false;
    }
    unsafe {
        core::ptr::write_bytes(state.batch_virt, 0, DIRECT_RCS_BATCH_BYTES);
        core::ptr::write_bytes(state.result_virt, 0, DIRECT_RCS_RESULT_BYTES);
        core::ptr::copy_nonoverlapping(
            code.as_ptr(),
            state.batch_virt.add(GENERATED_PIXEL_CODE_OFFSET),
            code.len(),
        );
        let payload = state.batch_virt.add(GENERATED_PIXEL_PAYLOAD).cast::<u32>();
        for (i, value) in [
            params.origin_x.to_bits(),
            params.origin_y.to_bits(),
            params.step_x.to_bits(),
            params.step_y.to_bits(),
            dst.width,
            dst.pitch_bytes,
            first_row,
            0,
        ]
        .into_iter()
        .enumerate()
        {
            payload.add(i).write(value);
        }
        for lane in 0..8 {
            payload.add(8 + lane).write(lane as u32);
        }
        state
            .batch_virt
            .add(GENERATED_PIXEL_BINDING)
            .cast::<u32>()
            .write(GENERATED_PIXEL_SURFACE as u32);
    }
    if !direct_rcs_write_interface_descriptor_at(
        state,
        GENERATED_PIXEL_IDD,
        GENERATED_PIXEL_BINDING,
        GENERATED_PIXEL_CODE_OFFSET as u64,
        1,
        3,
    ) || !direct_rcs_write_buffer_surface_state(
        state,
        GENERATED_PIXEL_SURFACE,
        dst.gpu,
        dst.bytes,
    ) {
        return false;
    }
    unsafe {
        // Fixed cross-thread r1..r3, no per-thread local-ID payload.
        state
            .batch_virt
            .add(GENERATED_PIXEL_IDD + 20)
            .cast::<u32>()
            .write(0);
        state
            .batch_virt
            .add(GENERATED_PIXEL_IDD + 8)
            .cast::<u32>()
            .write(IDD_THREAD_PREEMPTION_DISABLE | (1 << 19));
    }
    let batch = unsafe {
        core::slice::from_raw_parts_mut(state.batch_virt.cast::<u32>(), GENERATED_PIXEL_IDD / 4)
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
        GENERATED_PIXEL_IDD as u32,
        GPGPU_WALKER_CMD,
        0,
        128,
        GENERATED_PIXEL_PAYLOAD as u32,
        0,
        0,
        0,
        dst.width.div_ceil(8),
        0,
        0,
        rows,
        0,
        1,
        0xff,
        0xff,
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
        GENERATED_PIXEL_COOKIE,
    ) {
        return false;
    }
    super::dma_flush(state.batch_virt, DIRECT_RCS_BATCH_BYTES);
    super::dma_flush(state.result_virt, DIRECT_RCS_RESULT_BYTES);
    true
}

// All instructions are uncompressed 12p1 SIMD8 words. High GRFs are excluded
// by body admission, so x and the width survive arbitrary compiler arithmetic.
fn generated_pixel_envelope(body: &[u8]) -> Result<Vec<u8>, i32> {
    let mut code = Vec::new();
    code.try_reserve_exact(body.len() + 1024).map_err(|_| -12)?;
    let mut emit = |w: [u32; 4]| {
        for word in w {
            code.extend_from_slice(&word.to_le_bytes());
        }
    };
    emit(crate::gpu::native_xelp::PREFIX);
    // r112=x, r113=y. r0.1/r0.6 carry workgroup x/y; r2 holds lane IDs.
    emit([0x80030161, 0x70050220, 0x24, 0]);
    emit(generated_pixel_binary(0x41, 112, 112, 8, true, false));
    emit(generated_pixel_binary(0x40, 112, 112, 2, false, false));
    emit([0x80030161, 0x71050220, 0xc4, 0]);
    emit(generated_pixel_broadcast(0x40, 113, 113, 1, 6, false));
    emit(generated_pixel_broadcast(0x41, 114, 113, 1, 5, false));
    emit(generated_pixel_binary(0x69, 3, 112, 2, true, false));
    emit(generated_pixel_binary(0x40, 3, 3, 114, false, false));
    emit([0x00030161, 0x74050220, 0x00000184, 0]); // width r116 <- r1.4
    emit([0x00030161, 0x720502a0, 0x00467005, 0]); // x:UD -> r114:F
    emit(generated_pixel_broadcast(0x41, 114, 114, 1, 2, true));
    emit(generated_pixel_broadcast(0x40, 114, 114, 1, 0, true));
    emit([0x00030161, 0x730502a0, 0x00467105, 0]); // y:UD -> r115:F
    emit(generated_pixel_broadcast(0x41, 115, 115, 1, 3, true));
    emit(generated_pixel_broadcast(0x40, 115, 115, 1, 1, true));
    emit([0x00030161, 0x01050aa0, 0x00467205, 0]);
    emit([0x00030161, 0x02050aa0, 0x00467305, 0]);
    drop(emit);
    code.extend_from_slice(body);
    let mut emit = |w: [u32; 4]| {
        for word in w {
            code.extend_from_slice(&word.to_le_bytes());
        }
    };
    // NaN/negative -> 0, >1 -> 1; conversion truncates after scaling.
    emit([0x00030170, 0x00018aa0, 0x4a460405, 0]);
    emit([0x01030162, 0x75058aa0, 0x0a460405, 0]);
    emit([0x00030170, 0x00018aa0, 0x6a467505, 0x3f800000]);
    emit([0x01030162, 0x75058aa0, 0x0a467505, 0x3f800000]);
    emit(generated_pixel_binary(0x41, 117, 117, 255f32.to_bits(), true, true));
    emit([0x00030161, 0x75050a20, 0x00467505, 0]);
    emit(generated_pixel_binary(0x69, 118, 117, 8, true, false));
    emit(generated_pixel_binary(0x66, 117, 117, 118, false, false));
    emit(generated_pixel_binary(0x69, 118, 117, 16, true, false));
    emit(generated_pixel_binary(0x66, 117, 117, 118, false, false));
    emit(generated_pixel_binary(0x66, 117, 117, 0xff000000, true, false));
    // Store only valid tail pixels. Keep all group lanes active until here.
    emit([0x00030170, 0x00010220, 0x52467005, 0x00467405]);
    emit([0x00000101, 0x00010000, 0, 0]);
    emit([0x01034031, 0, 0xcc00030c, 0x009a750c]);
    for instruction in crate::gpu::native_xelp::SUFFIX.iter().skip(2) {
        emit(*instruction);
    }
    Ok(code)
}

fn generated_pixel_surface_fnv(dst: GpgpuRgba8Surface) -> u64 {
    let pixels = crate::phys::phys_to_virt(dst.phys as usize) as *mut u8;
    // GPU retirement precedes CPU acquisition; invalidation discards stale
    // framebuffer cache lines before reading. Hash only visible row bytes.
    super::dma_flush(pixels, dst.bytes);
    let mut hash = 0xcbf29ce484222325u64;
    for y in 0..dst.height as usize {
        for x in 0..dst.width as usize * 4 {
            let value =
                unsafe { core::ptr::read_volatile(pixels.add(y * dst.pitch_bytes as usize + x)) };
            hash = (hash ^ value as u64).wrapping_mul(0x100000001b3);
        }
    }
    hash
}

fn generated_pixel_binary(
    op: u32,
    dst: u32,
    src0: u32,
    src1: u32,
    immediate: bool,
    float: bool,
) -> [u32; 4] {
    let ty = if float { 10 } else { 2 };
    [
        0x00030100 | op,
        dst << 24 | 0x50000 | ty << 4 | ty << 8 | if immediate { 0x8000 } else { 0 },
        ty << 24 | 0x00460005 | src0 << 8,
        if immediate {
            src1
        } else {
            0x00460005 | src1 << 8
        },
    ]
}

fn generated_pixel_broadcast(
    op: u32,
    dst: u32,
    src0: u32,
    uniform: u32,
    subreg: u32,
    float: bool,
) -> [u32; 4] {
    let mut w = generated_pixel_binary(op, dst, src0, uniform, false, float);
    w[3] = uniform << 8 | subreg * 32 | 4;
    w
}
