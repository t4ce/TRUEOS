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
    patch_mandelbrot16_simd16_probe_variant(&mut words, mode, lhs, rhs, address_base, address_mode);
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
    if MANDELBROT16_CONSTANT_BODY_UPLOAD_DEBUG
        && (mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
            || mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE
            || mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
            || mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND
            || mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED
            || mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS
            || mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD
            || mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16
            || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
            || mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP
            || mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR)
    {
        let body = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
        crate::log!(
            "intel/gpgpu: mandelbrot16-simd16-constant-body-upload mode={} address_mode={} address_base=0x{:08X} prelude16=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] body32=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            mode,
            address_mode.label(),
            address_base,
            words[16],
            words[17],
            words[18],
            words[19],
            words[20],
            words[21],
            words[22],
            words[23],
            words[24],
            words[25],
            words[26],
            words[27],
            words[28],
            words[29],
            words[30],
            words[31],
            words[body],
            words[body + 1],
            words[body + 2],
            words[body + 3],
            words[body + 4],
            words[body + 5],
            words[body + 6],
            words[body + 7],
            words[body + 8],
            words[body + 9],
            words[body + 10],
            words[body + 11],
            words[body + 12],
            words[body + 13],
            words[body + 14],
            words[body + 15],
            words[body + 16],
            words[body + 17],
            words[body + 18],
            words[body + 19],
            words[body + 20],
            words[body + 21],
            words[body + 22],
            words[body + 23],
            words[body + 24],
            words[body + 25],
            words[body + 26],
            words[body + 27],
        );
    }
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
    const STORE_SEND8_G20_G22_1Q: [u32; 4] = [0x00030131, 0x00000000, 0xCC02140C, 0x009A160C];
    const STORE_SEND8_G21_G23_2Q: [u32; 4] = [0x00130131, 0x00000000, 0xCC02150C, 0x009A170C];
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
        patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G21_G23_2Q);
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
    patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
    patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G21_G23_2Q);
    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
    patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
}

fn patch_mandelbrot16_simd16_linear_constant_store_body(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    color: u32,
    address_base: u32,
    address_mode: Mandelbrot16AddressMode,
    store_variant: Mandelbrot16ConstantStoreVariant,
) {
    const NOP: [u32; 4] = [0x80000101, 0x00000000, 0x00000000, 0x00000000];
    const SUB_G22_G20_G20: [u32; 4] = [0x00040140, 0x16050660, 0x06461405, 0x02461405];
    const MOV_G22_G20: [u32; 4] = [0x00040161, 0x16050660, 0x00461405, 0x00000000];
    const OR_G22_G22_IMM: [u32; 4] = [0x00040166, 0x16058220, 0x02461605, 0x00000000];
    const ADD_G21_G20_32_1Q: [u32; 4] = [0x00030140, 0x15058660, 0x06461405, 0x00000020];
    const SUB_G22_G20_G20_1Q: [u32; 4] = [0x00030140, 0x16050660, 0x06461405, 0x02461405];
    const OR_G22_G22_IMM_1Q: [u32; 4] = [0x00030166, 0x16058220, 0x02461605, 0x00000000];
    const SUB_G23_G21_G21_1Q: [u32; 4] = [0x00030140, 0x17050660, 0x06461505, 0x02461505];
    const OR_G23_G23_IMM_1Q: [u32; 4] = [0x00030166, 0x17058220, 0x02461705, 0x00000000];
    const ADD_G20_G20_4_1Q: [u32; 4] = [0x00030140, 0x14058660, 0x06461405, 0x00000004];
    const STORE_SEND8_G20_G22_1Q: [u32; 4] = [0x00030131, 0x00000000, 0xCC02140C, 0x009A160C];
    const STORE_SEND8_G21_G23_2Q: [u32; 4] = [0x00130131, 0x00000000, 0xCC02150C, 0x009A170C];
    const STORE_SEND16_G20_G22: [u32; 4] = [0x00040131, 0x00000000, 0xCC021414, 0x00961614];
    const STORE_BTI1_G20_G22_1Q: [u32; 4] = [0x00030131, 0x00000000, 0x02026E01, 0x00000040];
    const STORE_BTI1_G21_G23_2Q: [u32; 4] = [0x00130131, 0x00000000, 0x02026E01, 0x00000040];
    const MOV_G126_G0: [u32; 4] = [0x80030061, 0x7E050220, 0x00460005, 0x00000000];
    const EOT_SEND_G126: [u32; 4] = [0x80030131, 0x00000004, 0x70007E0C, 0x00000000];

    let mut cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    let end = words.len();
    while cursor < end {
        patch_mandelbrot16_simd16_put(words, &mut cursor, NOP);
    }

    cursor = trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_BODY_DWORD;
    let mut or_color = OR_G22_G22_IMM;
    or_color[3] = color;
    let _ = (address_base, address_mode);
    if store_variant == Mandelbrot16ConstantStoreVariant::ExplicitWidePayload {
        let mut or_g22 = OR_G22_G22_IMM_1Q;
        let mut or_g23 = OR_G23_G23_IMM_1Q;
        or_g22[3] = color;
        or_g23[3] = color;
        patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G21_G20_32_1Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G22_G20_G20_1Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, or_g22);
        patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G23_G21_G21_1Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, or_g23);
        patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G21_G23_2Q);
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
        patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
        return;
    }
    if store_variant == Mandelbrot16ConstantStoreVariant::UnrolledScalar16
        || store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5
        || store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor
    {
        let mut or_g22 = OR_G22_G22_IMM_1Q;
        or_g22[3] = if store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor
        {
            0xFF00_0000
        } else {
            color
        };
        if store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor {
            patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G22_G20);
        } else {
            patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G22_G20_G20_1Q);
        }
        patch_mandelbrot16_simd16_put(words, &mut cursor, or_g22);
        let stamp_pixels = if store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5
            || store_variant == Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor
        {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES
                * MANDELBROT16_T38_STAMP_REPEATS as usize
        } else {
            trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_LANES
        };
        let mut pixel = 0usize;
        while pixel < stamp_pixels {
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
            pixel += 1;
            if pixel < stamp_pixels {
                patch_mandelbrot16_simd16_put(words, &mut cursor, ADD_G20_G20_4_1Q);
            }
        }
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
        patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
        return;
    }
    if store_variant == Mandelbrot16ConstantStoreVariant::AddressDataWitness {
        patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G22_G20);
    } else {
        patch_mandelbrot16_simd16_put(words, &mut cursor, SUB_G22_G20_G20);
    }
    patch_mandelbrot16_simd16_put(words, &mut cursor, or_color);
    match store_variant {
        Mandelbrot16ConstantStoreVariant::SingleSend => {
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND16_G20_G22);
        }
        Mandelbrot16ConstantStoreVariant::Bti1Untyped => {
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_BTI1_G20_G22_1Q);
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_BTI1_G21_G23_2Q);
        }
        Mandelbrot16ConstantStoreVariant::LegacyStateless => {
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G21_G23_2Q);
        }
        Mandelbrot16ConstantStoreVariant::AddressDataWitness => {
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G20_G22_1Q);
            patch_mandelbrot16_simd16_put(words, &mut cursor, STORE_SEND8_G21_G23_2Q);
        }
        Mandelbrot16ConstantStoreVariant::ExplicitWidePayload => {}
        Mandelbrot16ConstantStoreVariant::UnrolledScalar16 => {}
        Mandelbrot16ConstantStoreVariant::WideScalar16x5 => {}
        Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor => {}
    }
    patch_mandelbrot16_simd16_put(words, &mut cursor, MOV_G126_G0);
    patch_mandelbrot16_simd16_put(words, &mut cursor, EOT_SEND_G126);
}

fn patch_mandelbrot16_simd16_probe_variant(
    words: &mut [u32; trueos_eu::gfx12::PRIMARY_SCANOUT_MANDELBROT16_SIMD16_BW_WORD_DWORDS],
    mode: u32,
    lhs: u32,
    rhs: u32,
    address_base: u32,
    address_mode: Mandelbrot16AddressMode,
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
    const STORE_SEND8_G20_G22_1Q: [u32; 4] = [0x00030131, 0x00000000, 0xCC02140C, 0x009A160C];
    const STORE_SEND8_G21_G23_2Q: [u32; 4] = [0x00130131, 0x00000000, 0xCC02150C, 0x009A170C];
    const MOV_G126_G0: [u32; 4] = [0x80030061, 0x7E050220, 0x00460005, 0x00000000];
    const EOT_SEND_G126: [u32; 4] = [0x80030131, 0x00000004, 0x70007E0C, 0x00000000];

    if mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE
        || mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND
        || mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED
        || mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS
        || mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD
        || mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP
        || mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR
    {
        let store_variant = if mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND {
            Mandelbrot16ConstantStoreVariant::SingleSend
        } else if mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED {
            Mandelbrot16ConstantStoreVariant::Bti1Untyped
        } else if mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS {
            Mandelbrot16ConstantStoreVariant::AddressDataWitness
        } else if mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD {
            Mandelbrot16ConstantStoreVariant::ExplicitWidePayload
        } else if mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
            || mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16
            || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
        {
            Mandelbrot16ConstantStoreVariant::UnrolledScalar16
        } else if mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP {
            Mandelbrot16ConstantStoreVariant::WideScalar16x5
        } else if mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR {
            Mandelbrot16ConstantStoreVariant::WideScalar16x5AddressColor
        } else {
            Mandelbrot16ConstantStoreVariant::LegacyStateless
        };
        patch_mandelbrot16_simd16_linear_constant_store_body(
            words,
            lhs,
            address_base,
            address_mode,
            store_variant,
        );
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
                STORE_SEND8_G20_G22_1Q,
                STORE_SEND8_G21_G23_2Q,
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
                STORE_SEND8_G20_G22_1Q,
                STORE_SEND8_G21_G23_2Q,
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
        || mode == MANDELBROT16_T16_MODE_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T17_MODE_IMMEDIATE_CONSTANT_STORE
        || mode == MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE
        || mode == MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND
        || mode == MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED
        || mode == MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS
        || mode == MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD
        || mode == MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16
        || mode == MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP
        || mode == MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR
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
        MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE => {
            "mandelbrot16-t30-fullscreen-linear-groupid-unrolled-scalar16-visible-store"
        }
        MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND => {
            "mandelbrot16-t32-immediate-base-constant-single-send-visible-store"
        }
        MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED => {
            "mandelbrot16-t33-immediate-base-constant-bti1-untyped-visible-store"
        }
        MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS => {
            "mandelbrot16-t34-immediate-base-address-data-witness-visible-store"
        }
        MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD => {
            "mandelbrot16-t35-immediate-base-explicit-wide-payload-visible-store"
        }
        MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16 => {
            "mandelbrot16-t36-immediate-base-unrolled-scalar16-visible-store"
        }
        MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16 => {
            "mandelbrot16-t37-groupid-x-unrolled-scalar16-visible-store"
        }
        MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP => {
            "mandelbrot16-t38-immediate-base-wide-stamp-visible-store"
        }
        MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR => {
            "mandelbrot16-t39-immediate-base-wide-stamp-address-color-visible-store"
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
        MANDELBROT16_T30_MODE_FULLSCREEN_LINEAR_CONSTANT_STORE => lhs,
        MANDELBROT16_T32_MODE_IMMEDIATE_CONSTANT_SINGLE_SEND => lhs,
        MANDELBROT16_T33_MODE_IMMEDIATE_CONSTANT_BTI1_UNTYPED => lhs,
        MANDELBROT16_T34_MODE_IMMEDIATE_ADDRESS_DATA_WITNESS => lhs | rhs,
        MANDELBROT16_T35_MODE_IMMEDIATE_EXPLICIT_WIDE_PAYLOAD => lhs,
        MANDELBROT16_T36_MODE_IMMEDIATE_UNROLLED_SCALAR16 => lhs,
        MANDELBROT16_T37_MODE_GROUPID_X_UNROLLED_SCALAR16 => lhs,
        MANDELBROT16_T38_MODE_IMMEDIATE_WIDE_STAMP => lhs,
        MANDELBROT16_T39_MODE_IMMEDIATE_WIDE_STAMP_ADDRESS_COLOR => lhs,
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
