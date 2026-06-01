fn init_gen12_lrc_context_image(
    warm: RenderWarmState,
    ring_start: u32,
    ring_tail: u32,
    ring_ctl: u32,
) -> bool {
    let total_dwords = warm.context_len / core::mem::size_of::<u32>();
    if total_dwords <= LRC_STATE_OFFSET_DWORDS {
        return false;
    }

    let dwords =
        unsafe { core::slice::from_raw_parts_mut(warm.context_virt as *mut u32, total_dwords) };
    dwords.fill(0);

    let state = &mut dwords[LRC_STATE_OFFSET_DWORDS..];
    if state.len() < 192 {
        return false;
    }

    state[0] = MI_NOOP;
    let mut idx = 1usize;

    state[idx] = mi_lri_cmd(13, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2244;
    state[idx + 1] = rcs_ctx_control_value(false);
    state[idx + 2] = 0x2034;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2030;
    state[idx + 5] = ring_tail;
    state[idx + 6] = 0x2038;
    state[idx + 7] = ring_start;
    state[idx + 8] = 0x203C;
    state[idx + 9] = ring_ctl;
    state[idx + 10] = 0x2168;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2140;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2110;
    state[idx + 15] = 0;
    state[idx + 16] = 0x211C;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2114;
    state[idx + 19] = 0;
    state[idx + 20] = 0x2118;
    state[idx + 21] = 0;
    state[idx + 22] = 0x21C0;
    state[idx + 23] = 0;
    state[idx + 24] = 0x21C4;
    state[idx + 25] = 0;
    state[idx + 26] = 0x21C8;
    state[idx + 27] = GEN12_CTX_RCS_INDIRECT_CTX_OFFSET_DEFAULT;
    state[idx + 28] = 0x2180;
    state[idx + 29] = 0;
    idx += 30;

    push_mi_nops(state, &mut idx, 5);

    state[idx] = mi_lri_cmd(9, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x23A8;
    state[idx + 1] = 0;
    state[idx + 2] = 0x228C;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2288;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2284;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2280;
    state[idx + 9] = 0;
    state[idx + 10] = 0x227C;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2278;
    state[idx + 13] = 0;
    state[idx + 14] = 0x2274;
    state[idx + 15] = 0;
    state[idx + 16] = 0x2270;
    state[idx + 17] = 0;
    idx += 18;

    state[idx] = mi_lri_cmd(3, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x21B0;
    state[idx + 1] = 0;
    state[idx + 2] = 0x25A8;
    state[idx + 3] = 0;
    state[idx + 4] = 0x25AC;
    state[idx + 5] = 0;
    idx += 6;

    push_mi_nops(state, &mut idx, 6);

    state[idx] = mi_lri_cmd(1, 0);
    idx += 1;
    state[idx] = 0x20C8;
    state[idx + 1] = 0x7FFF_FFFF;
    idx += 2;

    push_mi_nops(state, &mut idx, 13);

    state[idx] = mi_lri_cmd(51, MI_LRI_FORCE_POSTED);
    idx += 1;
    state[idx] = 0x2588;
    state[idx + 1] = 0;
    state[idx + 2] = 0x2588;
    state[idx + 3] = 0;
    state[idx + 4] = 0x2588;
    state[idx + 5] = 0;
    state[idx + 6] = 0x2588;
    state[idx + 7] = 0;
    state[idx + 8] = 0x2588;
    state[idx + 9] = 0;
    state[idx + 10] = 0x2588;
    state[idx + 11] = 0;
    state[idx + 12] = 0x2028;
    state[idx + 13] = 0;
    state[idx + 14] = 0x209C;
    state[idx + 15] = masked_bit_disable(RING_MI_MODE_STOP_RING);
    state[idx + 16] = 0x20C0;
    state[idx + 17] = 0;
    state[idx + 18] = 0x2178;
    state[idx + 19] = 0;
    state[idx + 20] = 0x217C;
    state[idx + 21] = 0;
    state[idx + 22] = 0x2358;
    state[idx + 23] = 0;
    state[idx + 24] = 0x2170;
    state[idx + 25] = 0;
    state[idx + 26] = 0x2150;
    state[idx + 27] = 0;
    state[idx + 28] = 0x2154;
    state[idx + 29] = 0;
    state[idx + 30] = 0x2158;
    state[idx + 31] = 0;
    state[idx + 32] = 0x241C;
    state[idx + 33] = 0;
    state[idx + 34] = 0x2600;
    state[idx + 35] = 0;
    state[idx + 36] = 0x2604;
    state[idx + 37] = 0;
    state[idx + 38] = 0x2608;
    state[idx + 39] = 0;
    state[idx + 40] = 0x260C;
    state[idx + 41] = 0;
    state[idx + 42] = 0x2610;
    state[idx + 43] = 0;
    state[idx + 44] = 0x2614;
    state[idx + 45] = 0;
    state[idx + 46] = 0x2618;
    state[idx + 47] = 0;
    state[idx + 48] = 0x261C;
    state[idx + 49] = 0;
    state[idx + 50] = 0x2620;
    state[idx + 51] = 0;
    state[idx + 52] = 0x2624;
    state[idx + 53] = 0;
    state[idx + 54] = 0x2628;
    state[idx + 55] = 0;
    state[idx + 56] = 0x262C;
    state[idx + 57] = 0;
    state[idx + 58] = 0x2630;
    state[idx + 59] = 0;
    state[idx + 60] = 0x2634;
    state[idx + 61] = 0;
    state[idx + 62] = 0x2638;
    state[idx + 63] = 0;
    state[idx + 64] = 0x263C;
    state[idx + 65] = 0;
    state[idx + 66] = 0x2640;
    state[idx + 67] = 0;
    state[idx + 68] = 0x2644;
    state[idx + 69] = 0;
    state[idx + 70] = 0x2648;
    state[idx + 71] = 0;
    state[idx + 72] = 0x264C;
    state[idx + 73] = 0;
    state[idx + 74] = 0x2650;
    state[idx + 75] = 0;
    state[idx + 76] = 0x2654;
    state[idx + 77] = 0;
    state[idx + 78] = 0x2658;
    state[idx + 79] = 0;
    state[idx + 80] = 0x265C;
    state[idx + 81] = 0;
    state[idx + 82] = 0x2660;
    state[idx + 83] = 0;
    state[idx + 84] = 0x2664;
    state[idx + 85] = 0;
    state[idx + 86] = 0x2668;
    state[idx + 87] = 0;
    state[idx + 88] = 0x266C;
    state[idx + 89] = 0;
    state[idx + 90] = 0x2670;
    state[idx + 91] = 0;
    state[idx + 92] = 0x2674;
    state[idx + 93] = 0;
    state[idx + 94] = 0x2678;
    state[idx + 95] = 0;
    state[idx + 96] = 0x267C;
    state[idx + 97] = 0;
    state[idx + 98] = 0x2068;
    state[idx + 99] = 0;
    state[idx + 100] = 0x2084;
    state[idx + 101] = 0;
    idx += 102;

    state[idx] = MI_NOOP;
    idx += 1;
    state[idx] = MI_BATCH_BUFFER_END | 1;

    crate::intel::dma_flush(warm.context_virt, warm.context_len);
    true
}

fn encode_rgb_triangle_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w < TRIANGLE_MIN_DIM || rect_h < TRIANGLE_MIN_DIM {
        return Err("triangle-too-small");
    }

    let tri_w = rect_w.min(TRIANGLE_MAX_W).max(TRIANGLE_MIN_DIM);
    let tri_h = rect_h.min(TRIANGLE_MAX_H).max(TRIANGLE_MIN_DIM);
    let origin_x = rect_w.saturating_sub(tri_w) / 2;
    let origin_y = rect_h.saturating_sub(tri_h) / 2;
    let v0x = origin_x as i32 + (tri_w as i32 / 2);
    let v0y = origin_y as i32;
    let v1x = origin_x as i32;
    let v1y = origin_y as i32 + tri_h.saturating_sub(1) as i32;
    let v2x = origin_x as i32 + tri_w.saturating_sub(1) as i32;
    let v2y = v1y;
    let area = edge_fn(v0x, v0y, v1x, v1y, v2x, v2y);
    if area == 0 {
        return Err("triangle-degenerate");
    }

    let min_x = v0x.min(v1x).min(v2x).max(0) as usize;
    let max_x = (v0x.max(v1x).max(v2x) + 1).min(rect_w as i32) as usize;
    let min_y = v0y.min(v1y).min(v2y).max(0) as usize;
    let max_y = (v0y.max(v1y).max(v2y) + 1).min(rect_h as i32) as usize;

    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let mut idx = 0usize;

    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = (x as i32) * 2 + 1;
            let py = (y as i32) * 2 + 1;
            let w0 = edge_fn2(v1x, v1y, v2x, v2y, px, py);
            let w1 = edge_fn2(v2x, v2y, v0x, v0y, px, py);
            let w2 = edge_fn2(v0x, v0y, v1x, v1y, px, py);
            if !same_sign_or_zero(area, w0)
                || !same_sign_or_zero(area, w1)
                || !same_sign_or_zero(area, w2)
            {
                continue;
            }
            if idx + STORE_DWORDS > writable_limit {
                return Err("batch-exhausted");
            }

            let r = bary_to_u8(w0, area);
            let g = bary_to_u8(w1, area);
            let b = bary_to_u8(w2, area);
            let color = pack_xrgb8888(r, g, b);
            let dst = dst_gpu_addr
                .saturating_add((y as u64).saturating_mul(pitch as u64))
                .saturating_add((x as u64).saturating_mul(4));

            batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
            batch_dwords[idx + 1] = dst as u32;
            batch_dwords[idx + 2] = (dst >> 32) as u32;
            batch_dwords[idx + 3] = color;
            idx += STORE_DWORDS;
        }
    }

    if idx == 0 {
        return Err("triangle-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("batch-no-result-slot");
    }

    batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += RESERVED_END_DWORDS;
    Ok(idx * core::mem::size_of::<u32>())
}

fn encode_result_store_probe_batch(
    batch_dwords: &mut [u32],
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    if batch_dwords.len() < 6 {
        return Err("batch-too-small");
    }

    batch_dwords[0] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[1] = result_gpu_addr as u32;
    batch_dwords[2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[3] = done_value;
    batch_dwords[4] = MI_BATCH_BUFFER_END;
    batch_dwords[5] = MI_NOOP;
    Ok(6 * core::mem::size_of::<u32>())
}

fn encode_single_store_probe_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    value: u32,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    if batch_dwords.len() < 10 {
        return Err("batch-too-small");
    }

    batch_dwords[0] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[1] = dst_gpu_addr as u32;
    batch_dwords[2] = (dst_gpu_addr >> 32) as u32;
    batch_dwords[3] = value;
    batch_dwords[4] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[5] = result_gpu_addr as u32;
    batch_dwords[6] = (result_gpu_addr >> 32) as u32;
    batch_dwords[7] = done_value;
    batch_dwords[8] = MI_BATCH_BUFFER_END;
    batch_dwords[9] = MI_NOOP;
    Ok(10 * core::mem::size_of::<u32>())
}

fn encode_vertical_stripe_store_batch(
    batch_dwords: &mut [u32],
    dst_gpu_addr: u64,
    pitch: usize,
    rect_w: usize,
    rect_h: usize,
    x_phase: u32,
    result_gpu_addr: u64,
    done_value: u32,
) -> Result<usize, &'static str> {
    const RESERVED_END_DWORDS: usize = 2;
    const STORE_DWORDS: usize = 4;

    if batch_dwords.len() <= RESERVED_END_DWORDS + STORE_DWORDS {
        return Err("batch-too-small");
    }
    if rect_w == 0 || rect_h == 0 {
        return Err("stripe-empty-target");
    }

    let writable_limit = batch_dwords
        .len()
        .saturating_sub(RESERVED_END_DWORDS + STORE_DWORDS);
    let colors = [
        pack_xrgb8888(0xFF, 0x00, 0x00),
        pack_xrgb8888(0xFF, 0x80, 0x00),
        pack_xrgb8888(0xFF, 0xFF, 0x00),
        pack_xrgb8888(0x00, 0xFF, 0x00),
        pack_xrgb8888(0x00, 0xA0, 0xFF),
        pack_xrgb8888(0xFF, 0x00, 0xFF),
    ];
    let mut idx = 0usize;
    let phase = if rect_w == 0 {
        0
    } else {
        (x_phase as usize) % rect_w
    };

    for stripe_idx in 0..MI_STRIPE_COUNT {
        let center = ((((stripe_idx + 1) * rect_w) / (MI_STRIPE_COUNT + 1)) + phase) % rect_w;
        let x0 = center + rect_w - (MI_STRIPE_WIDTH_PX / 2);
        let color = colors[stripe_idx % colors.len()];
        for y in 0..rect_h {
            for stripe_dx in 0..MI_STRIPE_WIDTH_PX {
                let x = (x0 + stripe_dx) % rect_w;
                if idx + STORE_DWORDS > writable_limit {
                    return Err("stripe-batch-exhausted");
                }
                let dst = dst_gpu_addr
                    .saturating_add((y as u64).saturating_mul(pitch as u64))
                    .saturating_add((x as u64).saturating_mul(4));
                batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
                batch_dwords[idx + 1] = dst as u32;
                batch_dwords[idx + 2] = (dst >> 32) as u32;
                batch_dwords[idx + 3] = color;
                idx += STORE_DWORDS;
            }
        }
    }

    if idx == 0 {
        return Err("stripe-empty");
    }
    if idx + STORE_DWORDS > batch_dwords.len().saturating_sub(RESERVED_END_DWORDS) {
        return Err("stripe-no-result-slot");
    }

    batch_dwords[idx] = MI_STORE_DATA_IMM_GGTT_DW1;
    batch_dwords[idx + 1] = result_gpu_addr as u32;
    batch_dwords[idx + 2] = (result_gpu_addr >> 32) as u32;
    batch_dwords[idx + 3] = done_value;
    idx += STORE_DWORDS;

    batch_dwords[idx] = MI_BATCH_BUFFER_END;
    batch_dwords[idx + 1] = MI_NOOP;
    idx += RESERVED_END_DWORDS;
    Ok(idx * core::mem::size_of::<u32>())
}

fn edge_fn(ax: i32, ay: i32, bx: i32, by: i32, px: i32, py: i32) -> i64 {
    let ax = ax as i64;
    let ay = ay as i64;
    let bx = bx as i64;
    let by = by as i64;
    let px = px as i64;
    let py = py as i64;
    (px - ax) * (by - ay) - (py - ay) * (bx - ax)
}

fn edge_fn2(ax: i32, ay: i32, bx: i32, by: i32, px2: i32, py2: i32) -> i64 {
    let ax2 = (ax * 2) as i64;
    let ay2 = (ay * 2) as i64;
    let bx2 = (bx * 2) as i64;
    let by2 = (by * 2) as i64;
    let px2 = px2 as i64;
    let py2 = py2 as i64;
    (px2 - ax2) * (by2 - ay2) - (py2 - ay2) * (bx2 - ax2)
}

fn same_sign_or_zero(area: i64, value: i64) -> bool {
    if area >= 0 { value >= 0 } else { value <= 0 }
}

fn bary_to_u8(weight: i64, area: i64) -> u32 {
    let num = weight.unsigned_abs().saturating_mul(255);
    let den = area.unsigned_abs().max(1);
    ((num + (den / 2)) / den).min(255) as u32
}

fn pack_xrgb8888(r: u32, g: u32, b: u32) -> u32 {
    (r << 16) | (g << 8) | b
}

struct CursorPlaneCaps {
    platform: &'static str,
    layout: &'static str,
    max_width: u16,
    max_height: u16,
    pipe_count: u8,
}

struct SpritePlaneCaps {
    platform: &'static str,
    display_ver: u8,
    pipe_count: u8,
    overlays_per_pipe: u8,
    rotation: &'static str,
    reflect_x: bool,
    csc: &'static str,
    scaling_filter: &'static str,
    damage_clips: bool,
}

fn cursor_plane_caps(device_id: u16) -> CursorPlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => CursorPlaneCaps {
            platform: "ADL-S",
            layout: "TGL/XE_D",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => CursorPlaneCaps {
            platform: "ADL-P/N",
            layout: "TGL/XE_LPD",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
        _ => CursorPlaneCaps {
            platform: "unknown",
            layout: "generic",
            max_width: 256,
            max_height: 256,
            pipe_count: 4,
        },
    }
}

fn sprite_plane_caps(device_id: u16) -> SpritePlaneCaps {
    match device_id {
        0x4680 | 0x4682 | 0x4688 | 0x468A | 0x468B | 0x4690 | 0x4692 | 0x4693 => SpritePlaneCaps {
            platform: "ADL-S",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        0x46A0 | 0x46A1 | 0x46A2 | 0x46A3 | 0x46A6 | 0x46A8 | 0x46AA | 0x462A | 0x4626 | 0x4628
        | 0x46B0 | 0x46B1 | 0x46B2 | 0x46B3 => SpritePlaneCaps {
            platform: "ADL-P/N",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
        _ => SpritePlaneCaps {
            platform: "unknown",
            display_ver: 13,
            pipe_count: 4,
            overlays_per_pipe: 4,
            rotation: "0|180",
            reflect_x: true,
            csc: "BT601|BT709|BT2020",
            scaling_filter: "default|nearest",
            damage_clips: true,
        },
    }
}
