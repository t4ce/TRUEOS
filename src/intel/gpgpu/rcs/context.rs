fn direct_rcs_init_lrc_context_image(
    state: DirectRcsState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let lrc = &mut dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS..];
    if lrc.len() < 192 {
        return false;
    }

    lrc[0] = MI_NOOP;
    let mut idx = 1usize;

    lrc[idx] = direct_rcs_mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2244;
    lrc[idx + 1] = direct_rcs_ctx_control_value(false);
    lrc[idx + 2] = 0x2034;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2030;
    lrc[idx + 5] = ring_tail;
    lrc[idx + 6] = 0x2038;
    lrc[idx + 7] = ring_start;
    lrc[idx + 8] = 0x203C;
    lrc[idx + 9] = ring_ctl;
    lrc[idx + 10] = 0x2168;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2140;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2110;
    lrc[idx + 15] = 0;
    lrc[idx + 16] = 0x211C;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2114;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x2118;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x21C0;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x21C4;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x21C8;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2180;
    lrc[idx + 29] = 0;
    idx += 30;

    direct_rcs_push_nops(lrc, &mut idx, 5);

    lrc[idx] = direct_rcs_mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x23A8;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x228C;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2288;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2284;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2280;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x227C;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2278;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x2274;
    lrc[idx + 15] = (state.ppgtt_phys >> 32) as u32;
    lrc[idx + 16] = 0x2270;
    lrc[idx + 17] = state.ppgtt_phys as u32;
    idx += 18;

    lrc[idx] = direct_rcs_mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x21B0;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x25A8;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x25AC;
    lrc[idx + 5] = 0;
    idx += 6;

    direct_rcs_push_nops(lrc, &mut idx, 6);

    lrc[idx] = direct_rcs_mi_lri_cmd(1, 0);
    idx += 1;
    lrc[idx] = 0x20C8;
    lrc[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    direct_rcs_push_nops(lrc, &mut idx, 13);

    lrc[idx] = direct_rcs_mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    lrc[idx] = 0x2588;
    lrc[idx + 1] = 0;
    lrc[idx + 2] = 0x2588;
    lrc[idx + 3] = 0;
    lrc[idx + 4] = 0x2588;
    lrc[idx + 5] = 0;
    lrc[idx + 6] = 0x2588;
    lrc[idx + 7] = 0;
    lrc[idx + 8] = 0x2588;
    lrc[idx + 9] = 0;
    lrc[idx + 10] = 0x2588;
    lrc[idx + 11] = 0;
    lrc[idx + 12] = 0x2028;
    lrc[idx + 13] = 0;
    lrc[idx + 14] = 0x209C;
    lrc[idx + 15] = direct_rcs_masked_bit_disable(RING_MI_MODE_STOP_RING);
    lrc[idx + 16] = 0x20C0;
    lrc[idx + 17] = 0;
    lrc[idx + 18] = 0x2178;
    lrc[idx + 19] = 0;
    lrc[idx + 20] = 0x217C;
    lrc[idx + 21] = 0;
    lrc[idx + 22] = 0x2358;
    lrc[idx + 23] = 0;
    lrc[idx + 24] = 0x2170;
    lrc[idx + 25] = 0;
    lrc[idx + 26] = 0x2150;
    lrc[idx + 27] = 0;
    lrc[idx + 28] = 0x2154;
    lrc[idx + 29] = 0;
    lrc[idx + 30] = 0x2158;
    lrc[idx + 31] = 0;
    lrc[idx + 32] = 0x241C;
    lrc[idx + 33] = 0;
    lrc[idx + 34] = 0x2600;
    lrc[idx + 35] = 0;
    lrc[idx + 36] = 0x2604;
    lrc[idx + 37] = 0;
    lrc[idx + 38] = 0x2608;
    lrc[idx + 39] = 0;
    lrc[idx + 40] = 0x260C;
    lrc[idx + 41] = 0;
    lrc[idx + 42] = 0x2610;
    lrc[idx + 43] = 0;
    lrc[idx + 44] = 0x2614;
    lrc[idx + 45] = 0;
    lrc[idx + 46] = 0x2618;
    lrc[idx + 47] = 0;
    lrc[idx + 48] = 0x261C;
    lrc[idx + 49] = 0;
    lrc[idx + 50] = 0x2620;
    lrc[idx + 51] = 0;
    lrc[idx + 52] = 0x2624;
    lrc[idx + 53] = 0;
    lrc[idx + 54] = 0x2628;
    lrc[idx + 55] = 0;
    lrc[idx + 56] = 0x262C;
    lrc[idx + 57] = 0;
    lrc[idx + 58] = 0x2630;
    lrc[idx + 59] = 0;
    lrc[idx + 60] = 0x2634;
    lrc[idx + 61] = 0;
    lrc[idx + 62] = 0x2638;
    lrc[idx + 63] = 0;
    lrc[idx + 64] = 0x263C;
    lrc[idx + 65] = 0;
    lrc[idx + 66] = 0x2640;
    lrc[idx + 67] = 0;
    lrc[idx + 68] = 0x2644;
    lrc[idx + 69] = 0;
    lrc[idx + 70] = 0x2648;
    lrc[idx + 71] = 0;
    lrc[idx + 72] = 0x264C;
    lrc[idx + 73] = 0;
    lrc[idx + 74] = 0x2650;
    lrc[idx + 75] = 0;
    lrc[idx + 76] = 0x2654;
    lrc[idx + 77] = 0;
    lrc[idx + 78] = 0x2658;
    lrc[idx + 79] = 0;
    lrc[idx + 80] = 0x265C;
    lrc[idx + 81] = 0;
    lrc[idx + 82] = 0x2660;
    lrc[idx + 83] = 0;
    lrc[idx + 84] = 0x2664;
    lrc[idx + 85] = 0;
    lrc[idx + 86] = 0x2668;
    lrc[idx + 87] = 0;
    lrc[idx + 88] = 0x266C;
    lrc[idx + 89] = 0;
    lrc[idx + 90] = 0x2670;
    lrc[idx + 91] = 0;
    lrc[idx + 92] = 0x2674;
    lrc[idx + 93] = 0;
    lrc[idx + 94] = 0x2678;
    lrc[idx + 95] = 0;
    lrc[idx + 96] = 0x267C;
    lrc[idx + 97] = 0;
    lrc[idx + 98] = 0x2068;
    lrc[idx + 99] = 0;
    lrc[idx + 100] = 0x2084;
    lrc[idx + 101] = 0;
    idx += 102;

    lrc[idx] = MI_NOOP;
    idx += 1;
    lrc[idx] = MI_BATCH_BUFFER_END | 1;

    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
    true
}

fn direct_rcs_write_lrc_ring_tail(state: DirectRcsState, ring_tail: u32) {
    const LRC_CONTEXT_CONTROL_VALUE_DW: usize = 3;
    const LRC_RING_TAIL_VALUE_DW: usize = 7;

    let total_dwords = DIRECT_RCS_CONTEXT_BYTES / core::mem::size_of::<u32>();
    if total_dwords <= DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW {
        return;
    }
    let dwords =
        unsafe { core::slice::from_raw_parts_mut(state.context_virt as *mut u32, total_dwords) };
    let ctx_ctl = dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW];
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_RING_TAIL_VALUE_DW] = ring_tail;
    dwords[DIRECT_RCS_LRC_STATE_OFFSET_DWORDS + LRC_CONTEXT_CONTROL_VALUE_DW] = ctx_ctl;
    super::dma_flush(state.context_virt, DIRECT_RCS_CONTEXT_BYTES);
}

fn guc_rcs_context_descriptor(context_gpu_addr: u64) -> (u32, u32) {
    let base = (context_gpu_addr as u32) & 0xFFFF_F000;
    let descriptor = base
        | GEN8_CTX_VALID
        | GEN8_CTX_PRIVILEGE
        | (INTEL_LEGACY_64B_CONTEXT << GEN8_CTX_ADDRESSING_MODE_SHIFT);
    (descriptor, (context_gpu_addr >> 32) as u32)
}

fn direct_rcs_ring_ctl_value(size: usize) -> Option<u32> {
    let size = u32::try_from(size).ok()?;
    Some(size.checked_sub(4096)? | RING_VALID)
}

fn direct_rcs_ctx_control_value(inhibit_restore: bool) -> u32 {
    let mut ctl = direct_rcs_masked_bits_update(
        CTX_CTRL_INHIBIT_SYN_CTX_SWITCH,
        CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT,
    );
    if inhibit_restore {
        ctl |= CTX_CTRL_ENGINE_CTX_RESTORE_INHIBIT;
    }
    ctl
}

fn direct_rcs_wait_eq(dev: super::Dev, reg: usize, mask: u32, want: u32, n: usize) -> bool {
    for _ in 0..n {
        if (super::mmio_read(dev, reg) & mask) == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn direct_rcs_mi_lri_cmd(num_regs: u32, flags: u32) -> u32 {
    MI_LOAD_REGISTER_IMM | MI_LRI_CS_MMIO | flags | num_regs.saturating_mul(2).saturating_sub(1)
}

fn direct_rcs_push_nops(state: &mut [u32], idx: &mut usize, count: usize) {
    for _ in 0..count {
        state[*idx] = MI_NOOP;
        *idx += 1;
    }
}

fn direct_rcs_masked_bit_enable(bit: u32) -> u32 {
    bit | (bit << 16)
}

fn direct_rcs_masked_bit_disable(bit: u32) -> u32 {
    bit << 16
}

fn direct_rcs_masked_bits_update(set_bits: u32, clear_bits: u32) -> u32 {
    let update = set_bits | clear_bits;
    set_bits | (update << 16)
}

fn align_up(value: usize, align: usize) -> Option<usize> {
    let mask = align.checked_sub(1)?;
    value.checked_add(mask).map(|v| v & !mask)
}
