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
const MANDELBROT16_T11_MODE_LINEAR_FULL_BAND: u32 = 46;
const MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND: u32 = 47;
const MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE: u32 = 48;
const MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE: u32 = 49;
const MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE: u32 = 50;
const MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE: u32 = 51;
const MANDELBROT16_T13_Q12_X_STEP: u32 = 5;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Mandelbrot16AddressMode {
    ImmediateBase,
    GroupIdRowPitch,
    GroupIdLinear64,
}

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
    lhs: u32,
    rhs: u32,
    address_mode: Mandelbrot16AddressMode,
) -> bool {
    let mut words =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    patch_mandelbrot16_simd16_probe_variant(&mut words, mode, lhs, rhs, address_base);
    patch_mandelbrot16_simd16_probe_source(&mut words, mode, lhs);
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
    patch_mandelbrot16_simd16_address_prelude(&mut words, address_base, address_mode);
    let uploaded = upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &words);
    if uploaded {
        MANDELBROT_LINE1280_TEMPLATE_UPLOADED.store(false, Ordering::Release);
        MANDELBROT_GROUPID_LINE1280_TEMPLATE_UPLOADED.store(false, Ordering::Release);
    }
    uploaded
}

fn patch_mandelbrot16_simd16_address_prelude(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    address_base: u32,
    address_mode: Mandelbrot16AddressMode,
) {
    const NOP: [u32; 4] = [0x80000101, 0x00000000, 0x00000000, 0x00000000];
    if address_mode == Mandelbrot16AddressMode::GroupIdRowPitch {
        return;
    }
    if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        // Match the proven line320 group-id prelude: read GPGPU R0.1 via the
        // scalar source encoding, then shift by log2(16 pixels * 4 bytes).
        words[18] = 0x00000024;
        words[20] = 0x80030269;
        words[23] = 6;
        words[27] = address_base;
        return;
    }

    words[16] = 0x00040140;
    words[17] = 0x14058660;
    words[18] = 0x06461405;
    words[19] = address_base;
    words[20] = NOP[0];
    words[21] = NOP[1];
    words[22] = NOP[2];
    words[23] = NOP[3];
    words[24] = NOP[0];
    words[25] = NOP[1];
    words[26] = NOP[2];
    words[27] = NOP[3];
    words[28] = NOP[0];
    words[29] = NOP[1];
    words[30] = NOP[2];
    words[31] = NOP[3];
}

fn patch_mandelbrot16_simd16_put(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    cursor: &mut usize,
    instr: [u32; 4],
) {
    words[*cursor] = instr[0];
    words[*cursor + 1] = instr[1];
    words[*cursor + 2] = instr[2];
    words[*cursor + 3] = instr[3];
    *cursor += 4;
}

fn patch_mandelbrot16_simd16_fixed10_body(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    c_re_q12: u32,
    c_im_q12: u32,
    max_iter: u32,
    address_base: u32,
    derive_c_re_from_address: bool,
    escape_gradient: bool,
    raw_radius_color: bool,
) {
    const NOP: [u32; 4] = [0x80000101, 0x00000000, 0x00000000, 0x00000000];
    const MOV_G2_W_IMM: [u32; 4] = [0x00040161, 0x02054550, 0x00000000, 0x00000000];
    const MOV_G4_W_IMM: [u32; 4] = [0x00040161, 0x04054550, 0x00000000, 0x00000000];
    const ADD_G18_G20_IMM: [u32; 4] = [0x00040140, 0x12058660, 0x06461405, 0x00000000];
    const ASR_G18_G18_2D: [u32; 4] = [0x0004016C, 0x12058660, 0x06461205, 0x00000002];
    const MUL_G18_G18_IMM_D: [u32; 4] = [0x00040141, 0x12058660, 0x06461205, 0x00000000];
    const ADD_G18_G18_IMM: [u32; 4] = [0x00040140, 0x12058660, 0x06461205, 0x00000000];
    const MOV_G2_G18_W: [u32; 4] = [0x00040161, 0x02050550, 0x00561206, 0x00000000];
    const MOV_G6_W_ZERO: [u32; 4] = [0x00040161, 0x06054550, 0x00000000, 0x00000000];
    const MOV_G8_W_ZERO: [u32; 4] = [0x00040161, 0x08054550, 0x00000000, 0x00000000];
    const SUB_G26_G20_G20: [u32; 4] = [0x00040140, 0x1A050660, 0x06461405, 0x02461405];
    const MUL_G22_G2_G2_W_TO_D: [u32; 4] = [0x00040141, 0x16050560, 0x05560206, 0x00560206];
    const MUL_G22_G6_G6_W_TO_D: [u32; 4] = [0x00040141, 0x16050560, 0x05560606, 0x00560606];
    const ASR_G22_G22_12D: [u32; 4] = [0x0004016C, 0x16058660, 0x06461605, 0x0000000C];
    const MUL_G24_G4_G4_W_TO_D: [u32; 4] = [0x00040141, 0x18050560, 0x05560406, 0x00560406];
    const MUL_G24_G8_G8_W_TO_D: [u32; 4] = [0x00040141, 0x18050560, 0x05560806, 0x00560806];
    const ASR_G24_G24_12D: [u32; 4] = [0x0004016C, 0x18058660, 0x06461805, 0x0000000C];
    const SUB_G28_G22_G24: [u32; 4] = [0x00040140, 0x1C050660, 0x06461605, 0x02461805];
    const ADD_G28_G28_G2: [u32; 4] = [0x00040140, 0x1C050660, 0x06461C05, 0x00460205];
    const MUL_G30_G6_G8_W_TO_D: [u32; 4] = [0x00040141, 0x1E050560, 0x05560606, 0x00560806];
    const ASR_G30_G30_11D: [u32; 4] = [0x0004016C, 0x1E058660, 0x06461E05, 0x0000000B];
    const ADD_G30_G30_G4: [u32; 4] = [0x00040140, 0x1E050660, 0x06461E05, 0x00460405];
    const MOV_G6_G28_W: [u32; 4] = [0x00040161, 0x06050550, 0x00561C06, 0x00000000];
    const MOV_G8_G30_W: [u32; 4] = [0x00040161, 0x08050550, 0x00561E06, 0x00000000];
    const ADD_G22_G22_G24: [u32; 4] = [0x00040140, 0x16050660, 0x06461605, 0x00461805];
    const CMPGE_G22_G22_4Q12: [u32; 4] = [0x00040170, 0x16058660, 0x46461605, 0x00004000];
    const AND_G22_G22_RGB: [u32; 4] = [0x00040165, 0x16058220, 0x02461605, 0x00FFFFFF];
    const AND_G22_G22_GRADIENT_STEP: [u32; 4] = [0x00040165, 0x16058220, 0x02461605, 0x00191919];
    const ADD_G26_G26_G22: [u32; 4] = [0x00040140, 0x1A050660, 0x06461A05, 0x00461605];
    const OR_G22_G26_ALPHA: [u32; 4] = [0x00040166, 0x16058220, 0x02461A05, 0xFF000000];
    const OR_G22_G22_ALPHA: [u32; 4] = [0x00040166, 0x16058220, 0x02461605, 0xFF000000];
    const STORE_SEND_G20_G22: [u32; 4] = [0x00040131, 0x00000000, 0xCC021414, 0x00961614];
    const MOV_G126_G0: [u32; 4] = [0x80030061, 0x7E050220, 0x00460005, 0x00000000];
    const EOT_SEND_G126: [u32; 4] = [0x80030131, 0x00000004, 0x70007E0C, 0x00000000];

    let mut cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    let end = words.len();
    while cursor < end {
        patch_mandelbrot16_simd16_put(words, &mut cursor, NOP);
    }

    let c_re = c_re_q12 & 0xFFFF;
    let c_im = c_im_q12 & 0xFFFF;
    let mut mov_cre = MOV_G2_W_IMM;
    let mut mov_cim = MOV_G4_W_IMM;
    mov_cre[3] = c_re | (c_re << 16);
    mov_cim[3] = c_im | (c_im << 16);

    cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    if derive_c_re_from_address {
        let mut pixel_byte_delta = ADD_G18_G20_IMM;
        let mut pixel_scaled = MUL_G18_G18_IMM_D;
        let mut pixel_with_base = ADD_G18_G18_IMM;
        pixel_byte_delta[3] = address_base.wrapping_neg();
        pixel_scaled[3] = MANDELBROT16_T13_Q12_X_STEP;
        pixel_with_base[3] = ((c_re_q12 & 0xFFFF) as u16 as i16 as i32) as u32;
        patch_mandelbrot16_simd16_put(words, &mut cursor, pixel_byte_delta);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G18_G18_2D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, pixel_scaled);
        patch_mandelbrot16_simd16_put(words, &mut cursor, pixel_with_base);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G2_G18_W);
    } else {
        patch_mandelbrot16_simd16_put(words, &mut cursor, mov_cre);
    }
    patch_mandelbrot16_simd16_put(words, &mut cursor, mov_cim);

    if raw_radius_color && max_iter == 1 {
        patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G22_G2_G2_W_TO_D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G22_G22_12D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G24_G4_G4_W_TO_D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G24_G24_12D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G22_G22_G24);
        patch_mandelbrot16_simd16_put(words, &mut cursor, AND_G22_G22_RGB);
        patch_mandelbrot16_simd16_put(words, &mut cursor, OR_G22_G22_ALPHA);
        patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND_G20_G22);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
        patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
        return;
    }

    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G6_W_ZERO);
    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G8_W_ZERO);
    if escape_gradient {
        patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G26_G20_G20);
    }

    let mut iter = 0;
    while iter < max_iter {
        patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G22_G6_G6_W_TO_D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G22_G22_12D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G24_G8_G8_W_TO_D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G24_G24_12D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G28_G22_G24);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G28_G28_G2);
        if escape_gradient {
            patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G22_G22_G24);
            patch_mandelbrot16_simd16_put(words, &mut cursor, CMPGE_G22_G22_4Q12);
            patch_mandelbrot16_simd16_put(words, &mut cursor, AND_G22_G22_GRADIENT_STEP);
            patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G26_G26_G22);
        }
        patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G30_G6_G8_W_TO_D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G30_G30_11D);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G30_G30_G4);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G6_G28_W);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G8_G30_W);
        iter += 1;
    }

    patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G22_G6_G6_W_TO_D);
    patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G22_G22_12D);
    patch_mandelbrot16_simd16_put(words, &mut cursor, MUL_G24_G8_G8_W_TO_D);
    patch_mandelbrot16_simd16_put(words, &mut cursor, ASR_G24_G24_12D);
    patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G22_G22_G24);
    if raw_radius_color {
        patch_mandelbrot16_simd16_put(words, &mut cursor, AND_G22_G22_RGB);
        patch_mandelbrot16_simd16_put(words, &mut cursor, OR_G22_G22_ALPHA);
    } else if escape_gradient {
        patch_mandelbrot16_simd16_put(words, &mut cursor, CMPGE_G22_G22_4Q12);
        patch_mandelbrot16_simd16_put(words, &mut cursor, AND_G22_G22_GRADIENT_STEP);
        patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G26_G26_G22);
        patch_mandelbrot16_simd16_put(words, &mut cursor, OR_G22_G26_ALPHA);
    } else {
        patch_mandelbrot16_simd16_put(words, &mut cursor, CMPGE_G22_G22_4Q12);
        patch_mandelbrot16_simd16_put(words, &mut cursor, AND_G22_G22_RGB);
        patch_mandelbrot16_simd16_put(words, &mut cursor, OR_G22_G22_ALPHA);
    }
    patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND_G20_G22);
    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
    patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
}

fn patch_mandelbrot16_simd16_linear_constant_store_body(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    color: u32,
) {
    const NOP: [u32; 4] = [0x80000101, 0x00000000, 0x00000000, 0x00000000];
    const SUB_G22_G20_G20: [u32; 4] = [0x00040140, 0x16050660, 0x06461405, 0x02461405];
    const OR_G22_G22_IMM: [u32; 4] = [0x00040166, 0x16058220, 0x02461605, 0x00000000];
    const STORE_SEND_G20_G22: [u32; 4] = [0x00040131, 0x00000000, 0xCC021414, 0x00961614];
    const MOV_G126_G0: [u32; 4] = [0x80030061, 0x7E050220, 0x00460005, 0x00000000];
    const EOT_SEND_G126: [u32; 4] = [0x80030131, 0x00000004, 0x70007E0C, 0x00000000];

    let mut cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    let end = words.len();
    while cursor < end {
        patch_mandelbrot16_simd16_put(words, &mut cursor, NOP);
    }

    let mut or_color = OR_G22_G22_IMM;
    or_color[3] = color;
    cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G22_G20_G20);
    patch_mandelbrot16_simd16_put(words, &mut cursor, or_color);
    patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND_G20_G22);
    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
    patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
}

fn patch_mandelbrot16_simd16_probe_variant(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    mode: u32,
    lhs: u32,
    rhs: u32,
    address_base: u32,
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
    const MOV_G22_G6: [u32; 4] = [0x00040161, 0x16050660, 0x00460605, 0x00000000];
    const NEG_G22_G6: [u32; 4] = [0x00040161, 0x16052660, 0x00460605, 0x00000000];
    const ABS_G22_G6: [u32; 4] = [0x00040161, 0x16051660, 0x00460605, 0x00000000];
    const ADD_G22_G6_IMM: [u32; 4] = [0x00040140, 0x16058660, 0x06460605, 0x00000004];
    const SUB_G22_G6_IMM: [u32; 4] = [0x00040140, 0x16058660, 0x06460605, 0xFFFF_FFFF];
    const SUB_G22_G6_G6: [u32; 4] = [0x00040140, 0x16050660, 0x06460605, 0x02460605];
    const AND_G22_G6_IMM: [u32; 4] = [0x00040165, 0x16058220, 0x02460605, 0x00000001];
    const OR_G22_G6_IMM: [u32; 4] = [0x00040166, 0x16058220, 0x02460605, 0x00000004];
    const XOR_G22_G6_IMM: [u32; 4] = [0x00040167, 0x16058220, 0x02460605, 0x00000001];
    const SHL_G22_G6_IMM: [u32; 4] = [0x00040169, 0x16058660, 0x06460605, 0x00000001];
    const SHR_G22_G6_IMM: [u32; 4] = [0x00040168, 0x16058220, 0x02460605, 0x00000001];
    const ASR_G22_G6_IMM: [u32; 4] = [0x0004016C, 0x16058660, 0x06460605, 0x00000001];
    const NOT_G22_G6: [u32; 4] = [0x00040164, 0x16050220, 0x00460605, 0x00000000];
    const CMPGE_G22_G6_IMM: [u32; 4] = [0x00040170, 0x16058660, 0x46460605, 0x00000003];
    const MOV_G22_G6_W_PACKED: [u32; 4] = [0x00040161, 0x16050550, 0x00560606, 0x00000000];
    const MOV_G22_G6_UW_PACKED: [u32; 4] = [0x00040161, 0x16050110, 0x00560606, 0x00000000];
    const MUL_G22_G6_G6_W_TO_D: [u32; 4] = [0x00040141, 0x16050560, 0x05560606, 0x00560606];
    const MUL_G22_G6_G6_UW_TO_UD: [u32; 4] = [0x00040141, 0x16050120, 0x01560606, 0x00560606];
    const ASR_G22_G22_12D: [u32; 4] = [0x0004016C, 0x16058660, 0x06461605, 0x0000000C];
    const SHR_G22_G22_12UD: [u32; 4] = [0x00040168, 0x16058220, 0x02461605, 0x0000000C];
    const MOV_G8_W_IMM: [u32; 4] = [0x00040161, 0x08054550, 0x00000000, 0x00040004];
    const MUL_G24_G8_G8_W_TO_D: [u32; 4] = [0x00040141, 0x18050560, 0x05560806, 0x00560806];
    const ASR_G24_G24_12D: [u32; 4] = [0x0004016C, 0x18058660, 0x06461805, 0x0000000C];
    const SUB_G22_G22_G24: [u32; 4] = [0x00040140, 0x16050660, 0x06461605, 0x02461805];
    const ADD_G22_G22_IMM: [u32; 4] = [0x00040140, 0x16058660, 0x06461605, 0x00000800];
    const OR_G22_G22_IMM: [u32; 4] = [0x00040166, 0x16058220, 0x02461605, 0xFFFF0000];
    const STORE_SEND_G20_G22: [u32; 4] = [0x00040131, 0x00000000, 0xCC021414, 0x00961614];
    const MOV_G126_G0: [u32; 4] = [0x80030061, 0x7E050220, 0x00460005, 0x00000000];
    const EOT_SEND_G126: [u32; 4] = [0x80030131, 0x00000004, 0x70007E0C, 0x00000000];

    if mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE
    {
        patch_mandelbrot16_simd16_linear_constant_store_body(words, lhs);
        return;
    }

    if mode == 44
        || mode == 45
        || mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND
        || mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND
        || mode == MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE
        || mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE
    {
        let max_iter = if mode == 45 || mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE {
            1
        } else {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER
        };
        patch_mandelbrot16_simd16_fixed10_body(
            words,
            lhs,
            rhs,
            max_iter,
            address_base,
            mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND
                || mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND,
            mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND
                || mode == MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
            mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        );
        return;
    }

    if mode == 42 || mode == 43 {
        let mut mov_g8 = MOV_G8_W_IMM;
        let rhs16 = rhs & 0xFFFF;
        mov_g8[3] = rhs16 | (rhs16 << 16);
        let mut add_cre = ADD_G22_G22_IMM;
        add_cre[3] = ((lhs & 0xFFFF) as u16 as i16 as i32) as u32;
        let body = if mode == 43 {
            [
                NOP,
                mov_g8,
                MUL_G22_G6_G6_W_TO_D,
                ASR_G22_G22_12D,
                MUL_G24_G8_G8_W_TO_D,
                ASR_G24_G24_12D,
                SUB_G22_G22_G24,
                add_cre,
                OR_G22_G22_IMM,
                STORE_SEND_G20_G22,
                MOV_G126_G0,
                EOT_SEND_G126,
            ]
        } else {
            [
                NOP,
                mov_g8,
                MUL_G22_G6_G6_W_TO_D,
                ASR_G22_G22_12D,
                MUL_G24_G8_G8_W_TO_D,
                ASR_G24_G24_12D,
                SUB_G22_G22_G24,
                add_cre,
                NOP,
                STORE_SEND_G20_G22,
                MOV_G126_G0,
                EOT_SEND_G126,
            ]
        };
        let body_base = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
        let mut instr = 0usize;
        while instr < body.len() {
            let off = body_base + instr * 4;
            words[off] = body[instr][0];
            words[off + 1] = body[instr][1];
            words[off + 2] = body[instr][2];
            words[off + 3] = body[instr][3];
            instr += 1;
        }
        return;
    }

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
        14 => (MOV_G22_G6, NOP),
        15 => {
            let mut mov = MOV_G22_9;
            mov[3] = rhs;
            (NOP, mov)
        }
        16 => (NEG_G22_G6, NOP),
        17 => (ABS_G22_G6, NOP),
        18 => {
            let mut add = ADD_G22_G6_IMM;
            add[3] = rhs;
            (add, NOP)
        }
        19 => {
            let mut sub = SUB_G22_G6_IMM;
            sub[3] = rhs.wrapping_neg();
            (sub, NOP)
        }
        20 => (SUB_G22_G6_G6, NOP),
        21 => {
            let mut and = AND_G22_G6_IMM;
            and[3] = rhs;
            (and, NOP)
        }
        22 => {
            let mut or = OR_G22_G6_IMM;
            or[3] = rhs;
            (or, NOP)
        }
        23 => {
            let mut xor = XOR_G22_G6_IMM;
            xor[3] = rhs;
            (xor, NOP)
        }
        24 => {
            let mut shl = SHL_G22_G6_IMM;
            shl[3] = rhs & 31;
            (shl, NOP)
        }
        25 => {
            let mut shr = SHR_G22_G6_IMM;
            shr[3] = rhs & 31;
            (shr, NOP)
        }
        26 => {
            let mut asr = ASR_G22_G6_IMM;
            asr[3] = rhs & 31;
            (asr, NOP)
        }
        27 => (NOT_G22_G6, NOP),
        28 => {
            let mut cmp = CMPGE_G22_G6_IMM;
            cmp[3] = rhs;
            (cmp, NOP)
        }
        29 => (MOV_G22_G6, NOP),
        30 | 32 => (MOV_G22_G6_W_PACKED, NOP),
        31 | 33 => (MOV_G22_G6_UW_PACKED, NOP),
        34 => (MUL_G22_G6_G6_W, NOP),
        35 => (MUL_G22_G6_G6_UW, NOP),
        36 => (MUL_G24_G6_G6_W, MOV_G22_G24_W),
        37 => (MUL_G24_G6_G6_UW, MOV_G22_G24_UW),
        38 => (MUL_G22_G6_G6_W_TO_D, NOP),
        39 => (MUL_G22_G6_G6_UW_TO_UD, NOP),
        40 => (MUL_G22_G6_G6_W_TO_D, ASR_G22_G22_12D),
        41 => (MUL_G22_G6_G6_UW_TO_UD, SHR_G22_G22_12UD),
        _ => (MUL_G22_G6_G6, MOV_G22_9),
    };

    let body_base = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    words[body_base + 4] = first[0];
    words[body_base + 5] = first[1];
    words[body_base + 6] = first[2];
    words[body_base + 7] = first[3];
    words[body_base + 8] = second[0];
    words[body_base + 9] = second[1];
    words[body_base + 10] = second[2];
    words[body_base + 11] = second[3];
}

fn patch_mandelbrot16_simd16_probe_source(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    mode: u32,
    lhs: u32,
) {
    const MOV_G6_D_IMM: [u32; 4] = [0x80040061, 0x06054660, 0x00000000, 0x00000003];
    const MOV_G6_W_IMM: [u32; 4] = [0x00040161, 0x06054550, 0x00000000, 0x00030003];
    const MOV_G6_UW_IMM: [u32; 4] = [0x00040161, 0x06054110, 0x00000000, 0x00030003];

    if mode == 44
        || mode == 45
        || mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND
        || mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND
        || mode == MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE
        || mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE
    {
        return;
    }

    let replicated = (lhs & 0xFFFF) | ((lhs & 0xFFFF) << 16);
    let mut init = match mode {
        32 | 34 | 36 | 38 | 40 | 42 | 43 => MOV_G6_W_IMM,
        33 | 35 | 37 | 39 | 41 => MOV_G6_UW_IMM,
        _ => MOV_G6_D_IMM,
    };
    init[3] = if matches!(mode, 32 | 33 | 34 | 35 | 36 | 37 | 38 | 39 | 40 | 41 | 42 | 43) {
        replicated
    } else {
        lhs
    };
    let body_base = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    words[body_base] = init[0];
    words[body_base + 1] = init[1];
    words[body_base + 2] = init[2];
    words[body_base + 3] = init[3];
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
        14 => "mov16-dword-g6-to-g22",
        15 => "mov16-immediate-rhs-to-g22",
        16 => "neg16-dword-g6-to-g22",
        17 => "abs16-dword-g6-to-g22",
        18 => "add16-dword-g6-plus-rhs-g22",
        19 => "sub16-dword-g6-minus-rhs-g22",
        20 => "sub16-dword-g6-minus-g6-g22",
        21 => "and16-udword-g6-and-rhs-g22",
        22 => "or16-udword-g6-or-rhs-g22",
        23 => "xor16-udword-g6-xor-rhs-g22",
        24 => "shl16-dword-g6-by-rhs-g22",
        25 => "shr16-udword-g6-by-rhs-g22",
        26 => "asr16-dword-g6-by-rhs-g22",
        27 => "not16-udword-g6-to-g22",
        28 => "cmpge16-dword-g6-vs-rhs-mask-g22",
        29 => "dump16-dword-g6-to-g22",
        30 => "dump16-word-packed-g6-dword-init-to-g22",
        31 => "dump16-uword-packed-g6-dword-init-to-g22",
        32 => "dump16-word-packed-g6-replicated-halfword-init-to-g22",
        33 => "dump16-uword-packed-g6-replicated-halfword-init-to-g22",
        34 => "mul16-word-packed-g6-replicated-halfword-init-g22",
        35 => "mul16-uword-packed-g6-replicated-halfword-init-g22",
        36 => "mul16-word-replicated-halfword-init-g24-then-widen-g22",
        37 => "mul16-uword-replicated-halfword-init-g24-then-widen-g22",
        38 => "mul16-word-to-dword-g6-replicated-halfword-init-g22",
        39 => "mul16-uword-to-udword-g6-replicated-halfword-init-g22",
        40 => "mul16-word-to-dword-g6-replicated-halfword-init-g22-asr12",
        41 => "mul16-uword-to-udword-g6-replicated-halfword-init-g22-shr12",
        42 => "mandelbrot16-one-iteration-re2-minus-im2-plus-cre-store",
        43 => "mandelbrot16-one-iteration-re2-minus-im2-plus-cre-visible-store",
        44 => "mandelbrot16-fixed10-q12-escape-bw-visible-store",
        45 => "mandelbrot16-fixed1-q12-feedback-visible-store",
        MANDELBROT16_T11_MODE_LINEAR_FULL_BAND => {
            "mandelbrot16-t11-linear-groupid-fixed10-escape-bw-visible-store"
        }
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND => {
            "mandelbrot16-t15-linear-groupid-fixed10-escape-gradient-visible-store"
        }
        MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE => {
            "mandelbrot16-t16-linear-groupid-constant-visible-store"
        }
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE => {
            "mandelbrot16-t17-immediate-base-constant-visible-store"
        }
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE => {
            "mandelbrot16-t18-immediate-base-fixed10-escape-gradient-visible-store"
        }
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE => {
            "mandelbrot16-t19-immediate-base-fixed1-raw-radius-visible-store"
        }
        _ => "mul16-dword-g6-times-g6-g22-then-immediate-nine-isolation",
    }
}

fn mandelbrot16_oneiter_re1_q12(lhs: u32, rhs: u32) -> u32 {
    let c_re = (lhs as i16) as i32;
    let c_im = (rhs as i16) as i32;
    let re2 = c_re.wrapping_mul(c_re).wrapping_shr(12);
    let im2 = c_im.wrapping_mul(c_im).wrapping_shr(12);
    re2.wrapping_sub(im2).wrapping_add(c_re) as u32
}

fn mandelbrot16_fixed10_visible_q12(lhs: u32, rhs: u32) -> u32 {
    const FRAC_BITS: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    mandelbrot16_fixed_escape_bw_q12(
        lhs,
        rhs,
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER,
        FRAC_BITS,
    )
}

fn mandelbrot16_fixed10_gradient_q12(lhs: u32, rhs: u32) -> u32 {
    const FRAC_BITS: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    const MAX_ITER: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_MAX_ITER;
    let c_re = (lhs as i16) as i32;
    let c_im = (rhs as i16) as i32;
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    let mut color = 0u32;
    let mut iter = 0u32;
    while iter < MAX_ITER {
        let re2 = z_re.wrapping_mul(z_re) >> FRAC_BITS;
        let im2 = z_im.wrapping_mul(z_im) >> FRAC_BITS;
        let next_re = re2.wrapping_sub(im2).wrapping_add(c_re);
        if re2.wrapping_add(im2) >= (4i32 << FRAC_BITS) {
            color = color.wrapping_add(0x0019_1919);
        }
        let next_im = (z_re.wrapping_mul(z_im) >> (FRAC_BITS - 1)).wrapping_add(c_im);
        z_re = (next_re as i16) as i32;
        z_im = (next_im as i16) as i32;
        iter = iter.wrapping_add(1);
    }
    let re2 = z_re.wrapping_mul(z_re) >> FRAC_BITS;
    let im2 = z_im.wrapping_mul(z_im) >> FRAC_BITS;
    if re2.wrapping_add(im2) >= (4i32 << FRAC_BITS) {
        color = color.wrapping_add(0x0019_1919);
    }
    0xFF00_0000 | (color & 0x00FF_FFFF)
}

fn mandelbrot16_fixed_raw_radius_q12(lhs: u32, rhs: u32, max_iter: u32) -> u32 {
    const FRAC_BITS: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    let c_re = (lhs as i16) as i32;
    let c_im = (rhs as i16) as i32;
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    let mut iter = 0u32;
    while iter < max_iter {
        let re2 = z_re.wrapping_mul(z_re) >> FRAC_BITS;
        let im2 = z_im.wrapping_mul(z_im) >> FRAC_BITS;
        let next_re = re2.wrapping_sub(im2).wrapping_add(c_re);
        let next_im = (z_re.wrapping_mul(z_im) >> (FRAC_BITS - 1)).wrapping_add(c_im);
        z_re = (next_re as i16) as i32;
        z_im = (next_im as i16) as i32;
        iter = iter.wrapping_add(1);
    }
    let radius2 =
        (z_re.wrapping_mul(z_re) >> FRAC_BITS).wrapping_add(z_im.wrapping_mul(z_im) >> FRAC_BITS);
    0xFF00_0000 | (radius2 as u32 & 0x00FF_FFFF)
}

fn mandelbrot16_fixed1_visible_q12(lhs: u32, rhs: u32) -> u32 {
    const FRAC_BITS: u32 = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FRAC_BITS;
    mandelbrot16_fixed_visible_q12(lhs, rhs, 1, FRAC_BITS)
}

fn mandelbrot16_fixed_visible_q12(lhs: u32, rhs: u32, max_iter: u32, frac_bits: u32) -> u32 {
    let c_re = (lhs as i16) as i32;
    let c_im = (rhs as i16) as i32;
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    let mut iter = 0u32;
    while iter < max_iter {
        let re2 = z_re.wrapping_mul(z_re) >> frac_bits;
        let im2 = z_im.wrapping_mul(z_im) >> frac_bits;
        let next_re = re2.wrapping_sub(im2).wrapping_add(c_re);
        let next_im = (z_re.wrapping_mul(z_im) >> (frac_bits - 1)).wrapping_add(c_im);
        z_re = (next_re as i16) as i32;
        z_im = (next_im as i16) as i32;
        iter = iter.wrapping_add(1);
    }
    0xFF00_0000 | (z_re.wrapping_mul(z_re) >> frac_bits) as u32
}

fn mandelbrot16_fixed_escape_bw_q12(lhs: u32, rhs: u32, max_iter: u32, frac_bits: u32) -> u32 {
    let c_re = (lhs as i16) as i32;
    let c_im = (rhs as i16) as i32;
    let mut z_re = 0i32;
    let mut z_im = 0i32;
    let mut iter = 0u32;
    while iter < max_iter {
        let re2 = z_re.wrapping_mul(z_re) >> frac_bits;
        let im2 = z_im.wrapping_mul(z_im) >> frac_bits;
        let next_re = re2.wrapping_sub(im2).wrapping_add(c_re);
        let next_im = (z_re.wrapping_mul(z_im) >> (frac_bits - 1)).wrapping_add(c_im);
        z_re = (next_re as i16) as i32;
        z_im = (next_im as i16) as i32;
        iter = iter.wrapping_add(1);
    }
    let radius2 =
        (z_re.wrapping_mul(z_re) >> frac_bits).wrapping_add(z_im.wrapping_mul(z_im) >> frac_bits);
    if radius2 >= (4i32 << frac_bits) {
        0xFFFF_FFFF
    } else {
        0xFF00_0000
    }
}

fn mandelbrot16_simd16_probe_expected_first(mode: u32, lhs: u32, rhs: u32) -> u32 {
    match mode {
        1 => lhs.wrapping_add(lhs),
        9 | 10 => 0x0009_0009,
        14 => lhs,
        15 => rhs,
        16 => (lhs as i32).wrapping_neg() as u32,
        17 => (lhs as i32).wrapping_abs() as u32,
        18 => lhs.wrapping_add(rhs),
        19 => lhs.wrapping_sub(rhs),
        20 => 0,
        21 => lhs & rhs,
        22 => lhs | rhs,
        23 => lhs ^ rhs,
        24 => (lhs as i32).wrapping_shl(rhs & 31) as u32,
        25 => lhs.wrapping_shr(rhs & 31),
        26 => (lhs as i32).wrapping_shr(rhs & 31) as u32,
        27 => !lhs,
        28 => {
            if (lhs as i32) >= (rhs as i32) {
                0xFFFF_FFFF
            } else {
                0
            }
        }
        29 => lhs,
        30 | 31 => lhs & 0xFFFF,
        32 | 33 => (lhs & 0xFFFF) | ((lhs & 0xFFFF) << 16),
        34 | 35 => {
            let product = ((lhs & 0xFFFF).wrapping_mul(lhs & 0xFFFF)) & 0xFFFF;
            product | (product << 16)
        }
        36 | 37 => (lhs & 0xFFFF).wrapping_mul(lhs & 0xFFFF),
        38 => {
            let value = (lhs as i16) as i32;
            value.wrapping_mul(value) as u32
        }
        39 => (lhs & 0xFFFF).wrapping_mul(lhs & 0xFFFF),
        40 => {
            let value = (lhs as i16) as i32;
            value.wrapping_mul(value).wrapping_shr(12) as u32
        }
        41 => (lhs & 0xFFFF).wrapping_mul(lhs & 0xFFFF).wrapping_shr(12),
        42 => mandelbrot16_oneiter_re1_q12(lhs, rhs),
        43 => mandelbrot16_oneiter_re1_q12(lhs, rhs) | 0xFFFF_0000,
        44 => mandelbrot16_fixed10_visible_q12(lhs, rhs),
        45 => mandelbrot16_fixed1_visible_q12(lhs, rhs),
        MANDELBROT16_T11_MODE_LINEAR_FULL_BAND => mandelbrot16_fixed10_visible_q12(lhs, rhs),
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND => {
            mandelbrot16_fixed10_gradient_q12(lhs, rhs)
        }
        MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE => lhs,
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE => lhs,
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE => {
            mandelbrot16_fixed10_gradient_q12(lhs, rhs)
        }
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE => {
            mandelbrot16_fixed_raw_radius_q12(lhs, rhs, 1)
        }
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
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        mode, row_index, x_base, 1, lhs, rhs, true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet(
    mode: u32,
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        mode, row_index, x_base, 1, lhs, rhs, false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_rows(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        mode,
        row_index,
        x_base,
        row_groups.max(1),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_linear_band(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    x_blocks: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND,
        row_index,
        x_base,
        row_groups.max(1).saturating_mul(x_blocks.max(1)),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_band_probe(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    x_blocks: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND,
        row_index,
        x_base,
        row_groups.max(1).saturating_mul(x_blocks.max(1)),
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_gradient(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_gradient_rows(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_gradient_rows_batched(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_rows_batched_impl(
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        lhs,
        rhs,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_gradient_probe(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_quiet_immediate_raw_radius_rows(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        lhs,
        rhs,
        false,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_raw_radius_probe(
    row_index: u32,
    x_base: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE,
        row_index,
        x_base,
        1,
        lhs,
        rhs,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_linear_constant_probe(
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_probe(
    row_index: u32,
    x_base: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        1,
        color,
        0,
        true,
    )
}

pub(crate) fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_constant_rows_probe(
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    color: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
        MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE,
        row_index,
        x_base,
        row_groups.max(1),
        color,
        0,
        true,
    )
}

fn encode_gfx12_gpgpu_mandelbrot16_immediate_rows_batch(
    warm: RenderWarmState,
    batch_dwords: &mut [u32],
    store_surface: GpgpuStoreSurfaceState,
    program: GpgpuEuProgram,
    scanout_gpu: u64,
    target_pitch_bytes: usize,
    target_byte_len: usize,
    first_row_offset: usize,
    row_count: usize,
    completion_marker: u32,
) -> Result<usize, &'static str> {
    const WALKER_AND_MSF_DWORDS: usize = 17;
    const POST_WALKER_FLUSH_DWORDS: usize = 6;
    const IMMEDIATE_ADDRESS_BASE_DWORD: usize = 19;

    fn push(batch_dwords: &mut [u32], cursor: &mut usize, value: u32) -> Result<(), &'static str> {
        if *cursor >= batch_dwords.len() {
            return Err("mandelbrot16-immediate-row-batch-exhausted");
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

    if row_count == 0 {
        return Err("mandelbrot16-immediate-row-batch-empty");
    }
    if first_row_offset >= target_byte_len {
        return Err("mandelbrot16-immediate-row-batch-outside-scanout");
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
    let walker_start = walker_start.ok_or("mandelbrot16-immediate-row-batch-no-walker")?;
    let post_walker_flush_start = walker_start.saturating_add(WALKER_AND_MSF_DWORDS);
    let marker_start = post_walker_flush_start.saturating_add(POST_WALKER_FLUSH_DWORDS);
    let template_end = marker_start.saturating_add(4).saturating_add(2);
    if template_end > template_dwords {
        return Err("mandelbrot16-immediate-row-batch-template-short");
    }
    if batch_dwords[marker_start] != MI_STORE_DATA_IMM_GGTT_DW1 {
        return Err("mandelbrot16-immediate-row-batch-template-marker");
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

    let address_gpu = GPU_VA_DRAW_STATE_BASE
        + (GPGPU_EU_KERNEL_OFFSET_BYTES
            + IMMEDIATE_ADDRESS_BASE_DWORD * core::mem::size_of::<u32>()) as u64;
    let mut cursor = walker_start;
    let mut row = 0usize;
    while row < row_count {
        let row_offset = first_row_offset.saturating_add(row.saturating_mul(target_pitch_bytes));
        if row_offset >= target_byte_len {
            return Err("mandelbrot16-immediate-row-batch-row-outside-scanout");
        }
        let row_gpu = scanout_gpu.saturating_add(row_offset as u64);
        if row_gpu >> 32 != 0 {
            return Err("mandelbrot16-immediate-row-batch-gpu-high32");
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
        row += 1;
    }

    let marker_gpu = GPU_VA_RESULT_BASE
        + (RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD as u64) * core::mem::size_of::<u32>() as u64;
    push_store_imm32(batch_dwords, &mut cursor, marker_gpu, completion_marker)?;
    push(batch_dwords, &mut cursor, MI_BATCH_BUFFER_END)?;
    push(batch_dwords, &mut cursor, MI_NOOP)?;
    Ok(cursor * core::mem::size_of::<u32>())
}

fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_immediate_rows_batched_impl(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    let row_groups = row_groups.max(1);
    let expected_hw_lane_dispatch =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64 * row_groups as u64;
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
    let submit_span_bytes = (row_groups as usize)
        .saturating_sub(1)
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(BYTES);
    if row_offset.saturating_add(submit_span_bytes) > target.byte_len {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-outside-scanout",
            program,
            target.gpu,
        );
    }
    let row_gpu = target.gpu + row_offset as u64;
    if row_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-gpu-high32",
            program,
            row_gpu,
        );
    }
    let row_virt = unsafe { target.virt.add(row_offset) };
    let sample_before = unsafe { core::ptr::read_volatile(row_virt as *const u32) };
    let expected_first = mandelbrot16_simd16_probe_expected_first(mode, lhs, rhs);

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        0,
        mode,
        lhs,
        rhs,
        Mandelbrot16AddressMode::ImmediateBase,
    ) {
        return gpgpu_one_tile_sentinel_failure(
            "mandelbrot16-immediate-row-batch-program-upload",
            program,
            row_gpu,
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
        "stateless-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
    );
    let batch_bytes = match encode_gfx12_gpgpu_mandelbrot16_immediate_rows_batch(
        warm,
        batch,
        store_surface,
        program,
        target.gpu,
        target.pitch_bytes as usize,
        target.byte_len,
        row_offset,
        row_groups as usize,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
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
        "gpgpu-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(row_virt, submit_span_bytes);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let sample_after = unsafe { core::ptr::read_volatile(row_virt as *const u32) };
    let command_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= expected_hw_lane_dispatch;
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-simd16-immediate-row-batch",
            row_offset,
            submit_span_bytes,
        );
    crate::log!(
        "intel/gpgpu: t21-mandelbrot16-immediate-row-batch submitted=1 finished={} readback_ok={} row_index={} x_base={} row_groups={} row_gpu=0x{:X} span_bytes=0x{:X} sample_before=0x{:08X} sample_after=0x{:08X} sample_expected=0x{:08X} sample_match={} display_notified={} lane_dispatch_delta={} expected_hw_lane_dispatch={} finish_marker=0x{:08X} batch_bytes=0x{:X} program_source={} address_path=immediate-base-mi-patched-per-row proves=simd16-immediate-store-body-plus-multiwalker-row-coverage does_not_prove=groupid-row-address-prelude-or-smooth-coloring\n",
        finished as u8,
        command_ok as u8,
        y,
        x,
        row_groups,
        row_gpu,
        submit_span_bytes,
        sample_before,
        sample_after,
        expected_first,
        (sample_after == expected_first) as u8,
        display_notified as u8,
        dispatch_delta,
        expected_hw_lane_dispatch,
        finish_marker,
        batch_bytes,
        program.name,
    );

    crate::intel::GpgpuOneTileSentinelProof {
        submitted: true,
        finished,
        readback_ok: command_ok,
        reason: if command_ok {
            "mandelbrot16-immediate-row-batch-retired"
        } else {
            "mandelbrot16-immediate-row-batch-not-retired"
        },
        program_name: program.name,
        output_gpu: row_gpu,
        sentinel: expected_first,
        output_first_before: sample_before,
        output_first_after: sample_after,
        output_nonzero_before: (sample_before != 0) as usize,
        output_nonzero_after: (sample_after != 0) as usize,
        output_hits_lo64: (sample_after == expected_first) as u64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

fn submit_gpgpu_primary_scanout_mandelbrot16_simd16_bw_store_probe_impl(
    mode: u32,
    row_index: u32,
    x_base: u32,
    row_groups: u32,
    lhs: u32,
    rhs: u32,
    validate_readback: bool,
) -> crate::intel::GpgpuOneTileSentinelProof {
    const PIXELS: usize =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_PIXELS_PER_PROGRAM;
    const BYTES: usize = PIXELS * core::mem::size_of::<u32>();
    let row_groups = row_groups.max(1);
    let address_mode = if mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND
        || mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND
        || mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
    {
        Mandelbrot16AddressMode::GroupIdLinear64
    } else if row_groups > 1 {
        Mandelbrot16AddressMode::GroupIdRowPitch
    } else {
        Mandelbrot16AddressMode::ImmediateBase
    };
    let expected_hw_lane_dispatch =
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES as u64 * row_groups as u64;

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
    let x = if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        x_base as usize
    } else {
        core::cmp::min(x_base as usize, (target.width as usize).saturating_sub(PIXELS))
    };
    let row_offset = y
        .saturating_mul(target.pitch_bytes as usize)
        .saturating_add(x.saturating_mul(core::mem::size_of::<u32>()));
    let submit_span_bytes = if address_mode == Mandelbrot16AddressMode::GroupIdLinear64 {
        (row_groups as usize).saturating_mul(BYTES)
    } else {
        (row_groups as usize)
            .saturating_sub(1)
            .saturating_mul(target.pitch_bytes as usize)
            .saturating_add(BYTES)
    };
    if row_offset.saturating_add(submit_span_bytes) > target.byte_len {
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
    let expected_first = mandelbrot16_simd16_probe_expected_first(mode, lhs, rhs);
    let poison = expected_first ^ 0x00A5_A5A5;
    let mut lane = 0usize;
    if validate_readback {
        let mut group = 0usize;
        while group < row_groups as usize {
            let group_row_virt =
                unsafe { row_virt.add(group.saturating_mul(target.pitch_bytes as usize)) };
            lane = 0;
            while lane < PIXELS {
                unsafe {
                    core::ptr::write_volatile(
                        group_row_virt.add(lane * core::mem::size_of::<u32>()) as *mut u32,
                        poison,
                    );
                }
                lane += 1;
            }
            group += 1;
        }
        crate::intel::dma_flush(row_virt, submit_span_bytes);
    }
    let mut before_words = [0u32; PIXELS];
    if validate_readback {
        lane = 0;
        while lane < PIXELS {
            before_words[lane] = unsafe {
                core::ptr::read_volatile(
                    row_virt.add(lane * core::mem::size_of::<u32>()) as *const u32
                )
            };
            lane += 1;
        }
    }
    let output_first_before = before_words[0];
    let patched_color = 0;

    if !upload_primary_scanout_mandelbrot16_simd16_bw_artifact(
        warm,
        row_offset as u32,
        patched_color,
        mode,
        lhs,
        rhs,
        address_mode,
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
    let surface_note = if validate_readback {
        "stateless-primary-scanout-mandelbrot16-simd16-q12-plane"
    } else {
        "stateless-primary-scanout-mandelbrot16-simd16-q12-plane-quiet"
    };
    let store_surface = prepare_gpgpu_mandelbrot_store_surface_state_for_target_span(
        warm,
        target.gpu,
        target.byte_len,
        surface_note,
    );
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch,
        store_surface,
        program,
        row_groups,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, row_gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let submit_name = if validate_readback {
        "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane"
    } else {
        "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane-quiet"
    };
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        submit_name,
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
    let mut row_group_hit_mask = 0u64;
    let mut row_group_changed_mask = 0u64;
    let mut row_group_first_after = [0u32; 8];
    while readback_poll < readback_poll_limit {
        crate::intel::dma_flush(row_virt, submit_span_bytes);
        hits = 0;
        changed = 0;
        row_group_hit_mask = 0;
        row_group_changed_mask = 0;
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
        if row_groups > 1 {
            let mut group = 0usize;
            while group < row_groups as usize {
                let group_row_virt =
                    unsafe { row_virt.add(group.saturating_mul(target.pitch_bytes as usize)) };
                let after = unsafe { core::ptr::read_volatile(group_row_virt as *const u32) };
                if group < row_group_first_after.len() {
                    row_group_first_after[group] = after;
                }
                if group < 64 && after == expected_first {
                    row_group_hit_mask |= 1u64 << group;
                }
                if group < 64 && after != poison {
                    row_group_changed_mask |= 1u64 << group;
                }
                group += 1;
            }
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
    let command_ok = finished
        && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE
        && dispatch_delta >= expected_hw_lane_dispatch;
    let readback_ok = if validate_readback {
        command_ok && first_hit && any_changed
    } else {
        command_ok
    };
    let display_notified = command_ok
        && crate::intel::display::notify_primary_surface_external_write(
            "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane",
            row_offset,
            submit_span_bytes,
        );
    let is_one_iter = mode == 42 || mode == 43;
    let is_one_iter_visible = mode == 43;
    let is_t11_linear_band = mode == MANDELBROT16_T11_MODE_LINEAR_FULL_BAND;
    let is_t15_linear_gradient = mode == MANDELBROT16_T15_MODE_LINEAR_GRADIENT_FULL_BAND;
    let is_t16_linear_constant = mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE;
    let is_t17_immediate_constant = mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE;
    let is_t18_immediate_gradient = mode == MANDELBROT16_T18_MODE_IMMEDIATE_GRADIENT_STORE;
    let is_t19_immediate_raw_radius = mode == MANDELBROT16_T19_MODE_IMMEDIATE_RAW_RADIUS_STORE;
    let is_fixed10_visible = mode == 44 || is_t11_linear_band || is_t15_linear_gradient;
    let is_fixed10_gradient = is_t15_linear_gradient || is_t18_immediate_gradient;
    let is_fixed10_visible =
        is_fixed10_visible || is_t18_immediate_gradient || is_t19_immediate_raw_radius;
    let is_fixed1_visible = mode == 45;
    let is_fixed_iter_visible = is_fixed10_visible || is_fixed1_visible;
    let reason = if !validate_readback && command_ok {
        if is_fixed_iter_visible {
            if is_fixed1_visible {
                "mandelbrot16-simd16-q12-fixed1-feedback-quiet-submit-finished-no-readback"
            } else {
                if is_t19_immediate_raw_radius {
                    "mandelbrot16-t19-immediate-base-fixed1-raw-radius-quiet-submit-finished-no-readback"
                } else if is_t18_immediate_gradient {
                    "mandelbrot16-t18-immediate-base-fixed10-escape-gradient-quiet-submit-finished-no-readback"
                } else if is_t15_linear_gradient {
                    "mandelbrot16-t15-linear-groupid-fixed10-escape-gradient-quiet-submit-finished-no-readback"
                } else if is_t11_linear_band {
                    "mandelbrot16-t11-linear-groupid-fixed10-escape-bw-quiet-submit-finished-no-readback"
                } else {
                    "mandelbrot16-simd16-q12-fixed10-escape-bw-quiet-submit-finished-no-readback"
                }
            }
        } else {
            "mandelbrot16-simd16-q12-onevis-quiet-submit-finished-no-readback"
        }
    } else if readback_ok && is_fixed_iter_visible {
        if is_fixed1_visible {
            "mandelbrot16-simd16-q12-fixed1-feedback-color-store-visible"
        } else {
            if is_t19_immediate_raw_radius {
                "mandelbrot16-t19-immediate-base-fixed1-raw-radius-color-store-visible"
            } else if is_t18_immediate_gradient {
                "mandelbrot16-t18-immediate-base-fixed10-escape-gradient-color-store-visible"
            } else if is_t15_linear_gradient {
                "mandelbrot16-t15-linear-groupid-fixed10-escape-gradient-color-store-visible"
            } else if is_t11_linear_band {
                "mandelbrot16-t11-linear-groupid-fixed10-escape-bw-color-store-visible"
            } else {
                "mandelbrot16-simd16-q12-fixed10-escape-bw-color-store-visible"
            }
        }
    } else if readback_ok && is_one_iter {
        if is_one_iter_visible {
            "mandelbrot16-simd16-q12-one-iteration-visible-color-store-visible"
        } else {
            "mandelbrot16-simd16-q12-one-iteration-real-store-visible"
        }
    } else if readback_ok && is_t16_linear_constant {
        "mandelbrot16-t16-linear-groupid-constant-color-store-visible"
    } else if readback_ok && is_t17_immediate_constant {
        "mandelbrot16-t17-immediate-base-constant-color-store-visible"
    } else if readback_ok {
        "mandelbrot16-simd16-alu-store-witness-visible"
    } else if !finished {
        "mandelbrot16-simd16-alu-store-witness-submit-not-finished"
    } else if dispatch_delta == 0 {
        "mandelbrot16-simd16-alu-store-witness-no-eu-dispatch"
    } else if hits == 0 {
        "mandelbrot16-simd16-alu-store-witness-first-lane-no-expected-value"
    } else {
        "mandelbrot16-simd16-alu-store-witness-first-lane-ok-readback-side-observation"
    };
    let proves = if !validate_readback && command_ok {
        if is_fixed_iter_visible {
            if is_fixed1_visible {
                "simd16-q12-fixed1-feedback-submit-eot-no-readback-visual-exercise"
            } else {
                if is_t19_immediate_raw_radius {
                    "t19-simd16-immediate-base-raw-radius-submit-eot-no-readback-visual-exercise"
                } else if is_t18_immediate_gradient {
                    "t18-simd16-immediate-base-gradient-submit-eot-no-readback-visual-exercise"
                } else if is_t15_linear_gradient {
                    "t15-simd16-linear-groupid-full-band-gradient-submit-eot-no-readback-visual-exercise"
                } else if is_t11_linear_band {
                    "t11-simd16-linear-groupid-full-band-submit-eot-no-readback-visual-exercise"
                } else {
                    "simd16-q12-fixed10-escape-bw-submit-eot-no-readback-visual-exercise"
                }
            }
        } else {
            "simd16-q12-onevis-submit-eot-no-readback-visual-exercise"
        }
    } else if readback_ok && is_fixed_iter_visible {
        if is_fixed1_visible {
            "simd16-q12-fixed1-feedback-store-eot-first-lane-validation-once"
        } else {
            if is_t19_immediate_raw_radius {
                "t19-simd16-immediate-base-fixed1-raw-radius-store-eot-first-lane-validation-once"
            } else if is_t18_immediate_gradient {
                "t18-simd16-immediate-base-fixed10-escape-gradient-store-eot-first-lane-validation-once"
            } else if is_t15_linear_gradient {
                "t15-simd16-linear-groupid-fixed10-escape-gradient-store-eot-first-lane-validation-once"
            } else if is_t11_linear_band {
                "t11-simd16-linear-groupid-fixed10-escape-bw-store-eot-first-lane-validation-once"
            } else {
                "simd16-q12-fixed10-escape-bw-store-eot-first-lane-validation-once"
            }
        }
    } else if readback_ok && is_one_iter_visible {
        "simd16-q12-one-iteration-visible-color-store-eot-first-lane-validation-once"
    } else if readback_ok && is_one_iter {
        "simd16-q12-one-iteration-real-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t16_linear_constant {
        "t16-simd16-linear-groupid-constant-store-eot-first-lane-validation-once"
    } else if readback_ok && is_t17_immediate_constant {
        "t17-simd16-immediate-base-constant-store-eot-first-lane-validation-once"
    } else if readback_ok {
        "simd16-q12-or-alu-store-eot-first-lane-validation-once"
    } else if dispatch_delta >= expected_hw_lane_dispatch {
        "simd16-q12-or-alu-dispatch-plus-store-mismatch"
    } else if dispatch_delta != 0 {
        "partial-eu-dispatch"
    } else {
        "no-eu-dispatch"
    };
    let artifact_body = if is_fixed1_visible {
        "simd16-q12-fixed1-feedback-visible-color-store"
    } else if is_t16_linear_constant {
        "t16-simd16-linear-groupid-constant-visible-color-store"
    } else if is_t17_immediate_constant {
        "t17-simd16-immediate-base-constant-visible-color-store"
    } else if is_fixed10_visible {
        if is_t19_immediate_raw_radius {
            "t19-simd16-immediate-base-fixed1-raw-radius-visible-color-store"
        } else if is_t18_immediate_gradient {
            "t18-simd16-immediate-base-fixed10-escape-gradient-visible-color-store"
        } else if is_t15_linear_gradient {
            "t15-simd16-linear-groupid-fixed10-escape-gradient-visible-color-store"
        } else if is_t11_linear_band {
            "t11-simd16-linear-groupid-fixed10-escape-bw-visible-color-store"
        } else {
            "simd16-q12-fixed10-escape-bw-visible-color-store"
        }
    } else if is_one_iter_visible {
        "simd16-q12-one-iteration-visible-color-store"
    } else if is_one_iter {
        "simd16-q12-one-iteration-real-store"
    } else {
        "simd16-q12-wide-mul-or-alu-store"
    };
    let eu_work = if is_fixed1_visible {
        "q12-z0-cre-cim-one-feedback-iteration-visible-store"
    } else if is_t16_linear_constant {
        "groupid-linear64-address-constant-visible-store"
    } else if is_t17_immediate_constant {
        "immediate-base-address-constant-visible-store"
    } else if is_fixed10_visible {
        if is_t19_immediate_raw_radius {
            "immediate-base-address-q12-fixed1-raw-radius-visible-store"
        } else if is_t18_immediate_gradient {
            "immediate-base-address-q12-fixed10-escape-gradient-visible-store"
        } else if is_t15_linear_gradient {
            "groupid-linear64-address-q12-fixed10-escape-gradient-visible-store"
        } else if is_t11_linear_band {
            "groupid-linear64-address-q12-fixed10-escape-bw-visible-store"
        } else {
            "q12-z0-cre-cim-fixed10-escape-bw-visible-store"
        }
    } else if is_one_iter_visible {
        "q12-cre-cim-re2-minus-im2-plus-cre-or-visible-mask-store"
    } else if is_one_iter {
        "q12-cre-cim-re2-minus-im2-plus-cre-store"
    } else {
        mandelbrot16_simd16_probe_variant_name(mode)
    };
    let one_iter_dword = if is_one_iter {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD
    } else {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ONE_ITER_DWORD
    };
    let store_send_dword = if is_fixed_iter_visible {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_FIXED10_STORE_SEND_DWORD
    } else if is_t16_linear_constant || is_t17_immediate_constant {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 8
    } else if is_one_iter {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD + 36
    } else {
        trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SEND_DWORD
    };
    let address_contract = match address_mode {
        Mandelbrot16AddressMode::GroupIdLinear64 => {
            "groupid-linear-tile-times-64-plus-base-plus-laneid-g20"
        }
        Mandelbrot16AddressMode::GroupIdRowPitch => {
            "groupid-row-times-pitch-plus-base-plus-laneid-g20"
        }
        Mandelbrot16AddressMode::ImmediateBase => {
            "legacy-immediate-base-plus-laneid-g20-validation"
        }
    };

    if validate_readback {
        crate::log!(
            "intel/gpgpu: primary-scanout-mandelbrot16-simd16-q12-plane y={} x_base={} row_groups={} row_offset=0x{:X} row_gpu=0x{:X} target_gpu=0x{:X} target_phys=0x{:X} target_virt=0x{:X} pitch_bytes={} byte_len=0x{:X} q12_lhs=0x{:08X} q12_rhs=0x{:08X} patched_color=0x{:08X} expected_plane_value=0x{:08X} artifact_body={} payload_contract=mesa-send16-address-g20-data-g22-bti1 dispatch_contract=simd16-t10-groupid-row-walker-v1 eu_math_lanes_mask=0x{:04X} eu_store_lanes_mask=0x{:04X} cpu_patched_lanes_mask=0x0000 eu_color_lanes=0 cpu_color_dwords_patched=0 eu_address_alu={} eu_alu_variant={} eu_store_value=g22 validation_scope=first-kickoff-lane0-only validation_lanes=1 logical_lanes={} hdc_store_sends={} expected_store_pixels={} hit_mask=0x{:04X} changed_mask=0x{:04X} row_group_hit_mask=0x{:016X} row_group_changed_mask=0x{:016X} row_first8=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] readback_ok={} reason={} first_before=0x{:08X} first_after=0x{:08X} expected_first=0x{:08X} after0=0x{:08X} after1=0x{:08X} after2=0x{:08X} after3=0x{:08X} after4=0x{:08X} after5=0x{:08X} after6=0x{:08X} after7=0x{:08X} after8=0x{:08X} after9=0x{:08X} after10=0x{:08X} after11=0x{:08X} after12=0x{:08X} after13=0x{:08X} after14=0x{:08X} after15=0x{:08X} display_notified={} notify_bytes=0x{:X} finish_marker=0x{:08X} lane_dispatch_delta={} expected_hw_lane_dispatch={} program_source={} address_base_dword={} color_dword={} one_iter_dword={} color_from_depth_dword={} store_send_dword={} proves={} next={} does_not_prove=full-frame-mandelbrot\n",
            y,
            x,
            row_groups,
            row_offset,
            row_gpu,
            target.gpu,
            target.phys,
            target.virt as usize,
            target.pitch_bytes,
            target.byte_len,
            lhs,
            rhs,
            patched_color,
            expected_first,
            artifact_body,
            mandelbrot16_active_lane_mask(),
            mandelbrot16_active_lane_mask(),
            address_contract,
            eu_work,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_STORE_SENDS,
            PIXELS,
            hits,
            changed,
            row_group_hit_mask,
            row_group_changed_mask,
            row_group_first_after[0],
            row_group_first_after[1],
            row_group_first_after[2],
            row_group_first_after[3],
            row_group_first_after[4],
            row_group_first_after[5],
            row_group_first_after[6],
            row_group_first_after[7],
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
            submit_span_bytes,
            finish_marker,
            dispatch_delta,
            expected_hw_lane_dispatch,
            program.name,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_ADDRESS_BASE_DWORD,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_DWORD,
            one_iter_dword,
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_COLOR_FROM_DEPTH_DWORD,
            store_send_dword,
            proves,
            if readback_ok && is_fixed_iter_visible {
                if is_fixed1_visible {
                    "expand-feedback-loop-to-fixed10"
                } else if is_t19_immediate_raw_radius {
                    "fix-gradient-compare-accumulator-or-add-count-gradient"
                } else if is_fixed10_gradient {
                    "increase-iteration-budget-or-refine-gradient"
                } else {
                    "increase-iteration-budget-or-add-count-gradient"
                }
            } else if readback_ok && is_one_iter {
                "add-z-imaginary-and-iteration-count-color"
            } else if readback_ok {
                "replace-witness-with-coordinate-and-iteration-body"
            } else if finished {
                "fix-simd16-q12-readback-or-store"
            } else {
                "fix-simd16-q12-submit-or-eot"
            },
        );
    }
    if !finished {
        recover_render_engine_after_nonretired_submit(
            dev,
            warm,
            "gpgpu-primary-scanout-mandelbrot16-simd16-q12-plane",
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
