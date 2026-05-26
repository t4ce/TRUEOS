#![allow(dead_code)]

use super::*;

const MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_BYTES: &[u8] = include_bytes!(
    "../../../crates/trueos-shader/intel_userland_oracle/latest/dumps/000534_pre_exec_handle_9_off_0x2000_len_0x2000.bin",
);
const MANDELBROT_ORACLE_LATEST_HANDLE9_COMPLETION_MARKER: u32 = 0xC0DE_7732;
static MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_LOGGED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy)]
enum MandelbrotCommandStreamSource {
    DynamicEncoded,
    OracleLatestHandle9Batch,
}

const MANDELBROT_COMMAND_STREAM_SOURCE: MandelbrotCommandStreamSource =
    MandelbrotCommandStreamSource::DynamicEncoded;
// This toggles the compute-walker SIMD mask only. The EU artifact below remains
// the existing scalar groupid row writer; this is not a SIMD16 Mandelbrot body.
const MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE: bool = true;
const MANDELBROT_GROUPID_LINE1280_SIMD16_PROGRAM_NAME: &str = "gfx12-primary-scanout-groupid-line1280-rows-simd16-walker-mask-existing-scalar-row-writer-hdc1-stateless-unrolled-store-then-ts-eot";
const MANDELBROT_GROUPID_LINE1280_ARTIFACT_BODY: &str = "row-writer-scalar-bw-v1";
const MANDELBROT_GROUPID_LINE1280_PAYLOAD_CONTRACT: &str = "row-color-burst-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD16_DISPATCH_CONTRACT: &str = "simd16-mask-walker-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD8_DISPATCH_CONTRACT: &str = "simd8-mask-walker-v1";
const MANDELBROT_GROUPID_LINE1280_SIMD16_PROVES: &str = "simd16-walker-dispatch-over-row-writer";
const MANDELBROT_GROUPID_LINE1280_SIMD8_PROVES: &str = "simd8-walker-dispatch-over-row-writer";
const MANDELBROT_GROUPID_LINE1280_DOES_NOT_PROVE: &str = "simd16-mandelbrot-eu-body";

fn gpgpu_primary_scanout_pixel_quiet_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: "gfx12-t47-primary-scanout-pixel-quiet-hdc1-stateless-store-then-ts-eot",
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        store_send_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD),
    }
}

fn gpgpu_primary_scanout_mandelbrot8_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD
                .saturating_sub(3),
        ),
        visible_seed_dword: None,
    }
}

fn gpgpu_primary_scanout_mandelbrot8_gpu_color_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_STORE_SEND_DWORD,
        ),
        visible_seed_dword: None,
    }
}

fn gpgpu_primary_scanout_mandelbrot16_simd16_bw_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SEND_DWORD,
        ),
        visible_seed_dword: None,
    }
}

fn gpgpu_primary_scanout_groupid_line320_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STORE_SEND_DWORD,
        ),
        visible_seed_dword: None,
    }
}

fn gpgpu_primary_scanout_groupid_line1280_rows_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: if MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE {
            MANDELBROT_GROUPID_LINE1280_SIMD16_PROGRAM_NAME
        } else {
            artifact.name
        },
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_STORE_SEND_DWORD,
        ),
        visible_seed_dword: None,
    }
}

fn groupid_line1280_expected_dispatch_lanes() -> usize {
    if MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE {
        16
    } else {
        8
    }
}

fn groupid_line1280_dispatch_contract() -> &'static str {
    if MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE {
        MANDELBROT_GROUPID_LINE1280_SIMD16_DISPATCH_CONTRACT
    } else {
        MANDELBROT_GROUPID_LINE1280_SIMD8_DISPATCH_CONTRACT
    }
}

fn groupid_line1280_proves() -> &'static str {
    if MANDELBROT_GROUPID_LINE1280_SIMD16_MASK_PROBE {
        MANDELBROT_GROUPID_LINE1280_SIMD16_PROVES
    } else {
        MANDELBROT_GROUPID_LINE1280_SIMD8_PROVES
    }
}

fn upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
    warm: RenderWarmState,
    address_base: u32,
    color: u32,
    mode: u32,
) -> bool {
    let mut words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    patch_mandelbrot16_simd16_probe_variant(&mut words, mode);
    let mut address_slot = 0usize;
    while address_slot
        < trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORDS.len()
    {
        words[trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORDS
            [address_slot]] = address_base;
        address_slot += 1;
    }
    let color_dword = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_DWORD;
    if color_dword != usize::MAX {
        words[color_dword] = color;
    }
    let uploaded = upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &words);
    if uploaded {
        MANDELBROT_LINE1280_TEMPLATE_UPLOADED.store(false, Ordering::Release);
        MANDELBROT_GROUPID_LINE1280_TEMPLATE_UPLOADED.store(false, Ordering::Release);
    }
    uploaded
}

fn patch_mandelbrot16_simd16_probe_variant(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    mode: u32,
) {
    const NOP: [u32; 4] = [0x80000101, 0x00000000, 0x00000000, 0x00000000];
    const ADD_G22_G6_G6: [u32; 4] = [0x00040140, 0x16050660, 0x06460605, 0x00460605];
    const MUL_G22_G6_G6: [u32; 4] = [0x00040141, 0x16050660, 0x06460605, 0x00460605];
    const MUL_G22_G6_3D: [u32; 4] = [0x00040141, 0x16058660, 0x06460605, 0x00000003];
    const MUL_G22_G6_G6_UD: [u32; 4] = [0x00040141, 0x16050220, 0x02460605, 0x00460605];
    const MUL_ACC0_G6_G6: [u32; 4] = [0x00040141, 0x20010660, 0x06460605, 0x00460605];
    const MOV_G22_ACC0: [u32; 4] = [0x00040161, 0x16050660, 0x00462001, 0x00000000];
    const MUL_G22_G6_SCALAR_D: [u32; 4] = [0x00040141, 0x16050660, 0x06460605, 0x00000604];
    const MUL_G22_G6_G6_W: [u32; 4] = [0x00040141, 0x16050550, 0x05580605, 0x00580605];
    const MUL_G22_G6_G6_UW: [u32; 4] = [0x00040141, 0x16050110, 0x01580605, 0x00580605];
    const MUL8_G22_G6_G6_1Q: [u32; 4] = [0x00030141, 0x16050660, 0x06460605, 0x00460605];
    const MUL8_G23_G7_G7_2Q: [u32; 4] = [0x00130141, 0x17050660, 0x06460705, 0x00460705];
    const MUL_G24_G6_G6_W: [u32; 4] = [0x00040141, 0x18050550, 0x05580605, 0x00580605];
    const MOV_G22_G24_W: [u32; 4] = [0x00040161, 0x16050560, 0x00581805, 0x00000000];
    const MUL_G24_G6_G6_UW: [u32; 4] = [0x00040141, 0x18050110, 0x01580605, 0x00580605];
    const MOV_G22_G24_UW: [u32; 4] = [0x00040161, 0x16050120, 0x00581805, 0x00000000];
    const MOV_G22_9: [u32; 4] = [0x80040061, 0x16054660, 0x00000000, 0x00000009];

    let (first, second) = match mode {
        1 => (ADD_G22_G6_G6, NOP),
        2 => (MUL_G22_G6_G6, NOP),
        3 => (NOP, MOV_G22_9),
        4 => (MUL_G22_G6_3D, NOP),
        5 => (MUL_G22_G6_G6_UD, NOP),
        6 => (MUL_ACC0_G6_G6, MOV_G22_ACC0),
        7 => (MUL_G22_G6_G6, NOP),
        8 => (MUL_G22_G6_SCALAR_D, NOP),
        9 => (MUL_G22_G6_G6_W, NOP),
        10 => (MUL_G22_G6_G6_UW, NOP),
        11 => (MUL8_G22_G6_G6_1Q, MUL8_G23_G7_G7_2Q),
        12 => (MUL_G24_G6_G6_W, MOV_G22_G24_W),
        13 => (MUL_G24_G6_G6_UW, MOV_G22_G24_UW),
        _ => (MUL_G22_G6_G6, MOV_G22_9),
    };

    words[24] = first[0];
    words[25] = first[1];
    words[26] = first[2];
    words[27] = first[3];
    words[28] = second[0];
    words[29] = second[1];
    words[30] = second[2];
    words[31] = second[3];
}

fn mandelbrot16_simd16_probe_variant_name(mode: u32) -> &'static str {
    match mode {
        1 => "add16-dword-g6-plus-g6-g22-no-overwrite",
        2 => "mul16-dword-g6-times-g6-g22-no-overwrite",
        3 => "mov16-immediate-nine-g22-control",
        4 => "mul16-dword-g6-times-3d-g22-no-overwrite",
        5 => "mul16-udword-g6-times-g6-g22-no-overwrite",
        6 => "mul16-dword-g6-times-g6-acc0-then-mov-g22",
        7 => "mul16-dword-g6-times-g6-g22-then-sync-nop",
        8 => "mul16-dword-g6-times-g6-scalar-source-g22",
        9 => "mul16-word-g6-times-g6-packed-g22",
        10 => "mul16-uword-g6-times-g6-packed-g22",
        11 => "mul8x2-dword-g6-g7-times-self-g22-g23",
        12 => "mul16-word-g6-times-g6-g24-then-widen-g22",
        13 => "mul16-uword-g6-times-g6-g24-then-widen-g22",
        _ => "mul16-dword-g6-times-g6-g22-then-immediate-nine-isolation",
    }
}

fn mandelbrot16_simd16_probe_expected_first(mode: u32) -> u32 {
    match mode {
        1 => 6,
        9 | 10 => 0x0009_0009,
        _ => 9,
    }
}

fn mandelbrot16_fixed10_expected_depth(lane: usize) -> u32 {
    let _ = lane;
    9
}

#[allow(dead_code)]
fn mandelbrot16_fixed10_expected_depth_old(lane: usize) -> u32 {
    const FRAC_BITS: i32 =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS as i32;
    const MAX_ITER: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER;
    let c_re = (lane as i32).wrapping_mul(5).wrapping_sub(8192);
    let c_im = -4096i32;
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    let mut depth = 0u32;
    let mut iter = 0u32;
    while iter < MAX_ITER {
        let re2 = z_re.wrapping_mul(z_re) >> FRAC_BITS;
        let im2 = z_im.wrapping_mul(z_im) >> FRAC_BITS;
        let active = re2.wrapping_add(im2) < 16384;
        let next_re = re2.wrapping_sub(im2).wrapping_add(c_re);
        let next_im = (z_re.wrapping_mul(z_im) >> (FRAC_BITS - 1)).wrapping_add(c_im);
        if active {
            depth = depth.wrapping_add(1);
            z_re = next_re;
            z_im = next_im;
        }
        iter += 1;
    }
    depth
}

fn mandelbrot16_active_lane_mask() -> u32 {
    if trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES >= 32 {
        u32::MAX
    } else {
        (1u32 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES) - 1
    }
}

fn ensure_primary_scanout_groupid_line1280_rows_artifact_uploaded(warm: RenderWarmState) {
    if MANDELBROT_GROUPID_LINE1280_TEMPLATE_UPLOADED.load(Ordering::Acquire) {
        return;
    }

    let strip_words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS;
    unsafe {
        core::ptr::copy_nonoverlapping(
            strip_words.as_ptr() as *const u8,
            warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
            core::mem::size_of_val(&strip_words),
        );
    }
    crate::intel::dma_flush(
        unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
        core::mem::size_of_val(&strip_words),
    );
    MANDELBROT_GROUPID_LINE1280_TEMPLATE_UPLOADED.store(true, Ordering::Release);
}

fn prepare_primary_scanout_groupid_line1280_rows_command_stream(
    warm: RenderWarmState,
    target_gpu: u64,
    target_byte_len: usize,
    store_surface_label: &'static str,
    program: GpgpuEuProgram,
    base_gpu: u64,
    second_base_gpu: Option<u64>,
    color_seed: u32,
    row_group_count: u32,
) -> Result<(usize, u32), &'static str> {
    match MANDELBROT_COMMAND_STREAM_SOURCE {
        MandelbrotCommandStreamSource::DynamicEncoded => {
            prepare_primary_scanout_groupid_line1280_rows_dynamic_command_stream(
                warm,
                target_gpu,
                target_byte_len,
                store_surface_label,
                program,
                base_gpu,
                second_base_gpu,
                color_seed,
                row_group_count,
            )
        }
        MandelbrotCommandStreamSource::OracleLatestHandle9Batch => {
            prepare_primary_scanout_groupid_line1280_rows_oracle_latest_handle9_batch_command_stream(
                warm,
            )
        }
    }
}

fn prepare_primary_scanout_groupid_line1280_rows_dynamic_command_stream(
    warm: RenderWarmState,
    target_gpu: u64,
    target_byte_len: usize,
    store_surface_label: &'static str,
    program: GpgpuEuProgram,
    base_gpu: u64,
    second_base_gpu: Option<u64>,
    color_seed: u32,
    row_group_count: u32,
) -> Result<(usize, u32), &'static str> {
    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target_gpu,
        target_byte_len,
        store_surface_label,
    );
    let completion_marker = 0xC0DE_0000
        | (MANDELBROT_GROUPID_LINE1280_SUBMIT_SERIAL
            .fetch_add(1, Ordering::AcqRel)
            .wrapping_add(1)
            & 0x0000_FFFF);
    let batch_bytes = encode_gfx12_gpgpu_line1280_groupid_rows_batch(
        warm,
        batch,
        store_surface,
        program,
        base_gpu,
        second_base_gpu,
        color_seed,
        row_group_count,
        completion_marker,
    )?;
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);
    Ok((batch_bytes, completion_marker))
}

fn prepare_primary_scanout_groupid_line1280_rows_oracle_latest_handle9_batch_command_stream(
    warm: RenderWarmState,
) -> Result<(usize, u32), &'static str> {
    let batch_bytes = MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_BYTES.len();
    if batch_bytes > warm.batch_len {
        return Err("groupid-line1280-captured-batch-too-large");
    }

    if !MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_LOGGED.swap(true, Ordering::AcqRel) {
        crate::log!(
            "intel/gpgpu: mandelbrot command_stream_source=oracle-latest-handle9-batch batch_bytes=0x{:X} completion_marker=0x{:08X} caveat=linux-gpu-addresses-unpatched
",
            batch_bytes,
            MANDELBROT_ORACLE_LATEST_HANDLE9_COMPLETION_MARKER,
        );
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        core::ptr::copy_nonoverlapping(
            MANDELBROT_ORACLE_LATEST_HANDLE9_BATCH_BYTES.as_ptr(),
            warm.batch_virt,
            batch_bytes,
        );
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);
    Ok((batch_bytes, MANDELBROT_ORACLE_LATEST_HANDLE9_COMPLETION_MARKER))
}

fn gpgpu_primary_scanout_row2560_simd8_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_STORE_SEND_DWORD),
        visible_seed_dword: None,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_marker_probe() -> crate::intel::GpgpuOneTileSentinelProof
{
    let program = gpgpu_one_tile_output_sentinel_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if target.marker_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "marker-gpu-high32-unsupported",
            program,
            target.marker_gpu,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.marker_gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.marker_gpu);
    }

    unsafe {
        core::ptr::write_volatile(target.marker_virt as *mut u32, 0);
    }
    crate::intel::dma_flush(target.marker_virt, core::mem::size_of::<u32>());
    let output_first_before = unsafe { core::ptr::read_volatile(target.marker_virt as *const u32) };

    let mut sentinel_words = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    sentinel_words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD] = GPGPU_ONE_TILE_OUTPUT_SENTINEL;
    sentinel_words[7] = target.marker_gpu as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &sentinel_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, target.marker_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        for breadcrumb_slot in 23..=28 {
            core::ptr::write_volatile(
                warm.result_virt
                    .add(breadcrumb_slot * core::mem::size_of::<u32>()) as *mut u32,
                0,
            );
        }
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_store_surface_state_for_target(
        warm,
        target.marker_gpu,
        "bind-send-bti-to-primary-scanout-marker",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => {
                return gpgpu_one_tile_sentinel_failure(reason, program, target.marker_gpu);
            }
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-marker",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let display_notified = crate::intel::display::notify_primary_surface_external_write(
        "gpgpu-primary-scanout-marker",
        target.marker_offset,
        core::mem::size_of::<u32>(),
    );
    let output_first_after = unsafe { core::ptr::read_volatile(target.marker_virt as *const u32) };
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_hits_lo64 = if output_first_after == GPGPU_ONE_TILE_OUTPUT_SENTINEL {
        1
    } else {
        0
    };
    let readback_ok = output_first_before == 0
        && output_first_after == GPGPU_ONE_TILE_OUTPUT_SENTINEL
        && finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok && dispatch_delta == 0 {
        "scanout-sentinel-written-no-ts-delta"
    } else if readback_ok {
        "scanout-sentinel-written"
    } else if !finished {
        "submit-not-finished"
    } else if output_first_after != GPGPU_ONE_TILE_OUTPUT_SENTINEL {
        "scanout-sentinel-missing"
    } else {
        "scanout-sentinel-not-clean"
    };
    crate::log!(
        "intel/gpgpu: primary-scanout-marker submitted=1 finished={} readback_ok={} reason={} program_source={} primary_gpu=0x{:X} primary_phys=0x{:X} primary_bytes=0x{:X} marker_gpu=0x{:X} marker_off=0x{:X} xy={}x{} before=0x{:08X} after=0x{:08X} sentinel=0x{:08X} output_hits_lo64=0x{:016X} display_notified={} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next=expand-marker-to-visible-block-or-mandelbrot-pixels does_not_prove=fragment_shader_mandelbrot_pixels\n",
        finished as u8,
        readback_ok as u8,
        reason,
        program.name,
        target.gpu,
        target.phys,
        target.byte_len,
        target.marker_gpu,
        target.marker_offset,
        target.marker_x,
        target.marker_y,
        output_first_before,
        output_first_after,
        GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_hits_lo64,
        display_notified as u8,
        dispatch_delta,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        if readback_ok {
            "continue-framebuffer-target"
        } else {
            "fix-primary-scanout-target"
        },
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-primary-scanout-marker");
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: true,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: target.marker_gpu,
        sentinel: GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn mandelbrot_q12_x_step(width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    ((3 * scale + (width.max(1) as i64 / 2)) / width.max(1) as i64) as i32
}

fn mandelbrot_q12_c_re_base(x_base: usize, width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    (-2 * scale + (x_base as i64 * 3 * scale) / width.max(1) as i64) as i32
}

fn mandelbrot_q12_c_im(y: usize, height: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    (-scale + (y as i64 * 2 * scale) / height.max(1) as i64) as i32
}

fn mandelbrot_q12_expected_color(c_re: i32, c_im: i32) -> u32 {
    let frac_bits = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS;
    let escape_q12 = 4i64 << frac_bits;
    let mut zr = 0i64;
    let mut zi = 0i64;
    let mut iter = 0u32;
    let mut step = 0u32;
    while step < trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_MAX_ITER {
        let zr2 = zr.saturating_mul(zr) >> frac_bits;
        let zi2 = zi.saturating_mul(zi) >> frac_bits;
        if zr2.saturating_add(zi2) >= escape_q12 {
            break;
        }
        let zr_next = zr2.saturating_sub(zi2).saturating_add(c_re as i64);
        let zi_next = ((zr.saturating_mul(zi)).saturating_mul(2)) >> frac_bits;
        zr = zr_next;
        zi = zi_next.saturating_add(c_im as i64);
        iter = iter.saturating_add(1);
        step = step.saturating_add(1);
    }
    if iter >= trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_MAX_ITER {
        0
    } else {
        (iter << 18)
            .wrapping_add(iter << 10)
            .wrapping_add(iter << 2)
            .wrapping_add(0x0000_2040)
    }
}

fn mandelbrot16_q12_x_step(width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    ((3 * scale + (width.max(1) as i64 / 2)) / width.max(1) as i64) as i32
}

fn mandelbrot16_q12_c_re_base(x_base: usize, width: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    (-2 * scale + (x_base as i64 * 3 * scale) / width.max(1) as i64) as i32
}

fn mandelbrot16_q12_c_im(y: usize, height: usize) -> i32 {
    let scale = 1i64 << trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    (-scale + (y as i64 * 2 * scale) / height.max(1) as i64) as i32
}

fn mandelbrot16_q12_one_iteration_escape_mask(c_re0: i32, x_step: i32, c_im: i32) -> u32 {
    let frac_bits = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    let escape_radius_q24 = 4i64 << (frac_bits * 2);
    let mut mask = 0u32;
    let mut lane = 0usize;
    while lane < trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES {
        let c_re = c_re0 as i64 + (lane as i64 * x_step as i64);
        let c_im = c_im as i64;
        let mag_q24 = c_re
            .saturating_mul(c_re)
            .saturating_add(c_im.saturating_mul(c_im));
        if mag_q24 > escape_radius_q24 {
            mask |= 1u32 << lane;
        }
        lane += 1;
    }
    mask
}

fn mandelbrot16_q12_mag_after_one(c_re: i32, c_im: i32) -> u64 {
    let c_re = c_re as i64;
    let c_im = c_im as i64;
    c_re.saturating_mul(c_re)
        .saturating_add(c_im.saturating_mul(c_im)) as u64
}

fn submit_gpgpu_primary_scanout_pixel_quiet(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    program: GpgpuEuProgram,
    pixel_gpu: u64,
    pixel_virt: *mut u8,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    if pixel_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("pixel-gpu-high32-unsupported", program, pixel_gpu);
    }

    let output_first_before = unsafe { core::ptr::read_volatile(pixel_virt as *const u32) };
    let mut pixel_words = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    pixel_words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD] = color;
    pixel_words[7] = pixel_gpu as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &pixel_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, pixel_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_store_surface_state_for_target(
        warm,
        pixel_gpu,
        "bind-send-bti-to-primary-scanout-pixel-quiet",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, pixel_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-pixel-quiet",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(pixel_virt, core::mem::size_of::<u32>());
    let output_first_after = unsafe { core::ptr::read_volatile(pixel_virt as *const u32) };
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_hits_lo64 = if output_first_after == color { 1 } else { 0 };
    let readback_ok = output_first_after == color
        && finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok {
        "scanout-pixel-written"
    } else if !finished {
        "submit-not-finished"
    } else if output_first_after != color {
        "scanout-pixel-mismatch"
    } else {
        "scanout-pixel-not-clean"
    };
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-pixel-quiet",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: pixel_gpu,
        sentinel: color,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_mandelbrot_strip(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    program: GpgpuEuProgram,
    scanout_gpu: u64,
    scanout_bytes: usize,
    row_gpu: u64,
    row_virt: *mut u8,
    x_base: usize,
    y: usize,
    width: usize,
    height: usize,
    phase: usize,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM;
    const LANES_PER_SEND: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES;
    const SENDS_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM;

    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("strip-gpu-high32-unsupported", program, row_gpu);
    }
    if row_gpu < scanout_gpu {
        return gpgpu_one_tile_sentinel_failure("strip-before-scanout", program, row_gpu);
    }
    let row_offset = row_gpu - scanout_gpu;
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "strip-offset-high32-unsupported",
            program,
            row_gpu,
        );
    }
    crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
    let mut before_words = [0u32; PIXELS_PER_PROGRAM];
    let mut lane = 0usize;
    while lane < PIXELS_PER_PROGRAM {
        before_words[lane] = unsafe {
            core::ptr::read_volatile(row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32)
        };
        lane += 1;
    }
    let output_first_before = before_words[0];

    let mut strip_words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ESCAPE_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    let q12_x_step = mandelbrot_q12_x_step(width);
    let q12_c_im = mandelbrot_q12_c_im(y, height);
    let mut expected_words = [0u32; PIXELS_PER_PROGRAM];
    let mut strip = 0usize;
    while strip < SENDS_PER_PROGRAM {
        let strip_x = x_base.saturating_add(strip.saturating_mul(LANES_PER_SEND));
        let q12_c_re0 = mandelbrot_q12_c_re_base(strip_x, width);
        strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_X_STEP_DWORDS[strip]] =
            q12_x_step as u32;
        strip_words
            [trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_RE_BASE_DWORDS[strip]] =
            q12_c_re0 as u32;
        strip_words
            [trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ADDRESS_BASE_DWORDS[strip]] =
            row_gpu.saturating_add(
                (strip.saturating_mul(LANES_PER_SEND) * core::mem::size_of::<u32>()) as u64,
            ) as u32;
        lane = 0;
        while lane < LANES_PER_SEND {
            let pixel = strip.saturating_mul(LANES_PER_SEND).saturating_add(lane);
            let c_re = q12_c_re0.saturating_add(q12_x_step.saturating_mul(lane as i32));
            expected_words[pixel] = mandelbrot_q12_expected_color(c_re, q12_c_im);
            lane += 1;
        }
        strip += 1;
    }
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_C_IM_DWORD] =
        q12_c_im as u32;
    if x_base == 0 && y == 0 && !MANDELBROT_Q12_PATCH_LOGGED.swap(true, Ordering::AcqRel) {
        let artifact_bytes = strip_words.len() * core::mem::size_of::<u32>();
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot32-q12-patch scanout_gpu=0x{:X} row_gpu=0x{:X} row_virt=0x{:X} row={} x_base={} width={} height={} phase={} cpu_patches=q12-x-step-plus-four-c-re-bases-plus-c-im-plus-four-address-bases eu_computes=lane-coordinates-escape-count-color-store-payload simd_lanes_per_send={} hdc_store_sends={} pixels_per_program={} q12_frac_bits={} q12_max_iter={} q12_x_step={} q12_c_im={} first_c_re={} first_expected=0x{:08X} first_before=0x{:08X} first_address_base_dword={} first_address_base=0x{:X} first_store_exdesc_dword={} first_store_exdesc=0x{:08X} kernel_off=0x{:X} artifact_bytes=0x{:X} artifact_end_off=0x{:X} dynamic_state_off=0x{:X} bt_off=0x{:X} surf_off=0x{:X} store_state_after_artifact={} message_contract=4x-hdc-simd8-stateless-store\n",
            scanout_gpu,
            row_gpu,
            row_virt as usize,
            y,
            x_base,
            width,
            height,
            phase,
            LANES_PER_SEND,
            SENDS_PER_PROGRAM,
            PIXELS_PER_PROGRAM,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_FRAC_BITS,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_MAX_ITER,
            q12_x_step,
            q12_c_im,
            mandelbrot_q12_c_re_base(x_base, width),
            expected_words[0],
            output_first_before,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ADDRESS_BASE_DWORDS[0],
            strip_words
                [trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_ADDRESS_BASE_DWORDS[0]],
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD,
            strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STORE_EXDESC_DWORD],
            GPGPU_EU_KERNEL_OFFSET_BYTES,
            artifact_bytes,
            GPGPU_EU_KERNEL_OFFSET_BYTES.saturating_add(artifact_bytes),
            GPGPU_WALKER_SCRATCH_OFFSET_BYTES,
            GPGPU_MANDELBROT_STORE_BINDING_TABLE_OFFSET_BYTES,
            GPGPU_MANDELBROT_STORE_SURFACE_STATE_OFFSET_BYTES,
            (GPGPU_MANDELBROT_STORE_BINDING_TABLE_OFFSET_BYTES
                >= GPGPU_EU_KERNEL_OFFSET_BYTES.saturating_add(artifact_bytes)) as u8,
        );
    }
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words) {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        scanout_gpu,
        scanout_bytes,
        "bind-stateless-hdc253-to-primary-scanout-full-surface-quiet",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-mandelbrot8-strip",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let mut hits = 0u64;
    let readback_poll_limit = if finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else {
        1
    };
    let mut readback_poll = 0usize;
    let mut output_first_after = output_first_before;
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
        hits = 0;
        let mut changed = 0u64;
        let mut lane = 0usize;
        while lane < PIXELS_PER_PROGRAM {
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            if after == expected_words[lane] {
                hits |= 1u64 << lane;
            }
            if after != before_words[lane] {
                changed |= 1u64 << lane;
            }
            if lane == 0 {
                output_first_after = after;
            }
            lane += 1;
        }
        if hits == ((1u64 << PIXELS_PER_PROGRAM) - 1) {
            break;
        }
        let _ = changed;
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_hit_mask = (1u64 << PIXELS_PER_PROGRAM) - 1;
    let readback_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && hits == expected_hit_mask;
    let reason = if readback_ok {
        "mandelbrot32-q12-exact-readback"
    } else if !finished {
        "mandelbrot32-q12-submit-not-finished"
    } else if dispatch_delta == 0 {
        "mandelbrot32-q12-no-eu-dispatch"
    } else if hits == 0 {
        "mandelbrot32-q12-no-expected-pixels"
    } else {
        "mandelbrot32-q12-partial-readback"
    };
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-mandelbrot8-strip",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: output_first_before,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: hits,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_mandelbrot_gpu_color_witness_strip(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    scanout_gpu: u64,
    scanout_bytes: usize,
    row_gpu: u64,
    row_virt: *mut u8,
    x_base: usize,
    y: usize,
    phase: usize,
    requested_mode: u32,
    color_seed: u32,
    pilot_groups: u32,
    notify_bytes: usize,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS_PER_PROGRAM: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES;

    let program = gpgpu_primary_scanout_mandelbrot8_gpu_color_program();
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "gpu-color8-gpu-high32-unsupported",
            program,
            row_gpu,
        );
    }
    if row_gpu < scanout_gpu {
        return gpgpu_one_tile_sentinel_failure("gpu-color8-before-scanout", program, row_gpu);
    }
    let row_offset = row_gpu - scanout_gpu;
    if row_offset as usize + PIXELS_PER_PROGRAM * core::mem::size_of::<u32>() > scanout_bytes {
        return gpgpu_one_tile_sentinel_failure("gpu-color8-outside-scanout", program, row_gpu);
    }

    let mut output_first_before = 0;
    let mut before_samples = [0u32; 64];
    if MANDELBROT_LINE1280_VERIFY_SCANOUT_READBACK {
        crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
        output_first_before = unsafe { core::ptr::read_volatile(row_virt as *const u32) };
        let mut sample = 0usize;
        while sample < before_samples.len() {
            before_samples[sample] = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(sample * core::mem::size_of::<u32>()) as *const u32
                )
            };
            sample += 1;
        }
    }

    if !MANDELBROT_LINE1280_TEMPLATE_UPLOADED.load(Ordering::Acquire) {
        let strip_words =
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS;
        let uploaded = if MANDELBROT_LINE1280_VERIFY_PROGRAM_UPLOAD {
            upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words)
        } else {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    strip_words.as_ptr() as *const u8,
                    warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
                    core::mem::size_of_val(&strip_words),
                );
            }
            crate::intel::dma_flush(
                unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
                core::mem::size_of_val(&strip_words),
            );
            true
        };
        if !uploaded {
            return gpgpu_one_tile_sentinel_failure("gpu-color8-program-upload", program, row_gpu);
        }
        MANDELBROT_LINE1280_TEMPLATE_UPLOADED.store(true, Ordering::Release);
    }
    let color_dword = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_DWORD;
    let address_dword = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_ADDRESS_BASE_DWORD;
    unsafe {
        let program_words = warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) as *mut u32;
        core::ptr::write_volatile(program_words.add(color_dword), color_seed);
        core::ptr::write_volatile(program_words.add(address_dword), row_gpu as u32);
    }
    let patch_start_dword = core::cmp::min(color_dword, address_dword);
    let patch_end_dword = core::cmp::max(color_dword, address_dword).saturating_add(1);
    crate::intel::dma_flush(
        unsafe {
            warm.draw_state_virt.add(
                GPGPU_EU_KERNEL_OFFSET_BYTES
                    .saturating_add(patch_start_dword * core::mem::size_of::<u32>()),
            )
        },
        patch_end_dword
            .saturating_sub(patch_start_dword)
            .saturating_mul(core::mem::size_of::<u32>()),
    );

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        scanout_gpu,
        scanout_bytes,
        "stateless-hdc253-primary-scanout-line8-scalar8-witness-quiet",
    );
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch,
        store_surface,
        program,
        pilot_groups.max(1),
    ) {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-gpu-color8-witness",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let mut output_first_after = output_first_before;
    let mut after_color_pixels = if MANDELBROT_LINE1280_VERIFY_SCANOUT_READBACK {
        0
    } else {
        PIXELS_PER_PROGRAM as u32
    };
    let mut hits = 0u64;
    if MANDELBROT_LINE1280_VERIFY_SCANOUT_READBACK {
        let readback_poll_limit = if finished {
            MANDELBROT_STRIP_READBACK_POLLS
        } else {
            1
        };
        let mut readback_poll = 0usize;
        while readback_poll < readback_poll_limit {
            crate::intel::dma_flush(row_virt, PIXELS_PER_PROGRAM * core::mem::size_of::<u32>());
            hits = 0;
            after_color_pixels = 0;
            let mut lane = 0usize;
            while lane < PIXELS_PER_PROGRAM {
                let after = unsafe {
                    core::ptr::read_volatile(
                        row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                    )
                };
                if after == color_seed {
                    after_color_pixels = after_color_pixels.saturating_add(1);
                }
                if lane == 0 {
                    output_first_after = after;
                }
                lane += 1;
            }
            let mut sample = 0usize;
            while sample < before_samples.len() {
                let after = unsafe {
                    core::ptr::read_volatile(
                        row_virt.add(sample * core::mem::size_of::<u32>()) as *const u32
                    )
                };
                if after != before_samples[sample] {
                    hits |= 1u64 << sample;
                }
                sample += 1;
            }
            if after_color_pixels as usize == PIXELS_PER_PROGRAM {
                break;
            }
            readback_poll += 1;
            core::hint::spin_loop();
        }
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let readback_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && (!MANDELBROT_LINE1280_VERIFY_SCANOUT_READBACK
            || after_color_pixels as usize == PIXELS_PER_PROGRAM);
    let display_notified = readback_ok
        && MANDELBROT_LINE1280_NOTIFY_SCANOUT_WRITES
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-gpu-color8-witness",
            row_offset as usize,
            notify_bytes,
        );
    let reason = if readback_ok && !MANDELBROT_LINE1280_VERIFY_SCANOUT_READBACK {
        "gpu-color8-retired-visual-only"
    } else if readback_ok && hits != 0 {
        "gpu-color8-program-changed"
    } else if readback_ok {
        "gpu-color8-program-idempotent"
    } else if !finished {
        "gpu-color8-submit-not-finished"
    } else if dispatch_delta == 0 {
        "gpu-color8-no-eu-dispatch"
    } else if after_color_pixels == 0 {
        "gpu-color8-no-visible-pixels"
    } else if hits == 0 {
        "gpu-color8-program-unchanged"
    } else {
        "gpu-color8-program-partial"
    };
    let should_log = if readback_ok {
        !MANDELBROT_GPU_COLOR_WITNESS_SUCCESS_LOGGED.swap(true, Ordering::AcqRel)
    } else {
        !MANDELBROT_GPU_COLOR_WITNESS_FAILURE_LOGGED.swap(true, Ordering::AcqRel)
    };
    if should_log {
        crate::log!(
            "intel/gpgpu: primary-scanout-line-pilot x_base={} y={} phase={} requested_mode={} row_gpu=0x{:X} color_seed=0x{:08X} setup_dwords=2 cpu_color_dwords_patched=1 cpu_address_dwords_patched=1 pilot_groups={} store_pixels_per_submit={} expected_lane_dispatch=8 after_color_pixels={} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} sample_change_mask=0x{:016X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} pilot_id_required_for_single_fullscreen_submit=1 pilot_id_proven=0 program_source={} color_dword={} address_base_dword={} address_base=0x{:X} deliverable=full-screen-line1280-segment\n",
            x_base,
            y,
            phase,
            requested_mode,
            row_gpu,
            color_seed,
            pilot_groups.max(1),
            PIXELS_PER_PROGRAM,
            after_color_pixels,
            readback_ok as u8,
            reason,
            output_first_before,
            output_first_after,
            hits,
            display_notified as u8,
            notify_bytes,
            finish_marker,
            dispatch_delta,
            program.name,
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_DWORD,
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_ADDRESS_BASE_DWORD,
            row_gpu as u32,
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-gpu-color8-witness",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: output_first_before,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: hits,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn line1280_rect_segment_offset(
    serial_index: usize,
    rect_x: usize,
    rect_y: usize,
    rect_width: usize,
    rect_height: usize,
    target_width: usize,
    target_height: usize,
    target_pitch_bytes: usize,
    target_byte_len: usize,
) -> Result<(usize, usize, usize), &'static str> {
    const LANES_PER_PILOT: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES;

    if rect_width < LANES_PER_PILOT || rect_height == 0 {
        return Err("line-pilot-rect-too-small");
    }
    let rect_x = core::cmp::min(rect_x, target_width.saturating_sub(rect_width));
    let rect_y = core::cmp::min(rect_y, target_height.saturating_sub(rect_height));
    let segments_per_row = rect_width.saturating_add(LANES_PER_PILOT - 1) / LANES_PER_PILOT;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    let segment = serial_index % segments_per_row;
    let y_in_rect = (serial_index / segments_per_row) % rect_height;
    let y = rect_y.saturating_add(y_in_rect);
    let x_in_rect = if segments_per_row <= 1 {
        0
    } else {
        core::cmp::min(
            segment.saturating_mul(LANES_PER_PILOT),
            rect_width.saturating_sub(LANES_PER_PILOT),
        )
    };
    let x_base = rect_x.saturating_add(x_in_rect);
    let row_offset = y
        .saturating_mul(target_pitch_bytes)
        .saturating_add(x_base.saturating_mul(core::mem::size_of::<u32>()));
    let pilot_bytes = LANES_PER_PILOT.saturating_mul(core::mem::size_of::<u32>());
    if row_offset.saturating_add(pilot_bytes) > target_byte_len {
        return Err("line-pilot-outside-scanout");
    }
    Ok((row_offset, x_base, y))
}

fn line1280_lane8rows_rect_segment_offset(
    serial_index: usize,
    rect_x: usize,
    rect_y: usize,
    rect_width: usize,
    rect_height: usize,
    target_width: usize,
    target_height: usize,
    target_pitch_bytes: usize,
    target_byte_len: usize,
) -> Result<(usize, usize, usize), &'static str> {
    const LANES_PER_PILOT: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES;
    const ROWS_PER_PILOT: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_LANE8ROWS_ROWS;

    if rect_width < LANES_PER_PILOT || rect_height == 0 {
        return Err("lane8rows-rect-too-small");
    }
    let rect_x = core::cmp::min(rect_x, target_width.saturating_sub(rect_width));
    let rect_y = core::cmp::min(rect_y, target_height.saturating_sub(rect_height));
    let segments_per_row = rect_width.saturating_add(LANES_PER_PILOT - 1) / LANES_PER_PILOT;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    let row_groups = rect_height.saturating_add(ROWS_PER_PILOT - 1) / ROWS_PER_PILOT;
    let row_groups = core::cmp::max(1, row_groups);
    let segment = serial_index % segments_per_row;
    let row_group = (serial_index / segments_per_row) % row_groups;
    let y_in_rect = row_group.saturating_mul(ROWS_PER_PILOT);
    let y = rect_y.saturating_add(y_in_rect);
    let x_in_rect = if segments_per_row <= 1 {
        0
    } else {
        core::cmp::min(
            segment.saturating_mul(LANES_PER_PILOT),
            rect_width.saturating_sub(LANES_PER_PILOT),
        )
    };
    let x_base = rect_x.saturating_add(x_in_rect);
    if y >= target_height || x_base >= target_width {
        return Err("lane8rows-outside-target");
    }
    let row_offset = y
        .saturating_mul(target_pitch_bytes)
        .saturating_add(x_base.saturating_mul(core::mem::size_of::<u32>()));
    let pilot_bytes = LANES_PER_PILOT.saturating_mul(core::mem::size_of::<u32>());
    let row_span_bytes = (ROWS_PER_PILOT - 1)
        .saturating_mul(target_pitch_bytes)
        .saturating_add(pilot_bytes);
    if row_offset.saturating_add(row_span_bytes) > target_byte_len {
        return Err("lane8rows-outside-scanout");
    }
    Ok((row_offset, x_base, y))
}

fn encode_gfx12_gpgpu_line1280_burst_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
    scanout_gpu: u64,
    target_width: usize,
    target_height: usize,
    target_pitch_bytes: usize,
    target_byte_len: usize,
    first_line_index: usize,
    segment_count: usize,
    rect_x: usize,
    rect_y: usize,
    rect_width: usize,
    rect_height: usize,
    color_seed: u32,
) -> Result<usize, &'static str> {
    const WALKER_AND_MSF_DWORDS: usize = 17;
    const POST_WALKER_FLUSH_DWORDS: usize = 6;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("line1280-burst-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_store_imm32(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        dst: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    if segment_count == 0 {
        return Err("line1280-burst-empty");
    }

    let template_bytes =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch_dwords, store_surface, program, 1)?;
    let template_dwords = template_bytes / core::mem::size_of::<u32>();
    let mut walker_start = None;
    let mut index = 0usize;
    while index < template_dwords {
        if batch_dwords[index] == GPGPU_WALKER_IPEHR_LEN13 {
            walker_start = Some(index);
            break;
        }
        index += 1;
    }
    let walker_start = walker_start.ok_or("line1280-burst-no-walker")?;
    let post_walker_flush_start = walker_start.saturating_add(WALKER_AND_MSF_DWORDS);
    let marker_start = post_walker_flush_start.saturating_add(POST_WALKER_FLUSH_DWORDS);
    let template_end = marker_start.saturating_add(4).saturating_add(2);
    if template_end > template_dwords {
        return Err("line1280-burst-template-short");
    }
    if batch_dwords[marker_start] != MI_STORE_DATA_IMM_GGTT_DW1 {
        return Err("line1280-burst-template-marker");
    }

    let mut walker_and_msf = [0u32; WALKER_AND_MSF_DWORDS];
    let mut post_walker_flush = [0u32; POST_WALKER_FLUSH_DWORDS];
    let mut copy = 0usize;
    while copy < WALKER_AND_MSF_DWORDS {
        walker_and_msf[copy] = batch_dwords[walker_start + copy];
        copy += 1;
    }
    copy = 0;
    while copy < POST_WALKER_FLUSH_DWORDS {
        post_walker_flush[copy] = batch_dwords[post_walker_flush_start + copy];
        copy += 1;
    }

    let color_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_DWORD
                * core::mem::size_of::<u32>()) as u64;
    let address_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_ADDRESS_BASE_DWORD
                * core::mem::size_of::<u32>()) as u64;

    let mut cursor = walker_start;
    push_store_imm32(batch_dwords, &mut cursor, color_gpu, color_seed)?;
    let mut segment = 0usize;
    while segment < segment_count {
        let serial = first_line_index.saturating_add(segment);
        let (row_offset, _x_base, _y) = line1280_rect_segment_offset(
            serial,
            rect_x,
            rect_y,
            rect_width,
            rect_height,
            target_width,
            target_height,
            target_pitch_bytes,
            target_byte_len,
        )?;
        let row_gpu = scanout_gpu.saturating_add(row_offset as u64);
        if row_gpu >> 32 != 0 {
            return Err("line1280-burst-gpu-high32-unsupported");
        }
        push_store_imm32(batch_dwords, &mut cursor, address_gpu, row_gpu as u32)?;
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
        copy = 0;
        while copy < WALKER_AND_MSF_DWORDS {
            push(batch_dwords, &mut cursor, walker_and_msf[copy])?;
            copy += 1;
        }
        copy = 0;
        while copy < POST_WALKER_FLUSH_DWORDS {
            push(batch_dwords, &mut cursor, post_walker_flush[copy])?;
            copy += 1;
        }
        segment += 1;
    }

    let marker_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    push_store_imm32(batch_dwords, &mut cursor, marker_gpu, RCS_EXEC_RESULT_COMPUTE_WALKER_DONE)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot_rect_color_burst(
    color_seed: u32,
    first_line_index: u32,
    segment_count: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_primary_scanout_mandelbrot8_gpu_color_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if segment_count == 0 {
        return gpgpu_one_tile_sentinel_failure("line1280-burst-empty", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let rect_width = core::cmp::min(rect_width as usize, target.width as usize);
    let rect_height = core::cmp::min(rect_height as usize, target.height as usize);
    let rect_x =
        core::cmp::min(rect_x as usize, (target.width as usize).saturating_sub(rect_width));
    let rect_y =
        core::cmp::min(rect_y as usize, (target.height as usize).saturating_sub(rect_height));
    let first_serial = first_line_index as usize;
    let (first_row_offset, first_x, first_y) = match line1280_rect_segment_offset(
        first_serial,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
        target.width as usize,
        target.height as usize,
        target.pitch_bytes as usize,
        target.byte_len,
    ) {
        Ok(offset) => offset,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, target.gpu),
    };
    let first_segment_color = color_seed;

    if !MANDELBROT_LINE1280_TEMPLATE_UPLOADED.load(Ordering::Acquire) {
        let strip_words =
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS;
        let uploaded = if MANDELBROT_LINE1280_VERIFY_PROGRAM_UPLOAD {
            upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words)
        } else {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    strip_words.as_ptr() as *const u8,
                    warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES),
                    core::mem::size_of_val(&strip_words),
                );
            }
            crate::intel::dma_flush(
                unsafe { warm.draw_state_virt.add(GPGPU_EU_KERNEL_OFFSET_BYTES) },
                core::mem::size_of_val(&strip_words),
            );
            true
        };
        if !uploaded {
            return gpgpu_one_tile_sentinel_failure(
                "line1280-burst-program-upload",
                program,
                target.gpu,
            );
        }
        MANDELBROT_LINE1280_TEMPLATE_UPLOADED.store(true, Ordering::Release);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "stateless-hdc253-primary-scanout-line1280-burst",
    );
    let segment_count = segment_count as usize;
    let batch_bytes = match encode_gfx12_gpgpu_line1280_burst_batch(
        warm,
        batch,
        store_surface,
        program,
        target.gpu,
        target.width as usize,
        target.height as usize,
        target.pitch_bytes as usize,
        target.byte_len,
        first_serial,
        segment_count,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
        color_seed,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, target.gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-line1280-burst",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let readback_ok = finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok {
        "line1280-burst-retired-visual-only"
    } else if !finished {
        "line1280-burst-submit-not-finished"
    } else if dispatch_delta == 0 {
        "line1280-burst-no-eu-dispatch"
    } else {
        "line1280-burst-marker-missing"
    };
    let should_log = if readback_ok {
        !MANDELBROT_LINE1280_BURST_SUCCESS_LOGGED.swap(true, Ordering::AcqRel)
    } else {
        !MANDELBROT_LINE1280_BURST_FAILURE_LOGGED.swap(true, Ordering::AcqRel)
    };
    if should_log {
        crate::log!(
            "intel/gpgpu: primary-scanout-line1280-burst first_serial={} segments={} first_x={} first_y={} rect={}x{}@{},{} base_color_seed=0x{:08X} first_segment_color=0x{:08X} segment_seed_pattern=scalar-line-color-seed artifact_color_step_pixels={} artifact_color_step=0x00010101 cpu_frame_color_params=1 cpu_segment_address_params={} cpu_batch_param_dwords={} store_pixels_per_segment={} rows_per_segment={} expected_lane_dispatch={} readback_ok={} reason={} finish_marker=0x{:08X} lane_dispatch_delta={} program_source={} deliverable=visible-window-line1280-scalar-baseline-burst\n",
            first_serial,
            segment_count,
            first_x,
            first_y,
            rect_width,
            rect_height,
            rect_x,
            rect_y,
            color_seed,
            first_segment_color,
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_COLOR_STEP_PIXELS,
            segment_count,
            segment_count.saturating_add(1),
            trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_SCALAR_BW_LANES,
            1,
            segment_count.saturating_mul(8),
            readback_ok as u8,
            reason,
            finish_marker,
            dispatch_delta,
            program.name,
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-line1280-burst",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: target.gpu + first_row_offset as u64,
        sentinel: 0,
        output_first_before: 0,
        output_first_after: first_segment_color,
        output_nonzero_before: 0,
        output_nonzero_after: (first_segment_color != 0) as usize,
        output_hits_lo64: segment_count.min(u64::MAX as usize) as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn line1280_groupid_rows_rect_base_offset(
    first_row_group: usize,
    x_segment: usize,
    row_group_count: usize,
    rect_x: usize,
    rect_y: usize,
    rect_width: usize,
    rect_height: usize,
    target_width: usize,
    target_height: usize,
    target_pitch_bytes: usize,
    target_byte_len: usize,
) -> Result<(usize, usize, usize), &'static str> {
    const LANES_PER_GROUP: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES;

    if rect_width < LANES_PER_GROUP || rect_height == 0 || row_group_count == 0 {
        return Err("groupid-line1280-rect-too-small");
    }
    if first_row_group >= rect_height {
        return Err("groupid-line1280-row-outside-rect");
    }
    let rect_x = core::cmp::min(rect_x, target_width.saturating_sub(rect_width));
    let rect_y = core::cmp::min(rect_y, target_height.saturating_sub(rect_height));
    let segments_per_row = rect_width.saturating_add(LANES_PER_GROUP - 1) / LANES_PER_GROUP;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    let segment = x_segment % segments_per_row;
    let x_in_rect = if segments_per_row <= 1 {
        0
    } else {
        core::cmp::min(
            segment.saturating_mul(LANES_PER_GROUP),
            rect_width.saturating_sub(LANES_PER_GROUP),
        )
    };
    let x_base = rect_x.saturating_add(x_in_rect);
    let y = rect_y.saturating_add(first_row_group);
    if y >= target_height || x_base >= target_width {
        return Err("groupid-line1280-base-outside-target");
    }
    let row_offset = y
        .saturating_mul(target_pitch_bytes)
        .saturating_add(x_base.saturating_mul(core::mem::size_of::<u32>()));
    let row_span = row_group_count
        .saturating_sub(1)
        .saturating_mul(target_pitch_bytes)
        .saturating_add(LANES_PER_GROUP.saturating_mul(core::mem::size_of::<u32>()));
    if row_offset.saturating_add(row_span) > target_byte_len {
        return Err("groupid-line1280-outside-scanout");
    }
    Ok((row_offset, x_base, y))
}

fn encode_gfx12_gpgpu_line1280_groupid_rows_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
    base_gpu: u64,
    second_base_gpu: Option<u64>,
    color_seed: u32,
    row_group_count: u32,
    completion_marker: u32,
) -> Result<usize, &'static str> {
    const WALKER_AND_MSF_DWORDS: usize = 17;
    const POST_WALKER_FLUSH_DWORDS: usize = 6;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("groupid-line1280-batch-exhausted");
        }
        batch_dwords[*cursor] = value;
        *cursor += 1;
        Ok(())
    }

    fn push_store_imm32(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        dst: u64,
        value: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, MI_STORE_DATA_IMM_GGTT_DW1)?;
        push(batch_dwords, cursor, dst as u32)?;
        push(batch_dwords, cursor, (dst >> 32) as u32)?;
        push(batch_dwords, cursor, value)
    }

    fn push_pipe_control(
        batch_dwords: &mut [u32],
        cursor: &mut usize,
        flags: u32,
    ) -> Result<(), &'static str> {
        push(batch_dwords, cursor, PIPE_CONTROL_CMD)?;
        push(batch_dwords, cursor, flags)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)?;
        push(batch_dwords, cursor, 0)
    }

    if row_group_count == 0 {
        return Err("groupid-line1280-empty");
    }
    if base_gpu >> 32 != 0 {
        return Err("groupid-line1280-gpu-high32-unsupported");
    }
    if let Some(second_base_gpu) = second_base_gpu
        && second_base_gpu >> 32 != 0
    {
        return Err("groupid-line1280-second-gpu-high32-unsupported");
    }

    let template_bytes = encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch_dwords,
        store_surface,
        program,
        row_group_count,
    )?;
    let template_dwords = template_bytes / core::mem::size_of::<u32>();
    let mut walker_start = None;
    let mut index = 0usize;
    while index < template_dwords {
        if batch_dwords[index] == GPGPU_WALKER_IPEHR_LEN13 {
            walker_start = Some(index);
            break;
        }
        index += 1;
    }
    let walker_start = walker_start.ok_or("groupid-line1280-no-walker")?;
    let post_walker_flush_start = walker_start.saturating_add(WALKER_AND_MSF_DWORDS);
    let marker_start = post_walker_flush_start.saturating_add(POST_WALKER_FLUSH_DWORDS);
    let template_end = marker_start.saturating_add(4).saturating_add(2);
    if template_end > template_dwords {
        return Err("groupid-line1280-template-short");
    }
    if batch_dwords[marker_start] != MI_STORE_DATA_IMM_GGTT_DW1 {
        return Err("groupid-line1280-template-marker");
    }

    let mut walker_and_msf = [0u32; WALKER_AND_MSF_DWORDS];
    let mut copy = 0usize;
    while copy < WALKER_AND_MSF_DWORDS {
        walker_and_msf[copy] = batch_dwords[walker_start + copy];
        copy += 1;
    }

    let color_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_DWORD
                * core::mem::size_of::<u32>()) as u64;
    let address_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_ADDRESS_BASE_DWORD
                * core::mem::size_of::<u32>()) as u64;

    let mut cursor = walker_start;
    push_store_imm32(batch_dwords, &mut cursor, color_gpu, color_seed)?;
    push_store_imm32(batch_dwords, &mut cursor, address_gpu, base_gpu as u32)?;
    push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
    copy = 0;
    while copy < WALKER_AND_MSF_DWORDS {
        push(batch_dwords, &mut cursor, walker_and_msf[copy])?;
        copy += 1;
    }
    if let Some(second_base_gpu) = second_base_gpu {
        push_store_imm32(batch_dwords, &mut cursor, address_gpu, second_base_gpu as u32)?;
        push_pipe_control(batch_dwords, &mut cursor, PIPE_CONTROL_INVALIDATE_BITS)?;
        copy = 0;
        while copy < WALKER_AND_MSF_DWORDS {
            push(batch_dwords, &mut cursor, walker_and_msf[copy])?;
            copy += 1;
        }
    }

    let marker_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    push_store_imm32(batch_dwords, &mut cursor, marker_gpu, completion_marker)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

pub(crate) fn submit_gpgpu_primary_scanout_line1280_groupid_rows_color_burst(
    color_seed: u32,
    first_row_group: u32,
    row_group_count: u32,
    x_segment: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_primary_scanout_groupid_line1280_rows_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_group_count == 0 {
        return gpgpu_one_tile_sentinel_failure("groupid-line1280-empty", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let rect_width = core::cmp::min(rect_width as usize, target.width as usize);
    let rect_height = core::cmp::min(rect_height as usize, target.height as usize);
    let rect_x =
        core::cmp::min(rect_x as usize, (target.width as usize).saturating_sub(rect_width));
    let rect_y =
        core::cmp::min(rect_y as usize, (target.height as usize).saturating_sub(rect_height));
    let first_row_group = first_row_group as usize;
    let available_rows = rect_height.saturating_sub(core::cmp::min(first_row_group, rect_height));
    let row_group_count = core::cmp::min(row_group_count as usize, available_rows);
    let (first_row_offset, first_x, first_y) = match line1280_groupid_rows_rect_base_offset(
        first_row_group,
        x_segment as usize,
        row_group_count,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
        target.width as usize,
        target.height as usize,
        target.pitch_bytes as usize,
        target.byte_len,
    ) {
        Ok(offset) => offset,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, target.gpu),
    };
    let base_gpu = target.gpu + first_row_offset as u64;
    if base_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line1280-gpu-high32-unsupported",
            program,
            base_gpu,
        );
    }

    ensure_primary_scanout_groupid_line1280_rows_artifact_uploaded(warm);

    let (batch_bytes, completion_marker) =
        match prepare_primary_scanout_groupid_line1280_rows_command_stream(
            warm,
            target.gpu,
            target.byte_len,
            "stateless-hdc253-primary-scanout-groupid-line1280-rows",
            program,
            base_gpu,
            None,
            color_seed,
            row_group_count as u32,
        ) {
            Ok(values) => values,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, base_gpu),
        };

    let submit_proof = submit_warm_render_batch_observed(
        dev,
        warm,
        completion_marker,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-groupid-line1280-rows",
        true,
    );
    let finished = submit_proof.completed;
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let dispatch_delta = submit_proof
        .dispatch_after
        .saturating_sub(submit_proof.dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_lane_dispatch =
        row_group_count.saturating_mul(groupid_line1280_expected_dispatch_lanes());
    let dispatch_contract = groupid_line1280_dispatch_contract();
    let proves = groupid_line1280_proves();
    let readback_ok = finished
        && finish_marker == completion_marker
        && dispatch_delta >= expected_lane_dispatch as u64;
    let reason = if readback_ok {
        "groupid-line1280-burst-retired-visual-only"
    } else if !finished {
        "groupid-line1280-submit-not-finished"
    } else if dispatch_delta == 0 {
        "groupid-line1280-no-eu-dispatch"
    } else {
        "groupid-line1280-marker-missing"
    };
    let should_log = if readback_ok {
        !MANDELBROT_GROUPID_LINE1280_BURST_SUCCESS_LOGGED.swap(true, Ordering::AcqRel)
    } else {
        !MANDELBROT_GROUPID_LINE1280_BURST_FAILURE_LOGGED.swap(true, Ordering::AcqRel)
    };
    if should_log {
        crate::log!(
            "intel/gpgpu: primary-scanout-groupid-line1280-rows first_row_group={} row_groups={} x_segment={} first_x={} first_y={} rect={}x{}@{},{} base_color_seed=0x{:08X} cpu_frame_color_params=1 cpu_burst_address_params=1 cpu_row_address_params=0 artifact_body={} payload_contract={} dispatch_contract={} artifact_pitch_bytes=0x{:X} artifact_color_step_pixels={} walker_groups={} store_pixels_per_group={} expected_store_pixels={} expected_lane_dispatch={} proves={} does_not_prove={} readback_ok={} reason={} finish_marker=0x{:08X} lane_dispatch_delta={} dispatch_before={} dispatch_after={} program_source={} color_dword={} address_base_dword={} address_base=0x{:X} deliverable=visible-window-line1280-groupid-row-burst\n",
            first_row_group,
            row_group_count,
            x_segment,
            first_x,
            first_y,
            rect_width,
            rect_height,
            rect_x,
            rect_y,
            color_seed,
            MANDELBROT_GROUPID_LINE1280_ARTIFACT_BODY,
            MANDELBROT_GROUPID_LINE1280_PAYLOAD_CONTRACT,
            dispatch_contract,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PITCH_BYTES,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_PIXELS,
            row_group_count,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES,
            row_group_count.saturating_mul(
                trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES
            ),
            expected_lane_dispatch,
            proves,
            MANDELBROT_GROUPID_LINE1280_DOES_NOT_PROVE,
            readback_ok as u8,
            reason,
            finish_marker,
            dispatch_delta,
            submit_proof.dispatch_before,
            submit_proof.dispatch_after,
            program.name,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_DWORD,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_ADDRESS_BASE_DWORD,
            base_gpu as u32,
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-groupid-line1280-rows",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: base_gpu,
        sentinel: 0,
        output_first_before: 0,
        output_first_after: color_seed,
        output_nonzero_before: 0,
        output_nonzero_after: (color_seed != 0) as usize,
        output_hits_lo64: row_group_count.min(u64::MAX as usize) as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: completion_marker,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_line1280_groupid_rows_fullwidth_color_burst(
    color_seed: u32,
    first_row_group: u32,
    row_group_count: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_primary_scanout_groupid_line1280_rows_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if row_group_count == 0 {
        return gpgpu_one_tile_sentinel_failure("groupid-line1280-full-empty", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let rect_width = core::cmp::min(rect_width as usize, target.width as usize);
    let rect_height = core::cmp::min(rect_height as usize, target.height as usize);
    let rect_x =
        core::cmp::min(rect_x as usize, (target.width as usize).saturating_sub(rect_width));
    let rect_y =
        core::cmp::min(rect_y as usize, (target.height as usize).saturating_sub(rect_height));
    let first_row_group = first_row_group as usize;
    let available_rows = rect_height.saturating_sub(core::cmp::min(first_row_group, rect_height));
    let row_group_count = core::cmp::min(row_group_count as usize, available_rows);
    let lanes_per_segment = trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_LANES;
    let segments_per_row = rect_width.saturating_add(lanes_per_segment - 1) / lanes_per_segment;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    if segments_per_row > 2 {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line1280-full-too-wide",
            program,
            target.gpu,
        );
    }

    let (first_row_offset, first_x, first_y) = match line1280_groupid_rows_rect_base_offset(
        first_row_group,
        0,
        row_group_count,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
        target.width as usize,
        target.height as usize,
        target.pitch_bytes as usize,
        target.byte_len,
    ) {
        Ok(offset) => offset,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, target.gpu),
    };
    let second_row_offset = if segments_per_row > 1 {
        match line1280_groupid_rows_rect_base_offset(
            first_row_group,
            1,
            row_group_count,
            rect_x,
            rect_y,
            rect_width,
            rect_height,
            target.width as usize,
            target.height as usize,
            target.pitch_bytes as usize,
            target.byte_len,
        ) {
            Ok((offset, _, _)) => Some(offset),
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, target.gpu),
        }
    } else {
        None
    };
    let base_gpu = target.gpu + first_row_offset as u64;
    let second_base_gpu = second_row_offset.map(|offset| target.gpu + offset as u64);
    if base_gpu >> 32 != 0 || second_base_gpu.is_some_and(|gpu| gpu >> 32 != 0) {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line1280-full-gpu-high32-unsupported",
            program,
            base_gpu,
        );
    }

    ensure_primary_scanout_groupid_line1280_rows_artifact_uploaded(warm);

    let (batch_bytes, completion_marker) =
        match prepare_primary_scanout_groupid_line1280_rows_command_stream(
            warm,
            target.gpu,
            target.byte_len,
            "stateless-hdc253-primary-scanout-groupid-line1280-fullwidth",
            program,
            base_gpu,
            second_base_gpu,
            color_seed,
            row_group_count as u32,
        ) {
            Ok(values) => values,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, base_gpu),
        };

    let submit_proof = submit_warm_render_batch_observed(
        dev,
        warm,
        completion_marker,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-groupid-line1280-fullwidth",
        true,
    );
    let finished = submit_proof.completed;
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    let dispatch_delta = submit_proof
        .dispatch_after
        .saturating_sub(submit_proof.dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let expected_lane_dispatch = row_group_count
        .saturating_mul(groupid_line1280_expected_dispatch_lanes())
        .saturating_mul(segments_per_row);
    let dispatch_contract = groupid_line1280_dispatch_contract();
    let proves = groupid_line1280_proves();
    let readback_ok = finished
        && finish_marker == completion_marker
        && dispatch_delta >= expected_lane_dispatch as u64;
    let reason = if readback_ok {
        "groupid-line1280-fullwidth-retired-visual-only"
    } else if !finished {
        "groupid-line1280-fullwidth-submit-not-finished"
    } else if dispatch_delta == 0 {
        "groupid-line1280-fullwidth-no-eu-dispatch"
    } else {
        "groupid-line1280-fullwidth-marker-missing"
    };
    let should_log = if readback_ok {
        !MANDELBROT_GROUPID_LINE1280_BURST_SUCCESS_LOGGED.swap(true, Ordering::AcqRel)
    } else {
        !MANDELBROT_GROUPID_LINE1280_BURST_FAILURE_LOGGED.swap(true, Ordering::AcqRel)
    };
    if should_log {
        crate::log!(
            "intel/gpgpu: primary-scanout-groupid-line1280-fullwidth first_row_group={} row_groups={} segments_per_row={} first_x={} first_y={} rect={}x{}@{},{} base_color_seed=0x{:08X} cpu_frame_color_params=1 cpu_burst_address_params={} cpu_row_address_params=0 artifact_body={} payload_contract={} dispatch_contract={} artifact_pitch_bytes=0x{:X} artifact_color_step_pixels={} walker_groups_per_segment={} store_pixels_per_group={} expected_store_pixels={} expected_lane_dispatch={} proves={} does_not_prove={} readback_ok={} reason={} finish_marker=0x{:08X} lane_dispatch_delta={} dispatch_before={} dispatch_after={} program_source={} color_dword={} address_base_dword={} address_base=0x{:X} second_address_base=0x{:X} deliverable=visible-window-line1280-groupid-fullwidth-burst\n",
            first_row_group,
            row_group_count,
            segments_per_row,
            first_x,
            first_y,
            rect_width,
            rect_height,
            rect_x,
            rect_y,
            color_seed,
            segments_per_row,
            MANDELBROT_GROUPID_LINE1280_ARTIFACT_BODY,
            MANDELBROT_GROUPID_LINE1280_PAYLOAD_CONTRACT,
            dispatch_contract,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_PITCH_BYTES,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_STEP_PIXELS,
            row_group_count,
            lanes_per_segment.saturating_mul(segments_per_row),
            row_group_count
                .saturating_mul(lanes_per_segment)
                .saturating_mul(segments_per_row),
            expected_lane_dispatch,
            proves,
            MANDELBROT_GROUPID_LINE1280_DOES_NOT_PROVE,
            readback_ok as u8,
            reason,
            finish_marker,
            dispatch_delta,
            submit_proof.dispatch_before,
            submit_proof.dispatch_after,
            program.name,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_COLOR_DWORD,
            trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE1280_ROWS_SCALAR_BW_ADDRESS_BASE_DWORD,
            base_gpu as u32,
            second_base_gpu.unwrap_or(0) as u32,
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-groupid-line1280-fullwidth",
        );
    }

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: base_gpu,
        sentinel: 0,
        output_first_before: 0,
        output_first_after: color_seed,
        output_nonzero_before: 0,
        output_nonzero_after: (color_seed != 0) as usize,
        output_hits_lo64: row_group_count.min(u64::MAX as usize) as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: completion_marker,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_groupid_line320_probe(
    mode: u32,
    row_index: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const GROUPS: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_GROUPS;
    const PIXELS_PER_GROUP: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_LANES;
    const GROUP_STRIDE_BYTES: usize =
        1usize << trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_STRIDE_SHIFT;
    const GROUP_MASK: u32 = (1u32 << GROUPS) - 1;
    const SAMPLE_A: usize = 0;
    const SAMPLE_B: usize = PIXELS_PER_GROUP / 2;
    const SAMPLE_C: usize = PIXELS_PER_GROUP - 1;

    let program = gpgpu_primary_scanout_groupid_line320_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = if target.height == 0 {
        0
    } else {
        row_index % target.height
    } as usize;
    let row_offset = y.saturating_mul(target.pitch_bytes as usize);
    let group_bytes = PIXELS_PER_GROUP.saturating_mul(core::mem::size_of::<u32>());
    let probe_bytes = GROUPS
        .saturating_sub(1)
        .saturating_mul(GROUP_STRIDE_BYTES)
        .saturating_add(group_bytes);
    if row_offset.saturating_add(probe_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line320-outside-scanout",
            program,
            target.gpu,
        );
    }
    let base_gpu = target.gpu + row_offset as u64;
    if base_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line320-gpu-high32-unsupported",
            program,
            base_gpu,
        );
    }
    let base_virt = unsafe { target.virt.add(row_offset) };
    let requested_mode = mode & 1;
    let color_seed = if requested_mode == 0 {
        0x0000_0000
    } else {
        0x00FF_FFFF
    };

    crate::intel::dma_flush(base_virt, probe_bytes);
    let mut before_a = [0u32; GROUPS];
    let mut before_b = [0u32; GROUPS];
    let mut before_c = [0u32; GROUPS];
    let mut group = 0usize;
    while group < GROUPS {
        let group_virt = unsafe { base_virt.add(group.saturating_mul(GROUP_STRIDE_BYTES)) };
        before_a[group] = unsafe {
            core::ptr::read_volatile(
                group_virt.add(SAMPLE_A * core::mem::size_of::<u32>()) as *const u32
            )
        };
        before_b[group] = unsafe {
            core::ptr::read_volatile(
                group_virt.add(SAMPLE_B * core::mem::size_of::<u32>()) as *const u32
            )
        };
        before_c[group] = unsafe {
            core::ptr::read_volatile(
                group_virt.add(SAMPLE_C * core::mem::size_of::<u32>()) as *const u32
            )
        };
        group += 1;
    }
    let output_first_before = before_a[0];

    let mut strip_words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS;
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_COLOR_DWORD] =
        color_seed;
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_ADDRESS_BASE_DWORD] =
        base_gpu as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words) {
        return gpgpu_one_tile_sentinel_failure(
            "groupid-line320-program-upload",
            program,
            base_gpu,
        );
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "stateless-hdc253-primary-scanout-groupid-line320-quiet",
    );
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch,
        store_surface,
        program,
        GROUPS as u32,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, base_gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-groupid-line320",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let readback_poll_limit = if finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else {
        1
    };
    let mut readback_poll = 0usize;
    let mut group_hit_mask = 0u32;
    let mut group_color_mask = 0u32;
    let mut after_color_pixels = 0usize;
    let mut output_first_after = output_first_before;
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(base_virt, probe_bytes);
        group_hit_mask = 0;
        group_color_mask = 0;
        after_color_pixels = 0;
        group = 0;
        while group < GROUPS {
            let group_virt = unsafe { base_virt.add(group.saturating_mul(GROUP_STRIDE_BYTES)) };
            let after_a = unsafe {
                core::ptr::read_volatile(
                    group_virt.add(SAMPLE_A * core::mem::size_of::<u32>()) as *const u32
                )
            };
            let after_b = unsafe {
                core::ptr::read_volatile(
                    group_virt.add(SAMPLE_B * core::mem::size_of::<u32>()) as *const u32
                )
            };
            let after_c = unsafe {
                core::ptr::read_volatile(
                    group_virt.add(SAMPLE_C * core::mem::size_of::<u32>()) as *const u32
                )
            };
            if group == 0 {
                output_first_after = after_a;
            }
            if after_a == color_seed
                && after_b == color_seed
                && after_c == color_seed
                && (after_a != before_a[group]
                    || after_b != before_b[group]
                    || after_c != before_c[group])
            {
                group_hit_mask |= 1u32 << group;
            }

            let mut pixel = 0usize;
            let mut group_color_pixels = 0usize;
            while pixel < PIXELS_PER_GROUP {
                let after = unsafe {
                    core::ptr::read_volatile(
                        group_virt.add(pixel * core::mem::size_of::<u32>()) as *const u32
                    )
                };
                if after == color_seed {
                    group_color_pixels = group_color_pixels.saturating_add(1);
                }
                pixel += 1;
            }
            after_color_pixels = after_color_pixels.saturating_add(group_color_pixels);
            if group_color_pixels == PIXELS_PER_GROUP {
                group_color_mask |= 1u32 << group;
            }
            group += 1;
        }
        if group_hit_mask == GROUP_MASK && group_color_mask == GROUP_MASK {
            break;
        }
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let readback_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && group_hit_mask == GROUP_MASK
        && group_color_mask == GROUP_MASK;
    let display_notified = readback_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-groupid-line320",
            row_offset,
            probe_bytes,
        );
    let reason = if readback_ok {
        "groupid-line320-all-groups-changed"
    } else if !finished {
        "groupid-line320-submit-not-finished"
    } else if dispatch_delta == 0 {
        "groupid-line320-no-eu-dispatch"
    } else if group_hit_mask == 1 || group_color_mask == 1 {
        "groupid-line320-collapsed-to-group0"
    } else if group_hit_mask == 0 && group_color_mask == 0 {
        "groupid-line320-no-visible-groups"
    } else {
        "groupid-line320-partial-visible-groups"
    };

    crate::log!(
        "intel/gpgpu: primary-scanout-groupid-line320 y={} requested_mode={} base_gpu=0x{:X} color_seed=0x{:08X} walker_groups={} group_stride_bytes=0x{:X} block_pixels={} expected_store_pixels={} after_color_pixels={} group_hit_mask=0x{:02X} group_color_mask=0x{:02X} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} expected_lane_dispatch={} program_source={} color_dword={} address_base_dword={} address_base=0x{:X} contract=workgroup_id_g0_1_direct deliverable=one-submit-multigroup-visible-blocks\n",
        y,
        requested_mode,
        base_gpu,
        color_seed,
        GROUPS,
        GROUP_STRIDE_BYTES,
        PIXELS_PER_GROUP,
        GROUPS * PIXELS_PER_GROUP,
        after_color_pixels,
        group_hit_mask,
        group_color_mask,
        readback_ok as u8,
        reason,
        output_first_before,
        output_first_after,
        display_notified as u8,
        probe_bytes,
        finish_marker,
        dispatch_delta,
        GROUPS as u64 * GPGPU_WALKER_SIMD8_LANES as u64,
        program.name,
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_COLOR_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_GROUPID_LINE320_SCALAR_BW_ADDRESS_BASE_DWORD,
        base_gpu as u32,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-groupid-line320",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: base_gpu,
        sentinel: color_seed,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: group_hit_mask as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_row2560_simd8_probe(
    mode: u32,
    row_index: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const ROW_PIXELS: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_PIXELS;
    const ROW_BYTES: usize = ROW_PIXELS * core::mem::size_of::<u32>();
    const SAMPLE_COUNT: usize = 8;

    let program = gpgpu_primary_scanout_row2560_simd8_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if target.width as usize != ROW_PIXELS {
        return gpgpu_one_tile_sentinel_failure("row2560-width-mismatch", program, target.gpu);
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = if target.height == 0 {
        0
    } else {
        row_index % target.height
    } as usize;
    let row_offset = y.saturating_mul(target.pitch_bytes as usize);
    if row_offset.saturating_add(ROW_BYTES) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("row2560-outside-scanout", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("row2560-gpu-high32", program, row_gpu);
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    let requested_mode = mode & 1;
    let color_seed = if requested_mode == 0 {
        0x0000_0000
    } else {
        0x00FF_FFFF
    };

    crate::intel::dma_flush(row_virt, ROW_BYTES);
    let mut before_samples = [0u32; SAMPLE_COUNT];
    let mut sample = 0usize;
    while sample < SAMPLE_COUNT {
        let pixel = sample * (ROW_PIXELS / SAMPLE_COUNT);
        before_samples[sample] = unsafe {
            core::ptr::read_volatile(row_virt.add(pixel * core::mem::size_of::<u32>()) as *const u32)
        };
        sample += 1;
    }
    let output_first_before = before_samples[0];

    let mut strip_words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_HDC1_STATELESS_UNROLLED_STORE_THEN_TS_EOT_WORDS;
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_COLOR_DWORD] = color_seed;
    strip_words[trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_ADDRESS_BASE_DWORD] =
        row_offset as u32;
    if !upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &strip_words) {
        return gpgpu_one_tile_sentinel_failure("row2560-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "stateless-primary-scanout-row2560-simd8-quiet",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-row2560-simd8",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let readback_poll_limit = if finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else {
        1
    };
    let mut readback_poll = 0usize;
    let mut after_color_pixels = 0usize;
    let mut sample_change_mask = 0u64;
    let mut output_first_after = output_first_before;
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, ROW_BYTES);
        after_color_pixels = 0;
        sample_change_mask = 0;
        let mut pixel = 0usize;
        while pixel < ROW_PIXELS {
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(pixel * core::mem::size_of::<u32>()) as *const u32
                )
            };
            if pixel == 0 {
                output_first_after = after;
            }
            if after == color_seed {
                after_color_pixels = after_color_pixels.saturating_add(1);
            }
            pixel += 1;
        }
        sample = 0;
        while sample < SAMPLE_COUNT {
            let pixel = sample * (ROW_PIXELS / SAMPLE_COUNT);
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(pixel * core::mem::size_of::<u32>()) as *const u32
                )
            };
            if after != before_samples[sample] {
                sample_change_mask |= 1u64 << sample;
            }
            sample += 1;
        }
        if after_color_pixels == ROW_PIXELS {
            break;
        }
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let readback_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && after_color_pixels == ROW_PIXELS
        && sample_change_mask != 0;
    let display_notified = readback_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-row2560-simd8",
            row_offset,
            ROW_BYTES,
        );
    let reason = if readback_ok {
        "row2560-simd8-full-row-changed"
    } else if !finished {
        "row2560-simd8-submit-not-finished"
    } else if dispatch_delta == 0 {
        "row2560-simd8-no-eu-dispatch"
    } else if after_color_pixels == 0 {
        "row2560-simd8-no-visible-pixels"
    } else if sample_change_mask == 0 {
        "row2560-simd8-unchanged"
    } else {
        "row2560-simd8-partial-row"
    };

    crate::log!(
        "intel/gpgpu: primary-scanout-row2560-simd8 y={} requested_mode={} row_offset=0x{:X} row_gpu=0x{:X} color_seed=0x{:08X} setup_dwords=2 cpu_color_dwords_patched=1 cpu_address_dwords_patched=1 walker_groups=1 simd8_sends={} expected_store_pixels={} after_color_pixels={} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} sample_change_mask=0x{:016X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} expected_lane_dispatch=8 program_source={} color_dword={} address_base_dword={} address_base=0x{:X} contract=one-submit-full-row-simd8-bti-offsets deliverable=full-width-visible-row\n",
        y,
        requested_mode,
        row_offset,
        row_gpu,
        color_seed,
        trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_SENDS,
        ROW_PIXELS,
        after_color_pixels,
        readback_ok as u8,
        reason,
        output_first_before,
        output_first_after,
        sample_change_mask,
        display_notified as u8,
        ROW_BYTES,
        finish_marker,
        dispatch_delta,
        program.name,
        trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_COLOR_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_ROW2560_SIMD8_BW_ADDRESS_BASE_DWORD,
        row_offset as u32,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-row2560-simd8",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: color_seed,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: sample_change_mask,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe(
    mode: u32,
    row_index: u32,
    x_base: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    const EXPECTED_HW_LANE_DISPATCH: u64 =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64;

    let program = gpgpu_primary_scanout_mandelbrot16_simd16_bw_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let y = if target.height == 0 {
        0
    } else {
        row_index % target.height
    } as usize;
    let x = core::cmp::min(x_base as usize, (target.width as usize).saturating_sub(PIXELS));
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    if row_offset.saturating_add(BYTES) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-outside-scanout",
            program,
            target.gpu,
        );
    }
    if row_offset >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-offset-high32", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-gpu-high32", program, row_gpu);
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    crate::intel::dma_flush(row_virt, BYTES);
    let mut before_words = [0u32; PIXELS];
    let mut lane = 0usize;
    while lane < PIXELS {
        before_words[lane] = unsafe {
            core::ptr::read_volatile(row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32)
        };
        lane += 1;
    }
    let output_first_before = before_words[0];
    let expected_first = mandelbrot16_simd16_probe_expected_first(mode);
    let patched_color = 0;

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        patched_color,
        mode,
    ) {
        return gpgpu_one_tile_sentinel_failure("mandelbrot16-program-upload", program, row_gpu);
    }

    unsafe {
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        "stateless-primary-scanout-mandelbrot16-simd16-mul-witness",
    );
    let batch_bytes =
        match encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1) {
            Ok(bytes) => bytes,
            Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
        };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-primary-scanout-mandelbrot16-simd16-mul-witness",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let readback_poll_limit = if finished {
        MANDELBROT_STRIP_READBACK_POLLS
    } else {
        1
    };
    let mut readback_poll = 0usize;
    let mut hits = 0u64;
    let mut changed = 0u64;
    let mut after_words = [0u32; PIXELS];
    let mut output_first_after = output_first_before;
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, BYTES);
        hits = 0;
        changed = 0;
        lane = 0;
        while lane < PIXELS {
            let after = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            after_words[lane] = after;
            if lane == 0 {
                output_first_after = after;
            }
            if lane == 0 && after == expected_first {
                hits |= 1u64 << lane;
            }
            if after != before_words[lane] {
                changed |= 1u64 << lane;
            }
            lane += 1;
        }
        if hits & 1 != 0 {
            break;
        }
        readback_poll += 1;
        core::hint::spin_loop();
    }

    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let first_hit = hits & 1 != 0;
    let any_changed = changed != 0;
    let readback_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= EXPECTED_HW_LANE_DISPATCH
        && first_hit
        && any_changed;
    let display_notified = readback_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-simd16-mul-witness",
            row_offset,
            BYTES,
        );
    let reason = if readback_ok {
        "mandelbrot16-simd16-mul-dword-witness-visible"
    } else if !finished {
        "mandelbrot16-simd16-mul-dword-witness-submit-not-finished"
    } else if dispatch_delta == 0 {
        "mandelbrot16-simd16-mul-dword-witness-no-eu-dispatch"
    } else if hits == 0 {
        "mandelbrot16-mul-dword-witness-first-lane-no-expected-value"
    } else {
        "mandelbrot16-mul-dword-witness-first-lane-ok-readback-side-observation"
    };
    let proves = if readback_ok {
        "simd16-mul-dword-witness-store-eot-first-lane-validation-once"
    } else if dispatch_delta >= EXPECTED_HW_LANE_DISPATCH {
        "simd16-mul-witness-dispatch-plus-store-mismatch"
    } else if dispatch_delta != 0 {
        "partial-eu-dispatch"
    } else {
        "no-eu-dispatch"
    };

    crate::log!(
        "intel/gpgpu: primary-scanout-mandelbrot16-simd16-mul-witness y={} x_base={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} target_virt=0x{:X} pitch_bytes={} byte_len=0x{:X} mul_lhs=0x{:08X} mul_rhs=0x{:08X} patched_color=0x{:08X} expected_value=0x{:08X} artifact_body=simd16-mul-dword-witness-store-v8-retired-but-header-source payload_contract=mesa-send16-split-address-g20-data-g22-bti1-mlen2-exmlen2 dispatch_contract=simd16-basics-walker-v1 eu_math_lanes_mask=0x{:04X} eu_store_lanes_mask=0x{:04X} cpu_patched_lanes_mask=0x0000 eu_color_lanes=0 cpu_color_dwords_patched=0 eu_address_alu=mul16-laneid-by-4-add-bound-surface-offset-g20 eu_alu_variant={} eu_store_value=split-src1-g22-not-yet-selected-by-hdc validation_scope=first-kickoff-lane0-only validation_lanes=1 logical_lanes={} hdc_store_sends={} expected_store_pixels={} hit_mask=0x{:04X} changed_mask=0x{:04X} readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} expected_first=0x{:08X} after0=0x{:08X} after1=0x{:08X} after2=0x{:08X} after3=0x{:08X} after4=0x{:08X} after5=0x{:08X} after6=0x{:08X} after7=0x{:08X} after8=0x{:08X} after9=0x{:08X} after10=0x{:08X} after11=0x{:08X} after12=0x{:08X} after13=0x{:08X} after14=0x{:08X} after15=0x{:08X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} expected_hw_lane_dispatch={} program_source={} address_base_dword={} color_dword={} one_iter_dword={} color_from_depth_dword={} store_send_dword={} proves={} next={} does_not_prove=full-frame-mandelbrot\n",
        y,
        x,
        row_offset,
        row_gpu,
        target.gpu,
        target.phys,
        target.virt as usize,
        target.pitch_bytes,
        target.byte_len,
        3u32,
        3u32,
        patched_color,
        expected_first,
        mandelbrot16_active_lane_mask(),
        mandelbrot16_active_lane_mask(),
        mandelbrot16_simd16_probe_variant_name(mode),
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SENDS,
        PIXELS,
        hits,
        changed,
        readback_ok as u8,
        reason,
        output_first_before,
        output_first_after,
        expected_first,
        after_words[0],
        after_words[1],
        after_words[2],
        after_words[3],
        after_words[4],
        after_words[5],
        after_words[6],
        after_words[7],
        after_words[8],
        after_words[9],
        after_words[10],
        after_words[11],
        after_words[12],
        after_words[13],
        after_words[14],
        after_words[15],
        display_notified as u8,
        BYTES,
        finish_marker,
        dispatch_delta,
        EXPECTED_HW_LANE_DISPATCH,
        program.name,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ONE_ITER_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_FROM_DEPTH_DWORD,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SEND_DWORD,
        proves,
        if readback_ok {
            "restore-fixed10-q12-body-after-mul-witness"
        } else if finished {
            "fix-simd16-mul-witness-readback-or-store"
        } else {
            "fix-simd16-mul-witness-submit-or-eot"
        },
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-mandelbrot16-simd16-mul-witness",
        );
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted: batch_bytes != 0,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: expected_first,
        output_first_before,
        output_first_after,
        output_nonzero_before: (output_first_before != 0) as usize,
        output_nonzero_after: (output_first_after != 0) as usize,
        output_hits_lo64: hits,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot(
    mode: u32,
    line_index: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_line_pilot_rect(mode, line_index, 0, 0, u32::MAX, u32::MAX)
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot_rect(
    mode: u32,
    line_index: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let color_seed = if mode & 1 == 0 {
        0x0000_0000
    } else {
        0x00FF_FFFF
    };
    submit_gpgpu_primary_scanout_line_pilot_rect_color(
        color_seed,
        line_index,
        rect_x,
        rect_y,
        rect_width,
        rect_height,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_line_pilot_rect_color(
    color_seed: u32,
    line_index: u32,
    rect_x: u32,
    rect_y: u32,
    rect_width: u32,
    rect_height: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const LANES_PER_PILOT: usize = trueos_eu::gfx12::PRIMARY_SCANOUT_LINE1280_LANE8ROWS_LANES;

    let program = gpgpu_primary_scanout_mandelbrot8_gpu_color_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, 0);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0);
    };
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu);
    }

    let target_width = target.width as usize;
    let target_height = target.height as usize;
    let rect_width = core::cmp::min(rect_width as usize, target_width);
    let rect_height = core::cmp::min(rect_height as usize, target_height);
    if rect_width < LANES_PER_PILOT || rect_height == 0 {
        return gpgpu_one_tile_sentinel_failure("line-pilot-rect-too-small", program, target.gpu);
    }
    let rect_x = core::cmp::min(rect_x as usize, target_width.saturating_sub(rect_width));
    let rect_y = core::cmp::min(rect_y as usize, target_height.saturating_sub(rect_height));
    let segments_per_row = rect_width.saturating_add(LANES_PER_PILOT - 1) / LANES_PER_PILOT;
    let segments_per_row = core::cmp::max(1, segments_per_row);
    let serial_index = line_index as usize;
    let segment = serial_index % segments_per_row;
    let y_in_rect = if rect_height == 0 {
        0
    } else {
        (serial_index / segments_per_row) % rect_height
    };
    let y = rect_y.saturating_add(y_in_rect);
    let x_in_rect = if segments_per_row <= 1 {
        0
    } else {
        core::cmp::min(
            segment.saturating_mul(LANES_PER_PILOT),
            rect_width.saturating_sub(LANES_PER_PILOT),
        )
    } as usize;
    let x_base = rect_x.saturating_add(x_in_rect);
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x_base.saturating_mul(core::mem::size_of::<u32>()));
    let pilot_bytes = LANES_PER_PILOT.saturating_mul(core::mem::size_of::<u32>());
    if row_offset.saturating_add(pilot_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure("line-pilot-outside-scanout", program, target.gpu);
    }
    let row_gpu = target.gpu + row_offset as u64;
    let row_virt = unsafe { target.virt.add(row_offset) };
    let pilot_groups = 1u32;
    let requested_mode = (color_seed != 0) as u32;

    submit_gpgpu_primary_scanout_mandelbrot_gpu_color_witness_strip(
        dev,
        warm,
        target.gpu,
        target.byte_len,
        row_gpu,
        row_virt,
        x_base,
        y,
        0,
        requested_mode,
        color_seed,
        pilot_groups,
        pilot_bytes,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot_preview(
    cursor: usize,
    target_phase: usize,
    pixel_budget: usize,
) -> (crate::intel::GpgpuOneTileSentinelProof, usize) {
    const STRIP_BURST_MAX: usize = 256;
    const STORES_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_STRIPS_PER_PROGRAM;
    const SIMD_LANES_PER_STORE: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_LANES;
    const PIXELS_PER_PROGRAM: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT8_SIMD8_Q12_PIXELS_PER_PROGRAM;

    let program = gpgpu_primary_scanout_mandelbrot8_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return (gpgpu_one_tile_sentinel_failure("no-device", program, 0), cursor);
    };
    let Some(warm) = warm_state() else {
        return (gpgpu_one_tile_sentinel_failure("no-warm-state", program, 0), cursor);
    };
    let Some(target) = crate::intel::display::primary_surface_gpgpu_marker_target() else {
        return (gpgpu_one_tile_sentinel_failure("no-primary-scanout", program, 0), cursor);
    };
    if !forcewake_render_acquire(warm) {
        return (gpgpu_one_tile_sentinel_failure("forcewake", program, target.gpu), cursor);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) {
        return (gpgpu_one_tile_sentinel_failure("ggtt-map", program, target.gpu), cursor);
    }

    let scanout_w = target.width as usize;
    let scanout_h = target.height as usize;
    let strips_per_row = scanout_w / PIXELS_PER_PROGRAM;
    let total_strips = strips_per_row.saturating_mul(scanout_h);
    if total_strips == 0 || pixel_budget < PIXELS_PER_PROGRAM {
        return (
            gpgpu_one_tile_sentinel_failure("empty-preview-scanout", program, target.gpu),
            cursor,
        );
    }

    let start_cursor = cursor % total_strips;
    let mut last_proof =
        gpgpu_one_tile_sentinel_failure_quiet("no-preview-strips-submitted", program, target.gpu);
    let strip_budget =
        core::cmp::min(core::cmp::max(1, pixel_budget / PIXELS_PER_PROGRAM), STRIP_BURST_MAX);
    let mut submitted_strips = 0usize;
    let mut finished_strips = 0usize;
    let mut accepted_strips = 0usize;
    let mut advanced_strips = 0usize;
    let mut idx = start_cursor;
    while submitted_strips < strip_budget {
        let strip_x = idx % strips_per_row;
        let py = idx / strips_per_row;
        let px = strip_x * PIXELS_PER_PROGRAM;
        let byte_offset = py
            .saturating_mul(target.pitch_bytes as usize)
            .saturating_add(px.saturating_mul(core::mem::size_of::<u32>()));
        if byte_offset.saturating_add(PIXELS_PER_PROGRAM * core::mem::size_of::<u32>())
            > target.byte_len
        {
            last_proof = gpgpu_one_tile_sentinel_failure(
                "preview-strip-outside-scanout",
                program,
                target.gpu,
            );
            break;
        }
        let row_gpu = target.gpu + byte_offset as u64;
        let row_virt = unsafe { target.virt.add(byte_offset) };
        let proof = submit_gpgpu_primary_scanout_mandelbrot_strip(
            dev,
            warm,
            program,
            target.gpu,
            target.byte_len,
            row_gpu,
            row_virt,
            px,
            py,
            scanout_w,
            scanout_h,
            target_phase,
        );
        submitted_strips += proof.submitted as usize;
        let expected_mask = (1u64 << PIXELS_PER_PROGRAM) - 1;
        let strip_changed = proof.finished
            && proof.finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
            && proof.output_hits_lo64 == expected_mask;
        let strip_finished =
            proof.finished && proof.finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
        if strip_finished {
            finished_strips += 1;
        }
        if strip_changed {
            accepted_strips += 1;
            advanced_strips += 1;
        } else {
            last_proof = proof;
            break;
        }
        last_proof = proof;
        idx += 1;
        if idx == total_strips {
            idx = 0;
        }
    }

    let flush_offset = 0usize;
    let flush_bytes = scanout_h
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(scanout_w.saturating_mul(core::mem::size_of::<u32>()));
    let display_notified = accepted_strips != 0
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-visual-preview",
            flush_offset,
            flush_bytes,
        );
    let next_cursor = (start_cursor + advanced_strips) % total_strips;
    let readback_ok =
        submitted_strips != 0 && submitted_strips == accepted_strips && last_proof.readback_ok;
    let first_failed_preview_log =
        !readback_ok && !MANDELBROT_PREVIEW_FAILURE_LOGGED.swap(true, Ordering::AcqRel);
    let should_log_preview = (accepted_strips != 0 && (start_cursor == 0 || next_cursor == 0))
        || first_failed_preview_log;
    if should_log_preview {
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot32-q12-preview scanout={}x{} submitted_programs={} finished_programs={} exact_programs={} advanced_programs={} hdc_sends_per_program={} simd_lanes_per_send={} pixels_per_program={} submitted_pixels={} exact_pixels={} strict_readback_ok={} reason={} program_source={} primary_gpu=0x{:X} primary_bytes=0x{:X} cursor_in={} cursor_out={} strip_budget={} burst_cap={} last_gpu=0x{:X} last_first_before=0x{:08X} last_first_after=0x{:08X} last_expected_mask=0x{:016X} display_notified={} finish_marker=0x{:08X} finish_expected=0x{:08X} lane_dispatch_delta={} scheduler=linear-scanout-32px-chunks cpu_runtime_patches=coords-and-address-bases eu_runtime_work=q12-iteration-color-and-hdc-message-payload action={} next={} deliverable=visible-q12-mandelbrot-pixels\n",
            scanout_w,
            scanout_h,
            submitted_strips,
            finished_strips,
            accepted_strips,
            advanced_strips,
            STORES_PER_PROGRAM,
            SIMD_LANES_PER_STORE,
            PIXELS_PER_PROGRAM,
            submitted_strips.saturating_mul(PIXELS_PER_PROGRAM),
            accepted_strips.saturating_mul(PIXELS_PER_PROGRAM),
            readback_ok as u8,
            last_proof.reason,
            program.name,
            target.gpu,
            target.byte_len,
            start_cursor,
            next_cursor,
            strip_budget,
            STRIP_BURST_MAX,
            last_proof.output_gpu,
            last_proof.output_first_before,
            last_proof.output_first_after,
            last_proof.output_hits_lo64,
            display_notified as u8,
            last_proof.finish_marker,
            last_proof.expected_finish_marker,
            last_proof.dispatch_delta,
            if readback_ok {
                "continue-gpgpu-visual-preview"
            } else {
                "hold-cursor-until-scanout-changes"
            },
            if next_cursor == 0 {
                "frame-covered"
            } else {
                "continue-visual-strips"
            },
        );
    }
    (last_proof, next_cursor)
}
