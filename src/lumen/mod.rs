//! Minimal TRUEOS ownership boundary for the vendored Lumen CPU backend.
//!
//! The previous integration routed Lumen's BF16 matrix-vector callback through
//! the Burn-named AP job system.  Bringing the CPU path back does not require
//! that scheduler: Lumen already supplies its no-std tensor/model machinery and
//! calls this host-owned symbol only for the TRUEOS BF16 fast path.  The scalar
//! implementation below is intentionally boring and deterministic.  It is the
//! correctness fallback that a later SIMD, multicore, or FPGA implementation
//! can replace without changing Lumen's graph-facing API.

pub(crate) fn log_cpu_backend_once() {
    let x = [2.0f32, -1.0];
    let f32_weights = [3.0f32, 4.0, -2.0, 0.5];
    let mut f32_out = [0.0f32; 2];
    ::lumen::ops::matmul::matvec_rowmajor_parallel(&x, &f32_weights, 2, 2, &mut f32_out);

    // The same two rows encoded as little-endian BF16 values. Keeping this
    // probe tiny makes the feature safe to leave enabled during normal boots.
    let bf16_weights = [0x40, 0x40, 0x80, 0x40, 0x00, 0xC0, 0x00, 0x3F];
    let mut bf16_out = [0.0f32; 2];
    let bf16_status = unsafe {
        lumen_trueos_matvec_rowmajor_f32_bf16(
            x.as_ptr(),
            x.len(),
            bf16_weights.as_ptr(),
            bf16_weights.len(),
            2,
            2,
            bf16_out.as_mut_ptr(),
            bf16_out.len(),
        )
    };
    let smoke_ok = f32_out == [2.0, -4.5] && bf16_status == 0 && bf16_out == f32_out;

    crate::log_info!(
        target: "boot";
        "lumen: backend={} bridge=bf16-rowmajor-scalar smoke={} model_io=host-owned\n",
        ::lumen::backend::default_backend_name(),
        if smoke_ok { "pass" } else { "fail" },
    );
}

/// Execute Lumen's host-owned BF16 row-major matrix-vector callback.
///
/// The ABI and validation behavior intentionally match the historical Lumen
/// callback. BF16 values arrive as little-endian bytes because that is the
/// representation used by the existing TRUEOS model path.
///
/// # Safety
///
/// Every non-null pointer must be valid for the corresponding supplied length.
/// `out` must be writable and must not alias either input for the duration of
/// the call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lumen_trueos_matvec_rowmajor_f32_bf16(
    x: *const f32,
    x_len: usize,
    w_rowmajor_bf16: *const u8,
    w_len: usize,
    n_rows: usize,
    k_dim: usize,
    out: *mut f32,
    out_len: usize,
) -> i32 {
    if x.is_null() || w_rowmajor_bf16.is_null() || out.is_null() {
        return -1;
    }

    let Some(expected_w_len) = n_rows
        .checked_mul(k_dim)
        .and_then(|elements| elements.checked_mul(2))
    else {
        return -1;
    };
    if x_len < k_dim || w_len < expected_w_len || out_len < n_rows {
        return -1;
    }

    let x = unsafe { core::slice::from_raw_parts(x, k_dim) };
    let weights = unsafe { core::slice::from_raw_parts(w_rowmajor_bf16, expected_w_len) };
    let out = unsafe { core::slice::from_raw_parts_mut(out, n_rows) };

    for (row, result) in out.iter_mut().enumerate() {
        let row_start = row * k_dim * 2;
        let row_weights = &weights[row_start..row_start + k_dim * 2];
        let mut sum = 0.0f32;
        for (value, encoded) in x.iter().zip(row_weights.chunks_exact(2)) {
            let bits = u16::from_le_bytes([encoded[0], encoded[1]]);
            let weight = f32::from_bits((bits as u32) << 16);
            sum += value * weight;
        }
        *result = sum;
    }

    0
}
