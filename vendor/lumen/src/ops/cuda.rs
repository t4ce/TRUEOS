use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

const CUDA_MATMUL_MIN_WORK: usize = 1 << 18;
const CUDA_BATCH_MATMUL_MIN_WORK: usize = 1 << 18;
const CUDA_ELEMENTWISE_MIN_WORK: usize = 1 << 14;
const CUDA_SOFTMAX_MIN_WORK: usize = 1 << 13;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    Relu = 0,
    Sigmoid = 1,
    Tanh = 2,
    Silu = 3,
    Gelu = 4,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Add = 0,
    Sub = 1,
    Mul = 2,
}

struct CudaBufferInner {
    handle: u64,
    len: usize,
}

#[derive(Clone)]
pub struct CudaBuffer(Arc<CudaBufferInner>);

impl CudaBuffer {
    #[cfg(feature = "cuda")]
    pub(crate) fn from_raw(handle: u64, len: usize) -> Self {
        Self(Arc::new(CudaBufferInner { handle, len }))
    }

    pub fn handle(&self) -> u64 {
        self.0.handle
    }

    pub fn len(&self) -> usize {
        self.0.len
    }
}

impl Drop for CudaBufferInner {
    fn drop(&mut self) {
        if self.handle != 0 {
            imp::free_f32(self.handle, self.len);
        }
    }
}

fn env_enabled() -> bool {
    std::env::var("LUMEN_CUDA")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

fn cuda_toggle() -> &'static AtomicBool {
    static CUDA_ENABLED: OnceLock<AtomicBool> = OnceLock::new();
    CUDA_ENABLED.get_or_init(|| AtomicBool::new(env_enabled()))
}

pub fn set_enabled(enabled: bool) {
    cuda_toggle().store(enabled, Ordering::Relaxed);
}

pub fn is_enabled() -> bool {
    cuda_toggle().load(Ordering::Relaxed) && is_available()
}

pub struct CudaEnabledGuard {
    previous: bool,
}

pub fn set_enabled_scoped(enabled: bool) -> CudaEnabledGuard {
    let previous = cuda_toggle().load(Ordering::Relaxed);
    set_enabled(enabled);
    CudaEnabledGuard { previous }
}

impl Drop for CudaEnabledGuard {
    fn drop(&mut self) {
        set_enabled(self.previous);
    }
}

pub fn should_accelerate_matmul(m: usize, n: usize, k: usize) -> bool {
    is_enabled()
        && m.checked_mul(n)
            .and_then(|value| value.checked_mul(k))
            .is_some_and(|work| work >= CUDA_MATMUL_MIN_WORK)
}

pub fn should_accelerate_batch_matmul(batch_count: usize, m: usize, n: usize, k: usize) -> bool {
    is_enabled()
        && batch_count
            .checked_mul(m)
            .and_then(|value| value.checked_mul(n))
            .and_then(|value| value.checked_mul(k))
            .is_some_and(|work| work >= CUDA_BATCH_MATMUL_MIN_WORK)
}

pub fn should_accelerate_elementwise(len: usize) -> bool {
    is_enabled() && len >= CUDA_ELEMENTWISE_MIN_WORK
}

pub fn should_accelerate_softmax(outer: usize, last_dim: usize) -> bool {
    is_enabled()
        && outer
            .checked_mul(last_dim)
            .is_some_and(|work| work >= CUDA_SOFTMAX_MIN_WORK)
}

#[cfg(feature = "cuda")]
mod imp {
    use super::{BinaryOp, CudaBuffer, UnaryOp};
    use std::ffi::CStr;
    use std::os::raw::{c_char, c_int};

    unsafe extern "C" {
        fn lumen_cuda_is_available() -> c_int;
        fn lumen_cuda_alloc_f32(len: usize, out_handle: *mut u64) -> c_int;
        fn lumen_cuda_upload_f32(handle: u64, src: *const f32, len: usize) -> c_int;
        fn lumen_cuda_upload_f32_offset(
            handle: u64,
            src: *const f32,
            offset: usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_copy_f32_offset(
            dst_handle: u64,
            dst_offset: usize,
            src_handle: u64,
            src_offset: usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_append_kv_cache_f32_device(
            dst_handle: u64,
            src_handle: u64,
            batch_size: usize,
            num_heads: usize,
            src_seq_len: usize,
            dst_seq_len: usize,
            dim: usize,
            dst_start: usize,
        ) -> c_int;
        fn lumen_cuda_append_kv_cache_pair_f32_device(
            k_dst_handle: u64,
            v_dst_handle: u64,
            k_src_handle: u64,
            v_src_handle: u64,
            batch_size: usize,
            num_heads: usize,
            src_seq_len: usize,
            dst_seq_len: usize,
            dim: usize,
            dst_start: usize,
        ) -> c_int;
        fn lumen_cuda_decode_rope_q_append_kv_f32_device(
            q_src_handle: u64,
            k_src_handle: u64,
            v_src_handle: u64,
            cos_handle: u64,
            sin_handle: u64,
            q_out_handle: u64,
            k_cache_handle: u64,
            v_cache_handle: u64,
            batch_size: usize,
            num_heads: usize,
            num_kv_heads: usize,
            dim: usize,
            dst_seq_len: usize,
            offset: usize,
            cache_seq_len: usize,
        ) -> c_int;
        fn lumen_cuda_kv_cache_prefix_f32_device(
            src_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_heads: usize,
            active_seq_len: usize,
            src_seq_len: usize,
            dim: usize,
        ) -> c_int;
        fn lumen_cuda_download_f32(handle: u64, dst: *mut f32, len: usize) -> c_int;
        fn lumen_cuda_download_f32_offset(
            handle: u64,
            dst: *mut f32,
            offset: usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_free_f32(handle: u64, len: usize);
        fn lumen_cuda_synchronize() -> c_int;
        fn lumen_cuda_matvec_argmax_f32_device(
            input_handle: u64,
            weight_handle: u64,
            out_indices: *mut usize,
            batch_size: usize,
            vocab_size: usize,
            hidden_size: usize,
        ) -> c_int;
        fn lumen_cuda_matmul_f32_device(
            a_handle: u64,
            b_handle: u64,
            out_handle: u64,
            m: usize,
            n: usize,
            k: usize,
        ) -> c_int;
        fn lumen_cuda_batch_matmul_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            out_handle: u64,
            batch_count: usize,
            m: usize,
            n: usize,
            k: usize,
        ) -> c_int;
        fn lumen_cuda_unary_f32_device(
            input_handle: u64,
            out_handle: u64,
            len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_unary_backward_f32_device(
            input_handle: u64,
            output_handle: u64,
            grad_handle: u64,
            out_handle: u64,
            len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_binary_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            out_handle: u64,
            len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_binary_backward_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            grad_handle: u64,
            grad_lhs_handle: u64,
            grad_rhs_handle: u64,
            len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_binary_broadcast_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            out_handle: u64,
            ndim: usize,
            out_shape: *const usize,
            out_strides: *const usize,
            lhs_shape: *const usize,
            lhs_strides: *const usize,
            rhs_shape: *const usize,
            rhs_strides: *const usize,
            len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_binary_broadcast_backward_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            grad_handle: u64,
            grad_lhs_handle: u64,
            grad_rhs_handle: u64,
            ndim: usize,
            out_shape: *const usize,
            out_strides: *const usize,
            lhs_shape: *const usize,
            lhs_strides: *const usize,
            rhs_shape: *const usize,
            rhs_strides: *const usize,
            out_len: usize,
            lhs_len: usize,
            rhs_len: usize,
            op: c_int,
        ) -> c_int;
        fn lumen_cuda_sum_f32_device(input_handle: u64, out_handle: u64, len: usize) -> c_int;
        fn lumen_cuda_fill_scalar_f32_device(out_handle: u64, len: usize, value: f32) -> c_int;
        fn lumen_cuda_mse_backward_f32_device(
            diff_handle: u64,
            grad_output_handle: u64,
            grad_target_handle: u64,
            len: usize,
            factor: f32,
        ) -> c_int;
        fn lumen_cuda_cross_entropy_backward_f32_device(
            softmax_handle: u64,
            target_handle: u64,
            out_handle: u64,
            len: usize,
            factor: f32,
        ) -> c_int;
        fn lumen_cuda_cross_entropy_loss_f32_device(
            softmax_handle: u64,
            target_handle: u64,
            out_handle: u64,
            len: usize,
            factor: f32,
        ) -> c_int;
        fn lumen_cuda_sgd_update_f32_device(
            param_handle: u64,
            grad_handle: u64,
            len: usize,
            lr: f32,
        ) -> c_int;
        fn lumen_cuda_sgd_momentum_update_f32_device(
            param_handle: u64,
            grad_handle: u64,
            velocity_handle: u64,
            len: usize,
            lr: f32,
            momentum: f32,
        ) -> c_int;
        fn lumen_cuda_adam_update_f32_device(
            param_handle: u64,
            grad_handle: u64,
            exp_avg_handle: u64,
            exp_avg_sq_handle: u64,
            len: usize,
            lr: f32,
            beta1: f32,
            beta2: f32,
            bias_correction1: f32,
            bias_correction2: f32,
            eps: f32,
        ) -> c_int;
        fn lumen_cuda_softmax_lastdim_f32_device(
            input_handle: u64,
            out_handle: u64,
            outer: usize,
            last_dim: usize,
        ) -> c_int;
        fn lumen_cuda_softmax_lastdim_backward_f32_device(
            output_handle: u64,
            grad_handle: u64,
            out_handle: u64,
            outer: usize,
            last_dim: usize,
        ) -> c_int;
        fn lumen_cuda_fused_softmax_f32_device(
            input_handle: u64,
            out_handle: u64,
            batch_heads: usize,
            q_len: usize,
            k_len: usize,
            scale: f32,
            is_causal: c_int,
        ) -> c_int;
        fn lumen_cuda_fused_softmax_backward_f32_device(
            output_handle: u64,
            grad_handle: u64,
            out_handle: u64,
            batch_heads: usize,
            q_len: usize,
            k_len: usize,
            scale: f32,
        ) -> c_int;
        fn lumen_cuda_fused_softmax_f32_with_past_device(
            input_handle: u64,
            out_handle: u64,
            batch_heads: usize,
            q_len: usize,
            k_len: usize,
            scale: f32,
            is_causal: c_int,
            past_len: usize,
        ) -> c_int;
        fn lumen_cuda_embedding_f32_device(
            indices_handle: u64,
            weight_handle: u64,
            out_handle: u64,
            num_indices: usize,
            vocab_size: usize,
            embed_dim: usize,
        ) -> c_int;
        fn lumen_cuda_embedding_backward_f32_device(
            indices_handle: u64,
            grad_handle: u64,
            grad_weight_handle: u64,
            num_indices: usize,
            vocab_size: usize,
            embed_dim: usize,
        ) -> c_int;
        fn lumen_cuda_rms_norm_f32_device(
            input_handle: u64,
            weight_handle: u64,
            out_handle: u64,
            rows: usize,
            dim: usize,
            eps: f32,
        ) -> c_int;
        fn lumen_cuda_rms_norm_backward_f32_device(
            input_handle: u64,
            weight_handle: u64,
            grad_handle: u64,
            grad_input_handle: u64,
            grad_weight_handle: u64,
            rows: usize,
            dim: usize,
            eps: f32,
        ) -> c_int;
        fn lumen_cuda_permute_f32_device(
            input_handle: u64,
            out_handle: u64,
            ndim: usize,
            out_shape: *const usize,
            out_strides: *const usize,
            mapped_input_strides: *const usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_slice_lastdim_f32_device(
            input_handle: u64,
            out_handle: u64,
            outer: usize,
            input_last_dim: usize,
            start: usize,
            slice_len: usize,
        ) -> c_int;
        fn lumen_cuda_slice_lastdim_backward_f32_device(
            grad_handle: u64,
            out_handle: u64,
            outer: usize,
            input_last_dim: usize,
            start: usize,
            slice_len: usize,
        ) -> c_int;
        fn lumen_cuda_cat_f32_device(
            lhs_handle: u64,
            rhs_handle: u64,
            out_handle: u64,
            ndim: usize,
            out_shape: *const usize,
            out_strides: *const usize,
            lhs_strides: *const usize,
            rhs_strides: *const usize,
            axis: usize,
            lhs_axis_len: usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_cat_backward_slice_f32_device(
            grad_handle: u64,
            out_handle: u64,
            ndim: usize,
            input_shape: *const usize,
            input_strides: *const usize,
            out_strides: *const usize,
            axis: usize,
            axis_start: usize,
            len: usize,
        ) -> c_int;
        fn lumen_cuda_repeat_kv_f32_device(
            input_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_kv_heads: usize,
            seq_len: usize,
            dim: usize,
            n_rep: usize,
        ) -> c_int;
        fn lumen_cuda_repeat_kv_backward_f32_device(
            grad_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_kv_heads: usize,
            seq_len: usize,
            dim: usize,
            n_rep: usize,
        ) -> c_int;
        fn lumen_cuda_decode_attention_f32_device(
            q_handle: u64,
            k_handle: u64,
            v_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_heads: usize,
            num_kv_heads: usize,
            active_seq_len: usize,
            cache_seq_len: usize,
            dim: usize,
            n_rep: usize,
            scale: f32,
        ) -> c_int;
        fn lumen_cuda_prefill_attention_f32_device(
            q_handle: u64,
            k_handle: u64,
            v_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_heads: usize,
            num_kv_heads: usize,
            q_seq_len: usize,
            active_seq_len: usize,
            cache_seq_len: usize,
            dim: usize,
            n_rep: usize,
            past_len: usize,
            scale: f32,
            is_causal: c_int,
        ) -> c_int;
        fn lumen_cuda_fused_gate_up_silu_f32_device(
            input_handle: u64,
            gate_handle: u64,
            up_handle: u64,
            out_handle: u64,
            rows: usize,
            n_dim: usize,
            k_dim: usize,
        ) -> c_int;
        fn lumen_cuda_fused_qkv_f32_device(
            input_handle: u64,
            q_handle: u64,
            k_handle: u64,
            v_handle: u64,
            q_out_handle: u64,
            k_out_handle: u64,
            v_out_handle: u64,
            rows: usize,
            q_n: usize,
            k_n: usize,
            k_dim: usize,
        ) -> c_int;
        fn lumen_cuda_rope_f32_device(
            input_handle: u64,
            cos_handle: u64,
            sin_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_heads: usize,
            seq_len: usize,
            dim: usize,
            offset: usize,
            cache_seq_len: usize,
        ) -> c_int;
        fn lumen_cuda_rope_backward_f32_device(
            grad_handle: u64,
            cos_handle: u64,
            sin_handle: u64,
            out_handle: u64,
            batch_size: usize,
            num_heads: usize,
            seq_len: usize,
            dim: usize,
            offset: usize,
            cache_seq_len: usize,
        ) -> c_int;
        fn lumen_cuda_conv2d_f32_device(
            input_handle: u64,
            weight_handle: u64,
            bias_handle: u64,
            out_handle: u64,
            batch_size: usize,
            in_channels: usize,
            in_h: usize,
            in_w: usize,
            out_channels: usize,
            k_h: usize,
            k_w: usize,
            pad_h: usize,
            pad_w: usize,
            stride_h: usize,
            stride_w: usize,
            out_h: usize,
            out_w: usize,
        ) -> c_int;
        fn lumen_cuda_conv2d_backward_f32_device(
            input_handle: u64,
            weight_handle: u64,
            grad_output_handle: u64,
            grad_input_handle: u64,
            grad_weight_handle: u64,
            grad_bias_handle: u64,
            batch_size: usize,
            in_channels: usize,
            in_h: usize,
            in_w: usize,
            out_channels: usize,
            k_h: usize,
            k_w: usize,
            pad_h: usize,
            pad_w: usize,
            stride_h: usize,
            stride_w: usize,
            out_h: usize,
            out_w: usize,
        ) -> c_int;
        fn lumen_cuda_max_pool2d_f32_device(
            input_handle: u64,
            out_handle: u64,
            batch_size: usize,
            channels: usize,
            in_h: usize,
            in_w: usize,
            kernel_h: usize,
            kernel_w: usize,
            stride_h: usize,
            stride_w: usize,
            out_h: usize,
            out_w: usize,
        ) -> c_int;
        fn lumen_cuda_max_pool2d_backward_f32_device(
            input_handle: u64,
            grad_output_handle: u64,
            grad_input_handle: u64,
            batch_size: usize,
            channels: usize,
            in_h: usize,
            in_w: usize,
            kernel_h: usize,
            kernel_w: usize,
            stride_h: usize,
            stride_w: usize,
            out_h: usize,
            out_w: usize,
        ) -> c_int;
        fn lumen_cuda_last_error_message() -> *const c_char;
    }

    fn last_error_message() -> String {
        unsafe {
            let ptr = lumen_cuda_last_error_message();
            if ptr.is_null() {
                return "unknown CUDA error".to_string();
            }
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }

    fn row_major_strides(shape: &[usize], context: &str) -> Result<Vec<usize>, String> {
        let mut strides = vec![0usize; shape.len()];
        let mut stride = 1usize;
        for i in (0..shape.len()).rev() {
            strides[i] = stride;
            stride = stride
                .checked_mul(shape[i])
                .ok_or_else(|| format!("CUDA {context} stride overflow"))?;
        }
        Ok(strides)
    }

    fn aligned_broadcast_metadata(
        lhs_shape: &[usize],
        rhs_shape: &[usize],
        out_shape: &[usize],
    ) -> Result<(Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>, Vec<usize>), String> {
        let ndim = out_shape.len();
        if ndim == 0 {
            return Err("CUDA broadcast expects at least 1 dimension".to_string());
        }
        if lhs_shape.len() > ndim || rhs_shape.len() > ndim {
            return Err(format!(
                "CUDA broadcast rank mismatch: lhs={:?}, rhs={:?}, out={:?}",
                lhs_shape, rhs_shape, out_shape
            ));
        }

        let mut lhs_aligned = vec![1usize; ndim];
        let mut rhs_aligned = vec![1usize; ndim];
        let lhs_offset = ndim - lhs_shape.len();
        let rhs_offset = ndim - rhs_shape.len();
        lhs_aligned[lhs_offset..].copy_from_slice(lhs_shape);
        rhs_aligned[rhs_offset..].copy_from_slice(rhs_shape);

        for i in 0..ndim {
            let lhs_dim = lhs_aligned[i];
            let rhs_dim = rhs_aligned[i];
            let out_dim = out_shape[i];
            let expected = lhs_dim.max(rhs_dim);
            if (lhs_dim != out_dim && lhs_dim != 1)
                || (rhs_dim != out_dim && rhs_dim != 1)
                || expected != out_dim
            {
                return Err(format!(
                    "CUDA broadcast shape mismatch: lhs={:?}, rhs={:?}, out={:?}",
                    lhs_shape, rhs_shape, out_shape
                ));
            }
        }

        let out_strides = row_major_strides(out_shape, "broadcast output")?;
        let lhs_raw_strides = row_major_strides(&lhs_aligned, "broadcast lhs")?;
        let rhs_raw_strides = row_major_strides(&rhs_aligned, "broadcast rhs")?;
        let lhs_strides = lhs_aligned
            .iter()
            .zip(lhs_raw_strides.iter())
            .map(|(&dim, &stride)| if dim == 1 { 0 } else { stride })
            .collect::<Vec<_>>();
        let rhs_strides = rhs_aligned
            .iter()
            .zip(rhs_raw_strides.iter())
            .map(|(&dim, &stride)| if dim == 1 { 0 } else { stride })
            .collect::<Vec<_>>();
        Ok((
            lhs_aligned,
            rhs_aligned,
            out_strides,
            lhs_strides,
            rhs_strides,
        ))
    }

    pub fn is_available() -> bool {
        unsafe { lumen_cuda_is_available() == 1 }
    }

    pub fn synchronize() -> Result<(), String> {
        let status = unsafe { lumen_cuda_synchronize() };
        if status == 0 {
            Ok(())
        } else {
            Err(last_error_message())
        }
    }

    pub fn alloc_f32(len: usize) -> Result<CudaBuffer, String> {
        let mut handle = 0u64;
        let status = unsafe { lumen_cuda_alloc_f32(len, &mut handle as *mut u64) };
        if status == 0 {
            Ok(CudaBuffer::from_raw(handle, len))
        } else {
            Err(last_error_message())
        }
    }

    pub fn upload_f32(src: &[f32]) -> Result<CudaBuffer, String> {
        let buffer = alloc_f32(src.len())?;
        let status = unsafe { lumen_cuda_upload_f32(buffer.handle(), src.as_ptr(), src.len()) };
        if status == 0 {
            Ok(buffer)
        } else {
            Err(last_error_message())
        }
    }

    pub fn download_f32(buffer: &CudaBuffer) -> Result<Vec<f32>, String> {
        let mut out = vec![0.0f32; buffer.len()];
        let status =
            unsafe { lumen_cuda_download_f32(buffer.handle(), out.as_mut_ptr(), buffer.len()) };
        if status == 0 {
            Ok(out)
        } else {
            Err(last_error_message())
        }
    }

    pub fn download_f32_offset(
        buffer: &CudaBuffer,
        offset: usize,
        len: usize,
    ) -> Result<Vec<f32>, String> {
        if offset > buffer.len() || len > buffer.len().saturating_sub(offset) {
            return Err(format!(
                "CUDA download offset out of bounds: offset={}, len={}, buffer_len={}",
                offset,
                len,
                buffer.len()
            ));
        }
        let mut out = vec![0.0f32; len];
        let status = unsafe {
            lumen_cuda_download_f32_offset(buffer.handle(), out.as_mut_ptr(), offset, len)
        };
        if status == 0 {
            Ok(out)
        } else {
            Err(last_error_message())
        }
    }

    pub fn matvec_argmax_f32(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        batch_size: usize,
        vocab_size: usize,
        hidden_size: usize,
    ) -> Result<Vec<usize>, String> {
        if batch_size == 0 || vocab_size == 0 || hidden_size == 0 {
            return Err("CUDA matvec argmax dimensions must be greater than zero".to_string());
        }
        let input_len = batch_size
            .checked_mul(hidden_size)
            .ok_or_else(|| "CUDA matvec argmax input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA matvec argmax input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        let weight_len = vocab_size
            .checked_mul(hidden_size)
            .ok_or_else(|| "CUDA matvec argmax weight length overflow".to_string())?;
        if weight.len() != weight_len {
            return Err(format!(
                "CUDA matvec argmax weight length mismatch: expected {}, got {}",
                weight_len,
                weight.len()
            ));
        }

        let mut out = vec![0usize; batch_size];
        let status = unsafe {
            lumen_cuda_matvec_argmax_f32_device(
                input.handle(),
                weight.handle(),
                out.as_mut_ptr(),
                batch_size,
                vocab_size,
                hidden_size,
            )
        };
        if status == 0 {
            Ok(out)
        } else {
            Err(last_error_message())
        }
    }

    pub fn upload_f32_offset(
        buffer: &CudaBuffer,
        offset: usize,
        src: &[f32],
    ) -> Result<(), String> {
        if offset > buffer.len() || src.len() > buffer.len() - offset {
            return Err(format!(
                "CUDA upload offset out of bounds: offset={}, len={}, buffer_len={}",
                offset,
                src.len(),
                buffer.len()
            ));
        }
        let status = unsafe {
            lumen_cuda_upload_f32_offset(buffer.handle(), src.as_ptr(), offset, src.len())
        };
        if status == 0 {
            Ok(())
        } else {
            Err(last_error_message())
        }
    }

    pub fn copy_f32_offset(
        dst: &CudaBuffer,
        dst_offset: usize,
        src: &CudaBuffer,
        src_offset: usize,
        len: usize,
    ) -> Result<(), String> {
        if dst_offset > dst.len() || len > dst.len().saturating_sub(dst_offset) {
            return Err(format!(
                "CUDA copy dst out of bounds: dst_offset={}, len={}, dst_len={}",
                dst_offset,
                len,
                dst.len()
            ));
        }
        if src_offset > src.len() || len > src.len().saturating_sub(src_offset) {
            return Err(format!(
                "CUDA copy src out of bounds: src_offset={}, len={}, src_len={}",
                src_offset,
                len,
                src.len()
            ));
        }
        let status = unsafe {
            lumen_cuda_copy_f32_offset(dst.handle(), dst_offset, src.handle(), src_offset, len)
        };
        if status == 0 {
            Ok(())
        } else {
            Err(last_error_message())
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_kv_cache_f32(
        dst: &CudaBuffer,
        src: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        src_seq_len: usize,
        dst_seq_len: usize,
        dim: usize,
        dst_start: usize,
    ) -> Result<(), String> {
        let src_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(src_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache source length overflow".to_string())?;
        if src.len() != src_len {
            return Err(format!(
                "CUDA KV cache source length mismatch: expected {}, got {}",
                src_len,
                src.len()
            ));
        }

        let dst_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(dst_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache destination length overflow".to_string())?;
        if dst.len() != dst_len {
            return Err(format!(
                "CUDA KV cache destination length mismatch: expected {}, got {}",
                dst_len,
                dst.len()
            ));
        }
        if dst_start > dst_seq_len || src_seq_len > dst_seq_len.saturating_sub(dst_start) {
            return Err(format!(
                "CUDA KV cache append range out of bounds: start={}, src_seq_len={}, dst_seq_len={}",
                dst_start, src_seq_len, dst_seq_len
            ));
        }

        let status = unsafe {
            lumen_cuda_append_kv_cache_f32_device(
                dst.handle(),
                src.handle(),
                batch_size,
                num_heads,
                src_seq_len,
                dst_seq_len,
                dim,
                dst_start,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(last_error_message())
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_kv_cache_pair_f32(
        k_dst: &CudaBuffer,
        v_dst: &CudaBuffer,
        k_src: &CudaBuffer,
        v_src: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        src_seq_len: usize,
        dst_seq_len: usize,
        dim: usize,
        dst_start: usize,
    ) -> Result<(), String> {
        let src_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(src_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache pair source length overflow".to_string())?;
        if k_src.len() != src_len || v_src.len() != src_len {
            return Err(format!(
                "CUDA KV cache pair source length mismatch: expected {}, got k={}, v={}",
                src_len,
                k_src.len(),
                v_src.len()
            ));
        }

        let dst_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(dst_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache pair destination length overflow".to_string())?;
        if k_dst.len() != dst_len || v_dst.len() != dst_len {
            return Err(format!(
                "CUDA KV cache pair destination length mismatch: expected {}, got k={}, v={}",
                dst_len,
                k_dst.len(),
                v_dst.len()
            ));
        }
        if dst_start > dst_seq_len || src_seq_len > dst_seq_len.saturating_sub(dst_start) {
            return Err(format!(
                "CUDA KV cache pair append range out of bounds: start={}, src_seq_len={}, dst_seq_len={}",
                dst_start, src_seq_len, dst_seq_len
            ));
        }

        let status = unsafe {
            lumen_cuda_append_kv_cache_pair_f32_device(
                k_dst.handle(),
                v_dst.handle(),
                k_src.handle(),
                v_src.handle(),
                batch_size,
                num_heads,
                src_seq_len,
                dst_seq_len,
                dim,
                dst_start,
            )
        };
        if status == 0 {
            Ok(())
        } else {
            Err(last_error_message())
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_rope_q_append_kv_f32_buffer(
        q_src: &CudaBuffer,
        k_src: &CudaBuffer,
        v_src: &CudaBuffer,
        cos: &CudaBuffer,
        sin: &CudaBuffer,
        k_cache: &CudaBuffer,
        v_cache: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        num_kv_heads: usize,
        dim: usize,
        dst_seq_len: usize,
        offset: usize,
        cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        let q_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA decode RoPE Q length overflow".to_string())?;
        let kv_step_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA decode RoPE KV step length overflow".to_string())?;
        let cache_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|v| v.checked_mul(dst_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA decode RoPE KV cache length overflow".to_string())?;
        let rope_cache_len = cache_seq_len
            .checked_mul(dim)
            .ok_or_else(|| "CUDA decode RoPE cache length overflow".to_string())?;
        if q_src.len() != q_len {
            return Err(format!(
                "CUDA decode RoPE Q length mismatch: expected {}, got {}",
                q_len,
                q_src.len()
            ));
        }
        if k_src.len() != kv_step_len || v_src.len() != kv_step_len {
            return Err(format!(
                "CUDA decode RoPE KV step length mismatch: expected {}, got k={}, v={}",
                kv_step_len,
                k_src.len(),
                v_src.len()
            ));
        }
        if k_cache.len() != cache_len || v_cache.len() != cache_len {
            return Err(format!(
                "CUDA decode RoPE KV cache length mismatch: expected {}, got k={}, v={}",
                cache_len,
                k_cache.len(),
                v_cache.len()
            ));
        }
        if cos.len() != rope_cache_len || sin.len() != rope_cache_len {
            return Err(format!(
                "CUDA decode RoPE cache length mismatch: expected {}, got cos={}, sin={}",
                rope_cache_len,
                cos.len(),
                sin.len()
            ));
        }
        if batch_size == 0 || num_heads == 0 || num_kv_heads == 0 || dim == 0 || dst_seq_len == 0 {
            return Err("CUDA decode RoPE dimensions must be greater than zero".to_string());
        }
        if dim % 2 != 0 {
            return Err(format!(
                "CUDA decode RoPE expects a positive even dimension, got {}",
                dim
            ));
        }
        if offset >= dst_seq_len || offset >= cache_seq_len {
            return Err(format!(
                "CUDA decode RoPE offset out of bounds: offset={}, dst_seq_len={}, cache_seq_len={}",
                offset, dst_seq_len, cache_seq_len
            ));
        }

        let q_out = alloc_f32(q_len)?;
        let status = unsafe {
            lumen_cuda_decode_rope_q_append_kv_f32_device(
                q_src.handle(),
                k_src.handle(),
                v_src.handle(),
                cos.handle(),
                sin.handle(),
                q_out.handle(),
                k_cache.handle(),
                v_cache.handle(),
                batch_size,
                num_heads,
                num_kv_heads,
                dim,
                dst_seq_len,
                offset,
                cache_seq_len,
            )
        };
        if status == 0 {
            Ok(q_out)
        } else {
            Err(last_error_message())
        }
    }

    pub fn kv_cache_prefix_f32_buffer(
        src: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        active_seq_len: usize,
        src_seq_len: usize,
        dim: usize,
    ) -> Result<CudaBuffer, String> {
        let src_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(src_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache source length overflow".to_string())?;
        if src.len() != src_len {
            return Err(format!(
                "CUDA KV cache source length mismatch: expected {}, got {}",
                src_len,
                src.len()
            ));
        }
        if active_seq_len == 0 || active_seq_len > src_seq_len {
            return Err(format!(
                "CUDA KV cache prefix range out of bounds: active_seq_len={}, src_seq_len={}",
                active_seq_len, src_seq_len
            ));
        }
        let out_len = batch_size
            .checked_mul(num_heads)
            .and_then(|v| v.checked_mul(active_seq_len))
            .and_then(|v| v.checked_mul(dim))
            .ok_or_else(|| "CUDA KV cache prefix output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_kv_cache_prefix_f32_device(
                src.handle(),
                out.handle(),
                batch_size,
                num_heads,
                active_seq_len,
                src_seq_len,
                dim,
            )
        };
        if status == 0 {
            Ok(out)
        } else {
            Err(last_error_message())
        }
    }

    pub fn free_f32(handle: u64, len: usize) {
        unsafe { lumen_cuda_free_f32(handle, len) };
    }

    pub fn matmul_f32(
        a: &CudaBuffer,
        b: &CudaBuffer,
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = matmul_f32_no_host(a, b, m, n, k)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn matmul_f32_no_host(
        a: &CudaBuffer,
        b: &CudaBuffer,
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<CudaBuffer, String> {
        if a.len() != m * k {
            return Err(format!(
                "CUDA matmul A length mismatch: expected {}, got {}",
                m * k,
                a.len()
            ));
        }
        if b.len() != n * k {
            return Err(format!(
                "CUDA matmul B length mismatch: expected {}, got {}",
                n * k,
                b.len()
            ));
        }
        let out = alloc_f32(m * n)?;
        let status =
            unsafe { lumen_cuda_matmul_f32_device(a.handle(), b.handle(), out.handle(), m, n, k) };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn batch_matmul_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        batch_count: usize,
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = batch_matmul_f32_no_host(lhs, rhs, batch_count, m, n, k)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn batch_matmul_f32_no_host(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        batch_count: usize,
        m: usize,
        n: usize,
        k: usize,
    ) -> Result<CudaBuffer, String> {
        if lhs.len() != batch_count * m * k {
            return Err(format!(
                "CUDA batch_matmul lhs length mismatch: expected {}, got {}",
                batch_count * m * k,
                lhs.len()
            ));
        }
        if rhs.len() != batch_count * k * n {
            return Err(format!(
                "CUDA batch_matmul rhs length mismatch: expected {}, got {}",
                batch_count * k * n,
                rhs.len()
            ));
        }
        let out = alloc_f32(batch_count * m * n)?;
        let status = unsafe {
            lumen_cuda_batch_matmul_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                batch_count,
                m,
                n,
                k,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn unary_f32(input: &CudaBuffer, op: UnaryOp) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = unary_f32_buffer(input, op)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn unary_f32_buffer(input: &CudaBuffer, op: UnaryOp) -> Result<CudaBuffer, String> {
        let out = alloc_f32(input.len())?;
        let status = unsafe {
            lumen_cuda_unary_f32_device(input.handle(), out.handle(), input.len(), op as c_int)
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn unary_backward_f32(
        input: &CudaBuffer,
        output: &CudaBuffer,
        grad: &CudaBuffer,
        op: UnaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = unary_backward_f32_buffer(input, output, grad, op)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn unary_backward_f32_buffer(
        input: &CudaBuffer,
        output: &CudaBuffer,
        grad: &CudaBuffer,
        op: UnaryOp,
    ) -> Result<CudaBuffer, String> {
        if input.len() != output.len() || input.len() != grad.len() {
            return Err(format!(
                "CUDA unary backward length mismatch: input={}, output={}, grad={}",
                input.len(),
                output.len(),
                grad.len()
            ));
        }
        let out = alloc_f32(input.len())?;
        let status = unsafe {
            lumen_cuda_unary_backward_f32_device(
                input.handle(),
                output.handle(),
                grad.handle(),
                out.handle(),
                input.len(),
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn binary_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        op: BinaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        if lhs.len() != rhs.len() {
            return Err(format!(
                "CUDA binary op length mismatch: lhs={}, rhs={}",
                lhs.len(),
                rhs.len()
            ));
        }
        let out = alloc_f32(lhs.len())?;
        let status = unsafe {
            lumen_cuda_binary_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                lhs.len(),
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn binary_f32_buffer(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        op: BinaryOp,
    ) -> Result<CudaBuffer, String> {
        if lhs.len() != rhs.len() {
            return Err(format!(
                "CUDA binary op length mismatch: lhs={}, rhs={}",
                lhs.len(),
                rhs.len()
            ));
        }
        let out = alloc_f32(lhs.len())?;
        let status = unsafe {
            lumen_cuda_binary_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                lhs.len(),
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn binary_backward_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        grad: &CudaBuffer,
        op: BinaryOp,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        let (grad_lhs, grad_rhs) = binary_backward_f32_buffers(lhs, rhs, grad, op)?;
        let grad_lhs_host = download_f32(&grad_lhs)?;
        let grad_rhs_host = download_f32(&grad_rhs)?;
        Ok(((grad_lhs, grad_lhs_host), (grad_rhs, grad_rhs_host)))
    }

    pub fn binary_backward_f32_buffers(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        grad: &CudaBuffer,
        op: BinaryOp,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        if lhs.len() != rhs.len() || lhs.len() != grad.len() {
            return Err(format!(
                "CUDA binary backward length mismatch: lhs={}, rhs={}, grad={}",
                lhs.len(),
                rhs.len(),
                grad.len()
            ));
        }
        let grad_lhs = alloc_f32(lhs.len())?;
        let grad_rhs = alloc_f32(lhs.len())?;
        let status = unsafe {
            lumen_cuda_binary_backward_f32_device(
                lhs.handle(),
                rhs.handle(),
                grad.handle(),
                grad_lhs.handle(),
                grad_rhs.handle(),
                lhs.len(),
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((grad_lhs, grad_rhs))
    }

    pub fn binary_broadcast_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        lhs_shape: &[usize],
        rhs_shape: &[usize],
        out_shape: &[usize],
        op: BinaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = binary_broadcast_f32_buffer(lhs, rhs, lhs_shape, rhs_shape, out_shape, op)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn binary_broadcast_f32_buffer(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        lhs_shape: &[usize],
        rhs_shape: &[usize],
        out_shape: &[usize],
        op: BinaryOp,
    ) -> Result<CudaBuffer, String> {
        let len = out_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast output length overflow".to_string())?;
        let lhs_len = lhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast lhs length overflow".to_string())?;
        let rhs_len = rhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast rhs length overflow".to_string())?;
        if lhs.len() != lhs_len || rhs.len() != rhs_len {
            return Err(format!(
                "CUDA binary broadcast input length mismatch: expected lhs={}, rhs={}, got lhs={}, rhs={}",
                lhs_len,
                rhs_len,
                lhs.len(),
                rhs.len()
            ));
        }
        let (lhs_aligned, rhs_aligned, out_strides, lhs_strides, rhs_strides) =
            aligned_broadcast_metadata(lhs_shape, rhs_shape, out_shape)?;
        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_binary_broadcast_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                out_shape.len(),
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                lhs_aligned.as_ptr(),
                lhs_strides.as_ptr(),
                rhs_aligned.as_ptr(),
                rhs_strides.as_ptr(),
                len,
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn binary_broadcast_backward_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        grad: &CudaBuffer,
        lhs_shape: &[usize],
        rhs_shape: &[usize],
        out_shape: &[usize],
        op: BinaryOp,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        let (grad_lhs, grad_rhs) = binary_broadcast_backward_f32_buffers(
            lhs, rhs, grad, lhs_shape, rhs_shape, out_shape, op,
        )?;
        let grad_lhs_host = download_f32(&grad_lhs)?;
        let grad_rhs_host = download_f32(&grad_rhs)?;
        Ok(((grad_lhs, grad_lhs_host), (grad_rhs, grad_rhs_host)))
    }

    pub fn binary_broadcast_backward_f32_buffers(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        grad: &CudaBuffer,
        lhs_shape: &[usize],
        rhs_shape: &[usize],
        out_shape: &[usize],
        op: BinaryOp,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        let out_len = out_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast backward output length overflow".to_string())?;
        let lhs_len = lhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast backward lhs length overflow".to_string())?;
        let rhs_len = rhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA binary broadcast backward rhs length overflow".to_string())?;
        if lhs.len() != lhs_len || rhs.len() != rhs_len || grad.len() != out_len {
            return Err(format!(
                "CUDA binary broadcast backward length mismatch: expected lhs={}, rhs={}, grad={}, got lhs={}, rhs={}, grad={}",
                lhs_len,
                rhs_len,
                out_len,
                lhs.len(),
                rhs.len(),
                grad.len()
            ));
        }
        let (lhs_aligned, rhs_aligned, out_strides, lhs_strides, rhs_strides) =
            aligned_broadcast_metadata(lhs_shape, rhs_shape, out_shape)?;
        let grad_lhs = alloc_f32(lhs_len)?;
        let grad_rhs = alloc_f32(rhs_len)?;
        let status = unsafe {
            lumen_cuda_binary_broadcast_backward_f32_device(
                lhs.handle(),
                rhs.handle(),
                grad.handle(),
                grad_lhs.handle(),
                grad_rhs.handle(),
                out_shape.len(),
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                lhs_aligned.as_ptr(),
                lhs_strides.as_ptr(),
                rhs_aligned.as_ptr(),
                rhs_strides.as_ptr(),
                out_len,
                lhs_len,
                rhs_len,
                op as c_int,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((grad_lhs, grad_rhs))
    }

    pub fn sum_f32(input: &CudaBuffer) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = alloc_f32(1)?;
        let status =
            unsafe { lumen_cuda_sum_f32_device(input.handle(), out.handle(), input.len()) };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn fill_scalar_f32(len: usize, value: f32) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = alloc_f32(len)?;
        let status = unsafe { lumen_cuda_fill_scalar_f32_device(out.handle(), len, value) };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn fill_scalar_f32_buffer(len: usize, value: f32) -> Result<CudaBuffer, String> {
        let out = alloc_f32(len)?;
        let status = unsafe { lumen_cuda_fill_scalar_f32_device(out.handle(), len, value) };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn mse_backward_f32(
        diff: &CudaBuffer,
        factor: f32,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        let (grad_output, grad_target) = mse_backward_f32_buffers(diff, factor)?;
        let grad_output_host = download_f32(&grad_output)?;
        let grad_target_host = download_f32(&grad_target)?;
        Ok((
            (grad_output, grad_output_host),
            (grad_target, grad_target_host),
        ))
    }

    pub fn mse_backward_f32_buffers(
        diff: &CudaBuffer,
        factor: f32,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        let grad_output = alloc_f32(diff.len())?;
        let grad_target = alloc_f32(diff.len())?;
        let status = unsafe {
            lumen_cuda_mse_backward_f32_device(
                diff.handle(),
                grad_output.handle(),
                grad_target.handle(),
                diff.len(),
                factor,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((grad_output, grad_target))
    }

    pub fn cross_entropy_backward_f32(
        softmax: &CudaBuffer,
        target: &CudaBuffer,
        factor: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = cross_entropy_backward_f32_buffer(softmax, target, factor)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn cross_entropy_backward_f32_buffer(
        softmax: &CudaBuffer,
        target: &CudaBuffer,
        factor: f32,
    ) -> Result<CudaBuffer, String> {
        if softmax.len() != target.len() {
            return Err(format!(
                "CUDA cross_entropy backward length mismatch: softmax={}, target={}",
                softmax.len(),
                target.len()
            ));
        }
        let out = alloc_f32(softmax.len())?;
        let status = unsafe {
            lumen_cuda_cross_entropy_backward_f32_device(
                softmax.handle(),
                target.handle(),
                out.handle(),
                softmax.len(),
                factor,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn cross_entropy_loss_f32(
        softmax: &CudaBuffer,
        target: &CudaBuffer,
        batch_size: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        if batch_size == 0 {
            return Err("CUDA cross_entropy loss batch size must be greater than zero".to_string());
        }
        if softmax.len() != target.len() {
            return Err(format!(
                "CUDA cross_entropy loss length mismatch: softmax={}, target={}",
                softmax.len(),
                target.len()
            ));
        }
        let out = alloc_f32(1)?;
        let factor = 1.0 / batch_size as f32;
        let status = unsafe {
            lumen_cuda_cross_entropy_loss_f32_device(
                softmax.handle(),
                target.handle(),
                out.handle(),
                softmax.len(),
                factor,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn sgd_update_f32(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        lr: f32,
    ) -> Result<Vec<f32>, String> {
        sgd_update_f32_no_host(param, grad, lr)?;
        download_f32(param)
    }

    pub fn sgd_update_f32_no_host(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        lr: f32,
    ) -> Result<(), String> {
        if param.len() != grad.len() {
            return Err(format!(
                "CUDA SGD length mismatch: param={}, grad={}",
                param.len(),
                grad.len()
            ));
        }
        let status = unsafe {
            lumen_cuda_sgd_update_f32_device(param.handle(), grad.handle(), param.len(), lr)
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(())
    }

    pub fn sgd_momentum_update_f32(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        velocity: &CudaBuffer,
        lr: f32,
        momentum: f32,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        sgd_momentum_update_f32_no_host(param, grad, velocity, lr, momentum)?;
        Ok((download_f32(param)?, download_f32(velocity)?))
    }

    pub fn sgd_momentum_update_f32_no_host(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        velocity: &CudaBuffer,
        lr: f32,
        momentum: f32,
    ) -> Result<(), String> {
        if param.len() != grad.len() || param.len() != velocity.len() {
            return Err(format!(
                "CUDA SGD momentum length mismatch: param={}, grad={}, velocity={}",
                param.len(),
                grad.len(),
                velocity.len()
            ));
        }
        let status = unsafe {
            lumen_cuda_sgd_momentum_update_f32_device(
                param.handle(),
                grad.handle(),
                velocity.handle(),
                param.len(),
                lr,
                momentum,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adam_update_f32(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        exp_avg: &CudaBuffer,
        exp_avg_sq: &CudaBuffer,
        lr: f32,
        beta1: f32,
        beta2: f32,
        bias_correction1: f32,
        bias_correction2: f32,
        eps: f32,
    ) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>), String> {
        adam_update_f32_no_host(
            param,
            grad,
            exp_avg,
            exp_avg_sq,
            lr,
            beta1,
            beta2,
            bias_correction1,
            bias_correction2,
            eps,
        )?;
        Ok((
            download_f32(param)?,
            download_f32(exp_avg)?,
            download_f32(exp_avg_sq)?,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adam_update_f32_no_host(
        param: &CudaBuffer,
        grad: &CudaBuffer,
        exp_avg: &CudaBuffer,
        exp_avg_sq: &CudaBuffer,
        lr: f32,
        beta1: f32,
        beta2: f32,
        bias_correction1: f32,
        bias_correction2: f32,
        eps: f32,
    ) -> Result<(), String> {
        if param.len() != grad.len()
            || param.len() != exp_avg.len()
            || param.len() != exp_avg_sq.len()
        {
            return Err(format!(
                "CUDA Adam length mismatch: param={}, grad={}, exp_avg={}, exp_avg_sq={}",
                param.len(),
                grad.len(),
                exp_avg.len(),
                exp_avg_sq.len()
            ));
        }
        let status = unsafe {
            lumen_cuda_adam_update_f32_device(
                param.handle(),
                grad.handle(),
                exp_avg.handle(),
                exp_avg_sq.handle(),
                param.len(),
                lr,
                beta1,
                beta2,
                bias_correction1,
                bias_correction2,
                eps,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(())
    }

    pub fn softmax_lastdim_f32(
        input: &CudaBuffer,
        outer: usize,
        last_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        if input.len() != outer * last_dim {
            return Err(format!(
                "CUDA softmax input length mismatch: expected {}, got {}",
                outer * last_dim,
                input.len()
            ));
        }
        let out = alloc_f32(input.len())?;
        let status = unsafe {
            lumen_cuda_softmax_lastdim_f32_device(input.handle(), out.handle(), outer, last_dim)
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn softmax_lastdim_f32_no_host(
        input: &CudaBuffer,
        outer: usize,
        last_dim: usize,
    ) -> Result<CudaBuffer, String> {
        if input.len() != outer * last_dim {
            return Err(format!(
                "CUDA softmax input length mismatch: expected {}, got {}",
                outer * last_dim,
                input.len()
            ));
        }
        let out = alloc_f32(input.len())?;
        let status = unsafe {
            lumen_cuda_softmax_lastdim_f32_device(input.handle(), out.handle(), outer, last_dim)
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn softmax_lastdim_backward_f32(
        output: &CudaBuffer,
        grad: &CudaBuffer,
        outer: usize,
        last_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = softmax_lastdim_backward_f32_buffer(output, grad, outer, last_dim)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn softmax_lastdim_backward_f32_buffer(
        output: &CudaBuffer,
        grad: &CudaBuffer,
        outer: usize,
        last_dim: usize,
    ) -> Result<CudaBuffer, String> {
        let len = outer
            .checked_mul(last_dim)
            .ok_or_else(|| "CUDA softmax backward length overflow".to_string())?;
        if output.len() != len || grad.len() != len {
            return Err(format!(
                "CUDA softmax backward length mismatch: expected {}, output={}, grad={}",
                len,
                output.len(),
                grad.len()
            ));
        }
        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_softmax_lastdim_backward_f32_device(
                output.handle(),
                grad.handle(),
                out.handle(),
                outer,
                last_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn fused_softmax_f32(
        input: &CudaBuffer,
        batch_heads: usize,
        q_len: usize,
        k_len: usize,
        scale: f32,
        is_causal: bool,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = fused_softmax_f32_no_host(input, batch_heads, q_len, k_len, scale, is_causal)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn fused_softmax_f32_no_host(
        input: &CudaBuffer,
        batch_heads: usize,
        q_len: usize,
        k_len: usize,
        scale: f32,
        is_causal: bool,
    ) -> Result<CudaBuffer, String> {
        let expected_len = batch_heads
            .checked_mul(q_len)
            .and_then(|value| value.checked_mul(k_len))
            .ok_or_else(|| "CUDA fused_softmax input length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA fused_softmax input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_fused_softmax_f32_device(
                input.handle(),
                out.handle(),
                batch_heads,
                q_len,
                k_len,
                scale,
                if is_causal { 1 } else { 0 },
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn fused_softmax_backward_f32(
        output: &CudaBuffer,
        grad: &CudaBuffer,
        batch_heads: usize,
        q_len: usize,
        k_len: usize,
        scale: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out =
            fused_softmax_backward_f32_buffer(output, grad, batch_heads, q_len, k_len, scale)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn fused_softmax_backward_f32_buffer(
        output: &CudaBuffer,
        grad: &CudaBuffer,
        batch_heads: usize,
        q_len: usize,
        k_len: usize,
        scale: f32,
    ) -> Result<CudaBuffer, String> {
        let expected_len = batch_heads
            .checked_mul(q_len)
            .and_then(|value| value.checked_mul(k_len))
            .ok_or_else(|| "CUDA fused_softmax backward input length overflow".to_string())?;
        if output.len() != expected_len || grad.len() != expected_len {
            return Err(format!(
                "CUDA fused_softmax backward length mismatch: expected {}, output={}, grad={}",
                expected_len,
                output.len(),
                grad.len()
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_fused_softmax_backward_f32_device(
                output.handle(),
                grad.handle(),
                out.handle(),
                batch_heads,
                q_len,
                k_len,
                scale,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn fused_softmax_f32_with_past(
        input: &CudaBuffer,
        batch_heads: usize,
        q_len: usize,
        k_len: usize,
        scale: f32,
        is_causal: bool,
        past_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let expected_len = batch_heads
            .checked_mul(q_len)
            .and_then(|value| value.checked_mul(k_len))
            .ok_or_else(|| "CUDA fused_softmax_with_past input length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA fused_softmax_with_past input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        if batch_heads == 0 || q_len == 0 || k_len == 0 {
            return Err(
                "CUDA fused_softmax_with_past dimensions must be greater than zero".to_string(),
            );
        }
        let causal_window_end = past_len
            .checked_add(q_len)
            .ok_or_else(|| "CUDA fused_softmax_with_past causal window overflow".to_string())?;
        if causal_window_end > k_len {
            return Err(format!(
                "CUDA fused_softmax_with_past causal window out of bounds: past_len({}) + q_len({}) > k_len({})",
                past_len, q_len, k_len
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_fused_softmax_f32_with_past_device(
                input.handle(),
                out.handle(),
                batch_heads,
                q_len,
                k_len,
                scale,
                if is_causal { 1 } else { 0 },
                past_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn embedding_f32(
        indices: &CudaBuffer,
        weight: &CudaBuffer,
        num_indices: usize,
        vocab_size: usize,
        embed_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        if indices.len() != num_indices {
            return Err(format!(
                "CUDA embedding indices length mismatch: expected {}, got {}",
                num_indices,
                indices.len()
            ));
        }
        if weight.len() != vocab_size * embed_dim {
            return Err(format!(
                "CUDA embedding weight length mismatch: expected {}, got {}",
                vocab_size * embed_dim,
                weight.len()
            ));
        }
        let out = alloc_f32(num_indices * embed_dim)?;
        let status = unsafe {
            lumen_cuda_embedding_f32_device(
                indices.handle(),
                weight.handle(),
                out.handle(),
                num_indices,
                vocab_size,
                embed_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn embedding_backward_f32(
        indices: &CudaBuffer,
        grad: &CudaBuffer,
        num_indices: usize,
        vocab_size: usize,
        embed_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let grad_weight =
            embedding_backward_f32_buffer(indices, grad, num_indices, vocab_size, embed_dim)?;
        let host = download_f32(&grad_weight)?;
        Ok((grad_weight, host))
    }

    pub fn embedding_backward_f32_buffer(
        indices: &CudaBuffer,
        grad: &CudaBuffer,
        num_indices: usize,
        vocab_size: usize,
        embed_dim: usize,
    ) -> Result<CudaBuffer, String> {
        if indices.len() != num_indices {
            return Err(format!(
                "CUDA embedding backward indices length mismatch: expected {}, got {}",
                num_indices,
                indices.len()
            ));
        }
        if grad.len() != num_indices * embed_dim {
            return Err(format!(
                "CUDA embedding backward grad length mismatch: expected {}, got {}",
                num_indices * embed_dim,
                grad.len()
            ));
        }
        let grad_weight = alloc_f32(vocab_size * embed_dim)?;
        let status = unsafe {
            lumen_cuda_embedding_backward_f32_device(
                indices.handle(),
                grad.handle(),
                grad_weight.handle(),
                num_indices,
                vocab_size,
                embed_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(grad_weight)
    }

    pub fn rms_norm_f32(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        rows: usize,
        dim: usize,
        eps: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        if input.len() != rows * dim {
            return Err(format!(
                "CUDA RMSNorm input length mismatch: expected {}, got {}",
                rows * dim,
                input.len()
            ));
        }
        if weight.len() != dim {
            return Err(format!(
                "CUDA RMSNorm weight length mismatch: expected {}, got {}",
                dim,
                weight.len()
            ));
        }
        let out = alloc_f32(rows * dim)?;
        let status = unsafe {
            lumen_cuda_rms_norm_f32_device(
                input.handle(),
                weight.handle(),
                out.handle(),
                rows,
                dim,
                eps,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn rms_norm_backward_f32(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        grad: &CudaBuffer,
        rows: usize,
        dim: usize,
        eps: f32,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        let (grad_input, grad_weight) =
            rms_norm_backward_f32_buffers(input, weight, grad, rows, dim, eps)?;
        let grad_input_host = download_f32(&grad_input)?;
        let grad_weight_host = download_f32(&grad_weight)?;
        Ok((
            (grad_input, grad_input_host),
            (grad_weight, grad_weight_host),
        ))
    }

    pub fn rms_norm_backward_f32_buffers(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        grad: &CudaBuffer,
        rows: usize,
        dim: usize,
        eps: f32,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        let len = rows
            .checked_mul(dim)
            .ok_or_else(|| "CUDA RMSNorm backward length overflow".to_string())?;
        if input.len() != len || grad.len() != len {
            return Err(format!(
                "CUDA RMSNorm backward input/grad length mismatch: expected {}, input={}, grad={}",
                len,
                input.len(),
                grad.len()
            ));
        }
        if weight.len() != dim {
            return Err(format!(
                "CUDA RMSNorm backward weight length mismatch: expected {}, got {}",
                dim,
                weight.len()
            ));
        }
        let grad_input = alloc_f32(len)?;
        let grad_weight = alloc_f32(dim)?;
        let status = unsafe {
            lumen_cuda_rms_norm_backward_f32_device(
                input.handle(),
                weight.handle(),
                grad.handle(),
                grad_input.handle(),
                grad_weight.handle(),
                rows,
                dim,
                eps,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((grad_input, grad_weight))
    }

    pub fn permute_f32(
        input: &CudaBuffer,
        out_shape: &[usize],
        axes: &[usize],
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let ndim = out_shape.len();
        if axes.len() != ndim {
            return Err(format!(
                "CUDA permute axes length mismatch: axes={}, ndim={}",
                axes.len(),
                ndim
            ));
        }
        if ndim == 0 {
            return Err("CUDA permute expects at least 1 dimension".to_string());
        }

        let len = out_shape.iter().product::<usize>();
        if input.len() != len {
            return Err(format!(
                "CUDA permute input length mismatch: expected {}, got {}",
                len,
                input.len()
            ));
        }

        let mut seen = vec![false; ndim];
        for &axis in axes {
            if axis >= ndim || seen[axis] {
                return Err(format!("CUDA permute axes are invalid: {:?}", axes));
            }
            seen[axis] = true;
        }

        let mut input_shape = vec![0usize; ndim];
        for (out_dim, &input_axis) in axes.iter().enumerate() {
            input_shape[input_axis] = out_shape[out_dim];
        }

        let mut input_strides = vec![0usize; ndim];
        let mut stride = 1usize;
        for i in (0..ndim).rev() {
            input_strides[i] = stride;
            stride = stride
                .checked_mul(input_shape[i])
                .ok_or_else(|| "CUDA permute stride overflow".to_string())?;
        }

        let mut out_strides = vec![0usize; ndim];
        stride = 1usize;
        for i in (0..ndim).rev() {
            out_strides[i] = stride;
            stride = stride
                .checked_mul(out_shape[i])
                .ok_or_else(|| "CUDA permute output stride overflow".to_string())?;
        }

        let mapped_input_strides = axes
            .iter()
            .map(|&axis| input_strides[axis])
            .collect::<Vec<_>>();
        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_permute_f32_device(
                input.handle(),
                out.handle(),
                ndim,
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                mapped_input_strides.as_ptr(),
                len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn permute_f32_buffer(
        input: &CudaBuffer,
        out_shape: &[usize],
        axes: &[usize],
    ) -> Result<CudaBuffer, String> {
        let ndim = out_shape.len();
        if axes.len() != ndim {
            return Err(format!(
                "CUDA permute axes length mismatch: axes={}, ndim={}",
                axes.len(),
                ndim
            ));
        }
        if ndim == 0 {
            return Err("CUDA permute expects at least 1 dimension".to_string());
        }

        let len = out_shape.iter().product::<usize>();
        if input.len() != len {
            return Err(format!(
                "CUDA permute input length mismatch: expected {}, got {}",
                len,
                input.len()
            ));
        }

        let mut seen = vec![false; ndim];
        for &axis in axes {
            if axis >= ndim || seen[axis] {
                return Err(format!("CUDA permute axes are invalid: {:?}", axes));
            }
            seen[axis] = true;
        }

        let mut input_shape = vec![0usize; ndim];
        for (out_dim, &input_axis) in axes.iter().enumerate() {
            input_shape[input_axis] = out_shape[out_dim];
        }

        let mut input_strides = vec![0usize; ndim];
        let mut stride = 1usize;
        for i in (0..ndim).rev() {
            input_strides[i] = stride;
            stride = stride
                .checked_mul(input_shape[i])
                .ok_or_else(|| "CUDA permute stride overflow".to_string())?;
        }

        let mut out_strides = vec![0usize; ndim];
        stride = 1usize;
        for i in (0..ndim).rev() {
            out_strides[i] = stride;
            stride = stride
                .checked_mul(out_shape[i])
                .ok_or_else(|| "CUDA permute output stride overflow".to_string())?;
        }

        let mapped_input_strides = axes
            .iter()
            .map(|&axis| input_strides[axis])
            .collect::<Vec<_>>();
        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_permute_f32_device(
                input.handle(),
                out.handle(),
                ndim,
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                mapped_input_strides.as_ptr(),
                len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn slice_lastdim_f32(
        input: &CudaBuffer,
        outer: usize,
        input_last_dim: usize,
        start: usize,
        slice_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let expected_len = outer
            .checked_mul(input_last_dim)
            .ok_or_else(|| "CUDA slice length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA slice input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        if start + slice_len > input_last_dim {
            return Err(format!(
                "CUDA slice range out of bounds: start={}, len={}, input_last_dim={}",
                start, slice_len, input_last_dim
            ));
        }
        let out = alloc_f32(
            outer
                .checked_mul(slice_len)
                .ok_or_else(|| "CUDA slice output length overflow".to_string())?,
        )?;
        let status = unsafe {
            lumen_cuda_slice_lastdim_f32_device(
                input.handle(),
                out.handle(),
                outer,
                input_last_dim,
                start,
                slice_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn slice_lastdim_f32_buffer(
        input: &CudaBuffer,
        outer: usize,
        input_last_dim: usize,
        start: usize,
        slice_len: usize,
    ) -> Result<CudaBuffer, String> {
        let expected_len = outer
            .checked_mul(input_last_dim)
            .ok_or_else(|| "CUDA slice length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA slice input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        if start + slice_len > input_last_dim {
            return Err(format!(
                "CUDA slice range out of bounds: start={}, len={}, input_last_dim={}",
                start, slice_len, input_last_dim
            ));
        }
        let out = alloc_f32(
            outer
                .checked_mul(slice_len)
                .ok_or_else(|| "CUDA slice output length overflow".to_string())?,
        )?;
        let status = unsafe {
            lumen_cuda_slice_lastdim_f32_device(
                input.handle(),
                out.handle(),
                outer,
                input_last_dim,
                start,
                slice_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn slice_lastdim_backward_f32(
        grad: &CudaBuffer,
        outer: usize,
        input_last_dim: usize,
        start: usize,
        slice_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = slice_lastdim_backward_f32_buffer(grad, outer, input_last_dim, start, slice_len)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn slice_lastdim_backward_f32_buffer(
        grad: &CudaBuffer,
        outer: usize,
        input_last_dim: usize,
        start: usize,
        slice_len: usize,
    ) -> Result<CudaBuffer, String> {
        let grad_len = outer
            .checked_mul(slice_len)
            .ok_or_else(|| "CUDA slice backward grad length overflow".to_string())?;
        if grad.len() != grad_len {
            return Err(format!(
                "CUDA slice backward grad length mismatch: expected {}, got {}",
                grad_len,
                grad.len()
            ));
        }
        if start + slice_len > input_last_dim {
            return Err(format!(
                "CUDA slice backward range out of bounds: start={}, len={}, input_last_dim={}",
                start, slice_len, input_last_dim
            ));
        }
        let out_len = outer
            .checked_mul(input_last_dim)
            .ok_or_else(|| "CUDA slice backward output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_slice_lastdim_backward_f32_device(
                grad.handle(),
                out.handle(),
                outer,
                input_last_dim,
                start,
                slice_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn cat_f32(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        out_shape: &[usize],
        axis: usize,
        lhs_axis_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let ndim = out_shape.len();
        if ndim == 0 {
            return Err("CUDA cat expects at least 1 dimension".to_string());
        }
        if axis >= ndim {
            return Err(format!(
                "CUDA cat axis out of bounds: axis={}, ndim={}",
                axis, ndim
            ));
        }
        let len = out_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat output length overflow".to_string())?;
        if len == 0 {
            return Err("CUDA cat does not support empty outputs".to_string());
        }
        let out_axis_len = out_shape[axis];
        if lhs_axis_len > out_axis_len {
            return Err(format!(
                "CUDA cat lhs axis length out of bounds: lhs_axis_len={}, out_axis_len={}",
                lhs_axis_len, out_axis_len
            ));
        }
        let rhs_axis_len = out_axis_len - lhs_axis_len;

        let mut lhs_shape = out_shape.to_vec();
        lhs_shape[axis] = lhs_axis_len;
        let mut rhs_shape = out_shape.to_vec();
        rhs_shape[axis] = rhs_axis_len;

        let lhs_expected = lhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat lhs length overflow".to_string())?;
        let rhs_expected = rhs_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat rhs length overflow".to_string())?;
        if lhs.len() != lhs_expected || rhs.len() != rhs_expected {
            return Err(format!(
                "CUDA cat input length mismatch: expected lhs={}, rhs={}, got lhs={}, rhs={}",
                lhs_expected,
                rhs_expected,
                lhs.len(),
                rhs.len()
            ));
        }

        fn row_major_strides(shape: &[usize]) -> Result<Vec<usize>, String> {
            let mut strides = vec![0usize; shape.len()];
            let mut stride = 1usize;
            for i in (0..shape.len()).rev() {
                strides[i] = stride;
                stride = stride
                    .checked_mul(shape[i])
                    .ok_or_else(|| "CUDA cat stride overflow".to_string())?;
            }
            Ok(strides)
        }

        let out_strides = row_major_strides(out_shape)?;
        let lhs_strides = row_major_strides(&lhs_shape)?;
        let rhs_strides = row_major_strides(&rhs_shape)?;

        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_cat_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                ndim,
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                lhs_strides.as_ptr(),
                rhs_strides.as_ptr(),
                axis,
                lhs_axis_len,
                len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn cat_f32_buffer(
        lhs: &CudaBuffer,
        rhs: &CudaBuffer,
        out_shape: &[usize],
        axis: usize,
        lhs_axis_len: usize,
    ) -> Result<CudaBuffer, String> {
        let ndim = out_shape.len();
        if ndim == 0 {
            return Err("CUDA cat expects at least 1 dimension".to_string());
        }
        if axis >= ndim {
            return Err(format!(
                "CUDA cat axis out of bounds: axis={}, ndim={}",
                axis, ndim
            ));
        }
        let len = out_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat output length overflow".to_string())?;
        if len == 0 {
            return Err("CUDA cat does not support empty outputs".to_string());
        }
        let out_axis_len = out_shape[axis];
        if lhs_axis_len > out_axis_len {
            return Err(format!(
                "CUDA cat lhs axis length out of bounds: lhs_axis_len={}, out_axis_len={}",
                lhs_axis_len, out_axis_len
            ));
        }
        let rhs_axis_len = out_axis_len - lhs_axis_len;

        let lhs_len = out_shape
            .iter()
            .enumerate()
            .try_fold(1usize, |acc, (idx, &dim)| {
                let actual = if idx == axis { lhs_axis_len } else { dim };
                acc.checked_mul(actual)
            })
            .ok_or_else(|| "CUDA cat lhs length overflow".to_string())?;
        let rhs_len = out_shape
            .iter()
            .enumerate()
            .try_fold(1usize, |acc, (idx, &dim)| {
                let actual = if idx == axis { rhs_axis_len } else { dim };
                acc.checked_mul(actual)
            })
            .ok_or_else(|| "CUDA cat rhs length overflow".to_string())?;
        if lhs.len() != lhs_len || rhs.len() != rhs_len {
            return Err(format!(
                "CUDA cat input length mismatch: expected lhs={}, rhs={}, got lhs={}, rhs={}",
                lhs_len,
                rhs_len,
                lhs.len(),
                rhs.len()
            ));
        }

        let mut out_strides = vec![0usize; ndim];
        let mut stride = 1usize;
        for i in (0..ndim).rev() {
            out_strides[i] = stride;
            stride = stride
                .checked_mul(out_shape[i])
                .ok_or_else(|| "CUDA cat stride overflow".to_string())?;
        }

        let lhs_shape = out_shape
            .iter()
            .enumerate()
            .map(|(idx, &dim)| if idx == axis { lhs_axis_len } else { dim })
            .collect::<Vec<_>>();
        let rhs_shape = out_shape
            .iter()
            .enumerate()
            .map(|(idx, &dim)| if idx == axis { rhs_axis_len } else { dim })
            .collect::<Vec<_>>();

        let mut lhs_strides = vec![0usize; ndim];
        stride = 1usize;
        for i in (0..ndim).rev() {
            lhs_strides[i] = stride;
            stride = stride
                .checked_mul(lhs_shape[i])
                .ok_or_else(|| "CUDA cat lhs stride overflow".to_string())?;
        }

        let mut rhs_strides = vec![0usize; ndim];
        stride = 1usize;
        for i in (0..ndim).rev() {
            rhs_strides[i] = stride;
            stride = stride
                .checked_mul(rhs_shape[i])
                .ok_or_else(|| "CUDA cat rhs stride overflow".to_string())?;
        }

        let out = alloc_f32(len)?;
        let status = unsafe {
            lumen_cuda_cat_f32_device(
                lhs.handle(),
                rhs.handle(),
                out.handle(),
                ndim,
                out_shape.as_ptr(),
                out_strides.as_ptr(),
                lhs_strides.as_ptr(),
                rhs_strides.as_ptr(),
                axis,
                lhs_axis_len,
                len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn cat_backward_slice_f32(
        grad: &CudaBuffer,
        input_shape: &[usize],
        out_shape: &[usize],
        axis: usize,
        axis_start: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = cat_backward_slice_f32_buffer(grad, input_shape, out_shape, axis, axis_start)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn cat_backward_slice_f32_buffer(
        grad: &CudaBuffer,
        input_shape: &[usize],
        out_shape: &[usize],
        axis: usize,
        axis_start: usize,
    ) -> Result<CudaBuffer, String> {
        let ndim = out_shape.len();
        if ndim == 0 || input_shape.len() != ndim {
            return Err(format!(
                "CUDA cat backward shape rank mismatch: input_ndim={}, out_ndim={}",
                input_shape.len(),
                ndim
            ));
        }
        if axis >= ndim {
            return Err(format!(
                "CUDA cat backward axis out of bounds: axis={}, ndim={}",
                axis, ndim
            ));
        }
        for (idx, (&in_dim, &out_dim)) in input_shape.iter().zip(out_shape.iter()).enumerate() {
            if idx == axis {
                if axis_start + in_dim > out_dim {
                    return Err(format!(
                        "CUDA cat backward axis range out of bounds: start={}, len={}, out_dim={}",
                        axis_start, in_dim, out_dim
                    ));
                }
            } else if in_dim != out_dim {
                return Err(format!(
                    "CUDA cat backward non-axis dim mismatch at {}: input={}, output={}",
                    idx, in_dim, out_dim
                ));
            }
        }

        let out_len = out_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat backward output grad length overflow".to_string())?;
        if grad.len() != out_len {
            return Err(format!(
                "CUDA cat backward grad length mismatch: expected {}, got {}",
                out_len,
                grad.len()
            ));
        }
        let input_len = input_shape
            .iter()
            .try_fold(1usize, |acc, &dim| acc.checked_mul(dim))
            .ok_or_else(|| "CUDA cat backward input grad length overflow".to_string())?;

        fn row_major_strides(shape: &[usize]) -> Result<Vec<usize>, String> {
            let mut strides = vec![0usize; shape.len()];
            let mut stride = 1usize;
            for i in (0..shape.len()).rev() {
                strides[i] = stride;
                stride = stride
                    .checked_mul(shape[i])
                    .ok_or_else(|| "CUDA cat backward stride overflow".to_string())?;
            }
            Ok(strides)
        }

        let input_strides = row_major_strides(input_shape)?;
        let out_strides = row_major_strides(out_shape)?;
        let out = alloc_f32(input_len)?;
        let status = unsafe {
            lumen_cuda_cat_backward_slice_f32_device(
                grad.handle(),
                out.handle(),
                ndim,
                input_shape.as_ptr(),
                input_strides.as_ptr(),
                out_strides.as_ptr(),
                axis,
                axis_start,
                input_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn repeat_kv_f32(
        input: &CudaBuffer,
        batch_size: usize,
        num_kv_heads: usize,
        seq_len: usize,
        dim: usize,
        n_rep: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let input_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA repeat_kv input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA repeat_kv input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        if batch_size == 0 || num_kv_heads == 0 || seq_len == 0 || dim == 0 || n_rep == 0 {
            return Err("CUDA repeat_kv dimensions must be greater than zero".to_string());
        }
        let out_len = input_len
            .checked_mul(n_rep)
            .ok_or_else(|| "CUDA repeat_kv output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_repeat_kv_f32_device(
                input.handle(),
                out.handle(),
                batch_size,
                num_kv_heads,
                seq_len,
                dim,
                n_rep,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn repeat_kv_f32_buffer(
        input: &CudaBuffer,
        batch_size: usize,
        num_kv_heads: usize,
        seq_len: usize,
        dim: usize,
        n_rep: usize,
    ) -> Result<CudaBuffer, String> {
        let input_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA repeat_kv input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA repeat_kv input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        if batch_size == 0 || num_kv_heads == 0 || seq_len == 0 || dim == 0 || n_rep == 0 {
            return Err("CUDA repeat_kv dimensions must be greater than zero".to_string());
        }
        let out_len = input_len
            .checked_mul(n_rep)
            .ok_or_else(|| "CUDA repeat_kv output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_repeat_kv_f32_device(
                input.handle(),
                out.handle(),
                batch_size,
                num_kv_heads,
                seq_len,
                dim,
                n_rep,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn repeat_kv_backward_f32(
        grad: &CudaBuffer,
        batch_size: usize,
        num_kv_heads: usize,
        seq_len: usize,
        dim: usize,
        n_rep: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out =
            repeat_kv_backward_f32_buffer(grad, batch_size, num_kv_heads, seq_len, dim, n_rep)?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn repeat_kv_backward_f32_buffer(
        grad: &CudaBuffer,
        batch_size: usize,
        num_kv_heads: usize,
        seq_len: usize,
        dim: usize,
        n_rep: usize,
    ) -> Result<CudaBuffer, String> {
        let grad_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(n_rep))
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA repeat_kv backward grad length overflow".to_string())?;
        if grad.len() != grad_len {
            return Err(format!(
                "CUDA repeat_kv backward grad length mismatch: expected {}, got {}",
                grad_len,
                grad.len()
            ));
        }
        let out_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA repeat_kv backward output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_repeat_kv_backward_f32_device(
                grad.handle(),
                out.handle(),
                batch_size,
                num_kv_heads,
                seq_len,
                dim,
                n_rep,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_attention_f32(
        q: &CudaBuffer,
        k: &CudaBuffer,
        v: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        num_kv_heads: usize,
        active_seq_len: usize,
        cache_seq_len: usize,
        dim: usize,
        n_rep: usize,
        scale: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let q_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention q length overflow".to_string())?;
        let kv_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(cache_seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention kv length overflow".to_string())?;
        if q.len() != q_len {
            return Err(format!(
                "CUDA decode attention q length mismatch: expected {}, got {}",
                q_len,
                q.len()
            ));
        }
        if k.len() != kv_len || v.len() != kv_len {
            return Err(format!(
                "CUDA decode attention kv length mismatch: expected {}, got k={}, v={}",
                kv_len,
                k.len(),
                v.len()
            ));
        }
        if batch_size == 0
            || num_heads == 0
            || num_kv_heads == 0
            || active_seq_len == 0
            || cache_seq_len == 0
            || dim == 0
            || n_rep == 0
        {
            return Err("CUDA decode attention dimensions must be greater than zero".to_string());
        }
        if active_seq_len > cache_seq_len {
            return Err(format!(
                "CUDA decode attention active_seq_len out of bounds: {} > {}",
                active_seq_len, cache_seq_len
            ));
        }
        let out_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_decode_attention_f32_device(
                q.handle(),
                k.handle(),
                v.handle(),
                out.handle(),
                batch_size,
                num_heads,
                num_kv_heads,
                active_seq_len,
                cache_seq_len,
                dim,
                n_rep,
                scale,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_attention_f32_buffer(
        q: &CudaBuffer,
        k: &CudaBuffer,
        v: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        num_kv_heads: usize,
        active_seq_len: usize,
        cache_seq_len: usize,
        dim: usize,
        n_rep: usize,
        scale: f32,
    ) -> Result<CudaBuffer, String> {
        let q_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention q length overflow".to_string())?;
        let kv_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(cache_seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention kv length overflow".to_string())?;
        if q.len() != q_len {
            return Err(format!(
                "CUDA decode attention q length mismatch: expected {}, got {}",
                q_len,
                q.len()
            ));
        }
        if k.len() != kv_len || v.len() != kv_len {
            return Err(format!(
                "CUDA decode attention kv length mismatch: expected {}, got k={}, v={}",
                kv_len,
                k.len(),
                v.len()
            ));
        }
        if batch_size == 0
            || num_heads == 0
            || num_kv_heads == 0
            || active_seq_len == 0
            || cache_seq_len == 0
            || dim == 0
            || n_rep == 0
        {
            return Err("CUDA decode attention dimensions must be greater than zero".to_string());
        }
        if active_seq_len > cache_seq_len {
            return Err(format!(
                "CUDA decode attention active_seq_len out of bounds: {} > {}",
                active_seq_len, cache_seq_len
            ));
        }
        let out_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA decode attention output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_decode_attention_f32_device(
                q.handle(),
                k.handle(),
                v.handle(),
                out.handle(),
                batch_size,
                num_heads,
                num_kv_heads,
                active_seq_len,
                cache_seq_len,
                dim,
                n_rep,
                scale,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prefill_attention_f32_buffer(
        q: &CudaBuffer,
        k: &CudaBuffer,
        v: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        num_kv_heads: usize,
        q_seq_len: usize,
        active_seq_len: usize,
        cache_seq_len: usize,
        dim: usize,
        n_rep: usize,
        past_len: usize,
        scale: f32,
        is_causal: bool,
    ) -> Result<CudaBuffer, String> {
        let q_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(q_seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA prefill attention q length overflow".to_string())?;
        let kv_len = batch_size
            .checked_mul(num_kv_heads)
            .and_then(|value| value.checked_mul(cache_seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA prefill attention kv length overflow".to_string())?;
        if q.len() != q_len {
            return Err(format!(
                "CUDA prefill attention q length mismatch: expected {}, got {}",
                q_len,
                q.len()
            ));
        }
        if k.len() != kv_len || v.len() != kv_len {
            return Err(format!(
                "CUDA prefill attention kv length mismatch: expected {}, got k={}, v={}",
                kv_len,
                k.len(),
                v.len()
            ));
        }
        if batch_size == 0
            || num_heads == 0
            || num_kv_heads == 0
            || q_seq_len == 0
            || active_seq_len == 0
            || cache_seq_len == 0
            || dim == 0
            || n_rep == 0
        {
            return Err("CUDA prefill attention dimensions must be greater than zero".to_string());
        }
        if active_seq_len > cache_seq_len || past_len + q_seq_len > active_seq_len {
            return Err(format!(
                "CUDA prefill attention sequence range out of bounds: past_len={}, q_seq_len={}, active_seq_len={}, cache_seq_len={}",
                past_len, q_seq_len, active_seq_len, cache_seq_len
            ));
        }
        let out_len = batch_size
            .checked_mul(q_seq_len)
            .and_then(|value| value.checked_mul(num_heads))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA prefill attention output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_prefill_attention_f32_device(
                q.handle(),
                k.handle(),
                v.handle(),
                out.handle(),
                batch_size,
                num_heads,
                num_kv_heads,
                q_seq_len,
                active_seq_len,
                cache_seq_len,
                dim,
                n_rep,
                past_len,
                scale,
                if is_causal { 1 } else { 0 },
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    pub fn fused_gate_up_silu_f32(
        input: &CudaBuffer,
        gate: &CudaBuffer,
        up: &CudaBuffer,
        rows: usize,
        n_dim: usize,
        k_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let input_len = rows
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused gate/up input length overflow".to_string())?;
        let weight_len = n_dim
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused gate/up weight length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA fused gate/up input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        if gate.len() != weight_len || up.len() != weight_len {
            return Err(format!(
                "CUDA fused gate/up weight length mismatch: expected {}, got gate={}, up={}",
                weight_len,
                gate.len(),
                up.len()
            ));
        }
        if rows == 0 || n_dim == 0 || k_dim == 0 {
            return Err("CUDA fused gate/up dimensions must be greater than zero".to_string());
        }
        let out_len = rows
            .checked_mul(n_dim)
            .ok_or_else(|| "CUDA fused gate/up output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_fused_gate_up_silu_f32_device(
                input.handle(),
                gate.handle(),
                up.handle(),
                out.handle(),
                rows,
                n_dim,
                k_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn fused_qkv_f32(
        input: &CudaBuffer,
        q: &CudaBuffer,
        k: &CudaBuffer,
        v: &CudaBuffer,
        rows: usize,
        q_n: usize,
        k_n: usize,
        k_dim: usize,
    ) -> Result<
        (
            (CudaBuffer, Vec<f32>),
            (CudaBuffer, Vec<f32>),
            (CudaBuffer, Vec<f32>),
        ),
        String,
    > {
        let input_len = rows
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA fused qkv input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        let q_weight_len = q_n
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv q weight length overflow".to_string())?;
        let kv_weight_len = k_n
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv kv weight length overflow".to_string())?;
        if q.len() != q_weight_len || k.len() != kv_weight_len || v.len() != kv_weight_len {
            return Err(format!(
                "CUDA fused qkv weight length mismatch: expected q={}, k/v={}, got q={}, k={}, v={}",
                q_weight_len,
                kv_weight_len,
                q.len(),
                k.len(),
                v.len()
            ));
        }
        if rows == 0 || q_n == 0 || k_n == 0 || k_dim == 0 {
            return Err("CUDA fused qkv dimensions must be greater than zero".to_string());
        }
        let q_out = alloc_f32(
            rows.checked_mul(q_n)
                .ok_or_else(|| "CUDA fused qkv q output length overflow".to_string())?,
        )?;
        let k_out = alloc_f32(
            rows.checked_mul(k_n)
                .ok_or_else(|| "CUDA fused qkv k output length overflow".to_string())?,
        )?;
        let v_out = alloc_f32(
            rows.checked_mul(k_n)
                .ok_or_else(|| "CUDA fused qkv v output length overflow".to_string())?,
        )?;
        let status = unsafe {
            lumen_cuda_fused_qkv_f32_device(
                input.handle(),
                q.handle(),
                k.handle(),
                v.handle(),
                q_out.handle(),
                k_out.handle(),
                v_out.handle(),
                rows,
                q_n,
                k_n,
                k_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let q_host = download_f32(&q_out)?;
        let k_host = download_f32(&k_out)?;
        let v_host = download_f32(&v_out)?;
        Ok(((q_out, q_host), (k_out, k_host), (v_out, v_host)))
    }

    pub fn fused_qkv_f32_buffer(
        input: &CudaBuffer,
        q: &CudaBuffer,
        k: &CudaBuffer,
        v: &CudaBuffer,
        rows: usize,
        q_n: usize,
        k_n: usize,
        k_dim: usize,
    ) -> Result<(CudaBuffer, CudaBuffer, CudaBuffer), String> {
        let input_len = rows
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA fused qkv input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        let q_weight_len = q_n
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv q weight length overflow".to_string())?;
        let kv_weight_len = k_n
            .checked_mul(k_dim)
            .ok_or_else(|| "CUDA fused qkv kv weight length overflow".to_string())?;
        if q.len() != q_weight_len || k.len() != kv_weight_len || v.len() != kv_weight_len {
            return Err(format!(
                "CUDA fused qkv weight length mismatch: expected q={}, k/v={}, got q={}, k={}, v={}",
                q_weight_len,
                kv_weight_len,
                q.len(),
                k.len(),
                v.len()
            ));
        }
        if rows == 0 || q_n == 0 || k_n == 0 || k_dim == 0 {
            return Err("CUDA fused qkv dimensions must be greater than zero".to_string());
        }
        let q_out = alloc_f32(
            rows.checked_mul(q_n)
                .ok_or_else(|| "CUDA fused qkv q output length overflow".to_string())?,
        )?;
        let k_out = alloc_f32(
            rows.checked_mul(k_n)
                .ok_or_else(|| "CUDA fused qkv k output length overflow".to_string())?,
        )?;
        let v_out = alloc_f32(
            rows.checked_mul(k_n)
                .ok_or_else(|| "CUDA fused qkv v output length overflow".to_string())?,
        )?;
        let status = unsafe {
            lumen_cuda_fused_qkv_f32_device(
                input.handle(),
                q.handle(),
                k.handle(),
                v.handle(),
                q_out.handle(),
                k_out.handle(),
                v_out.handle(),
                rows,
                q_n,
                k_n,
                k_dim,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((q_out, k_out, v_out))
    }

    pub fn rope_f32(
        input: &CudaBuffer,
        cos: &CudaBuffer,
        sin: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        seq_len: usize,
        dim: usize,
        offset: usize,
        cache_seq_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let expected_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA RoPE input length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA RoPE input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        if dim == 0 || dim % 2 != 0 {
            return Err(format!(
                "CUDA RoPE expects a positive even dimension, got {}",
                dim
            ));
        }
        if offset + seq_len > cache_seq_len {
            return Err(format!(
                "CUDA RoPE offset out of bounds: offset {} + seq_len {} > cache_seq_len {}",
                offset, seq_len, cache_seq_len
            ));
        }
        let cache_expected_len = cache_seq_len
            .checked_mul(dim)
            .ok_or_else(|| "CUDA RoPE cache length overflow".to_string())?;
        if cos.len() != cache_expected_len || sin.len() != cache_expected_len {
            return Err(format!(
                "CUDA RoPE cache length mismatch: expected {}, got cos={}, sin={}",
                cache_expected_len,
                cos.len(),
                sin.len()
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_rope_f32_device(
                input.handle(),
                cos.handle(),
                sin.handle(),
                out.handle(),
                batch_size,
                num_heads,
                seq_len,
                dim,
                offset,
                cache_seq_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    pub fn rope_f32_buffer(
        input: &CudaBuffer,
        cos: &CudaBuffer,
        sin: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        seq_len: usize,
        dim: usize,
        offset: usize,
        cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        let expected_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA RoPE input length overflow".to_string())?;
        if input.len() != expected_len {
            return Err(format!(
                "CUDA RoPE input length mismatch: expected {}, got {}",
                expected_len,
                input.len()
            ));
        }
        if dim == 0 || dim % 2 != 0 {
            return Err(format!(
                "CUDA RoPE expects a positive even dimension, got {}",
                dim
            ));
        }
        if offset + seq_len > cache_seq_len {
            return Err(format!(
                "CUDA RoPE offset out of bounds: offset {} + seq_len {} > cache_seq_len {}",
                offset, seq_len, cache_seq_len
            ));
        }
        let cache_len = cache_seq_len
            .checked_mul(dim)
            .ok_or_else(|| "CUDA RoPE cache length overflow".to_string())?;
        if cos.len() != cache_len || sin.len() != cache_len {
            return Err(format!(
                "CUDA RoPE cache length mismatch: expected {}, got cos={}, sin={}",
                cache_len,
                cos.len(),
                sin.len()
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_rope_f32_device(
                input.handle(),
                cos.handle(),
                sin.handle(),
                out.handle(),
                batch_size,
                num_heads,
                seq_len,
                dim,
                offset,
                cache_seq_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_backward_f32(
        grad: &CudaBuffer,
        cos: &CudaBuffer,
        sin: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        seq_len: usize,
        dim: usize,
        offset: usize,
        cache_seq_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let out = rope_backward_f32_buffer(
            grad,
            cos,
            sin,
            batch_size,
            num_heads,
            seq_len,
            dim,
            offset,
            cache_seq_len,
        )?;
        let host = download_f32(&out)?;
        Ok((out, host))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_backward_f32_buffer(
        grad: &CudaBuffer,
        cos: &CudaBuffer,
        sin: &CudaBuffer,
        batch_size: usize,
        num_heads: usize,
        seq_len: usize,
        dim: usize,
        offset: usize,
        cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        let expected_len = batch_size
            .checked_mul(num_heads)
            .and_then(|value| value.checked_mul(seq_len))
            .and_then(|value| value.checked_mul(dim))
            .ok_or_else(|| "CUDA RoPE backward grad length overflow".to_string())?;
        if grad.len() != expected_len {
            return Err(format!(
                "CUDA RoPE backward grad length mismatch: expected {}, got {}",
                expected_len,
                grad.len()
            ));
        }
        if dim == 0 || dim % 2 != 0 {
            return Err(format!(
                "CUDA RoPE backward expects a positive even dimension, got {}",
                dim
            ));
        }
        if offset + seq_len > cache_seq_len {
            return Err(format!(
                "CUDA RoPE backward offset out of bounds: offset {} + seq_len {} > cache_seq_len {}",
                offset, seq_len, cache_seq_len
            ));
        }
        let cache_len = cache_seq_len
            .checked_mul(dim)
            .ok_or_else(|| "CUDA RoPE backward cache length overflow".to_string())?;
        if cos.len() != cache_len || sin.len() != cache_len {
            return Err(format!(
                "CUDA RoPE backward cache length mismatch: expected {}, got cos={}, sin={}",
                cache_len,
                cos.len(),
                sin.len()
            ));
        }
        let out = alloc_f32(expected_len)?;
        let status = unsafe {
            lumen_cuda_rope_backward_f32_device(
                grad.handle(),
                cos.handle(),
                sin.handle(),
                out.handle(),
                batch_size,
                num_heads,
                seq_len,
                dim,
                offset,
                cache_seq_len,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_f32(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        bias: Option<&CudaBuffer>,
        batch_size: usize,
        in_channels: usize,
        in_h: usize,
        in_w: usize,
        out_channels: usize,
        k_h: usize,
        k_w: usize,
        pad_h: usize,
        pad_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
        let (out, out_h, out_w) = conv2d_f32_buffer(
            input,
            weight,
            bias,
            batch_size,
            in_channels,
            in_h,
            in_w,
            out_channels,
            k_h,
            k_w,
            pad_h,
            pad_w,
            stride_h,
            stride_w,
        )?;
        let host = download_f32(&out)?;
        Ok((out, host, out_h, out_w))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_f32_buffer(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        bias: Option<&CudaBuffer>,
        batch_size: usize,
        in_channels: usize,
        in_h: usize,
        in_w: usize,
        out_channels: usize,
        k_h: usize,
        k_w: usize,
        pad_h: usize,
        pad_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<(CudaBuffer, usize, usize), String> {
        if input.len() != batch_size * in_channels * in_h * in_w {
            return Err(format!(
                "CUDA conv2d input length mismatch: expected {}, got {}",
                batch_size * in_channels * in_h * in_w,
                input.len()
            ));
        }
        if weight.len() != out_channels * in_channels * k_h * k_w {
            return Err(format!(
                "CUDA conv2d weight length mismatch: expected {}, got {}",
                out_channels * in_channels * k_h * k_w,
                weight.len()
            ));
        }
        if let Some(bias) = bias {
            if bias.len() != out_channels {
                return Err(format!(
                    "CUDA conv2d bias length mismatch: expected {}, got {}",
                    out_channels,
                    bias.len()
                ));
            }
        }
        if in_h + 2 * pad_h < k_h || in_w + 2 * pad_w < k_w {
            return Err("CUDA conv2d kernel is larger than the padded input".to_string());
        }
        let out_h = (in_h + 2 * pad_h - k_h) / stride_h + 1;
        let out_w = (in_w + 2 * pad_w - k_w) / stride_w + 1;
        let out = alloc_f32(batch_size * out_channels * out_h * out_w)?;
        let bias_handle = bias.map(|buf| buf.handle()).unwrap_or(0);
        let status = unsafe {
            lumen_cuda_conv2d_f32_device(
                input.handle(),
                weight.handle(),
                bias_handle,
                out.handle(),
                batch_size,
                in_channels,
                in_h,
                in_w,
                out_channels,
                k_h,
                k_w,
                pad_h,
                pad_w,
                stride_h,
                stride_w,
                out_h,
                out_w,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((out, out_h, out_w))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_backward_f32(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        grad_output: &CudaBuffer,
        compute_bias_grad: bool,
        batch_size: usize,
        in_channels: usize,
        in_h: usize,
        in_w: usize,
        out_channels: usize,
        k_h: usize,
        k_w: usize,
        pad_h: usize,
        pad_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<
        (
            CudaBuffer,
            Vec<f32>,
            CudaBuffer,
            Vec<f32>,
            Option<(CudaBuffer, Vec<f32>)>,
        ),
        String,
    > {
        let (grad_input, grad_weight, grad_bias) = conv2d_backward_f32_buffers(
            input,
            weight,
            grad_output,
            compute_bias_grad,
            batch_size,
            in_channels,
            in_h,
            in_w,
            out_channels,
            k_h,
            k_w,
            pad_h,
            pad_w,
            stride_h,
            stride_w,
        )?;
        let grad_input_host = download_f32(&grad_input)?;
        let grad_weight_host = download_f32(&grad_weight)?;
        let grad_bias_host = match grad_bias {
            Some(buffer) => Some((buffer.clone(), download_f32(&buffer)?)),
            None => None,
        };
        Ok((
            grad_input,
            grad_input_host,
            grad_weight,
            grad_weight_host,
            grad_bias_host,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_backward_f32_buffers(
        input: &CudaBuffer,
        weight: &CudaBuffer,
        grad_output: &CudaBuffer,
        compute_bias_grad: bool,
        batch_size: usize,
        in_channels: usize,
        in_h: usize,
        in_w: usize,
        out_channels: usize,
        k_h: usize,
        k_w: usize,
        pad_h: usize,
        pad_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<(CudaBuffer, CudaBuffer, Option<CudaBuffer>), String> {
        if in_h + 2 * pad_h < k_h || in_w + 2 * pad_w < k_w {
            return Err("CUDA conv2d backward kernel is larger than the padded input".to_string());
        }
        let out_h = (in_h + 2 * pad_h - k_h) / stride_h + 1;
        let out_w = (in_w + 2 * pad_w - k_w) / stride_w + 1;
        let input_len = batch_size * in_channels * in_h * in_w;
        let weight_len = out_channels * in_channels * k_h * k_w;
        let grad_output_len = batch_size * out_channels * out_h * out_w;
        if input.len() != input_len {
            return Err(format!(
                "CUDA conv2d backward input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        if weight.len() != weight_len {
            return Err(format!(
                "CUDA conv2d backward weight length mismatch: expected {}, got {}",
                weight_len,
                weight.len()
            ));
        }
        if grad_output.len() != grad_output_len {
            return Err(format!(
                "CUDA conv2d backward grad output length mismatch: expected {}, got {}",
                grad_output_len,
                grad_output.len()
            ));
        }
        let grad_input = alloc_f32(input_len)?;
        let grad_weight = alloc_f32(weight_len)?;
        let grad_bias = if compute_bias_grad {
            Some(alloc_f32(out_channels)?)
        } else {
            None
        };
        let status = unsafe {
            lumen_cuda_conv2d_backward_f32_device(
                input.handle(),
                weight.handle(),
                grad_output.handle(),
                grad_input.handle(),
                grad_weight.handle(),
                grad_bias.as_ref().map(|buf| buf.handle()).unwrap_or(0),
                batch_size,
                in_channels,
                in_h,
                in_w,
                out_channels,
                k_h,
                k_w,
                pad_h,
                pad_w,
                stride_h,
                stride_w,
                out_h,
                out_w,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok((grad_input, grad_weight, grad_bias))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_f32(
        input: &CudaBuffer,
        batch_size: usize,
        channels: usize,
        in_h: usize,
        in_w: usize,
        kernel_h: usize,
        kernel_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
        if kernel_h == 0 || kernel_w == 0 || stride_h == 0 || stride_w == 0 {
            return Err("CUDA max_pool2d kernel and stride must be greater than zero".to_string());
        }
        if in_h < kernel_h || in_w < kernel_w {
            return Err("CUDA max_pool2d kernel is larger than input".to_string());
        }
        let input_len = batch_size
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(in_h))
            .and_then(|v| v.checked_mul(in_w))
            .ok_or_else(|| "CUDA max_pool2d input length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA max_pool2d input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        let out_h = (in_h - kernel_h) / stride_h + 1;
        let out_w = (in_w - kernel_w) / stride_w + 1;
        let out_len = batch_size
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(out_h))
            .and_then(|v| v.checked_mul(out_w))
            .ok_or_else(|| "CUDA max_pool2d output length overflow".to_string())?;
        let out = alloc_f32(out_len)?;
        let status = unsafe {
            lumen_cuda_max_pool2d_f32_device(
                input.handle(),
                out.handle(),
                batch_size,
                channels,
                in_h,
                in_w,
                kernel_h,
                kernel_w,
                stride_h,
                stride_w,
                out_h,
                out_w,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        let host = download_f32(&out)?;
        Ok((out, host, out_h, out_w))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_backward_f32(
        input: &CudaBuffer,
        grad_output: &CudaBuffer,
        batch_size: usize,
        channels: usize,
        in_h: usize,
        in_w: usize,
        kernel_h: usize,
        kernel_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        let grad_input = max_pool2d_backward_f32_buffer(
            input,
            grad_output,
            batch_size,
            channels,
            in_h,
            in_w,
            kernel_h,
            kernel_w,
            stride_h,
            stride_w,
        )?;
        let host = download_f32(&grad_input)?;
        Ok((grad_input, host))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_backward_f32_buffer(
        input: &CudaBuffer,
        grad_output: &CudaBuffer,
        batch_size: usize,
        channels: usize,
        in_h: usize,
        in_w: usize,
        kernel_h: usize,
        kernel_w: usize,
        stride_h: usize,
        stride_w: usize,
    ) -> Result<CudaBuffer, String> {
        if kernel_h == 0 || kernel_w == 0 || stride_h == 0 || stride_w == 0 {
            return Err(
                "CUDA max_pool2d backward kernel and stride must be greater than zero".to_string(),
            );
        }
        if in_h < kernel_h || in_w < kernel_w {
            return Err("CUDA max_pool2d backward kernel is larger than input".to_string());
        }
        let out_h = (in_h - kernel_h) / stride_h + 1;
        let out_w = (in_w - kernel_w) / stride_w + 1;
        let input_len = batch_size
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(in_h))
            .and_then(|v| v.checked_mul(in_w))
            .ok_or_else(|| "CUDA max_pool2d backward input length overflow".to_string())?;
        let grad_output_len = batch_size
            .checked_mul(channels)
            .and_then(|v| v.checked_mul(out_h))
            .and_then(|v| v.checked_mul(out_w))
            .ok_or_else(|| "CUDA max_pool2d backward grad output length overflow".to_string())?;
        if input.len() != input_len {
            return Err(format!(
                "CUDA max_pool2d backward input length mismatch: expected {}, got {}",
                input_len,
                input.len()
            ));
        }
        if grad_output.len() != grad_output_len {
            return Err(format!(
                "CUDA max_pool2d backward grad output length mismatch: expected {}, got {}",
                grad_output_len,
                grad_output.len()
            ));
        }
        let grad_input = alloc_f32(input_len)?;
        let status = unsafe {
            lumen_cuda_max_pool2d_backward_f32_device(
                input.handle(),
                grad_output.handle(),
                grad_input.handle(),
                batch_size,
                channels,
                in_h,
                in_w,
                kernel_h,
                kernel_w,
                stride_h,
                stride_w,
                out_h,
                out_w,
            )
        };
        if status != 0 {
            return Err(last_error_message());
        }
        Ok(grad_input)
    }
}

#[cfg(not(feature = "cuda"))]
mod imp {
    use super::{BinaryOp, CudaBuffer, UnaryOp};

    pub fn is_available() -> bool {
        false
    }

    pub fn synchronize() -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn alloc_f32(_len: usize) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn upload_f32(_src: &[f32]) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn download_f32(_buffer: &CudaBuffer) -> Result<Vec<f32>, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn download_f32_offset(
        _buffer: &CudaBuffer,
        _offset: usize,
        _len: usize,
    ) -> Result<Vec<f32>, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn matvec_argmax_f32(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _batch_size: usize,
        _vocab_size: usize,
        _hidden_size: usize,
    ) -> Result<Vec<usize>, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn upload_f32_offset(
        _buffer: &CudaBuffer,
        _offset: usize,
        _src: &[f32],
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn copy_f32_offset(
        _dst: &CudaBuffer,
        _dst_offset: usize,
        _src: &CudaBuffer,
        _src_offset: usize,
        _len: usize,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_kv_cache_f32(
        _dst: &CudaBuffer,
        _src: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _src_seq_len: usize,
        _dst_seq_len: usize,
        _dim: usize,
        _dst_start: usize,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn append_kv_cache_pair_f32(
        _k_dst: &CudaBuffer,
        _v_dst: &CudaBuffer,
        _k_src: &CudaBuffer,
        _v_src: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _src_seq_len: usize,
        _dst_seq_len: usize,
        _dim: usize,
        _dst_start: usize,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_rope_q_append_kv_f32_buffer(
        _q_src: &CudaBuffer,
        _k_src: &CudaBuffer,
        _v_src: &CudaBuffer,
        _cos: &CudaBuffer,
        _sin: &CudaBuffer,
        _k_cache: &CudaBuffer,
        _v_cache: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _num_kv_heads: usize,
        _dim: usize,
        _dst_seq_len: usize,
        _offset: usize,
        _cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn prefill_attention_f32_buffer(
        _q: &CudaBuffer,
        _k: &CudaBuffer,
        _v: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _num_kv_heads: usize,
        _q_seq_len: usize,
        _active_seq_len: usize,
        _cache_seq_len: usize,
        _dim: usize,
        _n_rep: usize,
        _past_len: usize,
        _scale: f32,
        _is_causal: bool,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn kv_cache_prefix_f32_buffer(
        _src: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _active_seq_len: usize,
        _src_seq_len: usize,
        _dim: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn free_f32(_handle: u64, _len: usize) {}

    pub fn matmul_f32(
        _a: &CudaBuffer,
        _b: &CudaBuffer,
        _m: usize,
        _n: usize,
        _k: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn matmul_f32_no_host(
        _a: &CudaBuffer,
        _b: &CudaBuffer,
        _m: usize,
        _n: usize,
        _k: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn batch_matmul_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _batch_count: usize,
        _m: usize,
        _n: usize,
        _k: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn batch_matmul_f32_no_host(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _batch_count: usize,
        _m: usize,
        _n: usize,
        _k: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn unary_f32(_input: &CudaBuffer, _op: UnaryOp) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn unary_f32_buffer(_input: &CudaBuffer, _op: UnaryOp) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn unary_backward_f32(
        _input: &CudaBuffer,
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _op: UnaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn unary_backward_f32_buffer(
        _input: &CudaBuffer,
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _op: UnaryOp,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _op: BinaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_f32_buffer(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _op: BinaryOp,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_backward_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _grad: &CudaBuffer,
        _op: BinaryOp,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_backward_f32_buffers(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _grad: &CudaBuffer,
        _op: BinaryOp,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_broadcast_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _lhs_shape: &[usize],
        _rhs_shape: &[usize],
        _out_shape: &[usize],
        _op: BinaryOp,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_broadcast_f32_buffer(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _lhs_shape: &[usize],
        _rhs_shape: &[usize],
        _out_shape: &[usize],
        _op: BinaryOp,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_broadcast_backward_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _grad: &CudaBuffer,
        _lhs_shape: &[usize],
        _rhs_shape: &[usize],
        _out_shape: &[usize],
        _op: BinaryOp,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn binary_broadcast_backward_f32_buffers(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _grad: &CudaBuffer,
        _lhs_shape: &[usize],
        _rhs_shape: &[usize],
        _out_shape: &[usize],
        _op: BinaryOp,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn sum_f32(_input: &CudaBuffer) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fill_scalar_f32(_len: usize, _value: f32) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fill_scalar_f32_buffer(_len: usize, _value: f32) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn mse_backward_f32(
        _diff: &CudaBuffer,
        _factor: f32,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn mse_backward_f32_buffers(
        _diff: &CudaBuffer,
        _factor: f32,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cross_entropy_backward_f32(
        _softmax: &CudaBuffer,
        _target: &CudaBuffer,
        _factor: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cross_entropy_backward_f32_buffer(
        _softmax: &CudaBuffer,
        _target: &CudaBuffer,
        _factor: f32,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cross_entropy_loss_f32(
        _softmax: &CudaBuffer,
        _target: &CudaBuffer,
        _batch_size: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn sgd_update_f32(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _lr: f32,
    ) -> Result<Vec<f32>, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn sgd_update_f32_no_host(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _lr: f32,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn sgd_momentum_update_f32(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _velocity: &CudaBuffer,
        _lr: f32,
        _momentum: f32,
    ) -> Result<(Vec<f32>, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn sgd_momentum_update_f32_no_host(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _velocity: &CudaBuffer,
        _lr: f32,
        _momentum: f32,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adam_update_f32(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _exp_avg: &CudaBuffer,
        _exp_avg_sq: &CudaBuffer,
        _lr: f32,
        _beta1: f32,
        _beta2: f32,
        _bias_correction1: f32,
        _bias_correction2: f32,
        _eps: f32,
    ) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn adam_update_f32_no_host(
        _param: &CudaBuffer,
        _grad: &CudaBuffer,
        _exp_avg: &CudaBuffer,
        _exp_avg_sq: &CudaBuffer,
        _lr: f32,
        _beta1: f32,
        _beta2: f32,
        _bias_correction1: f32,
        _bias_correction2: f32,
        _eps: f32,
    ) -> Result<(), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn softmax_lastdim_f32(
        _input: &CudaBuffer,
        _outer: usize,
        _last_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn softmax_lastdim_f32_no_host(
        _input: &CudaBuffer,
        _outer: usize,
        _last_dim: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn softmax_lastdim_backward_f32(
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _outer: usize,
        _last_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn softmax_lastdim_backward_f32_buffer(
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _outer: usize,
        _last_dim: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_softmax_f32(
        _input: &CudaBuffer,
        _batch_heads: usize,
        _q_len: usize,
        _k_len: usize,
        _scale: f32,
        _is_causal: bool,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_softmax_f32_no_host(
        _input: &CudaBuffer,
        _batch_heads: usize,
        _q_len: usize,
        _k_len: usize,
        _scale: f32,
        _is_causal: bool,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_softmax_backward_f32(
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _batch_heads: usize,
        _q_len: usize,
        _k_len: usize,
        _scale: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_softmax_backward_f32_buffer(
        _output: &CudaBuffer,
        _grad: &CudaBuffer,
        _batch_heads: usize,
        _q_len: usize,
        _k_len: usize,
        _scale: f32,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_softmax_f32_with_past(
        _input: &CudaBuffer,
        _batch_heads: usize,
        _q_len: usize,
        _k_len: usize,
        _scale: f32,
        _is_causal: bool,
        _past_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn embedding_f32(
        _indices: &CudaBuffer,
        _weight: &CudaBuffer,
        _num_indices: usize,
        _vocab_size: usize,
        _embed_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn embedding_backward_f32(
        _indices: &CudaBuffer,
        _grad: &CudaBuffer,
        _num_indices: usize,
        _vocab_size: usize,
        _embed_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn embedding_backward_f32_buffer(
        _indices: &CudaBuffer,
        _grad: &CudaBuffer,
        _num_indices: usize,
        _vocab_size: usize,
        _embed_dim: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn rms_norm_f32(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _rows: usize,
        _dim: usize,
        _eps: f32,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn rms_norm_backward_f32(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _grad: &CudaBuffer,
        _rows: usize,
        _dim: usize,
        _eps: f32,
    ) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn rms_norm_backward_f32_buffers(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _grad: &CudaBuffer,
        _rows: usize,
        _dim: usize,
        _eps: f32,
    ) -> Result<(CudaBuffer, CudaBuffer), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn permute_f32(
        _input: &CudaBuffer,
        _out_shape: &[usize],
        _axes: &[usize],
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn permute_f32_buffer(
        _input: &CudaBuffer,
        _out_shape: &[usize],
        _axes: &[usize],
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn slice_lastdim_f32(
        _input: &CudaBuffer,
        _outer: usize,
        _input_last_dim: usize,
        _start: usize,
        _slice_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn slice_lastdim_f32_buffer(
        _input: &CudaBuffer,
        _outer: usize,
        _input_last_dim: usize,
        _start: usize,
        _slice_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn slice_lastdim_backward_f32(
        _grad: &CudaBuffer,
        _outer: usize,
        _input_last_dim: usize,
        _start: usize,
        _slice_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn slice_lastdim_backward_f32_buffer(
        _grad: &CudaBuffer,
        _outer: usize,
        _input_last_dim: usize,
        _start: usize,
        _slice_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cat_f32(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _out_shape: &[usize],
        _axis: usize,
        _lhs_axis_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cat_f32_buffer(
        _lhs: &CudaBuffer,
        _rhs: &CudaBuffer,
        _out_shape: &[usize],
        _axis: usize,
        _lhs_axis_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cat_backward_slice_f32(
        _grad: &CudaBuffer,
        _input_shape: &[usize],
        _out_shape: &[usize],
        _axis: usize,
        _axis_start: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn cat_backward_slice_f32_buffer(
        _grad: &CudaBuffer,
        _input_shape: &[usize],
        _out_shape: &[usize],
        _axis: usize,
        _axis_start: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn repeat_kv_f32(
        _input: &CudaBuffer,
        _batch_size: usize,
        _num_kv_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _n_rep: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn repeat_kv_f32_buffer(
        _input: &CudaBuffer,
        _batch_size: usize,
        _num_kv_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _n_rep: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn repeat_kv_backward_f32(
        _grad: &CudaBuffer,
        _batch_size: usize,
        _num_kv_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _n_rep: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn repeat_kv_backward_f32_buffer(
        _grad: &CudaBuffer,
        _batch_size: usize,
        _num_kv_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _n_rep: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn decode_attention_f32_buffer(
        _q: &CudaBuffer,
        _k: &CudaBuffer,
        _v: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _num_kv_heads: usize,
        _active_seq_len: usize,
        _cache_seq_len: usize,
        _dim: usize,
        _n_rep: usize,
        _scale: f32,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_gate_up_silu_f32(
        _input: &CudaBuffer,
        _gate: &CudaBuffer,
        _up: &CudaBuffer,
        _rows: usize,
        _n_dim: usize,
        _k_dim: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_qkv_f32(
        _input: &CudaBuffer,
        _q: &CudaBuffer,
        _k: &CudaBuffer,
        _v: &CudaBuffer,
        _rows: usize,
        _q_n: usize,
        _k_n: usize,
        _k_dim: usize,
    ) -> Result<
        (
            (CudaBuffer, Vec<f32>),
            (CudaBuffer, Vec<f32>),
            (CudaBuffer, Vec<f32>),
        ),
        String,
    > {
        Err("CUDA feature is disabled".to_string())
    }

    pub fn fused_qkv_f32_buffer(
        _input: &CudaBuffer,
        _q: &CudaBuffer,
        _k: &CudaBuffer,
        _v: &CudaBuffer,
        _rows: usize,
        _q_n: usize,
        _k_n: usize,
        _k_dim: usize,
    ) -> Result<(CudaBuffer, CudaBuffer, CudaBuffer), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_f32(
        _input: &CudaBuffer,
        _cos: &CudaBuffer,
        _sin: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _offset: usize,
        _cache_seq_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_f32_buffer(
        _input: &CudaBuffer,
        _cos: &CudaBuffer,
        _sin: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _offset: usize,
        _cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_backward_f32(
        _grad: &CudaBuffer,
        _cos: &CudaBuffer,
        _sin: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _offset: usize,
        _cache_seq_len: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn rope_backward_f32_buffer(
        _grad: &CudaBuffer,
        _cos: &CudaBuffer,
        _sin: &CudaBuffer,
        _batch_size: usize,
        _num_heads: usize,
        _seq_len: usize,
        _dim: usize,
        _offset: usize,
        _cache_seq_len: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_f32(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _bias: Option<&CudaBuffer>,
        _batch_size: usize,
        _in_channels: usize,
        _in_h: usize,
        _in_w: usize,
        _out_channels: usize,
        _k_h: usize,
        _k_w: usize,
        _pad_h: usize,
        _pad_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_f32_buffer(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _bias: Option<&CudaBuffer>,
        _batch_size: usize,
        _in_channels: usize,
        _in_h: usize,
        _in_w: usize,
        _out_channels: usize,
        _k_h: usize,
        _k_w: usize,
        _pad_h: usize,
        _pad_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<(CudaBuffer, usize, usize), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_backward_f32(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _grad_output: &CudaBuffer,
        _compute_bias_grad: bool,
        _batch_size: usize,
        _in_channels: usize,
        _in_h: usize,
        _in_w: usize,
        _out_channels: usize,
        _k_h: usize,
        _k_w: usize,
        _pad_h: usize,
        _pad_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<
        (
            CudaBuffer,
            Vec<f32>,
            CudaBuffer,
            Vec<f32>,
            Option<(CudaBuffer, Vec<f32>)>,
        ),
        String,
    > {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn conv2d_backward_f32_buffers(
        _input: &CudaBuffer,
        _weight: &CudaBuffer,
        _grad_output: &CudaBuffer,
        _compute_bias_grad: bool,
        _batch_size: usize,
        _in_channels: usize,
        _in_h: usize,
        _in_w: usize,
        _out_channels: usize,
        _k_h: usize,
        _k_w: usize,
        _pad_h: usize,
        _pad_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<(CudaBuffer, CudaBuffer, Option<CudaBuffer>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_f32(
        _input: &CudaBuffer,
        _batch_size: usize,
        _channels: usize,
        _in_h: usize,
        _in_w: usize,
        _kernel_h: usize,
        _kernel_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_backward_f32(
        _input: &CudaBuffer,
        _grad_output: &CudaBuffer,
        _batch_size: usize,
        _channels: usize,
        _in_h: usize,
        _in_w: usize,
        _kernel_h: usize,
        _kernel_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<(CudaBuffer, Vec<f32>), String> {
        Err("CUDA feature is disabled".to_string())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn max_pool2d_backward_f32_buffer(
        _input: &CudaBuffer,
        _grad_output: &CudaBuffer,
        _batch_size: usize,
        _channels: usize,
        _in_h: usize,
        _in_w: usize,
        _kernel_h: usize,
        _kernel_w: usize,
        _stride_h: usize,
        _stride_w: usize,
    ) -> Result<CudaBuffer, String> {
        Err("CUDA feature is disabled".to_string())
    }
}

pub fn is_available() -> bool {
    imp::is_available()
}

pub fn synchronize() -> Result<(), String> {
    imp::synchronize()
}

pub fn alloc_f32(len: usize) -> Result<CudaBuffer, String> {
    imp::alloc_f32(len)
}

pub fn upload_f32(src: &[f32]) -> Result<CudaBuffer, String> {
    imp::upload_f32(src)
}

pub fn download_f32(buffer: &CudaBuffer) -> Result<Vec<f32>, String> {
    imp::download_f32(buffer)
}

pub fn download_f32_offset(
    buffer: &CudaBuffer,
    offset: usize,
    len: usize,
) -> Result<Vec<f32>, String> {
    imp::download_f32_offset(buffer, offset, len)
}

pub fn matvec_argmax_f32(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    batch_size: usize,
    vocab_size: usize,
    hidden_size: usize,
) -> Result<Vec<usize>, String> {
    imp::matvec_argmax_f32(input, weight, batch_size, vocab_size, hidden_size)
}

pub fn upload_f32_offset(buffer: &CudaBuffer, offset: usize, src: &[f32]) -> Result<(), String> {
    imp::upload_f32_offset(buffer, offset, src)
}

pub fn copy_f32_offset(
    dst: &CudaBuffer,
    dst_offset: usize,
    src: &CudaBuffer,
    src_offset: usize,
    len: usize,
) -> Result<(), String> {
    imp::copy_f32_offset(dst, dst_offset, src, src_offset, len)
}

#[allow(clippy::too_many_arguments)]
pub fn append_kv_cache_f32(
    dst: &CudaBuffer,
    src: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    src_seq_len: usize,
    dst_seq_len: usize,
    dim: usize,
    dst_start: usize,
) -> Result<(), String> {
    imp::append_kv_cache_f32(
        dst,
        src,
        batch_size,
        num_heads,
        src_seq_len,
        dst_seq_len,
        dim,
        dst_start,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn append_kv_cache_pair_f32(
    k_dst: &CudaBuffer,
    v_dst: &CudaBuffer,
    k_src: &CudaBuffer,
    v_src: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    src_seq_len: usize,
    dst_seq_len: usize,
    dim: usize,
    dst_start: usize,
) -> Result<(), String> {
    imp::append_kv_cache_pair_f32(
        k_dst,
        v_dst,
        k_src,
        v_src,
        batch_size,
        num_heads,
        src_seq_len,
        dst_seq_len,
        dim,
        dst_start,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decode_rope_q_append_kv_f32_buffer(
    q_src: &CudaBuffer,
    k_src: &CudaBuffer,
    v_src: &CudaBuffer,
    cos: &CudaBuffer,
    sin: &CudaBuffer,
    k_cache: &CudaBuffer,
    v_cache: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    num_kv_heads: usize,
    dim: usize,
    dst_seq_len: usize,
    offset: usize,
    cache_seq_len: usize,
) -> Result<CudaBuffer, String> {
    imp::decode_rope_q_append_kv_f32_buffer(
        q_src,
        k_src,
        v_src,
        cos,
        sin,
        k_cache,
        v_cache,
        batch_size,
        num_heads,
        num_kv_heads,
        dim,
        dst_seq_len,
        offset,
        cache_seq_len,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn prefill_attention_f32_buffer(
    q: &CudaBuffer,
    k: &CudaBuffer,
    v: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    num_kv_heads: usize,
    q_seq_len: usize,
    active_seq_len: usize,
    cache_seq_len: usize,
    dim: usize,
    n_rep: usize,
    past_len: usize,
    scale: f32,
    is_causal: bool,
) -> Result<CudaBuffer, String> {
    imp::prefill_attention_f32_buffer(
        q,
        k,
        v,
        batch_size,
        num_heads,
        num_kv_heads,
        q_seq_len,
        active_seq_len,
        cache_seq_len,
        dim,
        n_rep,
        past_len,
        scale,
        is_causal,
    )
}

pub fn kv_cache_prefix_f32_buffer(
    src: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    active_seq_len: usize,
    src_seq_len: usize,
    dim: usize,
) -> Result<CudaBuffer, String> {
    imp::kv_cache_prefix_f32_buffer(src, batch_size, num_heads, active_seq_len, src_seq_len, dim)
}

pub fn matmul_f32(
    a: &CudaBuffer,
    b: &CudaBuffer,
    m: usize,
    n: usize,
    k: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::matmul_f32(a, b, m, n, k)
}

pub fn matmul_f32_no_host(
    a: &CudaBuffer,
    b: &CudaBuffer,
    m: usize,
    n: usize,
    k: usize,
) -> Result<CudaBuffer, String> {
    imp::matmul_f32_no_host(a, b, m, n, k)
}

pub fn batch_matmul_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    batch_count: usize,
    m: usize,
    n: usize,
    k: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::batch_matmul_f32(lhs, rhs, batch_count, m, n, k)
}

pub fn batch_matmul_f32_no_host(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    batch_count: usize,
    m: usize,
    n: usize,
    k: usize,
) -> Result<CudaBuffer, String> {
    imp::batch_matmul_f32_no_host(lhs, rhs, batch_count, m, n, k)
}

pub fn unary_f32(input: &CudaBuffer, op: UnaryOp) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::unary_f32(input, op)
}

pub fn unary_f32_buffer(input: &CudaBuffer, op: UnaryOp) -> Result<CudaBuffer, String> {
    imp::unary_f32_buffer(input, op)
}

pub fn unary_backward_f32(
    input: &CudaBuffer,
    output: &CudaBuffer,
    grad: &CudaBuffer,
    op: UnaryOp,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::unary_backward_f32(input, output, grad, op)
}

pub fn unary_backward_f32_buffer(
    input: &CudaBuffer,
    output: &CudaBuffer,
    grad: &CudaBuffer,
    op: UnaryOp,
) -> Result<CudaBuffer, String> {
    imp::unary_backward_f32_buffer(input, output, grad, op)
}

pub fn binary_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    op: BinaryOp,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::binary_f32(lhs, rhs, op)
}

pub fn binary_f32_buffer(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    op: BinaryOp,
) -> Result<CudaBuffer, String> {
    imp::binary_f32_buffer(lhs, rhs, op)
}

pub fn binary_backward_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    grad: &CudaBuffer,
    op: BinaryOp,
) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
    imp::binary_backward_f32(lhs, rhs, grad, op)
}

pub fn binary_backward_f32_buffers(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    grad: &CudaBuffer,
    op: BinaryOp,
) -> Result<(CudaBuffer, CudaBuffer), String> {
    imp::binary_backward_f32_buffers(lhs, rhs, grad, op)
}

pub fn binary_broadcast_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    lhs_shape: &[usize],
    rhs_shape: &[usize],
    out_shape: &[usize],
    op: BinaryOp,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::binary_broadcast_f32(lhs, rhs, lhs_shape, rhs_shape, out_shape, op)
}

pub fn binary_broadcast_f32_buffer(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    lhs_shape: &[usize],
    rhs_shape: &[usize],
    out_shape: &[usize],
    op: BinaryOp,
) -> Result<CudaBuffer, String> {
    imp::binary_broadcast_f32_buffer(lhs, rhs, lhs_shape, rhs_shape, out_shape, op)
}

pub fn binary_broadcast_backward_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    grad: &CudaBuffer,
    lhs_shape: &[usize],
    rhs_shape: &[usize],
    out_shape: &[usize],
    op: BinaryOp,
) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
    imp::binary_broadcast_backward_f32(lhs, rhs, grad, lhs_shape, rhs_shape, out_shape, op)
}

pub fn binary_broadcast_backward_f32_buffers(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    grad: &CudaBuffer,
    lhs_shape: &[usize],
    rhs_shape: &[usize],
    out_shape: &[usize],
    op: BinaryOp,
) -> Result<(CudaBuffer, CudaBuffer), String> {
    imp::binary_broadcast_backward_f32_buffers(lhs, rhs, grad, lhs_shape, rhs_shape, out_shape, op)
}

pub fn sum_f32(input: &CudaBuffer) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::sum_f32(input)
}

pub fn fill_scalar_f32(len: usize, value: f32) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::fill_scalar_f32(len, value)
}

pub fn fill_scalar_f32_buffer(len: usize, value: f32) -> Result<CudaBuffer, String> {
    imp::fill_scalar_f32_buffer(len, value)
}

pub fn mse_backward_f32(
    diff: &CudaBuffer,
    factor: f32,
) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
    imp::mse_backward_f32(diff, factor)
}

pub fn mse_backward_f32_buffers(
    diff: &CudaBuffer,
    factor: f32,
) -> Result<(CudaBuffer, CudaBuffer), String> {
    imp::mse_backward_f32_buffers(diff, factor)
}

pub fn cross_entropy_backward_f32(
    softmax: &CudaBuffer,
    target: &CudaBuffer,
    factor: f32,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::cross_entropy_backward_f32(softmax, target, factor)
}

pub fn cross_entropy_backward_f32_buffer(
    softmax: &CudaBuffer,
    target: &CudaBuffer,
    factor: f32,
) -> Result<CudaBuffer, String> {
    imp::cross_entropy_backward_f32_buffer(softmax, target, factor)
}

pub fn cross_entropy_loss_f32(
    softmax: &CudaBuffer,
    target: &CudaBuffer,
    batch_size: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::cross_entropy_loss_f32(softmax, target, batch_size)
}

pub fn sgd_update_f32(param: &CudaBuffer, grad: &CudaBuffer, lr: f32) -> Result<Vec<f32>, String> {
    imp::sgd_update_f32(param, grad, lr)
}

pub fn sgd_update_f32_no_host(
    param: &CudaBuffer,
    grad: &CudaBuffer,
    lr: f32,
) -> Result<(), String> {
    imp::sgd_update_f32_no_host(param, grad, lr)
}

pub fn sgd_momentum_update_f32(
    param: &CudaBuffer,
    grad: &CudaBuffer,
    velocity: &CudaBuffer,
    lr: f32,
    momentum: f32,
) -> Result<(Vec<f32>, Vec<f32>), String> {
    imp::sgd_momentum_update_f32(param, grad, velocity, lr, momentum)
}

pub fn sgd_momentum_update_f32_no_host(
    param: &CudaBuffer,
    grad: &CudaBuffer,
    velocity: &CudaBuffer,
    lr: f32,
    momentum: f32,
) -> Result<(), String> {
    imp::sgd_momentum_update_f32_no_host(param, grad, velocity, lr, momentum)
}

#[allow(clippy::too_many_arguments)]
pub fn adam_update_f32(
    param: &CudaBuffer,
    grad: &CudaBuffer,
    exp_avg: &CudaBuffer,
    exp_avg_sq: &CudaBuffer,
    lr: f32,
    beta1: f32,
    beta2: f32,
    bias_correction1: f32,
    bias_correction2: f32,
    eps: f32,
) -> Result<(Vec<f32>, Vec<f32>, Vec<f32>), String> {
    imp::adam_update_f32(
        param,
        grad,
        exp_avg,
        exp_avg_sq,
        lr,
        beta1,
        beta2,
        bias_correction1,
        bias_correction2,
        eps,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn adam_update_f32_no_host(
    param: &CudaBuffer,
    grad: &CudaBuffer,
    exp_avg: &CudaBuffer,
    exp_avg_sq: &CudaBuffer,
    lr: f32,
    beta1: f32,
    beta2: f32,
    bias_correction1: f32,
    bias_correction2: f32,
    eps: f32,
) -> Result<(), String> {
    imp::adam_update_f32_no_host(
        param,
        grad,
        exp_avg,
        exp_avg_sq,
        lr,
        beta1,
        beta2,
        bias_correction1,
        bias_correction2,
        eps,
    )
}

pub fn softmax_lastdim_f32(
    input: &CudaBuffer,
    outer: usize,
    last_dim: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::softmax_lastdim_f32(input, outer, last_dim)
}

pub fn softmax_lastdim_f32_no_host(
    input: &CudaBuffer,
    outer: usize,
    last_dim: usize,
) -> Result<CudaBuffer, String> {
    imp::softmax_lastdim_f32_no_host(input, outer, last_dim)
}

pub fn softmax_lastdim_backward_f32(
    output: &CudaBuffer,
    grad: &CudaBuffer,
    outer: usize,
    last_dim: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::softmax_lastdim_backward_f32(output, grad, outer, last_dim)
}

pub fn softmax_lastdim_backward_f32_buffer(
    output: &CudaBuffer,
    grad: &CudaBuffer,
    outer: usize,
    last_dim: usize,
) -> Result<CudaBuffer, String> {
    imp::softmax_lastdim_backward_f32_buffer(output, grad, outer, last_dim)
}

pub fn fused_softmax_f32(
    input: &CudaBuffer,
    batch_heads: usize,
    q_len: usize,
    k_len: usize,
    scale: f32,
    is_causal: bool,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::fused_softmax_f32(input, batch_heads, q_len, k_len, scale, is_causal)
}

pub fn fused_softmax_f32_no_host(
    input: &CudaBuffer,
    batch_heads: usize,
    q_len: usize,
    k_len: usize,
    scale: f32,
    is_causal: bool,
) -> Result<CudaBuffer, String> {
    imp::fused_softmax_f32_no_host(input, batch_heads, q_len, k_len, scale, is_causal)
}

pub fn fused_softmax_backward_f32(
    output: &CudaBuffer,
    grad: &CudaBuffer,
    batch_heads: usize,
    q_len: usize,
    k_len: usize,
    scale: f32,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::fused_softmax_backward_f32(output, grad, batch_heads, q_len, k_len, scale)
}

pub fn fused_softmax_backward_f32_buffer(
    output: &CudaBuffer,
    grad: &CudaBuffer,
    batch_heads: usize,
    q_len: usize,
    k_len: usize,
    scale: f32,
) -> Result<CudaBuffer, String> {
    imp::fused_softmax_backward_f32_buffer(output, grad, batch_heads, q_len, k_len, scale)
}

pub fn fused_softmax_f32_with_past(
    input: &CudaBuffer,
    batch_heads: usize,
    q_len: usize,
    k_len: usize,
    scale: f32,
    is_causal: bool,
    past_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::fused_softmax_f32_with_past(input, batch_heads, q_len, k_len, scale, is_causal, past_len)
}

pub fn embedding_f32(
    indices: &CudaBuffer,
    weight: &CudaBuffer,
    num_indices: usize,
    vocab_size: usize,
    embed_dim: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::embedding_f32(indices, weight, num_indices, vocab_size, embed_dim)
}

pub fn embedding_backward_f32(
    indices: &CudaBuffer,
    grad: &CudaBuffer,
    num_indices: usize,
    vocab_size: usize,
    embed_dim: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::embedding_backward_f32(indices, grad, num_indices, vocab_size, embed_dim)
}

pub fn embedding_backward_f32_buffer(
    indices: &CudaBuffer,
    grad: &CudaBuffer,
    num_indices: usize,
    vocab_size: usize,
    embed_dim: usize,
) -> Result<CudaBuffer, String> {
    imp::embedding_backward_f32_buffer(indices, grad, num_indices, vocab_size, embed_dim)
}

pub fn rms_norm_f32(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    rows: usize,
    dim: usize,
    eps: f32,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::rms_norm_f32(input, weight, rows, dim, eps)
}

pub fn rms_norm_backward_f32(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    grad: &CudaBuffer,
    rows: usize,
    dim: usize,
    eps: f32,
) -> Result<((CudaBuffer, Vec<f32>), (CudaBuffer, Vec<f32>)), String> {
    imp::rms_norm_backward_f32(input, weight, grad, rows, dim, eps)
}

pub fn rms_norm_backward_f32_buffers(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    grad: &CudaBuffer,
    rows: usize,
    dim: usize,
    eps: f32,
) -> Result<(CudaBuffer, CudaBuffer), String> {
    imp::rms_norm_backward_f32_buffers(input, weight, grad, rows, dim, eps)
}

pub fn permute_f32(
    input: &CudaBuffer,
    out_shape: &[usize],
    axes: &[usize],
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::permute_f32(input, out_shape, axes)
}

pub fn permute_f32_buffer(
    input: &CudaBuffer,
    out_shape: &[usize],
    axes: &[usize],
) -> Result<CudaBuffer, String> {
    imp::permute_f32_buffer(input, out_shape, axes)
}

pub fn slice_lastdim_f32(
    input: &CudaBuffer,
    outer: usize,
    input_last_dim: usize,
    start: usize,
    slice_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::slice_lastdim_f32(input, outer, input_last_dim, start, slice_len)
}

pub fn slice_lastdim_f32_buffer(
    input: &CudaBuffer,
    outer: usize,
    input_last_dim: usize,
    start: usize,
    slice_len: usize,
) -> Result<CudaBuffer, String> {
    imp::slice_lastdim_f32_buffer(input, outer, input_last_dim, start, slice_len)
}

pub fn slice_lastdim_backward_f32(
    grad: &CudaBuffer,
    outer: usize,
    input_last_dim: usize,
    start: usize,
    slice_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::slice_lastdim_backward_f32(grad, outer, input_last_dim, start, slice_len)
}

pub fn slice_lastdim_backward_f32_buffer(
    grad: &CudaBuffer,
    outer: usize,
    input_last_dim: usize,
    start: usize,
    slice_len: usize,
) -> Result<CudaBuffer, String> {
    imp::slice_lastdim_backward_f32_buffer(grad, outer, input_last_dim, start, slice_len)
}

pub fn cat_f32(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    out_shape: &[usize],
    axis: usize,
    lhs_axis_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::cat_f32(lhs, rhs, out_shape, axis, lhs_axis_len)
}

pub fn cat_f32_buffer(
    lhs: &CudaBuffer,
    rhs: &CudaBuffer,
    out_shape: &[usize],
    axis: usize,
    lhs_axis_len: usize,
) -> Result<CudaBuffer, String> {
    imp::cat_f32_buffer(lhs, rhs, out_shape, axis, lhs_axis_len)
}

pub fn cat_backward_slice_f32(
    grad: &CudaBuffer,
    input_shape: &[usize],
    out_shape: &[usize],
    axis: usize,
    axis_start: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::cat_backward_slice_f32(grad, input_shape, out_shape, axis, axis_start)
}

pub fn cat_backward_slice_f32_buffer(
    grad: &CudaBuffer,
    input_shape: &[usize],
    out_shape: &[usize],
    axis: usize,
    axis_start: usize,
) -> Result<CudaBuffer, String> {
    imp::cat_backward_slice_f32_buffer(grad, input_shape, out_shape, axis, axis_start)
}

pub fn repeat_kv_f32(
    input: &CudaBuffer,
    batch_size: usize,
    num_kv_heads: usize,
    seq_len: usize,
    dim: usize,
    n_rep: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::repeat_kv_f32(input, batch_size, num_kv_heads, seq_len, dim, n_rep)
}

pub fn repeat_kv_f32_buffer(
    input: &CudaBuffer,
    batch_size: usize,
    num_kv_heads: usize,
    seq_len: usize,
    dim: usize,
    n_rep: usize,
) -> Result<CudaBuffer, String> {
    imp::repeat_kv_f32_buffer(input, batch_size, num_kv_heads, seq_len, dim, n_rep)
}

pub fn repeat_kv_backward_f32(
    grad: &CudaBuffer,
    batch_size: usize,
    num_kv_heads: usize,
    seq_len: usize,
    dim: usize,
    n_rep: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::repeat_kv_backward_f32(grad, batch_size, num_kv_heads, seq_len, dim, n_rep)
}

pub fn repeat_kv_backward_f32_buffer(
    grad: &CudaBuffer,
    batch_size: usize,
    num_kv_heads: usize,
    seq_len: usize,
    dim: usize,
    n_rep: usize,
) -> Result<CudaBuffer, String> {
    imp::repeat_kv_backward_f32_buffer(grad, batch_size, num_kv_heads, seq_len, dim, n_rep)
}

#[allow(clippy::too_many_arguments)]
pub fn decode_attention_f32(
    q: &CudaBuffer,
    k: &CudaBuffer,
    v: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    num_kv_heads: usize,
    active_seq_len: usize,
    cache_seq_len: usize,
    dim: usize,
    n_rep: usize,
    scale: f32,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::decode_attention_f32(
        q,
        k,
        v,
        batch_size,
        num_heads,
        num_kv_heads,
        active_seq_len,
        cache_seq_len,
        dim,
        n_rep,
        scale,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn decode_attention_f32_buffer(
    q: &CudaBuffer,
    k: &CudaBuffer,
    v: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    num_kv_heads: usize,
    active_seq_len: usize,
    cache_seq_len: usize,
    dim: usize,
    n_rep: usize,
    scale: f32,
) -> Result<CudaBuffer, String> {
    imp::decode_attention_f32_buffer(
        q,
        k,
        v,
        batch_size,
        num_heads,
        num_kv_heads,
        active_seq_len,
        cache_seq_len,
        dim,
        n_rep,
        scale,
    )
}

pub fn fused_gate_up_silu_f32(
    input: &CudaBuffer,
    gate: &CudaBuffer,
    up: &CudaBuffer,
    rows: usize,
    n_dim: usize,
    k_dim: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::fused_gate_up_silu_f32(input, gate, up, rows, n_dim, k_dim)
}

pub fn fused_qkv_f32(
    input: &CudaBuffer,
    q: &CudaBuffer,
    k: &CudaBuffer,
    v: &CudaBuffer,
    rows: usize,
    q_n: usize,
    k_n: usize,
    k_dim: usize,
) -> Result<
    (
        (CudaBuffer, Vec<f32>),
        (CudaBuffer, Vec<f32>),
        (CudaBuffer, Vec<f32>),
    ),
    String,
> {
    imp::fused_qkv_f32(input, q, k, v, rows, q_n, k_n, k_dim)
}

pub fn fused_qkv_f32_buffer(
    input: &CudaBuffer,
    q: &CudaBuffer,
    k: &CudaBuffer,
    v: &CudaBuffer,
    rows: usize,
    q_n: usize,
    k_n: usize,
    k_dim: usize,
) -> Result<(CudaBuffer, CudaBuffer, CudaBuffer), String> {
    imp::fused_qkv_f32_buffer(input, q, k, v, rows, q_n, k_n, k_dim)
}

#[allow(clippy::too_many_arguments)]
pub fn rope_f32(
    input: &CudaBuffer,
    cos: &CudaBuffer,
    sin: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    seq_len: usize,
    dim: usize,
    offset: usize,
    cache_seq_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::rope_f32(
        input,
        cos,
        sin,
        batch_size,
        num_heads,
        seq_len,
        dim,
        offset,
        cache_seq_len,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn rope_f32_buffer(
    input: &CudaBuffer,
    cos: &CudaBuffer,
    sin: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    seq_len: usize,
    dim: usize,
    offset: usize,
    cache_seq_len: usize,
) -> Result<CudaBuffer, String> {
    imp::rope_f32_buffer(
        input,
        cos,
        sin,
        batch_size,
        num_heads,
        seq_len,
        dim,
        offset,
        cache_seq_len,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn rope_backward_f32(
    grad: &CudaBuffer,
    cos: &CudaBuffer,
    sin: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    seq_len: usize,
    dim: usize,
    offset: usize,
    cache_seq_len: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::rope_backward_f32(
        grad,
        cos,
        sin,
        batch_size,
        num_heads,
        seq_len,
        dim,
        offset,
        cache_seq_len,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn rope_backward_f32_buffer(
    grad: &CudaBuffer,
    cos: &CudaBuffer,
    sin: &CudaBuffer,
    batch_size: usize,
    num_heads: usize,
    seq_len: usize,
    dim: usize,
    offset: usize,
    cache_seq_len: usize,
) -> Result<CudaBuffer, String> {
    imp::rope_backward_f32_buffer(
        grad,
        cos,
        sin,
        batch_size,
        num_heads,
        seq_len,
        dim,
        offset,
        cache_seq_len,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn conv2d_f32(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    bias: Option<&CudaBuffer>,
    batch_size: usize,
    in_channels: usize,
    in_h: usize,
    in_w: usize,
    out_channels: usize,
    k_h: usize,
    k_w: usize,
    pad_h: usize,
    pad_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
    imp::conv2d_f32(
        input,
        weight,
        bias,
        batch_size,
        in_channels,
        in_h,
        in_w,
        out_channels,
        k_h,
        k_w,
        pad_h,
        pad_w,
        stride_h,
        stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn conv2d_f32_buffer(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    bias: Option<&CudaBuffer>,
    batch_size: usize,
    in_channels: usize,
    in_h: usize,
    in_w: usize,
    out_channels: usize,
    k_h: usize,
    k_w: usize,
    pad_h: usize,
    pad_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<(CudaBuffer, usize, usize), String> {
    imp::conv2d_f32_buffer(
        input,
        weight,
        bias,
        batch_size,
        in_channels,
        in_h,
        in_w,
        out_channels,
        k_h,
        k_w,
        pad_h,
        pad_w,
        stride_h,
        stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn conv2d_backward_f32(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    grad_output: &CudaBuffer,
    compute_bias_grad: bool,
    batch_size: usize,
    in_channels: usize,
    in_h: usize,
    in_w: usize,
    out_channels: usize,
    k_h: usize,
    k_w: usize,
    pad_h: usize,
    pad_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<
    (
        CudaBuffer,
        Vec<f32>,
        CudaBuffer,
        Vec<f32>,
        Option<(CudaBuffer, Vec<f32>)>,
    ),
    String,
> {
    imp::conv2d_backward_f32(
        input,
        weight,
        grad_output,
        compute_bias_grad,
        batch_size,
        in_channels,
        in_h,
        in_w,
        out_channels,
        k_h,
        k_w,
        pad_h,
        pad_w,
        stride_h,
        stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn conv2d_backward_f32_buffers(
    input: &CudaBuffer,
    weight: &CudaBuffer,
    grad_output: &CudaBuffer,
    compute_bias_grad: bool,
    batch_size: usize,
    in_channels: usize,
    in_h: usize,
    in_w: usize,
    out_channels: usize,
    k_h: usize,
    k_w: usize,
    pad_h: usize,
    pad_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<(CudaBuffer, CudaBuffer, Option<CudaBuffer>), String> {
    imp::conv2d_backward_f32_buffers(
        input,
        weight,
        grad_output,
        compute_bias_grad,
        batch_size,
        in_channels,
        in_h,
        in_w,
        out_channels,
        k_h,
        k_w,
        pad_h,
        pad_w,
        stride_h,
        stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn max_pool2d_f32(
    input: &CudaBuffer,
    batch_size: usize,
    channels: usize,
    in_h: usize,
    in_w: usize,
    kernel_h: usize,
    kernel_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<(CudaBuffer, Vec<f32>, usize, usize), String> {
    imp::max_pool2d_f32(
        input, batch_size, channels, in_h, in_w, kernel_h, kernel_w, stride_h, stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn max_pool2d_backward_f32(
    input: &CudaBuffer,
    grad_output: &CudaBuffer,
    batch_size: usize,
    channels: usize,
    in_h: usize,
    in_w: usize,
    kernel_h: usize,
    kernel_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<(CudaBuffer, Vec<f32>), String> {
    imp::max_pool2d_backward_f32(
        input,
        grad_output,
        batch_size,
        channels,
        in_h,
        in_w,
        kernel_h,
        kernel_w,
        stride_h,
        stride_w,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn max_pool2d_backward_f32_buffer(
    input: &CudaBuffer,
    grad_output: &CudaBuffer,
    batch_size: usize,
    channels: usize,
    in_h: usize,
    in_w: usize,
    kernel_h: usize,
    kernel_w: usize,
    stride_h: usize,
    stride_w: usize,
) -> Result<CudaBuffer, String> {
    imp::max_pool2d_backward_f32_buffer(
        input,
        grad_output,
        batch_size,
        channels,
        in_h,
        in_w,
        kernel_h,
        kernel_w,
        stride_h,
        stride_w,
    )
}
