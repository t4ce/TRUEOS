#![allow(dead_code)]

use super::*;

#[derive(Copy, Clone)]
struct GpgpuOneRowMatvecProfile {
    program: GpgpuEuProgram,
    live_k_dim: usize,
    expected_sentinel: u32,
    requires_live_gpu_load: bool,
    scale_ladder: &'static [u32],
    log_prefix: &'static str,
    scale_prefix: &'static str,
    summary_label: &'static str,
    submit_label: &'static str,
    success_class: &'static str,
    success_reason: &'static str,
    success_reason_no_ts: &'static str,
    surface_note: &'static str,
}

#[derive(Copy, Clone)]
struct GpgpuPartialMatvecProfile {
    program: GpgpuEuProgram,
    live_k_dim: usize,
    partial_rows: usize,
    clear_output_before_submit: bool,
    log_label: &'static str,
    submit_label: &'static str,
    surface_note: &'static str,
    success_reason: &'static str,
    success_next: &'static str,
    failure_next: &'static str,
}

fn gpgpu_one_tile_output_compare_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: "gfx12-t48-one-tile-output-compare-dp4a-echo-hdc1-stateless-store-then-ts-eot",
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::HDC1_STATELESS_STATIC_DP4A_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::HDC1_STATELESS_STATIC_DP4A_BASE_DWORD),
    }
}

fn gpgpu_t5_one_row_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_SENTINEL_DWORD),
    }
}

fn gpgpu_t6_one_row_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T6_SMALL_LIVE8_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T6_SMALL_LIVE8_TRUEOS_ARENA_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::T6_SMALL_LIVE8_TRUEOS_ARENA_SENTINEL_DWORD),
    }
}

fn gpgpu_t61_one_row_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T61_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T61_LIVE16_TRUEOS_ARENA_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::T61_LIVE16_TRUEOS_ARENA_SENTINEL_DWORD),
    }
}

fn gpgpu_t62_partial_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T62_ROW_INDEXED_LIVE16_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T62_ROW_INDEXED_LIVE16_STORE_SEND_DWORD),
        visible_seed_dword: None,
    }
}

fn gpgpu_t63_partial_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T63_LANE_INDEXED_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T63_LANE_INDEXED_LIVE32_STORE_SEND_DWORD),
        visible_seed_dword: None,
    }
}

fn gpgpu_t63_accum16_hi_live32_partial_matvec_program() -> GpgpuEuProgram {
    let artifact =
        trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_TRUEOS_ARENA_BF16_DOT_HDC1_STATELESS_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: artifact.name,
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_STORE_SEND_DWORD),
        visible_seed_dword: None,
    }
}

fn gpgpu_t64_windowed_accum16_live48_partial_matvec_program() -> GpgpuEuProgram {
    let mut program = gpgpu_t63_accum16_hi_live32_partial_matvec_program();
    program.name = trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_PROGRAM_NAME;
    program
}

fn gpgpu_t65_windowed_accum16_live64_partial_matvec_program() -> GpgpuEuProgram {
    let mut program = gpgpu_t63_accum16_hi_live32_partial_matvec_program();
    program.name = trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_PROGRAM_NAME;
    program
}

fn gpgpu_t5_one_row_matvec_profile() -> GpgpuOneRowMatvecProfile {
    GpgpuOneRowMatvecProfile {
        program: gpgpu_t5_one_row_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K,
        expected_sentinel: trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
        requires_live_gpu_load: trueos_eu::gfx12::T5_ONE_ROW_MATVEC_REQUIRES_LIVE_GPU_LOAD,
        scale_ladder: GPGPU_T5_LIVE4_GROUP_X_DIM_LADDER,
        log_prefix: "t5",
        scale_prefix: "t5-live4",
        summary_label: "t5-small-live4-bf16-dot",
        submit_label: "gpgpu-t5-small-live4-scale",
        success_class: "t5-live4-packed-bf16-proven",
        success_reason: "t5-live4-written",
        success_reason_no_ts: "t5-live4-written-no-ts-delta",
        surface_note: "bind-send-bti-to-t5-trueos-arena-base",
    }
}

fn gpgpu_t6_one_row_matvec_profile() -> GpgpuOneRowMatvecProfile {
    GpgpuOneRowMatvecProfile {
        program: gpgpu_t6_one_row_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T6_ONE_ROW_MATVEC_LIVE_K,
        expected_sentinel: trueos_eu::gfx12::T6_SMALL_LIVE8_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
        requires_live_gpu_load: true,
        scale_ladder: GPGPU_T6_LIVE8_GROUP_X_DIM_LADDER,
        log_prefix: "t6",
        scale_prefix: "t6-live8",
        summary_label: "t6-small-live8-bf16-dot",
        submit_label: "gpgpu-t6-small-live8-scale",
        success_class: "t6-live8-packed-bf16-proven",
        success_reason: "t6-live8-written",
        success_reason_no_ts: "t6-live8-written-no-ts-delta",
        surface_note: "bind-send-bti-to-t6-trueos-arena-base",
    }
}

fn gpgpu_t61_one_row_matvec_profile() -> GpgpuOneRowMatvecProfile {
    GpgpuOneRowMatvecProfile {
        program: gpgpu_t61_one_row_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T61_ONE_ROW_MATVEC_LIVE_K,
        expected_sentinel: trueos_eu::gfx12::T61_LIVE16_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
        requires_live_gpu_load: true,
        scale_ladder: GPGPU_T61_LIVE16_GROUP_X_DIM_LADDER,
        log_prefix: "t6-1",
        scale_prefix: "t6-1-live16",
        summary_label: "t6-1-live16-bf16-dot",
        submit_label: "gpgpu-t6-1-live16-scale",
        success_class: "t6-1-live16-packed-bf16-proven",
        success_reason: "t6-1-live16-written",
        success_reason_no_ts: "t6-1-live16-written-no-ts-delta",
        surface_note: "bind-send-bti-to-t6-1-trueos-arena-base",
    }
}

fn gpgpu_t62_partial_matvec_profile() -> GpgpuPartialMatvecProfile {
    GpgpuPartialMatvecProfile {
        program: gpgpu_t62_partial_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T62_ROW_INDEXED_LIVE_K,
        partial_rows: trueos_eu::gfx12::T62_ROW_INDEXED_PARTIAL_ROWS,
        clear_output_before_submit: true,
        log_label: "t6-2-lane-indexed-live16-partial",
        submit_label: "gpgpu-t6-2-lane-indexed-live16",
        surface_note: "bind-send-bti-to-t6-2-lane-indexed-arena-base",
        success_reason: "t6-2-lane-indexed-live16-written",
        success_next: "raise-row-count-or-live-k",
        failure_next: "fix-t6-2-lane-indexed-live16",
    }
}

fn gpgpu_t63_partial_matvec_profile() -> GpgpuPartialMatvecProfile {
    GpgpuPartialMatvecProfile {
        program: gpgpu_t63_partial_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T63_LANE_INDEXED_LIVE_K,
        partial_rows: trueos_eu::gfx12::T63_LANE_INDEXED_PARTIAL_ROWS,
        clear_output_before_submit: true,
        log_label: "t6-3-lane-indexed-live32-partial",
        submit_label: "gpgpu-t6-3-lane-indexed-live32",
        surface_note: "bind-send-bti-to-t6-3-lane-indexed-arena-base",
        success_reason: "t6-3-lane-indexed-live32-written",
        success_next: "promote-row-block-owner-or-scale-live-k",
        failure_next: "fix-t6-3-lane-indexed-live32",
    }
}

fn gpgpu_t63_accum16_hi_live32_partial_matvec_profile() -> GpgpuPartialMatvecProfile {
    GpgpuPartialMatvecProfile {
        program: gpgpu_t63_accum16_hi_live32_partial_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_LIVE_K,
        partial_rows: trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_PARTIAL_ROWS,
        clear_output_before_submit: false,
        log_label: "t6-3-accum16-hi-live32-partial",
        submit_label: "gpgpu-t6-3-accum16-hi-live32",
        surface_note: "bind-send-bti-to-t6-3-accum16-hi-live32-arena-base",
        success_reason: "t6-3-accum16-hi-live32-written",
        success_next: "promote-row-block-owner-or-scale-live-k",
        failure_next: "fix-t6-3-accum16-hi-live32",
    }
}

fn gpgpu_t64_windowed_accum16_live48_partial_matvec_profile() -> GpgpuPartialMatvecProfile {
    GpgpuPartialMatvecProfile {
        program: gpgpu_t64_windowed_accum16_live48_partial_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T64_WINDOWED_ACCUM16_LIVE48_LIVE_K,
        partial_rows: trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_PARTIAL_ROWS,
        clear_output_before_submit: false,
        log_label: "t6-4-windowed-accum16-live48-partial",
        submit_label: "gpgpu-t6-4-windowed-accum16-live48",
        surface_note: "bind-send-bti-to-t6-4-windowed-accum16-live48-arena-base",
        success_reason: "t6-4-windowed-accum16-live48-written",
        success_next: "t6-5-windowed-accum16-live64-partial",
        failure_next: "fix-t6-4-windowed-accum16-live48",
    }
}

fn gpgpu_t65_windowed_accum16_live64_partial_matvec_profile() -> GpgpuPartialMatvecProfile {
    GpgpuPartialMatvecProfile {
        program: gpgpu_t65_windowed_accum16_live64_partial_matvec_program(),
        live_k_dim: trueos_eu::gfx12::T65_WINDOWED_ACCUM16_LIVE64_LIVE_K,
        partial_rows: trueos_eu::gfx12::T63_ACCUM16_HI_LIVE32_PARTIAL_ROWS,
        clear_output_before_submit: false,
        log_label: "t6-5-windowed-accum16-live64-partial",
        submit_label: "gpgpu-t6-5-windowed-accum16-live64",
        surface_note: "bind-send-bti-to-t6-5-windowed-accum16-live64-arena-base",
        success_reason: "t6-5-windowed-accum16-live64-written",
        success_next: "promote-row-block-owner-or-scale-live-k",
        failure_next: "fix-t6-5-windowed-accum16-live64",
    }
}

fn gpgpu_t5_store_only_control_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::T5_STORE_ONLY_ARENA_OFFSET_HDC1_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: "gfx12-tile-store-only-arena-offset-hdc1-store-then-ts-eot",
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
        store_send_dword: Some(trueos_eu::gfx12::T5_STORE_ONLY_ARENA_STORE_SEND_DWORD),
        visible_seed_dword: Some(trueos_eu::gfx12::T5_STORE_ONLY_ARENA_SENTINEL_DWORD),
    }
}

fn gpgpu_t5_load_echo_program() -> GpgpuEuProgram {
    let artifact = trueos_eu::gfx12::T5_LOAD_ECHO_TRUEOS_ARENA_RAW_OPERANDS_HDC1_STORE_THEN_TS_EOT;
    GpgpuEuProgram {
        name: "gfx12-tile-load-echo-raw-operands-hdc1-store-then-ts-eot",
        kind: artifact.kind,
        words: artifact.words,
        expects_store: artifact.expects_store,
        expected_store_value: 0,
        store_send_dword: Some(trueos_eu::gfx12::T5_LOAD_ECHO_TRUEOS_ARENA_FIRST_STORE_SEND_DWORD),
        visible_seed_dword: None,
    }
}

pub(crate) fn stage_gpgpu_one_tile_record_probe(
    x: &[f32],
    row_bf16: &[u8],
    k_dim: usize,
    row_index: usize,
    x_checksum: u64,
    row_checksum: u64,
    cpu_expected_bits: u32,
) -> crate::intel::GpgpuOneTileStageProof {
    let Some(dev) = crate::intel::claimed_device() else {
        return log_gpgpu_one_tile_stage_failure(
            "no-device",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    };
    let Some(warm) = warm_state() else {
        return log_gpgpu_one_tile_stage_failure(
            "no-warm-state",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return log_gpgpu_one_tile_stage_failure(
            "no-arena",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    }
    if k_dim != GPGPU_TILE_K_DIM {
        return log_gpgpu_one_tile_stage_failure(
            "k-dim-not-tile-k",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    }

    let x_bytes = k_dim.saturating_mul(GPGPU_TILE_X_BYTES_PER_ELEM);
    let row_bytes = k_dim.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);
    let output_bytes = GPGPU_OUTPUT_TILE_BYTES;
    if x.len() < k_dim || row_bf16.len() < row_bytes {
        return log_gpgpu_one_tile_stage_failure(
            "bad-shape",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    }

    let tile_slot = row_index / GPGPU_TILE_ROWS;
    let Some(tile_base_offset) = tile_slot.checked_mul(GPGPU_TILE_RECORD_BYTES) else {
        return log_gpgpu_one_tile_stage_failure(
            "tile-base-overflow",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    };
    let x_offset = tile_base_offset;
    let row_offset = tile_base_offset + GPGPU_X_VECTOR_BYTES;
    let output_offset = tile_base_offset + GPGPU_TILE_OUTPUT_OFFSET_BYTES;
    let Some(required_bytes) = tile_base_offset.checked_add(GPGPU_TILE_RECORD_USED_BYTES) else {
        return log_gpgpu_one_tile_stage_failure(
            "layout-overflow",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    };
    if warm.gpgpu_arena_len < required_bytes {
        return log_gpgpu_one_tile_stage_failure(
            "arena-too-small",
            k_dim,
            row_index,
            x_checksum,
            row_checksum,
            cpu_expected_bits,
        );
    }

    let arena_mapped = ensure_gpgpu_tile_arena_mapped(dev, warm);
    let x_virt = unsafe { warm.gpgpu_arena_virt.add(x_offset) };
    let row_virt = unsafe { warm.gpgpu_arena_virt.add(row_offset) };
    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };

    unsafe {
        core::ptr::copy_nonoverlapping(x.as_ptr() as *const u8, x_virt, x_bytes);
        core::ptr::write_bytes(row_virt, 0, GPGPU_WEIGHT_TILE_BYTES);
        core::ptr::copy_nonoverlapping(row_bf16.as_ptr(), row_virt, row_bytes);
        core::ptr::write_bytes(output_virt, 0, output_bytes);
    }
    crate::intel::dma_flush(x_virt, x_bytes);
    crate::intel::dma_flush(row_virt, GPGPU_WEIGHT_TILE_BYTES);
    crate::intel::dma_flush(output_virt, output_bytes);

    let staged_x_checksum = unsafe { gpgpu_stage_checksum_bytes(x_virt as *const u8, x_bytes) };
    let staged_row_checksum =
        unsafe { gpgpu_stage_checksum_bytes(row_virt as *const u8, row_bytes) };
    let output_checksum =
        unsafe { gpgpu_stage_checksum_bytes(output_virt as *const u8, output_bytes) };
    let output_first_bits = unsafe { core::ptr::read_volatile(output_virt as *const u32) };
    let output_nonzero_dwords =
        unsafe { gpgpu_stage_nonzero_dwords(output_virt as *const u32, GPGPU_TILE_ROWS) };
    let output_expected_hits_lo64 = unsafe {
        gpgpu_stage_dword_hits_mask_lo64(
            output_virt as *const u32,
            GPGPU_TILE_ROWS,
            cpu_expected_bits,
        )
    };
    let staged =
        arena_mapped && staged_x_checksum == x_checksum && staged_row_checksum == row_checksum;
    let output_zeroed = output_nonzero_dwords == 0;
    let readback_ok = staged && output_zeroed && output_expected_hits_lo64 == 0;
    let reason = if staged {
        "staged"
    } else if !arena_mapped {
        "arena-not-mapped"
    } else if staged_x_checksum != x_checksum {
        "x-checksum-mismatch"
    } else {
        "row-checksum-mismatch"
    };

    let proof = crate::intel::GpgpuOneTileStageProof {
        staged,
        reason,
        readback_ok,
        output_zeroed,
        arena_mapped,
        arena_gpu_base: GPU_VA_GPGPU_TILE_ARENA_BASE,
        x_gpu: GPU_VA_GPGPU_TILE_ARENA_BASE + x_offset as u64,
        row_gpu: GPU_VA_GPGPU_TILE_ARENA_BASE + row_offset as u64,
        output_gpu: GPU_VA_GPGPU_TILE_ARENA_BASE + output_offset as u64,
        x_bytes,
        row_bytes,
        output_bytes,
        tile_rows: GPGPU_TILE_ROWS,
        k_dim,
        output_first_bits,
        output_nonzero_dwords,
        output_expected_hits_lo64,
        output_checksum,
    };
    crate::log!(
        "intel/gpgpu: one-tile-stage staged={} reason={} arena_mapped={} arena_gpu_base=0x{:X} row={} tile_slot={} tile_record_bytes=0x{:X} tile_rows={} k_dim={} layout=tile-record-x-row-output x_gpu=0x{:X} row_gpu=0x{:X} output_gpu=0x{:X} x_bytes={} row_bytes={} output_bytes={} x_checksum=0x{:016X} staged_x_checksum=0x{:016X} row_checksum=0x{:016X} staged_row_checksum=0x{:016X} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=t5-live4-then-t6-live8 does_not_prove=gpu_live_load_or_model_matvec\n",
        proof.staged as u8,
        proof.reason,
        proof.arena_mapped as u8,
        proof.arena_gpu_base,
        row_index,
        tile_slot,
        GPGPU_TILE_RECORD_BYTES,
        proof.tile_rows,
        proof.k_dim,
        proof.x_gpu,
        proof.row_gpu,
        proof.output_gpu,
        proof.x_bytes,
        proof.row_bytes,
        proof.output_bytes,
        x_checksum,
        staged_x_checksum,
        row_checksum,
        staged_row_checksum,
        cpu_expected_bits,
    );
    crate::log!(
        "intel/gpgpu: one-tile-readback readback_ok={} staged={} x_match={} row_match={} output_zeroed={} output_first_bits=0x{:08X} output_nonzero_dwords={} output_expected_hits_lo64=0x{:016X} output_checksum=0x{:016X} cpu_expected_bits=0x{:08X} gpu_submission=0 scenario=one-worker-tile-before-submit plain=\"tile record holds live x and row bytes, output slot is untouched zero state\" next=t5-live4-then-t6-live8 does_not_prove=gpu_output_or_matvec\n",
        proof.readback_ok as u8,
        proof.staged as u8,
        (staged_x_checksum == x_checksum) as u8,
        (staged_row_checksum == row_checksum) as u8,
        proof.output_zeroed as u8,
        proof.output_first_bits,
        proof.output_nonzero_dwords,
        proof.output_expected_hits_lo64,
        proof.output_checksum,
        cpu_expected_bits,
    );
    proof
}

pub(crate) fn stage_gpgpu_tile_record_rows_probe(
    output_gpu: u64,
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
    rows_checksum: u64,
) -> crate::intel::GpgpuTileRowsStageProof {
    let Some(warm) = warm_state() else {
        return gpgpu_tile_rows_stage_failure(
            "no-warm-state",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_tile_rows_stage_failure(
            "no-arena",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_tile_rows_stage_failure(
            "output-gpu-before-arena",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }
    if k_dim != GPGPU_TILE_K_DIM || row_count == 0 || row_count > GPGPU_TILE_ROWS {
        return gpgpu_tile_rows_stage_failure(
            "bad-shape",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }
    let row_bytes = row_count
        .saturating_mul(k_dim)
        .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);
    if rows_bf16.len() < row_bytes {
        return gpgpu_tile_rows_stage_failure(
            "short-rows",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(tile_base_offset) = output_offset.checked_sub(GPGPU_TILE_OUTPUT_OFFSET_BYTES) else {
        return gpgpu_tile_rows_stage_failure(
            "output-before-tile-output-slot",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    };
    if tile_base_offset % GPGPU_TILE_RECORD_BYTES != 0 {
        return gpgpu_tile_rows_stage_failure(
            "output-gpu-not-tile-record-slot",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }
    let Some(tile_record_end) = tile_base_offset.checked_add(GPGPU_TILE_RECORD_USED_BYTES) else {
        return gpgpu_tile_rows_stage_failure(
            "tile-record-overflow",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    };
    if tile_record_end > warm.gpgpu_arena_len {
        return gpgpu_tile_rows_stage_failure(
            "tile-record-outside-arena",
            output_gpu,
            row_count,
            k_dim,
            rows_checksum,
        );
    }

    let row_offset = tile_base_offset + GPGPU_X_VECTOR_BYTES;
    let row_virt = unsafe { warm.gpgpu_arena_virt.add(row_offset) };
    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    unsafe {
        core::ptr::write_bytes(row_virt, 0, GPGPU_WEIGHT_TILE_BYTES);
        core::ptr::copy_nonoverlapping(rows_bf16.as_ptr(), row_virt, row_bytes);
        core::ptr::write_bytes(output_virt, 0, GPGPU_OUTPUT_TILE_BYTES);
    }
    crate::intel::dma_flush(row_virt, GPGPU_WEIGHT_TILE_BYTES);
    crate::intel::dma_flush(output_virt, GPGPU_OUTPUT_TILE_BYTES);

    let staged_rows_checksum =
        unsafe { gpgpu_stage_checksum_bytes(row_virt as *const u8, row_bytes) };
    let output_nonzero_dwords =
        unsafe { gpgpu_stage_nonzero_dwords(output_virt as *const u32, GPGPU_TILE_ROWS) };
    let output_zeroed = output_nonzero_dwords == 0;
    let staged = staged_rows_checksum == rows_checksum;
    let readback_ok = staged && output_zeroed;
    let reason = if readback_ok {
        "staged"
    } else if !staged {
        "rows-checksum-mismatch"
    } else {
        "output-not-zero"
    };
    crate::log!(
        "intel/gpgpu: tile-rows-stage staged={} reason={} output_gpu=0x{:X} row_count={} k_dim={} row_bytes={} rows_checksum=0x{:016X} staged_rows_checksum=0x{:016X} output_zeroed={} output_nonzero_dwords={} next=t6-2-lane-indexed-live16 does_not_prove=full_model_matvec\n",
        staged as u8,
        reason,
        output_gpu,
        row_count,
        k_dim,
        row_bytes,
        rows_checksum,
        staged_rows_checksum,
        output_zeroed as u8,
        output_nonzero_dwords,
    );
    crate::intel::GpgpuTileRowsStageProof {
        staged,
        reason,
        readback_ok,
        output_zeroed,
        output_gpu,
        row_count,
        row_bytes,
        rows_checksum,
        staged_rows_checksum,
        output_nonzero_dwords,
    }
}

pub(crate) fn stage_gpgpu_tile_record_accum16_window_probe(
    output_gpu: u64,
    x: &[f32],
    rows_bf16: &[u8],
    row_count: usize,
    k_dim: usize,
    source_start: usize,
) -> crate::intel::GpgpuTileRowsStageProof {
    const WINDOW_LANES: usize = 16;
    const ARTIFACT_WINDOW_START: usize = 16;
    let Some(warm) = warm_state() else {
        return gpgpu_tile_rows_stage_failure(
            "no-warm-state",
            output_gpu,
            row_count,
            k_dim,
            0,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_tile_rows_stage_failure("no-arena", output_gpu, row_count, k_dim, 0);
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_tile_rows_stage_failure(
            "output-gpu-before-arena",
            output_gpu,
            row_count,
            k_dim,
            0,
        );
    }
    if k_dim != GPGPU_TILE_K_DIM
        || row_count == 0
        || row_count > GPGPU_TILE_ROWS
        || source_start.saturating_add(WINDOW_LANES) > k_dim
        || x.len() < k_dim
    {
        return gpgpu_tile_rows_stage_failure("bad-shape", output_gpu, row_count, k_dim, 0);
    }
    let row_bytes = row_count
        .saturating_mul(k_dim)
        .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);
    if rows_bf16.len() < row_bytes {
        return gpgpu_tile_rows_stage_failure("short-rows", output_gpu, row_count, k_dim, 0);
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(tile_base_offset) = output_offset.checked_sub(GPGPU_TILE_OUTPUT_OFFSET_BYTES) else {
        return gpgpu_tile_rows_stage_failure(
            "output-before-tile-output-slot",
            output_gpu,
            row_count,
            k_dim,
            0,
        );
    };
    if tile_base_offset % GPGPU_TILE_RECORD_BYTES != 0 {
        return gpgpu_tile_rows_stage_failure(
            "output-gpu-not-tile-record-slot",
            output_gpu,
            row_count,
            k_dim,
            0,
        );
    }
    let Some(tile_record_end) = tile_base_offset.checked_add(GPGPU_TILE_RECORD_USED_BYTES) else {
        return gpgpu_tile_rows_stage_failure("tile-record-overflow", output_gpu, row_count, k_dim, 0);
    };
    if tile_record_end > warm.gpgpu_arena_len {
        return gpgpu_tile_rows_stage_failure(
            "tile-record-outside-arena",
            output_gpu,
            row_count,
            k_dim,
            0,
        );
    }

    let x_offset = tile_base_offset;
    let row_offset = tile_base_offset + GPGPU_X_VECTOR_BYTES;
    let x_virt = unsafe { warm.gpgpu_arena_virt.add(x_offset) };
    let row_virt = unsafe { warm.gpgpu_arena_virt.add(row_offset) };
    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let dst_x_byte = ARTIFACT_WINDOW_START.saturating_mul(core::mem::size_of::<f32>());
    let src_x_byte = source_start.saturating_mul(core::mem::size_of::<f32>());
    let window_x_bytes = WINDOW_LANES.saturating_mul(core::mem::size_of::<f32>());
    let dst_row_byte = ARTIFACT_WINDOW_START.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);
    let src_row_byte = source_start.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);
    let window_row_bytes = WINDOW_LANES.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM);

    unsafe {
        core::ptr::copy_nonoverlapping(
            (x.as_ptr() as *const u8).add(src_x_byte),
            x_virt.add(dst_x_byte),
            window_x_bytes,
        );
        for row in 0..row_count {
            let src_base = row
                .saturating_mul(k_dim)
                .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM)
                .saturating_add(src_row_byte);
            let dst_base = row
                .saturating_mul(k_dim)
                .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM)
                .saturating_add(dst_row_byte);
            core::ptr::copy_nonoverlapping(
                rows_bf16.as_ptr().add(src_base),
                row_virt.add(dst_base),
                window_row_bytes,
            );
        }
    }
    crate::intel::dma_flush(unsafe { x_virt.add(dst_x_byte) }, window_x_bytes);
    crate::intel::dma_flush(row_virt, row_bytes);

    let mut staged = true;
    unsafe {
        for lane in 0..WINDOW_LANES {
            let src = core::ptr::read_unaligned(x.as_ptr().add(source_start + lane) as *const u32);
            let dst = core::ptr::read_volatile(
                x_virt.add(dst_x_byte + lane * core::mem::size_of::<f32>()) as *const u32,
            );
            staged &= src == dst;
        }
        for row in 0..row_count {
            for lane in 0..WINDOW_LANES {
                let src_off = row
                    .saturating_mul(k_dim)
                    .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM)
                    .saturating_add(src_row_byte)
                    .saturating_add(lane.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM));
                let dst_off = row
                    .saturating_mul(k_dim)
                    .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM)
                    .saturating_add(dst_row_byte)
                    .saturating_add(lane.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM));
                let src = u16::from_le_bytes([rows_bf16[src_off], rows_bf16[src_off + 1]]);
                let dst = core::ptr::read_volatile(row_virt.add(dst_off) as *const u16);
                staged &= src == dst;
            }
        }
    }
    let output_nonzero_dwords =
        unsafe { gpgpu_stage_nonzero_dwords(output_virt as *const u32, GPGPU_TILE_ROWS) };
    let rows_checksum = unsafe { gpgpu_stage_checksum_bytes(row_virt as *const u8, row_bytes) };
    let reason = if staged { "window-staged" } else { "window-mismatch" };
    crate::log!(
        "intel/gpgpu: tile-accum16-window-stage staged={} reason={} output_gpu=0x{:X} row_count={} k_dim={} source_start={} source_end={} artifact_window={}..{} row_bytes={} rows_checksum=0x{:016X} output_preserved_nonzero_dwords={} next=t6-windowed-accum16-submit does_not_prove=full_model_matvec\n",
        staged as u8,
        reason,
        output_gpu,
        row_count,
        k_dim,
        source_start,
        source_start.saturating_add(WINDOW_LANES),
        ARTIFACT_WINDOW_START,
        ARTIFACT_WINDOW_START.saturating_add(WINDOW_LANES),
        row_bytes,
        rows_checksum,
        output_nonzero_dwords,
    );
    crate::intel::GpgpuTileRowsStageProof {
        staged,
        reason,
        readback_ok: staged,
        output_zeroed: false,
        output_gpu,
        row_count,
        row_bytes,
        rows_checksum,
        staged_rows_checksum: rows_checksum,
        output_nonzero_dwords,
    }
}

pub(crate) fn submit_gpgpu_one_tile_output_sentinel_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
) -> crate::intel::GpgpuOneTileSentinelProof {
    let program = gpgpu_one_tile_output_sentinel_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_sentinel_failure("no-device", program, output_gpu);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_sentinel_failure("no-warm-state", program, output_gpu);
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_one_tile_sentinel_failure("no-arena", program, output_gpu);
    }
    if output_bytes < core::mem::size_of::<u32>() {
        return gpgpu_one_tile_sentinel_failure("bad-output-bytes", program, output_gpu);
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_one_tile_sentinel_failure("output-gpu-before-arena", program, output_gpu);
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(output_end) = output_offset.checked_add(output_bytes) else {
        return gpgpu_one_tile_sentinel_failure("output-range-overflow", program, output_gpu);
    };
    if output_end > warm.gpgpu_arena_len {
        return gpgpu_one_tile_sentinel_failure("output-range-outside-arena", program, output_gpu);
    }
    if output_gpu >> 32 != 0 {
        return gpgpu_one_tile_sentinel_failure(
            "output-gpu-high32-unsupported",
            program,
            output_gpu,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_sentinel_failure("forcewake", program, output_gpu);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) || !ensure_gpgpu_tile_arena_mapped(dev, warm) {
        return gpgpu_one_tile_sentinel_failure("ppgtt-map", program, output_gpu);
    }

    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let output_count = output_bytes / core::mem::size_of::<u32>();
    let output_first_before = unsafe { core::ptr::read_volatile(output_virt as *const u32) };
    let output_nonzero_before =
        unsafe { gpgpu_stage_nonzero_dwords(output_virt as *const u32, output_count) };

    let mut sentinel_words = trueos_eu::gfx12::HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    sentinel_words[trueos_eu::gfx12::HDC1_BTI34_STORE_IMM_DWORD] = GPGPU_ONE_TILE_OUTPUT_SENTINEL;
    sentinel_words[7] = output_gpu as u32;
    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &sentinel_words);
    if !program_uploaded {
        return gpgpu_one_tile_sentinel_failure("program-upload", program, output_gpu);
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
        output_gpu,
        "bind-send-bti-to-one-tile-output-sentinel",
    );
    let batch_result =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1);
    let batch_bytes = match batch_result {
        Ok(bytes) => bytes,
        Err(reason) => return gpgpu_one_tile_sentinel_failure(reason, program, output_gpu),
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-one-tile-sentinel",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(output_virt, output_bytes);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_first_after = unsafe { core::ptr::read_volatile(output_virt as *const u32) };
    let output_nonzero_after =
        unsafe { gpgpu_stage_nonzero_dwords(output_virt as *const u32, output_count) };
    let output_hits_lo64 = unsafe {
        gpgpu_stage_dword_hits_mask_lo64(
            output_virt as *const u32,
            output_count,
            GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        )
    };
    let sentinel_written = output_first_after == GPGPU_ONE_TILE_OUTPUT_SENTINEL
        && (output_hits_lo64 & 1) != 0
        && output_first_before == 0;
    let readback_ok =
        sentinel_written && finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let submitted = batch_bytes != 0;
    let reason = if output_first_before != 0 {
        "output-not-zero-before-submit"
    } else if readback_ok && dispatch_delta == 0 {
        "sentinel-written-no-ts-delta"
    } else if readback_ok {
        "sentinel-written"
    } else if !finished {
        "submit-not-finished"
    } else if dispatch_delta == 0 {
        "no-dispatch-delta"
    } else if output_first_after != GPGPU_ONE_TILE_OUTPUT_SENTINEL {
        "sentinel-missing"
    } else {
        "sentinel-not-at-slot0"
    };
    crate::log!(
        "intel/gpgpu: one-tile-output-sentinel submitted={} finished={} readback_ok={} reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch={} output_gpu=0x{:X} output_first_before=0x{:08X} output_first_after=0x{:08X} sentinel=0x{:08X} output_nonzero_before={} output_nonzero_after={} output_hits_lo64=0x{:016X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} cpu_expected_bits=0x{:08X} output_owner=cpu-ap next=one-tile-output-compare does_not_prove=model_matvec\n",
        submitted as u8,
        finished as u8,
        readback_ok as u8,
        reason,
        program.name,
        dispatch_delta,
        output_gpu,
        output_first_before,
        output_first_after,
        GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_nonzero_before,
        output_nonzero_after,
        output_hits_lo64,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        cpu_expected_bits,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-one-tile-sentinel");
    }
    crate::intel::GpgpuOneTileSentinelProof {
        submitted,
        finished,
        readback_ok,
        reason,
        program_name: program.name,
        output_gpu,
        sentinel: GPGPU_ONE_TILE_OUTPUT_SENTINEL,
        output_first_before,
        output_first_after,
        output_nonzero_before,
        output_nonzero_after,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

pub(crate) fn submit_gpgpu_sidequest_target_buffer_probe() -> crate::intel::GpgpuOneTileSentinelProof
{
    const SIDEQUEST_TARGET_BYTES: usize = 64;
    const SIDEQUEST_CPU_EXPECTED_BITS: u32 = 0;

    let output_gpu = GPU_VA_GPGPU_TILE_ARENA_BASE + GPGPU_TILE_OUTPUT_OFFSET_BYTES as u64;
    if let Some(warm) = warm_state() {
        let output_offset = GPGPU_TILE_OUTPUT_OFFSET_BYTES;
        if !warm.gpgpu_arena_virt.is_null()
            && output_offset
                .checked_add(SIDEQUEST_TARGET_BYTES)
                .is_some_and(|end| end <= warm.gpgpu_arena_len)
        {
            let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
            unsafe {
                core::ptr::write_bytes(output_virt, 0, SIDEQUEST_TARGET_BYTES);
            }
            crate::intel::dma_flush(output_virt, SIDEQUEST_TARGET_BYTES);
            crate::log!(
                "intel/gpgpu: sidequest-target-buffer-clear target_gpu=0x{:X} target_off=0x{:X} target_bytes=0x{:X} action=prepare-proven-gpgpu-store\n",
                output_gpu,
                output_offset,
                SIDEQUEST_TARGET_BYTES,
            );
        } else {
            crate::log!(
                "intel/gpgpu: sidequest-target-buffer-clear target_gpu=0x{:X} target_off=0x{:X} target_bytes=0x{:X} action=skip reason=arena-not-ready\n",
                output_gpu,
                output_offset,
                SIDEQUEST_TARGET_BYTES,
            );
        }
    }

    let proof = submit_gpgpu_one_tile_output_sentinel_probe(
        output_gpu,
        SIDEQUEST_TARGET_BYTES,
        SIDEQUEST_CPU_EXPECTED_BITS,
    );
    crate::log!(
        "intel/gpgpu: sidequest-target-buffer-render submitted={} finished={} readback_ok={} reason={} program_source={} target_gpu=0x{:X} target_bytes=0x{:X} sentinel=0x{:08X} before=0x{:08X} after=0x{:08X} hits=0x{:016X} lane_dispatch={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} action={} next={} does_not_prove=fragment_shader_or_full_framebuffer\n",
        proof.submitted as u8,
        proof.finished as u8,
        proof.readback_ok as u8,
        proof.reason,
        proof.program_name,
        proof.output_gpu,
        SIDEQUEST_TARGET_BYTES,
        proof.sentinel,
        proof.output_first_before,
        proof.output_first_after,
        proof.output_hits_lo64,
        proof.dispatch_delta,
        proof.finish_marker,
        proof.expected_finish_marker,
        proof.batch_bytes,
        if proof.readback_ok {
            "release-lumen-load"
        } else {
            "hold-lumen-load"
        },
        if proof.readback_ok {
            "optional-visible-scanout-target"
        } else {
            "fix-gpgpu-target-buffer-render"
        },
    );
    proof
}

pub(crate) fn submit_gpgpu_one_tile_output_compare_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
) -> crate::intel::GpgpuOneTileCompareProof {
    let program = gpgpu_one_tile_output_compare_program();
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_one_tile_compare_failure("no-device", program, output_gpu, cpu_expected_bits);
    };
    let Some(warm) = warm_state() else {
        return gpgpu_one_tile_compare_failure(
            "no-warm-state",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_one_tile_compare_failure("no-arena", program, output_gpu, cpu_expected_bits);
    }
    if output_bytes < core::mem::size_of::<u32>() {
        return gpgpu_one_tile_compare_failure(
            "bad-output-bytes",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_one_tile_compare_failure(
            "output-gpu-before-arena",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(output_end) = output_offset.checked_add(output_bytes) else {
        return gpgpu_one_tile_compare_failure(
            "output-range-overflow",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    };
    if output_end > warm.gpgpu_arena_len {
        return gpgpu_one_tile_compare_failure(
            "output-range-outside-arena",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    }
    if output_gpu >> 32 != 0 {
        return gpgpu_one_tile_compare_failure(
            "output-gpu-high32-unsupported",
            program,
            output_gpu,
            cpu_expected_bits,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_one_tile_compare_failure("forcewake", program, output_gpu, cpu_expected_bits);
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) || !ensure_gpgpu_tile_arena_mapped(dev, warm) {
        return gpgpu_one_tile_compare_failure("ppgtt-map", program, output_gpu, cpu_expected_bits);
    }

    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let output_count = output_bytes / core::mem::size_of::<u32>();
    let output_first_before = unsafe { core::ptr::read_volatile(output_virt as *const u32) };

    let mut compare_words = trueos_eu::gfx12::STATIC_DP4A_HDC1_STATELESS_STORE_THEN_TS_EOT_WORDS;
    compare_words[trueos_eu::gfx12::HDC1_STATELESS_STATIC_DP4A_BASE_DWORD] =
        cpu_expected_bits.wrapping_sub(GPGPU_ONE_TILE_COMPARE_DP4A_ADDEND);
    compare_words[19] = output_gpu as u32;
    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, &compare_words);
    if !program_uploaded {
        return gpgpu_one_tile_compare_failure(
            "program-upload",
            program,
            output_gpu,
            cpu_expected_bits,
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
    let store_surface = prepare_gpgpu_store_surface_state_for_target(
        warm,
        output_gpu,
        "bind-send-bti-to-one-tile-output-compare",
    );
    let batch_result =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1);
    let batch_bytes = match batch_result {
        Ok(bytes) => bytes,
        Err(reason) => {
            return gpgpu_one_tile_compare_failure(reason, program, output_gpu, cpu_expected_bits);
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-one-tile-compare",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(output_virt, output_bytes);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_first_after = unsafe { core::ptr::read_volatile(output_virt as *const u32) };
    let output_hits_lo64 = unsafe {
        gpgpu_stage_dword_hits_mask_lo64(output_virt as *const u32, output_count, cpu_expected_bits)
    };
    let compare_ok = output_first_after == cpu_expected_bits && (output_hits_lo64 & 1) != 0;
    let readback_ok =
        compare_ok && finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let submitted = batch_bytes != 0;
    let reason = if readback_ok && dispatch_delta == 0 {
        "compare-written-no-ts-delta"
    } else if readback_ok {
        "compare-written"
    } else if !finished {
        "submit-not-finished"
    } else if output_first_after != cpu_expected_bits {
        "compare-mismatch"
    } else {
        "compare-not-at-slot0"
    };
    crate::log!(
        "intel/gpgpu: one-tile-output-compare submitted={} finished={} readback_ok={} compare_ok={} reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch={} output_gpu=0x{:X} output_first_before=0x{:08X} output_first_after=0x{:08X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} output_hits_lo64=0x{:016X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap next=t5-live4-packed-bf16-dot does_not_prove=model_matvec_or_gpu_live_load\n",
        submitted as u8,
        finished as u8,
        readback_ok as u8,
        compare_ok as u8,
        reason,
        program.name,
        dispatch_delta,
        output_gpu,
        output_first_before,
        output_first_after,
        output_first_after,
        cpu_expected_bits,
        output_hits_lo64,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-one-tile-compare");
    }
    crate::intel::GpgpuOneTileCompareProof {
        submitted,
        finished,
        readback_ok,
        compare_ok,
        reason,
        program_name: program.name,
        output_gpu,
        gpu_value: output_first_after,
        cpu_expected_bits,
        output_first_before,
        output_first_after,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    }
}

unsafe fn read_gpgpu_output_words8(output_virt: *mut u8) -> [u32; 8] {
    let words = output_virt as *const u32;
    [
        core::ptr::read_volatile(words.add(0)),
        core::ptr::read_volatile(words.add(1)),
        core::ptr::read_volatile(words.add(2)),
        core::ptr::read_volatile(words.add(3)),
        core::ptr::read_volatile(words.add(4)),
        core::ptr::read_volatile(words.add(5)),
        core::ptr::read_volatile(words.add(6)),
        core::ptr::read_volatile(words.add(7)),
    ]
}

unsafe fn clear_gpgpu_output_words(output_virt: *mut u8, dwords: usize) {
    let words = output_virt as *mut u32;
    for index in 0..dwords {
        core::ptr::write_volatile(words.add(index), 0);
    }
}

fn read_gpgpu_t5_load_echo_expected_at(
    warm: RenderWarmState,
    tile_base_offset: usize,
) -> ([u32; 4], [u32; 4]) {
    unsafe {
        let tile_base = warm.gpgpu_arena_virt.add(tile_base_offset);
        let x_words = tile_base as *const u32;
        let row_words = tile_base.add(GPGPU_X_VECTOR_BYTES) as *const u32;
        (
            [
                core::ptr::read_volatile(x_words.add(0)),
                core::ptr::read_volatile(x_words.add(1)),
                core::ptr::read_volatile(x_words.add(2)),
                core::ptr::read_volatile(x_words.add(3)),
            ],
            [
                core::ptr::read_volatile(row_words.add(0)),
                core::ptr::read_volatile(row_words.add(1)),
                core::ptr::read_volatile(row_words.add(2)),
                core::ptr::read_volatile(row_words.add(3)),
            ],
        )
    }
}

fn submit_gpgpu_t5_store_only_control_probe(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    output_gpu: u64,
    surface_gpu_base: u64,
    output_offset: usize,
    output_bytes: usize,
    t5_surface_bytes: usize,
) {
    let program = gpgpu_t5_store_only_control_program();
    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let output_count = output_bytes / core::mem::size_of::<u32>();
    let output_words_before = unsafe {
        let words = output_virt as *const u32;
        [
            core::ptr::read_volatile(words.add(0)),
            core::ptr::read_volatile(words.add(1)),
            core::ptr::read_volatile(words.add(2)),
            core::ptr::read_volatile(words.add(3)),
        ]
    };
    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, program.words);
    if !program_uploaded {
        crate::log!(
            "intel/gpgpu: tile-store-only-control submitted=0 finished=0 readback_ok=0 store_first_ok=0 payload_ok=0 reason=program-upload program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x00000000,0x00000000,0x00000000,0x00000000] expected_words=[0x{:08X},0x{:08X},0x{:08X},0x00000000] finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 surface_bytes=0x{:X} next=fix-tile-store-control-before-live-load does_not_prove=model_matvec_or_gpu_live_load\n",
            program.name,
            output_gpu,
            output_offset,
            output_words_before[0],
            output_words_before[1],
            output_words_before[2],
            output_words_before[3],
            trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
            trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K as u32,
            trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
            RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
            t5_surface_bytes,
        );
        return;
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
    let store_surface = prepare_gpgpu_store_surface_state_for_target_span(
        warm,
        surface_gpu_base,
        t5_surface_bytes,
        "bind-send-bti-to-tile-store-only-arena-base",
    );
    let batch_result =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1);
    let batch_bytes = match batch_result {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/gpgpu: tile-store-only-control submitted=0 finished=0 readback_ok=0 store_first_ok=0 payload_ok=0 reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x00000000,0x00000000,0x00000000,0x00000000] expected_words=[0x{:08X},0x{:08X},0x{:08X},0x00000000] finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 surface_bytes=0x{:X} next=fix-tile-store-control-before-live-load does_not_prove=model_matvec_or_gpu_live_load\n",
                reason,
                program.name,
                output_gpu,
                output_offset,
                output_words_before[0],
                output_words_before[1],
                output_words_before[2],
                output_words_before[3],
                trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
                trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K as u32,
                trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
                RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
                t5_surface_bytes,
            );
            return;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-tile-store-only-control",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(
        unsafe {
            warm.gpgpu_arena_virt
                .add(output_offset - GPGPU_TILE_OUTPUT_OFFSET_BYTES)
        },
        t5_surface_bytes,
    );
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_words_after = unsafe {
        let words = output_virt as *const u32;
        [
            core::ptr::read_volatile(words.add(0)),
            core::ptr::read_volatile(words.add(1)),
            core::ptr::read_volatile(words.add(2)),
            core::ptr::read_volatile(words.add(3)),
        ]
    };
    let output_hits_lo64 = unsafe {
        gpgpu_stage_dword_hits_mask_lo64(
            output_virt as *const u32,
            output_count,
            trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
        )
    };
    let store_first_ok = output_words_after[0]
        == trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32
        && (output_hits_lo64 & 1) != 0;
    let payload_ok = store_first_ok
        && output_words_after[1] == trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K as u32
        && output_words_after[2]
            == trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32
        && output_words_after[3] == 0;
    let readback_ok =
        store_first_ok && finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let submitted = batch_bytes != 0;
    let reason = if payload_ok && dispatch_delta == 0 {
        "store-payload-written-no-ts-delta"
    } else if payload_ok {
        "store-payload-written"
    } else if store_first_ok {
        "store-first-dword-written"
    } else if !finished {
        "submit-not-finished"
    } else {
        "store-missing"
    };
    crate::log!(
        "intel/gpgpu: tile-store-only-control submitted={} finished={} readback_ok={} store_first_ok={} payload_ok={} reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch={} output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_words=[0x{:08X},0x{:08X},0x{:08X},0x00000000] output_hits_lo64=0x{:016X} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} surface_bytes=0x{:X} next=run-live-dot-or-fix-store-payload does_not_prove=model_matvec_or_gpu_live_load\n",
        submitted as u8,
        finished as u8,
        readback_ok as u8,
        store_first_ok as u8,
        payload_ok as u8,
        reason,
        program.name,
        dispatch_delta,
        output_gpu,
        output_offset,
        output_words_before[0],
        output_words_before[1],
        output_words_before[2],
        output_words_before[3],
        output_words_after[0],
        output_words_after[1],
        output_words_after[2],
        output_words_after[3],
        trueos_eu::gfx12::T5_STORE_ONLY_ARENA_EXPECTED_RESULT_U32,
        trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K as u32,
        trueos_eu::gfx12::T5_SMALL_LIVE4_TRUEOS_ARENA_EXPECTED_SENTINEL_U32,
        output_hits_lo64,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        t5_surface_bytes,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-tile-store-only-control");
    }
}

fn submit_gpgpu_t5_load_echo_probe(
    dev: crate::intel::Dev,
    warm: RenderWarmState,
    output_gpu: u64,
    surface_gpu_base: u64,
    tile_base_offset: usize,
    output_offset: usize,
    t5_surface_bytes: usize,
) {
    let program = gpgpu_t5_load_echo_program();
    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let (expected_x, expected_row_words) =
        read_gpgpu_t5_load_echo_expected_at(warm, tile_base_offset);
    let output_words_before = unsafe { read_gpgpu_output_words8(output_virt) };
    unsafe {
        clear_gpgpu_output_words(output_virt, 8);
    }
    crate::intel::dma_flush(output_virt, 8 * core::mem::size_of::<u32>());
    let output_words_after_clear = unsafe { read_gpgpu_output_words8(output_virt) };

    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, program.words);
    if !program_uploaded {
        crate::log!(
            "intel/gpgpu: tile-load-echo submitted=0 finished=0 readback_ok=0 load_echo_ok=0 reason=program-upload program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after_clear=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000] expected_x=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_row_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row_bf16_first4=[0x{:04X},0x{:04X},0x{:04X},0x{:04X}] finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 surface_bytes=0x{:X} next=fix-tile-load-echo-upload does_not_prove=model_matvec_or_bf16_reduce\n",
            program.name,
            output_gpu,
            output_offset,
            output_words_before[0],
            output_words_before[1],
            output_words_before[2],
            output_words_before[3],
            output_words_before[4],
            output_words_before[5],
            output_words_before[6],
            output_words_before[7],
            output_words_after_clear[0],
            output_words_after_clear[1],
            output_words_after_clear[2],
            output_words_after_clear[3],
            output_words_after_clear[4],
            output_words_after_clear[5],
            output_words_after_clear[6],
            output_words_after_clear[7],
            expected_x[0],
            expected_x[1],
            expected_x[2],
            expected_x[3],
            expected_row_words[0],
            expected_row_words[1],
            expected_row_words[2],
            expected_row_words[3],
            (expected_row_words[0] & 0xFFFF) as u16,
            (expected_row_words[0] >> 16) as u16,
            (expected_row_words[1] & 0xFFFF) as u16,
            (expected_row_words[1] >> 16) as u16,
            RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
            t5_surface_bytes,
        );
        return;
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
    let store_surface = prepare_gpgpu_store_surface_state_for_target_span(
        warm,
        surface_gpu_base,
        t5_surface_bytes,
        "bind-send-bti-to-tile-load-echo-arena-base",
    );
    let batch_result =
        encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, 1);
    let batch_bytes = match batch_result {
        Ok(bytes) => bytes,
        Err(reason) => {
            crate::log!(
                "intel/gpgpu: tile-load-echo submitted=0 finished=0 readback_ok=0 load_echo_ok=0 reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after_clear=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000] expected_x=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_row_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row_bf16_first4=[0x{:04X},0x{:04X},0x{:04X},0x{:04X}] finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 surface_bytes=0x{:X} next=fix-tile-load-echo-batch does_not_prove=model_matvec_or_bf16_reduce\n",
                reason,
                program.name,
                output_gpu,
                output_offset,
                output_words_before[0],
                output_words_before[1],
                output_words_before[2],
                output_words_before[3],
                output_words_before[4],
                output_words_before[5],
                output_words_before[6],
                output_words_before[7],
                output_words_after_clear[0],
                output_words_after_clear[1],
                output_words_after_clear[2],
                output_words_after_clear[3],
                output_words_after_clear[4],
                output_words_after_clear[5],
                output_words_after_clear[6],
                output_words_after_clear[7],
                expected_x[0],
                expected_x[1],
                expected_x[2],
                expected_x[3],
                expected_row_words[0],
                expected_row_words[1],
                expected_row_words[2],
                expected_row_words[3],
                (expected_row_words[0] & 0xFFFF) as u16,
                (expected_row_words[0] >> 16) as u16,
                (expected_row_words[1] & 0xFFFF) as u16,
                (expected_row_words[1] >> 16) as u16,
                RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
                t5_surface_bytes,
            );
            return;
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        "gpgpu-tile-load-echo",
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(
        unsafe { warm.gpgpu_arena_virt.add(tile_base_offset) },
        t5_surface_bytes,
    );
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_words_after = unsafe { read_gpgpu_output_words8(output_virt) };
    let x_echo_ok = output_words_after[0] == expected_x[0]
        && output_words_after[1] == expected_x[1]
        && output_words_after[2] == expected_x[2]
        && output_words_after[3] == expected_x[3];
    let row_echo_ok = output_words_after[4] == expected_row_words[0]
        && output_words_after[5] == expected_row_words[1]
        && output_words_after[6] == expected_row_words[2]
        && output_words_after[7] == expected_row_words[3];
    let load_echo_ok = x_echo_ok && row_echo_ok;
    let readback_ok =
        load_echo_ok && finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let submitted = batch_bytes != 0;
    let reason = if readback_ok && dispatch_delta == 0 {
        "raw-load-echo-written-no-ts-delta"
    } else if readback_ok {
        "raw-load-echo-written"
    } else if !finished {
        "submit-not-finished"
    } else if !x_echo_ok {
        "x-echo-mismatch"
    } else if !row_echo_ok {
        "row-echo-mismatch"
    } else {
        "finish-marker-mismatch"
    };
    crate::log!(
        "intel/gpgpu: tile-load-echo submitted={} finished={} readback_ok={} load_echo_ok={} x_echo_ok={} row_echo_ok={} reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch={} output_gpu=0x{:X} output_off=0x{:X} output_words_before=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after_clear=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] output_words_after=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_x=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_row_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row_bf16_first4=[0x{:04X},0x{:04X},0x{:04X},0x{:04X}] finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} surface_bytes=0x{:X} next=run-live-dot-with-confirmed-loads does_not_prove=model_matvec_or_bf16_reduce\n",
        submitted as u8,
        finished as u8,
        readback_ok as u8,
        load_echo_ok as u8,
        x_echo_ok as u8,
        row_echo_ok as u8,
        reason,
        program.name,
        dispatch_delta,
        output_gpu,
        output_offset,
        output_words_before[0],
        output_words_before[1],
        output_words_before[2],
        output_words_before[3],
        output_words_before[4],
        output_words_before[5],
        output_words_before[6],
        output_words_before[7],
        output_words_after_clear[0],
        output_words_after_clear[1],
        output_words_after_clear[2],
        output_words_after_clear[3],
        output_words_after_clear[4],
        output_words_after_clear[5],
        output_words_after_clear[6],
        output_words_after_clear[7],
        output_words_after[0],
        output_words_after[1],
        output_words_after[2],
        output_words_after[3],
        output_words_after[4],
        output_words_after[5],
        output_words_after[6],
        output_words_after[7],
        expected_x[0],
        expected_x[1],
        expected_x[2],
        expected_x[3],
        expected_row_words[0],
        expected_row_words[1],
        expected_row_words[2],
        expected_row_words[3],
        (expected_row_words[0] & 0xFFFF) as u16,
        (expected_row_words[0] >> 16) as u16,
        (expected_row_words[1] & 0xFFFF) as u16,
        (expected_row_words[1] >> 16) as u16,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        t5_surface_bytes,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, "gpgpu-tile-load-echo");
    }
}

pub(crate) fn submit_gpgpu_t5_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> crate::intel::GpgpuT5OneRowMatvecProof {
    submit_gpgpu_one_row_matvec_probe_for(
        gpgpu_t5_one_row_matvec_profile(),
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t6_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> crate::intel::GpgpuT5OneRowMatvecProof {
    submit_gpgpu_one_row_matvec_probe_for(
        gpgpu_t6_one_row_matvec_profile(),
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t61_one_row_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> crate::intel::GpgpuT5OneRowMatvecProof {
    submit_gpgpu_one_row_matvec_probe_for(
        gpgpu_t61_one_row_matvec_profile(),
        output_gpu,
        output_bytes,
        cpu_expected_bits,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t62_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    submit_gpgpu_partial_matvec_probe_for(
        gpgpu_t62_partial_matvec_profile(),
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t63_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    submit_gpgpu_partial_matvec_probe_for(
        gpgpu_t63_partial_matvec_profile(),
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t63_accum16_hi_live32_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    submit_gpgpu_partial_matvec_probe_for(
        gpgpu_t63_accum16_hi_live32_partial_matvec_profile(),
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t64_windowed_accum16_live48_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    submit_gpgpu_partial_matvec_probe_for(
        gpgpu_t64_windowed_accum16_live48_partial_matvec_profile(),
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn submit_gpgpu_t65_windowed_accum16_live64_partial_matvec_probe(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    submit_gpgpu_partial_matvec_probe_for(
        gpgpu_t65_windowed_accum16_live64_partial_matvec_profile(),
        output_gpu,
        output_bytes,
        expected_words,
        row_count,
        live_k_dim,
    )
}

pub(crate) fn log_gpgpu_t63_first_tile_output_detail_once(
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) {
    if T63_FIRST_TILE_OUTPUT_DETAIL_LOGGED.swap(true, Ordering::AcqRel) {
        return;
    }

    let Some(warm) = warm_state() else {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=no-warm-state output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=no-arena output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=output-before-arena output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    }

    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let output_dwords = (output_bytes / core::mem::size_of::<u32>()).min(GPGPU_TILE_ROWS);
    let Some(output_span_bytes) = output_dwords.checked_mul(core::mem::size_of::<u32>()) else {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=output-span-overflow output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    };
    let Some(output_end) = output_offset.checked_add(output_span_bytes) else {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=output-range-overflow output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    };
    if output_end > warm.gpgpu_arena_len {
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail available=0 reason=output-outside-arena output_gpu=0x{:X} output_bytes=0x{:X} row_count={} live_k_dim={} note=one-shot-wide-output-window\n",
            output_gpu,
            output_bytes,
            row_count,
            live_k_dim,
        );
        return;
    }

    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    crate::intel::dma_flush(output_virt, output_span_bytes);
    let words = output_virt as *const u32;
    let expected_count = row_count.min(expected_words.len());
    let mut nonzero = 0usize;
    let mut nonzero_outside_first_rows = 0usize;
    let mut expected_hits = 0usize;
    let mut expected_misplaced_hits = 0usize;
    let mut expected_hit_mask_lo64 = 0u64;
    let mut first_nonzero_valid = false;
    let mut first_nonzero_slot = 0usize;
    let mut first_nonzero_value = 0u32;
    let mut first_outside_valid = false;
    let mut first_outside_slot = 0usize;
    let mut first_outside_value = 0u32;
    let mut first_expected_valid = false;
    let mut first_expected_slot = 0usize;
    let mut first_expected_value = 0u32;
    let mut digest = 0xCBF2_9CE4_8422_2325u64;

    for index in 0..output_dwords {
        let value = unsafe { core::ptr::read_volatile(words.add(index)) };
        digest ^= value as u64;
        digest = digest.wrapping_mul(0x0000_0100_0000_01B3);
        if value != 0 {
            nonzero += 1;
            if !first_nonzero_valid {
                first_nonzero_valid = true;
                first_nonzero_slot = index;
                first_nonzero_value = value;
            }
            if index >= row_count {
                nonzero_outside_first_rows += 1;
                if !first_outside_valid {
                    first_outside_valid = true;
                    first_outside_slot = index;
                    first_outside_value = value;
                }
            }
        }
        let mut matches_expected = false;
        for expected in expected_words.iter().take(expected_count) {
            if value == *expected {
                matches_expected = true;
                break;
            }
        }
        if matches_expected {
            expected_hits += 1;
            if index < 64 {
                expected_hit_mask_lo64 |= 1u64 << index;
            }
            if !first_expected_valid {
                first_expected_valid = true;
                first_expected_slot = index;
                first_expected_value = value;
            }
            if index >= row_count {
                expected_misplaced_hits += 1;
            }
        }
    }

    crate::log!(
        "intel/gpgpu: t6-3-first-tile-output-detail available=1 output_gpu=0x{:X} output_off=0x{:X} output_bytes=0x{:X} output_dwords={} row_count={} live_k_dim={} nonzero={} nonzero_outside_first_rows={} digest=0x{:016X} expected_hits={} expected_misplaced_hits={} expected_hit_mask_lo64=0x{:016X} first_nonzero_valid={} first_nonzero_slot={} first_nonzero_gpu=0x{:X} first_nonzero_value=0x{:08X} first_outside_valid={} first_outside_slot={} first_outside_gpu=0x{:X} first_outside_value=0x{:08X} first_expected_valid={} first_expected_slot={} first_expected_gpu=0x{:X} first_expected_value=0x{:08X} expected_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] note=one-shot-full-output-tile-window\n",
        output_gpu,
        output_offset,
        output_span_bytes,
        output_dwords,
        row_count,
        live_k_dim,
        nonzero,
        nonzero_outside_first_rows,
        digest,
        expected_hits,
        expected_misplaced_hits,
        expected_hit_mask_lo64,
        first_nonzero_valid as u8,
        first_nonzero_slot,
        output_gpu + (first_nonzero_slot * core::mem::size_of::<u32>()) as u64,
        first_nonzero_value,
        first_outside_valid as u8,
        first_outside_slot,
        output_gpu + (first_outside_slot * core::mem::size_of::<u32>()) as u64,
        first_outside_value,
        first_expected_valid as u8,
        first_expected_slot,
        output_gpu + (first_expected_slot * core::mem::size_of::<u32>()) as u64,
        first_expected_value,
        expected_words[0],
        expected_words[1],
        expected_words[2],
        expected_words[3],
        expected_words[4],
        expected_words[5],
        expected_words[6],
        expected_words[7],
    );

    let mut chunk = [0u32; 16];
    let mut chunk_base = 0usize;
    while chunk_base < output_dwords {
        for slot in chunk.iter_mut() {
            *slot = 0;
        }
        let chunk_len = (output_dwords - chunk_base).min(chunk.len());
        for (local, slot) in chunk.iter_mut().take(chunk_len).enumerate() {
            *slot = unsafe { core::ptr::read_volatile(words.add(chunk_base + local)) };
        }
        crate::log!(
            "intel/gpgpu: t6-3-first-tile-output-detail-chunk output_gpu=0x{:X} slot_base={} slot_end={} words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}]\n",
            output_gpu,
            chunk_base,
            chunk_base + chunk_len.saturating_sub(1),
            chunk[0],
            chunk[1],
            chunk[2],
            chunk[3],
            chunk[4],
            chunk[5],
            chunk[6],
            chunk[7],
            chunk[8],
            chunk[9],
            chunk[10],
            chunk[11],
            chunk[12],
            chunk[13],
            chunk[14],
            chunk[15],
        );
        chunk_base += chunk.len();
    }
}

fn submit_gpgpu_partial_matvec_probe_for(
    profile: GpgpuPartialMatvecProfile,
    output_gpu: u64,
    output_bytes: usize,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    let program = profile.program;
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "no-device",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    };
    let Some(warm) = warm_state() else {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "no-warm-state",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "no-arena",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    if row_count == 0 || row_count > profile.partial_rows || live_k_dim != profile.live_k_dim {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "bad-shape",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    if output_bytes < row_count.saturating_mul(core::mem::size_of::<u32>()) {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "bad-output-bytes",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "output-gpu-before-arena",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(output_end) = output_offset.checked_add(output_bytes) else {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "output-range-overflow",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    };
    if output_end > warm.gpgpu_arena_len {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "output-range-outside-arena",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    let Some(tile_base_offset) = output_offset.checked_sub(GPGPU_TILE_OUTPUT_OFFSET_BYTES) else {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "output-before-tile-output-slot",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    };
    if tile_base_offset % GPGPU_TILE_RECORD_BYTES != 0 {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "output-gpu-not-tile-record-slot",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    let surface_gpu_base = GPU_VA_GPGPU_TILE_ARENA_BASE + tile_base_offset as u64;
    let surface_bytes = GPGPU_TILE_RECORD_BYTES;
    if output_gpu >> 32 != 0 || surface_gpu_base >> 32 != 0 {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "tile-gpu-high32-unsupported",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "forcewake",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) || !ensure_gpgpu_tile_arena_mapped(dev, warm) {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "ppgtt-map",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }

    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    unsafe {
        if profile.clear_output_before_submit {
            clear_gpgpu_output_words(output_virt, row_count);
        }
        core::ptr::write_volatile(
            warm.result_virt
                .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                as *mut u32,
            0,
        );
        core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
        core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
    }
    crate::intel::dma_flush(output_virt, row_count * core::mem::size_of::<u32>());
    crate::intel::dma_flush(warm.result_virt, warm.result_len);

    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, program.words);
    if !program_uploaded {
        return gpgpu_t62_partial_matvec_failure(
            profile,
            "program-upload",
            program,
            output_gpu,
            expected_words,
            row_count,
            live_k_dim,
        );
    }
    let store_surface = prepare_gpgpu_store_surface_state_for_target_span(
        warm,
        surface_gpu_base,
        surface_bytes,
        profile.surface_note,
    );
    let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
    let batch =
        unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
    let dispatch_groups = 1u32;
    let batch_bytes = match encode_gfx12_gpgpu_walker_probe_batch(
        warm,
        batch,
        store_surface,
        program,
        dispatch_groups,
    ) {
        Ok(bytes) => bytes,
        Err(reason) => {
            return gpgpu_t62_partial_matvec_failure(
                profile,
                reason,
                program,
                output_gpu,
                expected_words,
                row_count,
                live_k_dim,
            );
        }
    };
    crate::intel::dma_flush(warm.batch_virt, batch_bytes);

    let dispatch_before = read_gpgpu_threads_dispatched(dev);
    let finished = submit_warm_render_batch(
        dev,
        warm,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
        profile.submit_label,
    );
    crate::intel::dma_flush(warm.result_virt, warm.result_len);
    crate::intel::dma_flush(unsafe { warm.gpgpu_arena_virt.add(tile_base_offset) }, surface_bytes);
    let dispatch_after = read_gpgpu_threads_dispatched(dev);
    let dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
    let finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
    let output_words = unsafe { read_gpgpu_output_words8(output_virt) };
    let mut compare_mask = 0u32;
    for index in 0..row_count {
        if output_words[index] == expected_words[index] {
            compare_mask |= 1u32 << index;
        }
    }
    let expected_mask = if row_count >= 32 {
        u32::MAX
    } else {
        (1u32 << row_count) - 1
    };
    let expected_lane_dispatch = dispatch_groups.saturating_mul(GPGPU_WALKER_SIMD8_LANES);
    let marker_ok = finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let lane_count_matches = dispatch_delta == expected_lane_dispatch as u64;
    let compare_ok = compare_mask == expected_mask;
    let readback_ok = finished && marker_ok && lane_count_matches && compare_ok;
    let reason = if readback_ok {
        profile.success_reason
    } else if !finished {
        "submit-not-finished"
    } else if !marker_ok {
        "finish-marker-mismatch"
    } else if !lane_count_matches {
        "lane-count-mismatch"
    } else {
        "partial-output-mismatch"
    };
    crate::log!(
        "intel/gpgpu: {} submitted=1 finished={} readback_ok={} compare_ok={} reason={} program_source={} groups={} expected_lane_dispatch={} observed_lane_dispatch={} output_gpu=0x{:X} row_count={} live_k_dim={} compare_mask=0x{:08X} expected_mask=0x{:08X} output_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap next={} does_not_prove=full_model_matvec\n",
        profile.log_label,
        finished as u8,
        readback_ok as u8,
        compare_ok as u8,
        reason,
        program.name,
        dispatch_groups,
        expected_lane_dispatch,
        dispatch_delta,
        output_gpu,
        row_count,
        live_k_dim,
        compare_mask,
        expected_mask,
        output_words[0],
        output_words[1],
        output_words[2],
        output_words[3],
        output_words[4],
        output_words[5],
        output_words[6],
        output_words[7],
        expected_words[0],
        expected_words[1],
        expected_words[2],
        expected_words[3],
        expected_words[4],
        expected_words[5],
        expected_words[6],
        expected_words[7],
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        profile.success_next,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, profile.submit_label);
    }
    crate::intel::GpgpuT62PartialMatvecProof {
        submitted: true,
        finished,
        readback_ok,
        compare_ok,
        reason,
        program_name: program.name,
        output_gpu,
        output_words,
        expected_words,
        compare_mask,
        expected_mask,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        row_count,
        live_k_dim,
    }
}

fn submit_gpgpu_one_row_matvec_probe_for(
    profile: GpgpuOneRowMatvecProfile,
    output_gpu: u64,
    output_bytes: usize,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> crate::intel::GpgpuT5OneRowMatvecProof {
    let program = profile.program;
    let Some(dev) = crate::intel::claimed_device() else {
        return gpgpu_t5_one_row_matvec_failure(
            "no-device",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    let Some(warm) = warm_state() else {
        return gpgpu_t5_one_row_matvec_failure(
            "no-warm-state",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    if warm.gpgpu_arena_virt.is_null() || warm.gpgpu_arena_len == 0 {
        return gpgpu_t5_one_row_matvec_failure(
            "no-arena",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    if output_bytes < core::mem::size_of::<u32>() {
        return gpgpu_t5_one_row_matvec_failure(
            "bad-output-bytes",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    if output_gpu < GPU_VA_GPGPU_TILE_ARENA_BASE {
        return gpgpu_t5_one_row_matvec_failure(
            "output-gpu-before-arena",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    let output_offset = (output_gpu - GPU_VA_GPGPU_TILE_ARENA_BASE) as usize;
    let Some(output_end) = output_offset.checked_add(output_bytes) else {
        return gpgpu_t5_one_row_matvec_failure(
            "output-range-overflow",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    if output_end > warm.gpgpu_arena_len {
        return gpgpu_t5_one_row_matvec_failure(
            "output-range-outside-arena",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    let Some(tile_base_offset) = output_offset.checked_sub(GPGPU_TILE_OUTPUT_OFFSET_BYTES) else {
        return gpgpu_t5_one_row_matvec_failure(
            "output-before-tile-output-slot",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    if tile_base_offset % GPGPU_TILE_RECORD_BYTES != 0 {
        return gpgpu_t5_one_row_matvec_failure(
            "output-gpu-not-tile-record-slot",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    let Some(tile_record_end) = tile_base_offset.checked_add(GPGPU_TILE_RECORD_USED_BYTES) else {
        return gpgpu_t5_one_row_matvec_failure(
            "tile-record-overflow",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    if tile_record_end > warm.gpgpu_arena_len {
        return gpgpu_t5_one_row_matvec_failure(
            "tile-record-outside-arena",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    let surface_gpu_base = GPU_VA_GPGPU_TILE_ARENA_BASE + tile_base_offset as u64;
    let surface_bytes = GPGPU_TILE_RECORD_BYTES;
    let Some(scan_bytes) = tile_base_offset.checked_add(surface_bytes).map(|bytes| {
        bytes
            .min(warm.gpgpu_arena_len)
            .saturating_sub(tile_base_offset)
    }) else {
        return gpgpu_t5_one_row_matvec_failure(
            "tile-scan-overflow",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    };
    if output_gpu >> 32 != 0 || surface_gpu_base >> 32 != 0 {
        return gpgpu_t5_one_row_matvec_failure(
            "tile-gpu-high32-unsupported",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    if live_k_dim != profile.live_k_dim {
        return gpgpu_t5_one_row_matvec_failure(
            "unexpected-live-k-dim",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    if !forcewake_render_acquire(warm) {
        return gpgpu_t5_one_row_matvec_failure(
            "forcewake",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }
    if !ensure_gpgpu_warm_buffers_mapped(dev, warm) || !ensure_gpgpu_tile_arena_mapped(dev, warm) {
        return gpgpu_t5_one_row_matvec_failure(
            "ppgtt-map",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }

    let output_virt = unsafe { warm.gpgpu_arena_virt.add(output_offset) };
    let output_count = output_bytes / core::mem::size_of::<u32>();
    let output_words_before_clear = unsafe {
        let words = output_virt as *const u32;
        [
            core::ptr::read_volatile(words.add(0)),
            core::ptr::read_volatile(words.add(1)),
            core::ptr::read_volatile(words.add(2)),
            core::ptr::read_volatile(words.add(3)),
        ]
    };
    unsafe {
        let words = output_virt as *mut u32;
        for index in 0..4 {
            core::ptr::write_volatile(words.add(index), 0);
        }
    }
    crate::intel::dma_flush(output_virt, 4 * core::mem::size_of::<u32>());
    let output_words_after_clear = unsafe {
        let words = output_virt as *const u32;
        [
            core::ptr::read_volatile(words.add(0)),
            core::ptr::read_volatile(words.add(1)),
            core::ptr::read_volatile(words.add(2)),
            core::ptr::read_volatile(words.add(3)),
        ]
    };
    crate::log!(
        "intel/gpgpu: {}-output-window stage=before-submit output_gpu=0x{:X} old=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] cleared=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] surface_bytes=0x{:X} cpu_expected_bits=0x{:08X}\n",
        profile.log_prefix,
        output_gpu,
        output_words_before_clear[0],
        output_words_before_clear[1],
        output_words_before_clear[2],
        output_words_before_clear[3],
        output_words_after_clear[0],
        output_words_after_clear[1],
        output_words_after_clear[2],
        output_words_after_clear[3],
        surface_bytes,
        cpu_expected_bits,
    );
    let t5_arena_before = probe_gpgpu_t5_arena_store_window(
        warm,
        tile_base_offset,
        scan_bytes,
        output_offset,
        output_bytes,
        cpu_expected_bits,
        profile,
    );
    log_gpgpu_t5_arena_store_probe(
        "before-submit",
        t5_arena_before,
        None,
        output_gpu,
        cpu_expected_bits,
        profile,
    );

    submit_gpgpu_t5_store_only_control_probe(
        dev,
        warm,
        output_gpu,
        surface_gpu_base,
        output_offset,
        output_bytes,
        surface_bytes,
    );
    crate::intel::dma_flush(unsafe { warm.gpgpu_arena_virt.add(tile_base_offset) }, scan_bytes);
    let t5_arena_after_store_only = probe_gpgpu_t5_arena_store_window(
        warm,
        tile_base_offset,
        scan_bytes,
        output_offset,
        output_bytes,
        cpu_expected_bits,
        profile,
    );
    log_gpgpu_t5_arena_store_probe(
        "after-store-only-control",
        t5_arena_after_store_only,
        Some(t5_arena_before),
        output_gpu,
        cpu_expected_bits,
        profile,
    );
    submit_gpgpu_t5_load_echo_probe(
        dev,
        warm,
        output_gpu,
        surface_gpu_base,
        tile_base_offset,
        output_offset,
        surface_bytes,
    );
    crate::intel::dma_flush(unsafe { warm.gpgpu_arena_virt.add(tile_base_offset) }, scan_bytes);
    let t5_arena_after_load_echo = probe_gpgpu_t5_arena_store_window(
        warm,
        tile_base_offset,
        scan_bytes,
        output_offset,
        output_bytes,
        cpu_expected_bits,
        profile,
    );
    log_gpgpu_t5_arena_store_probe(
        "after-load-echo",
        t5_arena_after_load_echo,
        Some(t5_arena_after_store_only),
        output_gpu,
        cpu_expected_bits,
        profile,
    );
    unsafe {
        clear_gpgpu_output_words(output_virt, 8);
    }
    crate::intel::dma_flush(output_virt, 8 * core::mem::size_of::<u32>());
    let output_words_before_live = unsafe {
        let words = output_virt as *const u32;
        [
            core::ptr::read_volatile(words.add(0)),
            core::ptr::read_volatile(words.add(1)),
            core::ptr::read_volatile(words.add(2)),
            core::ptr::read_volatile(words.add(3)),
        ]
    };
    crate::log!(
        "intel/gpgpu: {}-output-window stage=before-live-submit output_gpu=0x{:X} cleared=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] surface_bytes=0x{:X} cpu_expected_bits=0x{:08X}\n",
        profile.log_prefix,
        output_gpu,
        output_words_before_live[0],
        output_words_before_live[1],
        output_words_before_live[2],
        output_words_before_live[3],
        surface_bytes,
        cpu_expected_bits,
    );
    let t5_arena_before = probe_gpgpu_t5_arena_store_window(
        warm,
        tile_base_offset,
        scan_bytes,
        output_offset,
        output_bytes,
        cpu_expected_bits,
        profile,
    );
    log_gpgpu_t5_arena_store_probe(
        "before-live-submit",
        t5_arena_before,
        Some(t5_arena_after_load_echo),
        output_gpu,
        cpu_expected_bits,
        profile,
    );

    let program_uploaded =
        upload_and_verify_gpu_program_at(warm, GPGPU_EU_KERNEL_OFFSET_BYTES, program.words);
    if !program_uploaded {
        return gpgpu_t5_one_row_matvec_failure(
            "program-upload",
            profile,
            program,
            output_gpu,
            cpu_expected_bits,
            live_k_dim,
        );
    }

    let store_surface = prepare_gpgpu_store_surface_state_for_target_span(
        warm,
        surface_gpu_base,
        surface_bytes,
        profile.surface_note,
    );
    let t5_input_summary =
        read_gpgpu_t5_input_summary_at(warm, tile_base_offset, profile.live_k_dim);
    let mut submitted = false;
    let mut finished = false;
    let mut batch_bytes = 0usize;
    let mut dispatch_delta = 0u64;
    let mut finish_marker = 0u32;
    let mut output_words_after = [0u32; 4];
    let mut output_first_before = 0u32;
    let mut last_group_x_dim = 0u32;
    let mut last_expected_lane_dispatch = 0u32;
    let mut last_scale_clean = false;

    for (scale_index, &group_x_dim) in profile.scale_ladder.iter().enumerate() {
        let expected_hw_threads = group_x_dim.saturating_mul(GPGPU_WALKER_GROUP_THREADS);
        let expected_lane_dispatch = expected_hw_threads.saturating_mul(GPGPU_WALKER_SIMD8_LANES);
        last_group_x_dim = group_x_dim;
        last_expected_lane_dispatch = expected_lane_dispatch;

        unsafe {
            clear_gpgpu_output_words(output_virt, 8);
            core::ptr::write_volatile(
                warm.result_virt
                    .add(RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD * core::mem::size_of::<u32>())
                    as *mut u32,
                0,
            );
            core::ptr::write_bytes(warm.batch_virt, 0, warm.batch_len);
            core::ptr::write_bytes(warm.ring_virt, 0, warm.ring_len);
        }
        crate::intel::dma_flush(output_virt, 8 * core::mem::size_of::<u32>());
        crate::intel::dma_flush(warm.result_virt, warm.result_len);
        output_first_before = unsafe { core::ptr::read_volatile(output_virt as *const u32) };

        let batch_dwords = warm.batch_len / core::mem::size_of::<u32>();
        let batch =
            unsafe { core::slice::from_raw_parts_mut(warm.batch_virt as *mut u32, batch_dwords) };
        let batch_result =
            encode_gfx12_gpgpu_walker_probe_batch(warm, batch, store_surface, program, group_x_dim);
        batch_bytes = match batch_result {
            Ok(bytes) => bytes,
            Err(reason) => {
                crate::log!(
                    "intel/gpgpu: {}-scale-proof scale_index={} program_source={} requested_groups={} requested_group_count={} threads_per_group={} expected_hw_threads={} simd_lanes_per_thread={} expected_lane_dispatch={} observed_lane_dispatch=0 lane_count_matches=0 submitted=0 retired=0 finish_marker=0x00000000 finish_expected=0x{:08X} output_first_before=0x{:08X} output_first_after=0x00000000 gpu_matches_packed_bf16=0 gpu_matches_word_view=0 word_view_bits=0x{:08X} packed_bf16_bits=0x{:08X} failure_class={} batch_bytes=0x0 output_owner=cpu-ap does_not_prove=full_model_matvec\n",
                    profile.scale_prefix,
                    scale_index,
                    program.name,
                    group_x_dim,
                    group_x_dim,
                    GPGPU_WALKER_GROUP_THREADS,
                    expected_hw_threads,
                    GPGPU_WALKER_SIMD8_LANES,
                    expected_lane_dispatch,
                    RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
                    output_first_before,
                    t5_input_summary.shader_word_low_bits,
                    t5_input_summary.direct_bits,
                    reason,
                );
                crate::log!(
                    "intel/gpgpu: {}-scale-ladder stop_at_scale={} reason=batch-encode requested_groups={} expected_lane_dispatch={} observed_lane_dispatch=0\n",
                    profile.scale_prefix,
                    scale_index,
                    group_x_dim,
                    expected_lane_dispatch,
                );
                break;
            }
        };
        crate::intel::dma_flush(warm.batch_virt, batch_bytes);

        let dispatch_before = read_gpgpu_threads_dispatched(dev);
        finished = submit_warm_render_batch(
            dev,
            warm,
            RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
            RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD,
            profile.submit_label,
        );
        submitted = batch_bytes != 0;
        crate::intel::dma_flush(warm.result_virt, warm.result_len);
        crate::intel::dma_flush(unsafe { warm.gpgpu_arena_virt.add(tile_base_offset) }, scan_bytes);
        let dispatch_after = read_gpgpu_threads_dispatched(dev);
        dispatch_delta = dispatch_after.saturating_sub(dispatch_before);
        finish_marker = read_result_dword(warm, RESULT_SLOT_GPGPU_COMPUTE_WALKER_DWORD);
        output_words_after = unsafe {
            let words = output_virt as *const u32;
            [
                core::ptr::read_volatile(words.add(0)),
                core::ptr::read_volatile(words.add(1)),
                core::ptr::read_volatile(words.add(2)),
                core::ptr::read_volatile(words.add(3)),
            ]
        };
        if scale_index == 0 {
            log_gpgpu_t5_input_summary(
                "after-live-submit",
                profile,
                t5_input_summary,
                output_words_after[0],
                cpu_expected_bits,
            );
        }

        let lane_count_matches = dispatch_delta == expected_lane_dispatch as u64;
        let marker_ok = finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
        let packed_bf16_ok = output_words_after[0] == t5_input_summary.direct_bits;
        let shader_word_ok = output_words_after[0] == t5_input_summary.shader_word_low_bits;
        last_scale_clean =
            submitted && finished && marker_ok && lane_count_matches && packed_bf16_ok;
        let failure_class = if last_scale_clean {
            profile.success_class
        } else if !finished {
            "submit-not-finished"
        } else if !marker_ok {
            "finish-marker-mismatch"
        } else if !lane_count_matches {
            "lane-count-mismatch"
        } else if !packed_bf16_ok {
            "packed-bf16-output-mismatch"
        } else {
            "unknown"
        };
        crate::log!(
            "intel/gpgpu: {}-scale-proof scale_index={} program_source={} requested_groups={} requested_group_count={} threads_per_group={} expected_hw_threads={} simd_lanes_per_thread={} expected_lane_dispatch={} observed_lane_dispatch={} lane_count_matches={} submitted={} retired={} finish_marker=0x{:08X} finish_expected=0x{:08X} output_first_before=0x{:08X} output_first_after=0x{:08X} gpu_matches_packed_bf16={} gpu_matches_word_view={} word_view_bits=0x{:08X} packed_bf16_bits=0x{:08X} failure_class={} batch_bytes=0x{:X} output_owner=cpu-ap does_not_prove=full_model_matvec\n",
            profile.scale_prefix,
            scale_index,
            program.name,
            group_x_dim,
            group_x_dim,
            GPGPU_WALKER_GROUP_THREADS,
            expected_hw_threads,
            GPGPU_WALKER_SIMD8_LANES,
            expected_lane_dispatch,
            dispatch_delta,
            lane_count_matches as u8,
            submitted as u8,
            finished as u8,
            finish_marker,
            RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
            output_first_before,
            output_words_after[0],
            packed_bf16_ok as u8,
            shader_word_ok as u8,
            t5_input_summary.shader_word_low_bits,
            t5_input_summary.direct_bits,
            failure_class,
            batch_bytes,
        );
        if !finished {
            recover_render_engine_after_nonretired_submit(dev, warm, profile.submit_label);
        }
        if !last_scale_clean {
            crate::log!(
                "intel/gpgpu: {}-scale-ladder stop_at_scale={} reason=first-nonclean-proof requested_groups={} expected_lane_dispatch={} observed_lane_dispatch={}\n",
                profile.scale_prefix,
                scale_index,
                group_x_dim,
                expected_lane_dispatch,
                dispatch_delta,
            );
            break;
        }
    }

    let output_first_after = output_words_after[0];
    let output_hits_lo64 = unsafe {
        gpgpu_stage_dword_hits_mask_lo64(output_virt as *const u32, output_count, cpu_expected_bits)
    };
    crate::log!(
        "intel/gpgpu: {}-output-window stage=after-submit output_gpu=0x{:X} words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X}] expected_bits=0x{:08X} expected_meta=[0x{:08X},0x{:08X},0x00000000] finish_marker=0x{:08X} last_groups={}\n",
        profile.log_prefix,
        output_gpu,
        output_words_after[0],
        output_words_after[1],
        output_words_after[2],
        output_words_after[3],
        cpu_expected_bits,
        profile.live_k_dim as u32,
        profile.expected_sentinel,
        finish_marker,
        last_group_x_dim,
    );
    let t5_arena_after = probe_gpgpu_t5_arena_store_window(
        warm,
        tile_base_offset,
        scan_bytes,
        output_offset,
        output_bytes,
        cpu_expected_bits,
        profile,
    );
    log_gpgpu_t5_arena_store_probe(
        "after-submit",
        t5_arena_after,
        Some(t5_arena_before),
        output_gpu,
        cpu_expected_bits,
        profile,
    );
    let compare_ok = output_first_after == cpu_expected_bits && (output_hits_lo64 & 1) != 0;
    let readback_ok =
        compare_ok && finished && finish_marker == RCS_EXEC_RESULT_COMPUTE_WALKER_DONE;
    let reason = if readback_ok && dispatch_delta == 0 {
        profile.success_reason_no_ts
    } else if readback_ok {
        profile.success_reason
    } else if !finished {
        "submit-not-finished"
    } else if last_scale_clean {
        "packed-bf16-scale-clean-final-compare-mismatch"
    } else if output_first_after == t5_input_summary.shader_word_low_bits {
        "legacy-word-view-output"
    } else if output_first_after != cpu_expected_bits {
        "compare-mismatch"
    } else {
        "compare-not-at-slot0"
    };
    crate::log!(
        "intel/gpgpu: {} submitted={} finished={} readback_ok={} compare_ok={} reason={} program_source={} groups={} expected_lane_dispatch={} observed_lane_dispatch={} output_gpu=0x{:X} output_first_before=0x{:08X} output_first_after=0x{:08X} gpu_value=0x{:08X} cpu_expected_bits=0x{:08X} output_hits_lo64=0x{:016X} live_k_dim={} requires_live_gpu_load={} finish_marker=0x{:08X} finish_expected=0x{:08X} batch_bytes=0x{:X} output_owner=cpu-ap next=scale-live-k-or-row-count does_not_prove=full_model_matvec\n",
        profile.summary_label,
        submitted as u8,
        finished as u8,
        readback_ok as u8,
        compare_ok as u8,
        reason,
        program.name,
        last_group_x_dim,
        last_expected_lane_dispatch,
        dispatch_delta,
        output_gpu,
        output_first_before,
        output_first_after,
        output_first_after,
        cpu_expected_bits,
        output_hits_lo64,
        live_k_dim,
        profile.requires_live_gpu_load as u8,
        finish_marker,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
    );
    if !finished {
        recover_render_engine_after_nonretired_submit(dev, warm, profile.summary_label);
    }
    crate::intel::GpgpuT5OneRowMatvecProof {
        submitted,
        finished,
        readback_ok,
        compare_ok,
        reason,
        program_name: program.name,
        output_gpu,
        gpu_value: output_first_after,
        cpu_expected_bits,
        output_first_before,
        output_first_after,
        output_hits_lo64,
        dispatch_delta,
        finish_marker,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes,
        live_k_dim,
        requires_live_gpu_load: profile.requires_live_gpu_load,
    }
}

fn gpgpu_one_tile_compare_failure(
    reason: &'static str,
    program: GpgpuEuProgram,
    output_gpu: u64,
    cpu_expected_bits: u32,
) -> crate::intel::GpgpuOneTileCompareProof {
    crate::log!(
        "intel/gpgpu: one-tile-output-compare submitted=0 finished=0 readback_ok=0 compare_ok=0 reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_first_before=0x00000000 output_first_after=0x00000000 gpu_value=0x00000000 cpu_expected_bits=0x{:08X} output_hits_lo64=0x0000000000000000 finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 output_owner=cpu-ap next=fix-one-tile-output-compare does_not_prove=model_matvec_or_gpu_live_load\n",
        reason,
        program.name,
        output_gpu,
        cpu_expected_bits,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::intel::GpgpuOneTileCompareProof {
        submitted: false,
        finished: false,
        readback_ok: false,
        compare_ok: false,
        reason,
        program_name: program.name,
        output_gpu,
        gpu_value: 0,
        cpu_expected_bits,
        output_first_before: 0,
        output_first_after: 0,
        output_hits_lo64: 0,
        dispatch_delta: 0,
        finish_marker: 0,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes: 0,
    }
}

fn gpgpu_t5_one_row_matvec_failure(
    reason: &'static str,
    profile: GpgpuOneRowMatvecProfile,
    program: GpgpuEuProgram,
    output_gpu: u64,
    cpu_expected_bits: u32,
    live_k_dim: usize,
) -> crate::intel::GpgpuT5OneRowMatvecProof {
    crate::log!(
        "intel/gpgpu: {} submitted=0 finished=0 readback_ok=0 compare_ok=0 reason={} program_source={} groups=1 expected_lane_dispatch=8 observed_lane_dispatch=0 output_gpu=0x{:X} output_first_before=0x00000000 output_first_after=0x00000000 gpu_value=0x00000000 cpu_expected_bits=0x{:08X} output_hits_lo64=0x0000000000000000 live_k_dim={} requires_live_gpu_load={} finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 output_owner=cpu-ap next=fix-one-row-matvec-artifact does_not_prove=model_matvec_or_gpu_live_load\n",
        profile.summary_label,
        reason,
        program.name,
        output_gpu,
        cpu_expected_bits,
        live_k_dim,
        profile.requires_live_gpu_load as u8,
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
    );
    crate::intel::GpgpuT5OneRowMatvecProof {
        submitted: false,
        finished: false,
        readback_ok: false,
        compare_ok: false,
        reason,
        program_name: program.name,
        output_gpu,
        gpu_value: 0,
        cpu_expected_bits,
        output_first_before: 0,
        output_first_after: 0,
        output_hits_lo64: 0,
        dispatch_delta: 0,
        finish_marker: 0,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes: 0,
        live_k_dim,
        requires_live_gpu_load: profile.requires_live_gpu_load,
    }
}

fn gpgpu_t62_partial_matvec_failure(
    profile: GpgpuPartialMatvecProfile,
    reason: &'static str,
    program: GpgpuEuProgram,
    output_gpu: u64,
    expected_words: [u32; 8],
    row_count: usize,
    live_k_dim: usize,
) -> crate::intel::GpgpuT62PartialMatvecProof {
    crate::log!(
        "intel/gpgpu: {} submitted=0 finished=0 readback_ok=0 compare_ok=0 reason={} program_source={} groups={} expected_lane_dispatch=0 observed_lane_dispatch=0 output_gpu=0x{:X} row_count={} live_k_dim={} compare_mask=0x00000000 expected_mask=0x00000000 output_words=[0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000,0x00000000] expected_words=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] finish_marker=0x00000000 finish_expected=0x{:08X} batch_bytes=0x0 output_owner=cpu-ap next={} does_not_prove=full_model_matvec\n",
        profile.log_label,
        reason,
        program.name,
        row_count,
        output_gpu,
        row_count,
        live_k_dim,
        expected_words[0],
        expected_words[1],
        expected_words[2],
        expected_words[3],
        expected_words[4],
        expected_words[5],
        expected_words[6],
        expected_words[7],
        RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        profile.failure_next,
    );
    crate::intel::GpgpuT62PartialMatvecProof {
        submitted: false,
        finished: false,
        readback_ok: false,
        compare_ok: false,
        reason,
        program_name: program.name,
        output_gpu,
        output_words: [0; 8],
        expected_words,
        compare_mask: 0,
        expected_mask: 0,
        dispatch_delta: 0,
        finish_marker: 0,
        expected_finish_marker: RCS_EXEC_RESULT_COMPUTE_WALKER_DONE,
        batch_bytes: 0,
        row_count,
        live_k_dim,
    }
}

fn gpgpu_tile_rows_stage_failure(
    reason: &'static str,
    output_gpu: u64,
    row_count: usize,
    k_dim: usize,
    rows_checksum: u64,
) -> crate::intel::GpgpuTileRowsStageProof {
    crate::log!(
        "intel/gpgpu: tile-rows-stage staged=0 reason={} output_gpu=0x{:X} row_count={} k_dim={} row_bytes={} rows_checksum=0x{:016X} staged_rows_checksum=0x0000000000000000 output_zeroed=0 output_nonzero_dwords=0 next=fix-tile-rows-stage does_not_prove=full_model_matvec\n",
        reason,
        output_gpu,
        row_count,
        k_dim,
        row_count
            .saturating_mul(k_dim)
            .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM),
        rows_checksum,
    );
    crate::intel::GpgpuTileRowsStageProof {
        staged: false,
        reason,
        readback_ok: false,
        output_zeroed: false,
        output_gpu,
        row_count,
        row_bytes: row_count
            .saturating_mul(k_dim)
            .saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM),
        rows_checksum,
        staged_rows_checksum: 0,
        output_nonzero_dwords: 0,
    }
}

fn log_gpgpu_one_tile_stage_failure(
    reason: &'static str,
    k_dim: usize,
    row_index: usize,
    x_checksum: u64,
    row_checksum: u64,
    cpu_expected_bits: u32,
) -> crate::intel::GpgpuOneTileStageProof {
    let proof = crate::intel::GpgpuOneTileStageProof {
        staged: false,
        reason,
        readback_ok: false,
        output_zeroed: false,
        arena_mapped: false,
        arena_gpu_base: 0,
        x_gpu: 0,
        row_gpu: 0,
        output_gpu: 0,
        x_bytes: k_dim.saturating_mul(GPGPU_TILE_X_BYTES_PER_ELEM),
        row_bytes: k_dim.saturating_mul(GPGPU_TILE_WEIGHT_BYTES_PER_ELEM),
        output_bytes: 0,
        tile_rows: GPGPU_TILE_ROWS,
        k_dim,
        output_first_bits: 0,
        output_nonzero_dwords: 0,
        output_expected_hits_lo64: 0,
        output_checksum: 0,
    };
    crate::log!(
        "intel/gpgpu: one-tile-stage staged=0 reason={} arena_mapped=0 arena_gpu_base=0x0 row={} tile_rows={} k_dim={} x_bytes={} row_bytes={} output_bytes=0 x_checksum=0x{:016X} row_checksum=0x{:016X} cpu_expected_bits=0x{:08X} gpu_submission=0 output_owner=cpu-ap next=fix-one-tile-stage does_not_prove=gpu_live_load_or_model_matvec\n",
        reason,
        row_index,
        proof.tile_rows,
        proof.k_dim,
        proof.x_bytes,
        proof.row_bytes,
        x_checksum,
        row_checksum,
        cpu_expected_bits,
    );
    proof
}

#[derive(Copy, Clone)]
struct GpgpuT5ArenaRangeProbe {
    nonzero_dwords: usize,
    digest: u64,
}

#[derive(Copy, Clone)]
struct GpgpuT5ArenaMarkerProbe {
    hits: usize,
    misplaced_hits: usize,
    first_valid: bool,
    first_off: usize,
    first_misplaced_valid: bool,
    first_misplaced_off: usize,
}

#[derive(Copy, Clone)]
struct GpgpuT5ArenaStoreProbe {
    scan_base_offset: usize,
    scan_bytes: usize,
    output_offset: usize,
    output_record_bytes: usize,
    scan: GpgpuT5ArenaRangeProbe,
    x: GpgpuT5ArenaRangeProbe,
    row0: GpgpuT5ArenaRangeProbe,
    output: GpgpuT5ArenaRangeProbe,
    expected: GpgpuT5ArenaMarkerProbe,
    meta_k: GpgpuT5ArenaMarkerProbe,
    sentinel: GpgpuT5ArenaMarkerProbe,
}

#[derive(Copy, Clone)]
struct GpgpuT5InputSummary {
    x_bits: [u32; 8],
    row_le: [u16; 16],
    direct_bits: u32,
    shader_word_low_bits: u32,
    live_k_dim: usize,
}

fn empty_gpgpu_t5_arena_range_probe() -> GpgpuT5ArenaRangeProbe {
    GpgpuT5ArenaRangeProbe {
        nonzero_dwords: 0,
        digest: 0xCBF2_9CE4_8422_2325,
    }
}

fn empty_gpgpu_t5_arena_marker_probe() -> GpgpuT5ArenaMarkerProbe {
    GpgpuT5ArenaMarkerProbe {
        hits: 0,
        misplaced_hits: 0,
        first_valid: false,
        first_off: 0,
        first_misplaced_valid: false,
        first_misplaced_off: 0,
    }
}

fn gpgpu_t5_bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

fn gpgpu_packed_bf16_dot_bits(x_bits: &[u32; 8], row_le: &[u16; 16], live_k_dim: usize) -> u32 {
    let mut acc = 0.0f32;
    for lane in 0..live_k_dim.min(x_bits.len()).min(row_le.len()) {
        acc += f32::from_bits(x_bits[lane]) * gpgpu_t5_bf16_to_f32(row_le[lane]);
    }
    acc.to_bits()
}

fn gpgpu_t5_word_low_dot_bits(x_bits: &[u32; 8], row_le: &[u16; 16]) -> u32 {
    let mut acc = 0.0f32;
    for lane in 0..trueos_eu::gfx12::T5_ONE_ROW_MATVEC_LIVE_K {
        acc += f32::from_bits(x_bits[lane]) * gpgpu_t5_bf16_to_f32(row_le[lane * 2]);
    }
    acc.to_bits()
}

fn read_gpgpu_t5_input_summary_at(
    warm: RenderWarmState,
    tile_base_offset: usize,
    live_k_dim: usize,
) -> GpgpuT5InputSummary {
    let mut x_bits = [0u32; 8];
    let mut row_le = [0u16; 16];
    unsafe {
        let tile_base = warm.gpgpu_arena_virt.add(tile_base_offset);
        let x_ptr = tile_base as *const u32;
        for (index, slot) in x_bits.iter_mut().enumerate() {
            *slot = core::ptr::read_volatile(x_ptr.add(index));
        }
        let row_ptr = tile_base.add(GPGPU_X_VECTOR_BYTES) as *const u16;
        for (index, slot) in row_le.iter_mut().enumerate() {
            *slot = core::ptr::read_volatile(row_ptr.add(index));
        }
    }

    GpgpuT5InputSummary {
        x_bits,
        row_le,
        direct_bits: gpgpu_packed_bf16_dot_bits(&x_bits, &row_le, live_k_dim),
        shader_word_low_bits: gpgpu_t5_word_low_dot_bits(&x_bits, &row_le),
        live_k_dim,
    }
}

fn log_gpgpu_t5_input_summary(
    stage: &'static str,
    profile: GpgpuOneRowMatvecProfile,
    summary: GpgpuT5InputSummary,
    gpu_bits: u32,
    cpu_expected_bits: u32,
) {
    crate::log!(
        "intel/gpgpu: {}-input-summary stage={} gpu=0x{:08X} cpu_expected=0x{:08X} cpu_direct=0x{:08X} legacy_word_view=0x{:08X} live_k_dim={} direct_matches_cpu={} gpu_matches_direct={} gpu_matches_legacy_word_view={} shader_row_lanes=packed-prefix legacy_word_view_lanes=[0,2,4,6] x_bits=[0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X},0x{:08X}] row_le=[0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X},0x{:04X}] proof=cpu-arena-operands-and-packed-bf16-unpack\n",
        profile.log_prefix,
        stage,
        gpu_bits,
        cpu_expected_bits,
        summary.direct_bits,
        summary.shader_word_low_bits,
        summary.live_k_dim,
        (summary.direct_bits == cpu_expected_bits) as u8,
        (summary.direct_bits == gpu_bits) as u8,
        (summary.shader_word_low_bits == gpu_bits) as u8,
        summary.x_bits[0],
        summary.x_bits[1],
        summary.x_bits[2],
        summary.x_bits[3],
        summary.x_bits[4],
        summary.x_bits[5],
        summary.x_bits[6],
        summary.x_bits[7],
        summary.row_le[0],
        summary.row_le[1],
        summary.row_le[2],
        summary.row_le[3],
        summary.row_le[4],
        summary.row_le[5],
        summary.row_le[6],
        summary.row_le[7],
        summary.row_le[8],
        summary.row_le[9],
        summary.row_le[10],
        summary.row_le[11],
        summary.row_le[12],
        summary.row_le[13],
        summary.row_le[14],
        summary.row_le[15],
    );
}

fn gpgpu_t5_arena_range_step(range: &mut GpgpuT5ArenaRangeProbe, value: u32) {
    if value != 0 {
        range.nonzero_dwords += 1;
    }
    range.digest ^= value as u64;
    range.digest = range.digest.wrapping_mul(0x0000_0100_0000_01B3);
}

fn gpgpu_t5_arena_marker_step(
    marker: &mut GpgpuT5ArenaMarkerProbe,
    offset: usize,
    misplaced: bool,
) {
    marker.hits += 1;
    if !marker.first_valid {
        marker.first_valid = true;
        marker.first_off = offset;
    }
    if misplaced {
        marker.misplaced_hits += 1;
        if !marker.first_misplaced_valid {
            marker.first_misplaced_valid = true;
            marker.first_misplaced_off = offset;
        }
    }
}

fn probe_gpgpu_t5_arena_store_window(
    warm: RenderWarmState,
    scan_base_offset: usize,
    scan_bytes: usize,
    output_offset: usize,
    output_bytes: usize,
    cpu_expected_bits: u32,
    profile: GpgpuOneRowMatvecProfile,
) -> GpgpuT5ArenaStoreProbe {
    let scan_bytes = scan_base_offset
        .checked_add(scan_bytes)
        .map(|end| {
            end.min(warm.gpgpu_arena_len)
                .saturating_sub(scan_base_offset)
        })
        .unwrap_or(0)
        & !3usize;
    let output_record_bytes = output_bytes.min(4 * core::mem::size_of::<u32>());
    let output_record_end = output_offset.saturating_add(output_record_bytes);
    let output_end = output_offset.saturating_add(output_bytes);
    let x_end = scan_base_offset
        .saturating_add(GPGPU_X_VECTOR_BYTES)
        .min(scan_base_offset.saturating_add(scan_bytes));
    let row0_end = GPGPU_X_VECTOR_BYTES
        .saturating_add(GPGPU_WEIGHT_TILE_BYTES)
        .saturating_add(scan_base_offset)
        .min(scan_base_offset.saturating_add(scan_bytes));
    let mut probe = GpgpuT5ArenaStoreProbe {
        scan_base_offset,
        scan_bytes,
        output_offset,
        output_record_bytes,
        scan: empty_gpgpu_t5_arena_range_probe(),
        x: empty_gpgpu_t5_arena_range_probe(),
        row0: empty_gpgpu_t5_arena_range_probe(),
        output: empty_gpgpu_t5_arena_range_probe(),
        expected: empty_gpgpu_t5_arena_marker_probe(),
        meta_k: empty_gpgpu_t5_arena_marker_probe(),
        sentinel: empty_gpgpu_t5_arena_marker_probe(),
    };
    let words = unsafe { warm.gpgpu_arena_virt.add(scan_base_offset) } as *const u32;
    let scan_dwords = scan_bytes / core::mem::size_of::<u32>();
    for index in 0..scan_dwords {
        let offset = scan_base_offset + index * core::mem::size_of::<u32>();
        let value = unsafe { core::ptr::read_volatile(words.add(index)) };
        let outside_output_record = offset < output_offset || offset >= output_record_end;
        gpgpu_t5_arena_range_step(&mut probe.scan, value);
        if offset >= scan_base_offset && offset < x_end {
            gpgpu_t5_arena_range_step(&mut probe.x, value);
        } else if offset < row0_end {
            gpgpu_t5_arena_range_step(&mut probe.row0, value);
        }
        if offset >= output_offset && offset < output_end {
            gpgpu_t5_arena_range_step(&mut probe.output, value);
        }
        if value == cpu_expected_bits {
            gpgpu_t5_arena_marker_step(&mut probe.expected, offset, outside_output_record);
        }
        if value == profile.live_k_dim as u32 {
            gpgpu_t5_arena_marker_step(&mut probe.meta_k, offset, outside_output_record);
        }
        if value == profile.expected_sentinel {
            gpgpu_t5_arena_marker_step(&mut probe.sentinel, offset, outside_output_record);
        }
    }
    probe
}

fn gpgpu_t5_probe_gpu_addr(valid: bool, offset: usize) -> u64 {
    if valid {
        GPU_VA_GPGPU_TILE_ARENA_BASE + offset as u64
    } else {
        0
    }
}

fn log_gpgpu_t5_arena_store_probe(
    stage: &'static str,
    probe: GpgpuT5ArenaStoreProbe,
    before: Option<GpgpuT5ArenaStoreProbe>,
    output_gpu: u64,
    cpu_expected_bits: u32,
    profile: GpgpuOneRowMatvecProfile,
) {
    let scan_digest_changed = before
        .map(|before| before.scan.digest != probe.scan.digest)
        .unwrap_or(false);
    let scan_nonzero_changed = before
        .map(|before| before.scan.nonzero_dwords != probe.scan.nonzero_dwords)
        .unwrap_or(false);
    let x_digest_changed = before
        .map(|before| before.x.digest != probe.x.digest)
        .unwrap_or(false);
    let row0_digest_changed = before
        .map(|before| before.row0.digest != probe.row0.digest)
        .unwrap_or(false);
    let output_digest_changed = before
        .map(|before| before.output.digest != probe.output.digest)
        .unwrap_or(false);
    crate::log!(
        "intel/gpgpu: {}-arena-misplaced-store-probe stage={} scan_gpu=0x{:X} scan_bytes=0x{:X} output_off=0x{:X} output_gpu=0x{:X} output_record_bytes=0x{:X} scan_nonzero={} scan_digest=0x{:016X} scan_nonzero_changed={} scan_digest_changed={} x_nonzero={} x_digest=0x{:016X} x_digest_changed={} row0_nonzero={} row0_digest=0x{:016X} row0_digest_changed={} output_nonzero={} output_digest=0x{:016X} output_digest_changed={} cpu_expected_bits=0x{:08X} expected_hits={} expected_misplaced_hits={} expected_hit0_valid={} expected_hit0_off=0x{:X} expected_hit0_gpu=0x{:X} expected_misplaced0_valid={} expected_misplaced0_off=0x{:X} expected_misplaced0_gpu=0x{:X} meta_k=0x{:08X} meta_k_hits={} meta_k_misplaced_hits={} meta_k_hit0_valid={} meta_k_hit0_off=0x{:X} meta_k_hit0_gpu=0x{:X} meta_k_misplaced0_valid={} meta_k_misplaced0_off=0x{:X} meta_k_misplaced0_gpu=0x{:X} sentinel=0x{:08X} sentinel_hits={} sentinel_misplaced_hits={} sentinel_hit0_valid={} sentinel_hit0_off=0x{:X} sentinel_hit0_gpu=0x{:X} sentinel_misplaced0_valid={} sentinel_misplaced0_off=0x{:X} sentinel_misplaced0_gpu=0x{:X} note=scans-one-row-arena-prefix-not-full-dump\n",
        profile.log_prefix,
        stage,
        GPU_VA_GPGPU_TILE_ARENA_BASE + probe.scan_base_offset as u64,
        probe.scan_bytes,
        probe.output_offset,
        output_gpu,
        probe.output_record_bytes,
        probe.scan.nonzero_dwords,
        probe.scan.digest,
        scan_nonzero_changed as u8,
        scan_digest_changed as u8,
        probe.x.nonzero_dwords,
        probe.x.digest,
        x_digest_changed as u8,
        probe.row0.nonzero_dwords,
        probe.row0.digest,
        row0_digest_changed as u8,
        probe.output.nonzero_dwords,
        probe.output.digest,
        output_digest_changed as u8,
        cpu_expected_bits,
        probe.expected.hits,
        probe.expected.misplaced_hits,
        probe.expected.first_valid as u8,
        probe.expected.first_off,
        gpgpu_t5_probe_gpu_addr(probe.expected.first_valid, probe.expected.first_off),
        probe.expected.first_misplaced_valid as u8,
        probe.expected.first_misplaced_off,
        gpgpu_t5_probe_gpu_addr(
            probe.expected.first_misplaced_valid,
            probe.expected.first_misplaced_off,
        ),
        profile.live_k_dim as u32,
        probe.meta_k.hits,
        probe.meta_k.misplaced_hits,
        probe.meta_k.first_valid as u8,
        probe.meta_k.first_off,
        gpgpu_t5_probe_gpu_addr(probe.meta_k.first_valid, probe.meta_k.first_off),
        probe.meta_k.first_misplaced_valid as u8,
        probe.meta_k.first_misplaced_off,
        gpgpu_t5_probe_gpu_addr(
            probe.meta_k.first_misplaced_valid,
            probe.meta_k.first_misplaced_off,
        ),
        profile.expected_sentinel,
        probe.sentinel.hits,
        probe.sentinel.misplaced_hits,
        probe.sentinel.first_valid as u8,
        probe.sentinel.first_off,
        gpgpu_t5_probe_gpu_addr(probe.sentinel.first_valid, probe.sentinel.first_off),
        probe.sentinel.first_misplaced_valid as u8,
        probe.sentinel.first_misplaced_off,
        gpgpu_t5_probe_gpu_addr(
            probe.sentinel.first_misplaced_valid,
            probe.sentinel.first_misplaced_off,
        ),
    );
}

unsafe fn gpgpu_stage_checksum_bytes(ptr: *const u8, len: usize) -> u64 {
    let mut hash = 0xCBF2_9CE4_8422_2325u64;
    for i in 0..len {
        hash ^= unsafe { core::ptr::read_volatile(ptr.add(i)) } as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

unsafe fn gpgpu_stage_nonzero_dwords(ptr: *const u32, count: usize) -> usize {
    let mut nonzero = 0usize;
    for i in 0..count {
        if unsafe { core::ptr::read_volatile(ptr.add(i)) } != 0 {
            nonzero += 1;
        }
    }
    nonzero
}

unsafe fn gpgpu_stage_dword_hits_mask_lo64(ptr: *const u32, count: usize, expected: u32) -> u64 {
    let mut hits = 0u64;
    for i in 0..count.min(64) {
        if unsafe { core::ptr::read_volatile(ptr.add(i)) } == expected {
            hits |= 1u64 << i;
        }
    }
    hits
}
