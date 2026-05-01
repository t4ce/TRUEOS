use crate::autograd::{
    Device, StoragePreference, Tensor, TensorData, TensorStorageOwned, TensorStorageView,
    assert_native_device_support, is_no_grad, is_strict_device_execution,
};
use crate::layers::Linear;
use crate::layers::attention::encoding::RotaryEmbedding;
use crate::module::Module;
use crate::ops::cuda;
use crate::ops::fused::{
    fused_qkv_decode_infer_into, fused_qkv_decode_infer_tensors, fused_qkv_prefill_infer_tensors,
    fused_softmax, fused_softmax_with_past_infer,
};
use crate::ops::matmul::{batch_matmul, dot_unrolled};
use crate::ops::shape::{permute, reshape, slice_last_dim};
use crate::precision::{DType, default_activation_dtype, default_kv_cache_dtype};

use ndarray::linalg::general_mat_mul;
use ndarray::{Array, Array4, IxDyn};
use rayon::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

thread_local! {
    // attention scores buffer: S * L
    static ATT_SCORES_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    // attention ctx buffer: S * D
    static ATT_CTX_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    // decode(S=1) q RoPE buffer: D
    static ATT_Q_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    // decode(S=1) full attention output buffer: H * D
    static ATT_OUT_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    // decode(S=1) fused projection scratch buffers
    static ATT_QPROJ_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    static ATT_KPROJ_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
    static ATT_VPROJ_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
}

fn with_attention_work_buffers<R>(
    scores_len: usize,
    ctx_len: usize,
    f: impl FnOnce(&mut [f32], &mut [f32]) -> R,
) -> R {
    ATT_SCORES_BUF.with(|sb| {
        ATT_CTX_BUF.with(|cb| match (sb.try_borrow_mut(), cb.try_borrow_mut()) {
            (Ok(mut scores_buf), Ok(mut ctx_buf)) => {
                if scores_buf.len() < scores_len {
                    scores_buf.resize(scores_len, 0.0);
                }
                if ctx_buf.len() < ctx_len {
                    ctx_buf.resize(ctx_len, 0.0);
                }
                f(&mut scores_buf[..scores_len], &mut ctx_buf[..ctx_len])
            }
            _ => {
                let mut scores_buf = vec![0.0f32; scores_len];
                let mut ctx_buf = vec![0.0f32; ctx_len];
                f(&mut scores_buf, &mut ctx_buf)
            }
        })
    })
}

fn with_cache_f32_views<R>(
    k: &Tensor,
    v: &Tensor,
    f: impl FnOnce(ndarray::ArrayView4<'_, f32>, ndarray::ArrayView4<'_, f32>) -> R,
) -> R {
    k.with_storage_view_preferring(StoragePreference::F32Compute, |k_view| {
        v.with_storage_view_preferring(StoragePreference::F32Compute, |v_view| {
            let k4 = match k_view {
                TensorStorageView::F32(view) => view.into_dimensionality::<ndarray::Ix4>().unwrap(),
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
            };
            let v4 = match v_view {
                TensorStorageView::F32(view) => view.into_dimensionality::<ndarray::Ix4>().unwrap(),
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute preference should expose f32 view")
                }
            };
            f(k4, v4)
        })
    })
}

fn cache_prefix_tensor(cache: &Tensor, active_len: usize) -> Tensor {
    let shape = cache.shape_vec();
    assert_eq!(shape.len(), 4, "KV cache tensor must be [B,H,S,D]");
    let cache_len = shape[2];
    assert!(
        active_len <= cache_len,
        "KV cache prefix length out of bounds: {} > {}",
        active_len,
        cache_len
    );
    if active_len == cache_len {
        return cache.clone();
    }
    if cache.device() == Device::Cuda && cache.dtype() == DType::F32 && active_len > 0 {
        if let Some(buffer) = cache.cloned_cuda_f32_buffer() {
            match cuda::kv_cache_prefix_f32_buffer(
                &buffer, shape[0], shape[1], active_len, cache_len, shape[3],
            ) {
                Ok(prefix) => {
                    return Tensor::from_cuda_f32_buffer_no_host(
                        &[shape[0], shape[1], active_len, shape[3]],
                        prefix,
                        Device::Cuda,
                    );
                }
                Err(err) => {
                    assert!(
                        !is_strict_device_execution(),
                        "CUDA KV cache prefix failed in strict device execution mode: {err}"
                    );
                }
            }
        }
    }
    let transposed = permute(cache, vec![0, 1, 3, 2]);
    let sliced = slice_last_dim(&transposed, 0, active_len);
    permute(&sliced, vec![0, 1, 3, 2])
}

fn eval_attention_context_bshd(
    q_rot: &Tensor,
    k_cache: &Tensor,
    v_cache: &Tensor,
    total_len: usize,
    scale: f32,
    causal: bool,
    n_rep: usize,
    past_len: usize,
) -> Array4<f32> {
    with_cache_f32_views(k_cache, v_cache, |k4_full, v4_full| {
        let k4 = k4_full.slice(ndarray::s![.., .., 0..total_len, ..]);
        let v4 = v4_full.slice(ndarray::s![.., .., 0..total_len, ..]);
        q_rot.with_storage_view_preferring(StoragePreference::F32Compute, |q_view| {
            let q4 = match q_view {
                TensorStorageView::F32(view) => view.into_dimensionality::<ndarray::Ix4>().unwrap(),
                TensorStorageView::F16(_) => {
                    unreachable!("f32 compute view expected for attention")
                }
                TensorStorageView::BF16(_) => {
                    unreachable!("f32 compute view expected for attention")
                }
            };
            gqa_attention_no_repeat_bshd_view(&q4, &k4, &v4, scale, causal, n_rep, past_len)
        })
    })
}

pub struct KVCacheInner {
    pub k: Tensor,  // [B, H_kv, max_seq, D]
    pub v: Tensor,  // [B, H_kv, max_seq, D]
    pub len: usize, // 当前已写入的长度
    pub dtype: DType,
    pub follows_global_dtype: bool,
}

pub type KVCache = Rc<RefCell<KVCacheInner>>;

impl KVCacheInner {
    pub fn new(b: usize, h_kv: usize, max_seq: usize, d: usize) -> Self {
        Self::new_impl(b, h_kv, max_seq, d, default_kv_cache_dtype(), true)
    }

    fn new_impl(
        b: usize,
        h_kv: usize,
        max_seq: usize,
        d: usize,
        dtype: DType,
        follows_global_dtype: bool,
    ) -> Self {
        assert!(b > 0, "KV cache batch size must be > 0");
        assert!(h_kv > 0, "KV cache head count must be > 0");
        assert!(max_seq > 0, "KV cache max_seq must be > 0");
        assert!(d > 0, "KV cache head dim must be > 0");
        assert!(
            dtype.is_float(),
            "KV cache currently only supports floating runtime dtypes, got {:?}",
            dtype
        );
        let k = Tensor::from_array_no_grad(Array4::<f32>::zeros((b, h_kv, max_seq, d)).into_dyn());
        let v = Tensor::from_array_no_grad(Array4::<f32>::zeros((b, h_kv, max_seq, d)).into_dyn());
        k.cast_inplace(dtype);
        v.cast_inplace(dtype);
        Self {
            k,
            v,
            len: 0,
            dtype,
            follows_global_dtype,
        }
    }

    pub fn new_with_dtype(b: usize, h_kv: usize, max_seq: usize, d: usize, dtype: DType) -> Self {
        Self::new_impl(b, h_kv, max_seq, d, dtype, false)
    }

    pub fn reset(&mut self) {
        self.len = 0;
    }

    pub fn cast_inplace(&mut self, dtype: DType) {
        assert!(
            dtype.is_float(),
            "KV cache currently only supports floating runtime dtypes, got {:?}",
            dtype
        );
        self.k.cast_inplace(dtype);
        self.v.cast_inplace(dtype);
        self.dtype = dtype;
        self.follows_global_dtype = false;
    }

    pub fn device(&self) -> Device {
        let k_device = self.k.device();
        let v_device = self.v.device();
        assert_eq!(
            k_device, v_device,
            "KV cache K/V device mismatch: {:?} vs {:?}",
            k_device, v_device
        );
        k_device
    }

    pub fn to_device_inplace(&mut self, device: Device) {
        self.k.to_device_inplace(device);
        self.v.to_device_inplace(device);
    }

    fn validate_layout(
        &self,
        expected_b: usize,
        expected_h_kv: usize,
        expected_max_seq: usize,
        expected_d: usize,
        context: &str,
    ) {
        let expected_shape = vec![expected_b, expected_h_kv, expected_max_seq, expected_d];
        let k_shape = self.k.shape_vec();
        let v_shape = self.v.shape_vec();
        assert_eq!(
            k_shape.len(),
            4,
            "{} must store K as [B,H,S,D], got {:?}",
            context,
            k_shape
        );
        assert_eq!(
            v_shape.len(),
            4,
            "{} must store V as [B,H,S,D], got {:?}",
            context,
            v_shape
        );
        assert_eq!(
            k_shape, expected_shape,
            "{} shape mismatch: expected {:?}, got {:?}",
            context, expected_shape, k_shape
        );
        assert_eq!(
            v_shape, expected_shape,
            "{} value shape mismatch: expected {:?}, got {:?}",
            context, expected_shape, v_shape
        );
        assert!(
            self.dtype.is_float(),
            "{} currently only supports floating dtypes, got {:?}",
            context,
            self.dtype
        );
        assert_eq!(
            self.k.dtype(),
            self.dtype,
            "{} metadata/tensor dtype mismatch for K: {:?} vs {:?}",
            context,
            self.dtype,
            self.k.dtype()
        );
        assert_eq!(
            self.v.dtype(),
            self.dtype,
            "{} metadata/tensor dtype mismatch for V: {:?} vs {:?}",
            context,
            self.dtype,
            self.v.dtype()
        );
        assert_eq!(
            self.k.dtype(),
            self.v.dtype(),
            "{} K/V dtype mismatch: {:?} vs {:?}",
            context,
            self.k.dtype(),
            self.v.dtype()
        );
        assert_eq!(
            self.k.device(),
            self.v.device(),
            "{} K/V device mismatch: {:?} vs {:?}",
            context,
            self.k.device(),
            self.v.device()
        );
        assert!(
            self.len <= expected_max_seq,
            "{} current length out of bounds: {} > {}",
            context,
            self.len,
            expected_max_seq
        );
    }
}

pub struct SelfAttention {
    pub w_q: Linear,
    pub w_k: Linear,
    pub w_v: Linear,
    pub w_o: Linear,
    rope: RotaryEmbedding,
    n_head: usize,
    pub n_kv_head: usize,
    head_dim: usize,
    scale: f32,
    pub causal: bool,
    max_seq: usize, // 为 cache 预分配用
    activation_dtype: DType,
    kv_cache_dtype: DType,
}

impl SelfAttention {
    #[inline]
    pub fn activation_dtype(&self) -> DType {
        self.activation_dtype
    }

    #[inline]
    pub fn kv_cache_dtype(&self) -> DType {
        self.kv_cache_dtype
    }

    fn new_impl(
        embed_dim: usize,
        n_head: usize,
        n_kv_head: usize,
        max_seq_len: usize,
        rope_theta: f32,
        causal: bool,
        parameter_dtype: DType,
        activation_dtype: DType,
        kv_cache_dtype: DType,
    ) -> Self {
        assert!(embed_dim > 0, "embed_dim must be > 0");
        assert!(n_head > 0, "n_head must be > 0");
        assert!(n_kv_head > 0, "n_kv_head must be > 0");
        assert!(max_seq_len > 0, "max_seq_len must be > 0");
        assert_eq!(
            embed_dim % n_head,
            0,
            "Embed dim must be divisible by n_head"
        );
        assert_eq!(
            n_head % n_kv_head,
            0,
            "n_head must be divisible by n_kv_head"
        );
        assert!(
            kv_cache_dtype.is_float(),
            "SelfAttention KV cache dtype currently only supports floating types, got {:?}",
            kv_cache_dtype
        );

        let head_dim = embed_dim / n_head;
        let kv_dim = n_kv_head * head_dim;

        let rope =
            RotaryEmbedding::new_with_dtype(head_dim, max_seq_len, rope_theta, activation_dtype);

        Self {
            w_q: Linear::new_no_bias_with_dtype(embed_dim, embed_dim, parameter_dtype),
            w_k: Linear::new_no_bias_with_dtype(embed_dim, kv_dim, parameter_dtype),
            w_v: Linear::new_no_bias_with_dtype(embed_dim, kv_dim, parameter_dtype),
            w_o: Linear::new_no_bias_with_dtype(embed_dim, embed_dim, parameter_dtype),
            rope,
            n_head,
            n_kv_head,
            head_dim,
            scale: (head_dim as f32).sqrt().recip(),
            causal,
            max_seq: max_seq_len, // 存储正确的最大长度
            activation_dtype,
            kv_cache_dtype,
        }
    }

    pub fn new_with_runtime_dtypes(
        embed_dim: usize,
        n_head: usize,
        n_kv_head: usize,
        max_seq_len: usize,
        rope_theta: f32,
        causal: bool,
        parameter_dtype: DType,
        activation_dtype: DType,
        kv_cache_dtype: DType,
    ) -> Self {
        Self::new_impl(
            embed_dim,
            n_head,
            n_kv_head,
            max_seq_len,
            rope_theta,
            causal,
            parameter_dtype,
            activation_dtype,
            kv_cache_dtype,
        )
    }

    pub fn new_with_dtypes(
        embed_dim: usize,
        n_head: usize,
        n_kv_head: usize,
        max_seq_len: usize,
        rope_theta: f32,
        causal: bool,
        parameter_dtype: DType,
        runtime_dtype: DType,
    ) -> Self {
        Self::new_with_runtime_dtypes(
            embed_dim,
            n_head,
            n_kv_head,
            max_seq_len,
            rope_theta,
            causal,
            parameter_dtype,
            runtime_dtype,
            runtime_dtype,
        )
    }

    pub fn new(
        embed_dim: usize,
        n_head: usize,
        n_kv_head: usize,
        max_seq_len: usize,
        rope_theta: f32,
        causal: bool,
    ) -> Self {
        assert!(embed_dim > 0, "embed_dim must be > 0");
        assert!(n_head > 0, "n_head must be > 0");
        assert!(n_kv_head > 0, "n_kv_head must be > 0");
        assert!(max_seq_len > 0, "max_seq_len must be > 0");
        assert_eq!(
            embed_dim % n_head,
            0,
            "Embed dim must be divisible by n_head"
        );
        assert_eq!(
            n_head % n_kv_head,
            0,
            "n_head must be divisible by n_kv_head"
        );

        let activation_dtype = default_activation_dtype();
        let kv_cache_dtype = default_kv_cache_dtype();
        assert!(
            kv_cache_dtype.is_float(),
            "SelfAttention KV cache dtype currently only supports floating types, got {:?}",
            kv_cache_dtype
        );

        let head_dim = embed_dim / n_head;
        let kv_dim = n_kv_head * head_dim;
        let rope =
            RotaryEmbedding::new_with_dtype(head_dim, max_seq_len, rope_theta, activation_dtype);

        Self {
            w_q: Linear::new_no_bias(embed_dim, embed_dim),
            w_k: Linear::new_no_bias(embed_dim, kv_dim),
            w_v: Linear::new_no_bias(embed_dim, kv_dim),
            w_o: Linear::new_no_bias(embed_dim, embed_dim),
            rope,
            n_head,
            n_kv_head,
            head_dim,
            scale: (head_dim as f32).sqrt().recip(),
            causal,
            max_seq: max_seq_len,
            activation_dtype,
            kv_cache_dtype,
        }
    }

    pub fn new_with_dtype(
        embed_dim: usize,
        n_head: usize,
        n_kv_head: usize,
        max_seq_len: usize,
        rope_theta: f32,
        causal: bool,
        dtype: DType,
    ) -> Self {
        Self::new_impl(
            embed_dim,
            n_head,
            n_kv_head,
            max_seq_len,
            rope_theta,
            causal,
            dtype,
            dtype,
            dtype,
        )
    }

    pub(crate) fn assert_cache_compatible(
        &self,
        cache: &KVCache,
        batch_size: usize,
        context: &str,
    ) {
        cache.borrow().validate_layout(
            batch_size,
            self.n_kv_head,
            self.max_seq,
            self.head_dim,
            context,
        );
    }

    // forward：eval 用预分配 cache；train 走原逻辑（cat + repeat_kv）
    pub fn forward(&self, x: Tensor, cache: Option<KVCache>) -> (Tensor, Option<KVCache>) {
        let x_shape = x.shape_vec();
        assert_eq!(x_shape.len(), 3, "attention input must be [B,S,H]");
        let (b, s, _) = (x_shape[0], x_shape[1], x_shape[2]);
        let x_dtype = x.dtype();
        assert!(b > 0, "attention input batch size must be > 0");
        assert!(s > 0, "attention input sequence length must be > 0");
        assert!(self.n_kv_head > 0, "n_kv_head must be > 0");

        let h = self.n_head;
        let h_kv = self.n_kv_head;
        let d = self.head_dim;
        assert_eq!(h % h_kv, 0, "n_head must be divisible by n_kv_head");
        assert_eq!(
            x_shape[2], self.w_q.in_features,
            "attention input hidden size mismatch"
        );
        let n_rep = h / h_kv;
        let x_is_cuda = x.is_cuda();

        // eval 路径：尽量绕开 Tensor shape ops（不产生中间 Tensor，也不触发 copy）
        if is_no_grad() {
            // ------------- decode(S=1) ultra hot-path -------------
            // Goal:
            // - Only rotate NEW token's Q/K (with offset=past_len)
            // - Write rotated K directly into KV cache (no k_rot tensor)
            // - Fuse RoPE(Q) + online-softmax attention (no scores buffer)
            // - Output BSHD layout to avoid permute copies
            let cache_handle: KVCache = match cache {
                Some(c) => c,
                None => {
                    let mut cache =
                        KVCacheInner::new_with_dtype(b, h_kv, self.max_seq, d, self.kv_cache_dtype);
                    cache.to_device_inplace(x.device());
                    Rc::new(RefCell::new(cache))
                }
            };
            self.assert_cache_compatible(&cache_handle, b, "SelfAttention KV cache");
            {
                let cache_device = cache_handle.borrow().device();
                assert_eq!(
                    cache_device,
                    x.device(),
                    "SelfAttention KV cache device mismatch: cache={:?}, input={:?}. Move the cache with to_cuda()/to_cpu() before reuse.",
                    cache_device,
                    x.device()
                );
            }

            let past_len = cache_handle.borrow().len;
            if s == 1 {
                let q_proj_len = b * h * d;
                let kv_proj_len = b * h_kv * d;
                let q_batch_stride = h * d;
                let kv_batch_stride = h_kv * d;

                if x_is_cuda {
                    let (q_proj, k_proj, v_proj) = fused_qkv_decode_infer_tensors(
                        &x,
                        &self.w_q.weight,
                        &self.w_k.weight,
                        &self.w_v.weight,
                        h,
                        h_kv,
                    );
                    let q_proj_buf = q_proj
                        .cloned_cuda_f32_buffer()
                        .expect("CUDA decode Q projection missing resident buffer");
                    let k_proj_buf = k_proj
                        .cloned_cuda_f32_buffer()
                        .expect("CUDA decode K projection missing resident buffer");
                    let v_proj_buf = v_proj
                        .cloned_cuda_f32_buffer()
                        .expect("CUDA decode V projection missing resident buffer");

                    let caches_are_f32 = {
                        let c = cache_handle.borrow();
                        c.k.dtype() == DType::F32 && c.v.dtype() == DType::F32
                    };
                    let use_fused_rope_append = caches_are_f32 || is_strict_device_execution();
                    let new_len = past_len + 1;
                    assert!(
                        new_len <= self.max_seq,
                        "KV cache overflow: new_len={} > max_seq={}",
                        new_len,
                        self.max_seq
                    );
                    let q_rot_buf = if use_fused_rope_append {
                        let mut c = cache_handle.borrow_mut();
                        let KVCacheInner { k, v, len, .. } = &mut *c;
                        let k_cache_buf = k
                            .cloned_cuda_f32_buffer()
                            .expect("CUDA K cache missing resident buffer");
                        let v_cache_buf = v
                            .cloned_cuda_f32_buffer()
                            .expect("CUDA V cache missing resident buffer");
                        let q_rot_buf = self
                            .rope
                            .decode_rope_q_append_kv_cuda(
                                &q_proj_buf,
                                &k_proj_buf,
                                &v_proj_buf,
                                &k_cache_buf,
                                &v_cache_buf,
                                b,
                                h,
                                h_kv,
                                self.max_seq,
                                past_len,
                            )
                            .unwrap_or_else(|err| {
                                panic!("CUDA decode RoPE/cache append failed: {}", err)
                            });
                        if caches_are_f32 {
                            k.replace_cuda_f32_buffer_no_host_sync(k_cache_buf);
                            v.replace_cuda_f32_buffer_no_host_sync(v_cache_buf);
                        }
                        *len = new_len;
                        q_rot_buf
                    } else {
                        let q_rot = self.rope.forward(&q_proj, past_len);
                        let k_rot = self.rope.forward(&k_proj, past_len);
                        let q_rot_buf = q_rot
                            .cloned_cuda_f32_buffer()
                            .expect("CUDA rotated Q missing resident buffer");
                        let k_rot_buf = k_rot
                            .cloned_cuda_f32_buffer()
                            .expect("CUDA rotated K missing resident buffer");
                        {
                            let mut c = cache_handle.borrow_mut();
                            let KVCacheInner { k, v, len, .. } = &mut *c;
                            for bb in 0..b {
                                for hk in 0..h_kv {
                                    let src_off = (bb * h_kv + hk) * d;
                                    k.write_f32_row_4d_from_cuda_buffer_inplace(
                                        bb, hk, past_len, &k_rot_buf, src_off, d,
                                    );
                                    v.write_f32_row_4d_from_cuda_buffer_inplace(
                                        bb,
                                        hk,
                                        past_len,
                                        &v_proj_buf,
                                        src_off,
                                        d,
                                    );
                                }
                            }
                            *len = new_len;
                        }
                        q_rot_buf
                    };

                    let total_len = past_len + 1;
                    let force_cuda = is_strict_device_execution();
                    let cuda_out = {
                        let c = cache_handle.borrow();
                        let k_buf =
                            c.k.cloned_cuda_f32_buffer()
                                .expect("CUDA K cache missing resident buffer");
                        let v_buf =
                            c.v.cloned_cuda_f32_buffer()
                                .expect("CUDA V cache missing resident buffer");
                        cuda::decode_attention_f32_buffer(
                            &q_rot_buf,
                            &k_buf,
                            &v_buf,
                            b,
                            h,
                            h_kv,
                            total_len,
                            self.max_seq,
                            d,
                            n_rep,
                            self.scale,
                        )
                    };

                    let output = match cuda_out {
                        Ok(buffer) => {
                            let hidden = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                                &[b, 1, h * d],
                                buffer,
                                Device::Cuda,
                                x.dtype(),
                            );
                            self.w_o.forward(hidden)
                        }
                        Err(err) => {
                            if force_cuda {
                                panic!(
                                    "CUDA self-attention decode attention failed in strict device execution mode: {err}"
                                );
                            }
                            let c = cache_handle.borrow();
                            let q_rot = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                                &[b, h, 1, d],
                                q_rot_buf.clone(),
                                Device::Cuda,
                                x.dtype(),
                            );
                            let context_bshd = eval_attention_context_bshd(
                                &q_rot,
                                &c.k,
                                &c.v,
                                total_len,
                                self.scale,
                                self.causal,
                                n_rep,
                                past_len,
                            );
                            let context = Tensor::from_f32_data_no_grad_with_device_dtype(
                                context_bshd.into_dyn(),
                                x.dtype(),
                                Device::Cuda,
                            );
                            let context = reshape(&context, vec![b as i32, 1, (h * d) as i32]);
                            self.w_o.forward(context)
                        }
                    };

                    return (output, Some(cache_handle));
                }

                return ATT_QPROJ_BUF.with(|qpb| {
                    ATT_KPROJ_BUF.with(|kpb| {
                        ATT_VPROJ_BUF.with(|vpb| {
                            let mut qpb = qpb.borrow_mut();
                            let mut kpb = kpb.borrow_mut();
                            let mut vpb = vpb.borrow_mut();
                            if qpb.len() < q_proj_len {
                                qpb.resize(q_proj_len, 0.0);
                            }
                            if kpb.len() < kv_proj_len {
                                kpb.resize(kv_proj_len, 0.0);
                            }
                            if vpb.len() < kv_proj_len {
                                vpb.resize(kv_proj_len, 0.0);
                            }
                            {
                                let q_out = &mut qpb[..q_proj_len];
                                let k_out = &mut kpb[..kv_proj_len];
                                let v_out = &mut vpb[..kv_proj_len];
                                fused_qkv_decode_infer_into(
                                    &x,
                                    &self.w_q.weight,
                                    &self.w_k.weight,
                                    &self.w_v.weight,
                                    q_out,
                                    k_out,
                                    v_out,
                                );
                            }
                            let q_all: &[f32] = &qpb[..q_proj_len];
                            let k_new: &[f32] = &kpb[..kv_proj_len];
                            let v_new: &[f32] = &vpb[..kv_proj_len];

                            // 1) Write NEW token into KV cache. Rotate K on-the-fly into destination.
                            {
                                let mut c = cache_handle.borrow_mut();
                                let new_len = past_len + 1;
                                assert!(
                                    new_len <= self.max_seq,
                                    "KV cache overflow: new_len={} > max_seq={}",
                                    new_len,
                                    self.max_seq
                                );

                                let KVCacheInner { k, v, len, .. } = &mut *c;
                                for bb in 0..b {
                                    for hk in 0..h_kv {
                                        let src_off = bb * kv_batch_stride + hk * d;
                                        let src_k = &k_new[src_off..src_off + d];
                                        let src_v = &v_new[src_off..src_off + d];
                                        let mut rotated_k = vec![0.0f32; d];
                                        self.rope.rope_1token_copy(src_k, &mut rotated_k, past_len);
                                        k.write_f32_row_4d_inplace(bb, hk, past_len, &rotated_k);
                                        v.write_f32_row_4d_inplace(bb, hk, past_len, src_v);
                                    }
                                }
                                *len = new_len;
                            }

                            // 2) Fused attention: RoPE(Q) on-the-fly + online softmax + weighted sum.
                            let total_len = past_len + 1;
                            let output = if x_is_cuda {
                                let mut q_rot = vec![0.0f32; q_proj_len];
                                for bb in 0..b {
                                    for hh in 0..h {
                                        let q_off = bb * q_batch_stride + hh * d;
                                        self.rope.rope_1token_copy(
                                            &q_all[q_off..q_off + d],
                                            &mut q_rot[q_off..q_off + d],
                                            past_len,
                                        );
                                    }
                                }

                                let force_cuda = is_strict_device_execution();
                                let cuda_out = {
                                    let c = cache_handle.borrow();
                                    let k_buf =
                                        c.k.cloned_cuda_f32_buffer().expect("CUDA K cache missing resident buffer");
                                    let v_buf =
                                        c.v.cloned_cuda_f32_buffer().expect("CUDA V cache missing resident buffer");
                                    let q_buf = cuda::upload_f32(&q_rot);
                                    q_buf.and_then(|q_buf| {
                                        cuda::decode_attention_f32(
                                            &q_buf,
                                            &k_buf,
                                            &v_buf,
                                            b,
                                            h,
                                            h_kv,
                                            total_len,
                                            self.max_seq,
                                            d,
                                            n_rep,
                                            self.scale,
                                        )
                                    })
                                };

                                match cuda_out {
                                    Ok((buffer, out_host)) => {
                                        let hidden =
                                            Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                                                Array::from_shape_vec(IxDyn(&[b, 1, h * d]), out_host)
                                                    .expect("CUDA decode attention output shape build failed")
                                                    .into_dyn(),
                                                DType::F32,
                                                Device::Cuda,
                                                Some(buffer),
                                            );
                                        self.w_o.forward(hidden)
                                    }
                                    Err(err) => {
                                        if force_cuda {
                                            panic!(
                                                "CUDA self-attention decode attention failed in strict device execution mode: {err}"
                                            );
                                        }
                                        ATT_OUT_BUF.with(|ob| {
                                            let c = cache_handle.borrow();
                                            with_cache_f32_views(&c.k, &c.v, |k4, v4| {
                                                let mut ob = ob.borrow_mut();
                                                let out_len = b * h * d;
                                                if ob.len() < out_len {
                                                    ob.resize(out_len, 0.0);
                                                }
                                                let out_vec = &mut ob[..out_len];
                                                let scale = self.scale;
                                                let causal = self.causal;
                                                let (cos_row_vec, sin_row_vec) =
                                                    self.rope.cos_sin_row_vec(past_len);
                                                let half_d = d / 2;

                                                out_vec.par_chunks_mut(d).enumerate().for_each(
                                                    |(row, out_row)| {
                                                        let bb = row / h;
                                                        let hh = row % h;
                                                        let hk = hh / n_rep;

                                                        ATT_Q_BUF.with(|qb| {
                                                            let mut qb = qb.borrow_mut();
                                                            if qb.len() < d {
                                                                qb.resize(d, 0.0);
                                                            }
                                                            let qbuf = &mut qb[..d];

                                                            let q_off = bb * q_batch_stride + hh * d;
                                                            let q_src = &q_all[q_off..q_off + d];
                                                            for j in 0..half_d {
                                                                let x1 = q_src[j];
                                                                let x2 = q_src[j + half_d];
                                                                let c = cos_row_vec[j];
                                                                let s_val = sin_row_vec[j];
                                                                qbuf[j] = x1 * c - x2 * s_val;
                                                                qbuf[j + half_d] = x1 * s_val + x2 * c;
                                                            }

                                                            ATT_CTX_BUF.with(|cb| {
                                                                let mut cb = cb.borrow_mut();
                                                                if cb.len() < d {
                                                                    cb.resize(d, 0.0);
                                                                }
                                                                let ctx = &mut cb[..d];
                                                                for value in ctx.iter_mut() {
                                                                    *value = 0.0;
                                                                }

                                                                let mut m = f32::NEG_INFINITY;
                                                                let mut l = 0.0f32;

                                                                for j in 0..total_len {
                                                                    if causal && j > past_len {
                                                                        break;
                                                                    }
                                                                    let k_row =
                                                                        k4.slice(ndarray::s![bb, hk, j, ..]);
                                                                    let v_row =
                                                                        v4.slice(ndarray::s![bb, hk, j, ..]);
                                                                    let score = dot_unrolled(
                                                                        qbuf,
                                                                        k_row.as_slice().expect(
                                                                            "K cache row must be contiguous",
                                                                        ),
                                                                    ) * scale;
                                                                    if score > m {
                                                                        let s = (m - score).exp();
                                                                        for i in 0..d {
                                                                            ctx[i] = ctx[i] * s + v_row[i];
                                                                        }
                                                                        l = l * s + 1.0;
                                                                        m = score;
                                                                    } else {
                                                                        let w = (score - m).exp();
                                                                        for i in 0..d {
                                                                            ctx[i] += w * v_row[i];
                                                                        }
                                                                        l += w;
                                                                    }
                                                                }

                                                                let inv = 1.0f32 / (l + 1e-9);
                                                                for i in 0..d {
                                                                    out_row[i] = ctx[i] * inv;
                                                                }
                                                            });
                                                        });
                                                    },
                                                );

                                                self.w_o
                                                    .forward_decode_rows_no_bias(&ob[..out_len], b)
                                            })
                                        })
                                    }
                                }
                            } else {
                                ATT_OUT_BUF.with(|ob| {
                                    let c = cache_handle.borrow();
                                    with_cache_f32_views(&c.k, &c.v, |k4, v4| {
                                    let mut ob = ob.borrow_mut();
                                    let out_len = b * h * d;
                                    if ob.len() < out_len {
                                        ob.resize(out_len, 0.0);
                                    }
                                    let out_vec = &mut ob[..out_len]; // [B,H,D] for S=1

                                    let scale = self.scale;
                                    let causal = self.causal;
                                    let (cos_row_vec, sin_row_vec) =
                                        self.rope.cos_sin_row_vec(past_len);
                                    let half_d = d / 2;

                                    out_vec.par_chunks_mut(d).enumerate().for_each(
                                        |(row, out_row)| {
                                            let bb = row / h;
                                            let hh = row % h;
                                            let hk = hh / n_rep;

                                            ATT_Q_BUF.with(|qb| {
                                                let mut qb = qb.borrow_mut();
                                                if qb.len() < d {
                                                    qb.resize(d, 0.0);
                                                }
                                                let qbuf = &mut qb[..d];

                                                let q_off = bb * q_batch_stride + hh * d;
                                                let q_src = &q_all[q_off..q_off + d];
                                                for j in 0..half_d {
                                                    let x1 = q_src[j];
                                                    let x2 = q_src[j + half_d];
                                                    let c = cos_row_vec[j];
                                                    let s_val = sin_row_vec[j];
                                                    qbuf[j] = x1 * c - x2 * s_val;
                                                    qbuf[j + half_d] = x1 * s_val + x2 * c;
                                                }

                                                ATT_CTX_BUF.with(|cb| {
                                                    let mut cb = cb.borrow_mut();
                                                    if cb.len() < d {
                                                        cb.resize(d, 0.0);
                                                    }
                                                    let ctx = &mut cb[..d];
                                                    for value in ctx.iter_mut() {
                                                        *value = 0.0;
                                                    }

                                                    let mut m = f32::NEG_INFINITY;
                                                    let mut l = 0.0f32;

                                                    for j in 0..total_len {
                                                        if causal && j > past_len {
                                                            break;
                                                        }
                                                        let k_row =
                                                            k4.slice(ndarray::s![bb, hk, j, ..]);
                                                        let v_row =
                                                            v4.slice(ndarray::s![bb, hk, j, ..]);
                                                        let score = dot_unrolled(
                                                            qbuf,
                                                            k_row.as_slice().expect(
                                                                "K cache row must be contiguous",
                                                            ),
                                                        ) * scale;
                                                        if score > m {
                                                            let s = (m - score).exp();
                                                            for i in 0..d {
                                                                ctx[i] = ctx[i] * s + v_row[i];
                                                            }
                                                            l = l * s + 1.0;
                                                            m = score;
                                                        } else {
                                                            let w = (score - m).exp();
                                                            for i in 0..d {
                                                                ctx[i] += w * v_row[i];
                                                            }
                                                            l += w;
                                                        }
                                                    }

                                                    let inv = 1.0f32 / (l + 1e-9);
                                                    for i in 0..d {
                                                        out_row[i] = ctx[i] * inv;
                                                    }
                                                });
                                            });
                                        },
                                    );

                                    self.w_o.forward_decode_rows_no_bias(&ob[..out_len], b)
                                })
                                })
                            };

                            return (output, Some(cache_handle));
                        })
                    })
                });
            }
            let (q, k, v) = if x_is_cuda {
                fused_qkv_prefill_infer_tensors(
                    &x,
                    &self.w_q.weight,
                    &self.w_k.weight,
                    &self.w_v.weight,
                    h,
                    h_kv,
                )
                .unwrap_or_else(|| {
                    let q = self.w_q.forward(x.clone());
                    let k = self.w_k.forward(x.clone());
                    let v = self.w_v.forward(x.clone());
                    (
                        permute(
                            &reshape(&q, vec![b as i32, s as i32, h as i32, d as i32]),
                            vec![0, 2, 1, 3],
                        ),
                        permute(
                            &reshape(&k, vec![b as i32, s as i32, h_kv as i32, d as i32]),
                            vec![0, 2, 1, 3],
                        ),
                        permute(
                            &reshape(&v, vec![b as i32, s as i32, h_kv as i32, d as i32]),
                            vec![0, 2, 1, 3],
                        ),
                    )
                })
            } else {
                let q = self.w_q.forward(x.clone());
                let k = self.w_k.forward(x.clone());
                let v = self.w_v.forward(x);
                (
                    permute(
                        &reshape(&q, vec![b as i32, s as i32, h as i32, d as i32]),
                        vec![0, 2, 1, 3],
                    ),
                    permute(
                        &reshape(&k, vec![b as i32, s as i32, h_kv as i32, d as i32]),
                        vec![0, 2, 1, 3],
                    ),
                    permute(
                        &reshape(&v, vec![b as i32, s as i32, h_kv as i32, d as i32]),
                        vec![0, 2, 1, 3],
                    ),
                )
            };

            // 3) 初始化/取出 cache（预分配）
            // (cache_handle 已在上面创建)

            // 4) RoPE：offset = past_len（S>1 prefill 路径保持原实现）
            let q_rot = self.rope.forward(&q, past_len);
            let k_rot = self.rope.forward(&k, past_len);
            let k_rot_cuda_buf = if x_is_cuda {
                Some(
                    k_rot
                        .cloned_cuda_f32_buffer()
                        .expect("CUDA rotated K missing resident buffer"),
                )
            } else {
                None
            };
            let v_cuda_buf = if x_is_cuda {
                Some(
                    v.cloned_cuda_f32_buffer()
                        .expect("CUDA V projection missing resident buffer"),
                )
            } else {
                None
            };

            // 5) 写入 cache（不 cat）
            if x_is_cuda {
                let mut c = cache_handle.borrow_mut();
                let new_len = past_len + s;
                assert!(
                    new_len <= self.max_seq,
                    "KV cache overflow: new_len={} > max_seq={}",
                    new_len,
                    self.max_seq
                );
                let k_buf = k_rot_cuda_buf
                    .as_ref()
                    .expect("CUDA rotated K missing resident buffer");
                let v_buf = v_cuda_buf
                    .as_ref()
                    .expect("CUDA V projection missing resident buffer");
                let caches_are_f32 = c.k.dtype() == DType::F32 && c.v.dtype() == DType::F32;
                if caches_are_f32 || is_strict_device_execution() {
                    let k_cache_buf =
                        c.k.cloned_cuda_f32_buffer()
                            .expect("CUDA K cache missing resident buffer");
                    let v_cache_buf =
                        c.v.cloned_cuda_f32_buffer()
                            .expect("CUDA V cache missing resident buffer");
                    cuda::append_kv_cache_f32(
                        &k_cache_buf,
                        k_buf,
                        b,
                        h_kv,
                        s,
                        self.max_seq,
                        d,
                        past_len,
                    )
                    .unwrap_or_else(|err| panic!("CUDA KV cache K append failed: {}", err));
                    cuda::append_kv_cache_f32(
                        &v_cache_buf,
                        v_buf,
                        b,
                        h_kv,
                        s,
                        self.max_seq,
                        d,
                        past_len,
                    )
                    .unwrap_or_else(|err| panic!("CUDA KV cache V append failed: {}", err));
                    if caches_are_f32 {
                        c.k.replace_cuda_f32_buffer_no_host_sync(k_cache_buf);
                        c.v.replace_cuda_f32_buffer_no_host_sync(v_cache_buf);
                    }
                } else {
                    for bb in 0..b {
                        for hk in 0..h_kv {
                            for ss in 0..s {
                                let src_off = ((bb * h_kv + hk) * s + ss) * d;
                                c.k.write_f32_row_4d_from_cuda_buffer_inplace(
                                    bb,
                                    hk,
                                    past_len + ss,
                                    k_buf,
                                    src_off,
                                    d,
                                );
                                c.v.write_f32_row_4d_from_cuda_buffer_inplace(
                                    bb,
                                    hk,
                                    past_len + ss,
                                    v_buf,
                                    src_off,
                                    d,
                                );
                            }
                        }
                    }
                }
                c.len = new_len;
            } else {
                k_rot.with_storage_view_preferring(StoragePreference::F32Compute, |k_view| {
                    v.with_storage_view_preferring(StoragePreference::F32Compute, |v_view| {
                        let k_src = match k_view {
                            TensorStorageView::F32(view) => {
                                view.into_dimensionality::<ndarray::Ix4>().unwrap()
                            }
                            TensorStorageView::F16(_) => {
                                unreachable!("f32 compute view expected for cache write")
                            }
                            TensorStorageView::BF16(_) => {
                                unreachable!("f32 compute view expected for cache write")
                            }
                        };

                        let v_src = match v_view {
                            TensorStorageView::F32(view) => {
                                view.into_dimensionality::<ndarray::Ix4>().unwrap()
                            }
                            TensorStorageView::F16(_) => {
                                unreachable!("f32 compute view expected for cache write")
                            }
                            TensorStorageView::BF16(_) => {
                                unreachable!("f32 compute view expected for cache write")
                            }
                        };

                        let mut c = cache_handle.borrow_mut();
                        let new_len = past_len + s;
                        assert!(
                            new_len <= self.max_seq,
                            "KV cache overflow: new_len={} > max_seq={}",
                            new_len,
                            self.max_seq
                        );

                        let k_rot_cuda_buf = k_rot_cuda_buf.as_ref();
                        let v_cuda_buf = v_cuda_buf.as_ref();
                        if s == 1 {
                            let d = k_src.dim().3;
                            let h_kv = k_src.dim().1;
                            for bb in 0..b {
                                for hk in 0..h_kv {
                                    let src_k = k_src.slice(ndarray::s![bb, hk, 0, ..]);
                                    let src_v = v_src.slice(ndarray::s![bb, hk, 0, ..]);
                                    let src_off = (bb * h_kv + hk) * d;
                                    if let (Some(k_buf), Some(v_buf)) = (k_rot_cuda_buf, v_cuda_buf)
                                    {
                                        c.k.write_f32_row_4d_from_cuda_source_inplace(
                                            bb,
                                            hk,
                                            past_len,
                                            src_k.as_slice().expect("src_k not contiguous"),
                                            k_buf,
                                            src_off,
                                        );
                                        c.v.write_f32_row_4d_from_cuda_source_inplace(
                                            bb,
                                            hk,
                                            past_len,
                                            src_v.as_slice().expect("src_v not contiguous"),
                                            v_buf,
                                            src_off,
                                        );
                                    } else {
                                        c.k.write_f32_row_4d_inplace(
                                            bb,
                                            hk,
                                            past_len,
                                            src_k.as_slice().expect("src_k not contiguous"),
                                        );
                                        c.v.write_f32_row_4d_inplace(
                                            bb,
                                            hk,
                                            past_len,
                                            src_v.as_slice().expect("src_v not contiguous"),
                                        );
                                    }
                                    debug_assert_eq!(src_k.len(), d);
                                    debug_assert_eq!(src_v.len(), d);
                                }
                            }
                        } else {
                            for bb in 0..b {
                                for hk in 0..h_kv {
                                    for ss in 0..s {
                                        let src_k = k_src.slice(ndarray::s![bb, hk, ss, ..]);
                                        let src_v = v_src.slice(ndarray::s![bb, hk, ss, ..]);
                                        let src_off = ((bb * h_kv + hk) * s + ss) * d;
                                        if let (Some(k_buf), Some(v_buf)) =
                                            (k_rot_cuda_buf, v_cuda_buf)
                                        {
                                            c.k.write_f32_row_4d_from_cuda_source_inplace(
                                                bb,
                                                hk,
                                                past_len + ss,
                                                src_k.as_slice().expect("src_k not contiguous"),
                                                k_buf,
                                                src_off,
                                            );
                                            c.v.write_f32_row_4d_from_cuda_source_inplace(
                                                bb,
                                                hk,
                                                past_len + ss,
                                                src_v.as_slice().expect("src_v not contiguous"),
                                                v_buf,
                                                src_off,
                                            );
                                        } else {
                                            c.k.write_f32_row_4d_inplace(
                                                bb,
                                                hk,
                                                past_len + ss,
                                                src_k.as_slice().expect("src_k not contiguous"),
                                            );
                                            c.v.write_f32_row_4d_inplace(
                                                bb,
                                                hk,
                                                past_len + ss,
                                                src_v.as_slice().expect("src_v not contiguous"),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        c.len = new_len;
                    })
                });
            }

            // 6) GQA attention（不 repeat_kv）
            // 6) GQA attention（不 repeat_kv）。为了绕开 eval 的 permute/reshape copy，
            // 这里直接产出 [B,S,H,D]（BSHD）布局，后续 reshape 到 [B,S,H*D] 可视为 view。
            let total_len = cache_handle.borrow().len;
            let output = if x_is_cuda {
                let q_buf = q_rot
                    .cloned_cuda_f32_buffer()
                    .expect("CUDA rotated Q missing resident buffer");
                let direct_context = {
                    let c = cache_handle.borrow();
                    let k_buf =
                        c.k.cloned_cuda_f32_buffer()
                            .expect("CUDA K cache missing resident buffer");
                    let v_buf =
                        c.v.cloned_cuda_f32_buffer()
                            .expect("CUDA V cache missing resident buffer");
                    cuda::prefill_attention_f32_buffer(
                        &q_buf,
                        &k_buf,
                        &v_buf,
                        b,
                        h,
                        h_kv,
                        s,
                        total_len,
                        self.max_seq,
                        d,
                        n_rep,
                        past_len,
                        self.scale,
                        self.causal,
                    )
                };
                match direct_context {
                    Ok(buffer) => {
                        let context = Tensor::from_cuda_f32_buffer_no_host_with_dtype(
                            &[b, s, h * d],
                            buffer,
                            Device::Cuda,
                            x_dtype,
                        );
                        self.w_o.forward(context)
                    }
                    Err(err) => {
                        if is_strict_device_execution() {
                            panic!(
                                "CUDA self-attention prefill attention failed in strict device execution mode: {err}"
                            );
                        }
                        let (k_cache, v_cache) = {
                            let c = cache_handle.borrow();
                            (c.k.clone(), c.v.clone())
                        };
                        let k_active = cache_prefix_tensor(&k_cache, total_len);
                        let v_active = cache_prefix_tensor(&v_cache, total_len);
                        let k_up = if n_rep > 1 {
                            repeat_kv(k_active.clone(), n_rep)
                        } else {
                            k_active.clone()
                        };
                        let v_up = if n_rep > 1 {
                            repeat_kv(v_active.clone(), n_rep)
                        } else {
                            v_active.clone()
                        };
                        let k_t = permute(&k_up, vec![0, 1, 3, 2]);
                        let scores = batch_matmul(&q_rot, &k_t);
                        let attn_probs = fused_softmax_with_past_infer(
                            &scores,
                            self.scale,
                            self.causal,
                            past_len,
                        );
                        let context = batch_matmul(&attn_probs, &v_up);
                        let context = permute(&context, vec![0, 2, 1, 3]);
                        let context = reshape(&context, vec![b as i32, s as i32, (h * d) as i32]);
                        self.w_o.forward(context)
                    }
                }
            } else {
                let c = cache_handle.borrow();
                let context_bshd = eval_attention_context_bshd(
                    &q_rot,
                    &c.k,
                    &c.v,
                    total_len,
                    self.scale,
                    self.causal,
                    n_rep,
                    past_len,
                );

                let context = if x_is_cuda {
                    Tensor::from_f32_data_no_grad_with_device_dtype(
                        context_bshd.into_dyn(),
                        DType::F32,
                        Device::Cuda,
                    )
                } else {
                    Tensor::from_data_no_grad(context_bshd.into_dyn().into_shared())
                };
                let context = reshape(&context, vec![b as i32, s as i32, (h * d) as i32]);
                self.w_o.forward(context)
            };

            return (output, Some(cache_handle));
        }
        let q = { self.w_q.forward(x.clone()) };
        let k = { self.w_k.forward(x.clone()) };
        let v = { self.w_v.forward(x) };

        // train 路径：走原来的逻辑（可导）
        let q = permute(
            &reshape(&q, vec![b as i32, s as i32, h as i32, d as i32]),
            vec![0, 2, 1, 3],
        );
        let k = permute(
            &reshape(&k, vec![b as i32, s as i32, h_kv as i32, d as i32]),
            vec![0, 2, 1, 3],
        );
        let v = permute(
            &reshape(&v, vec![b as i32, s as i32, h_kv as i32, d as i32]),
            vec![0, 2, 1, 3],
        );

        // 希望训练时禁止传 cache：
        if cache.is_some() {
            panic!("Train path does not accept eval KVCache. Use eval_mode + cache for decoding.");
        }

        // 3) RoPE（训练全序列 offset=0）
        let q_rot = self.rope.forward(&q, 0);
        let k_rot = self.rope.forward(&k, 0);

        // 4) Repeat KV heads
        let k_up = if n_rep > 1 {
            repeat_kv(k_rot.clone(), n_rep)
        } else {
            k_rot.clone()
        };

        let v_up = if n_rep > 1 {
            repeat_kv(v.clone(), n_rep)
        } else {
            v.clone()
        };

        // 5) Attention
        let k_t = permute(&k_up, vec![0, 1, 3, 2]);
        let scores = batch_matmul(&q_rot, &k_t);
        let attn_probs = fused_softmax(&scores, self.scale, self.causal);
        let context = batch_matmul(&attn_probs, &v_up);

        // 6) Output
        let context = permute(&context, vec![0, 2, 1, 3]);
        let context = reshape(&context, vec![b as i32, s as i32, (h * d) as i32]);
        let output = self.w_o.forward(context);

        // 训练路径默认不返回 cache
        (output, None)
    }
}

impl Module for SelfAttention {
    fn forward(&self, x: Tensor) -> Tensor {
        let (out, _) = self.forward(x, None);
        out
    }

    fn parameters(&self) -> Vec<Tensor> {
        let mut p = self.w_q.parameters();
        p.extend(self.w_k.parameters());
        p.extend(self.w_v.parameters());
        p.extend(self.w_o.parameters());
        p
    }

    fn to_device(&self, device: Device) {
        self.w_q.to_device(device);
        self.w_k.to_device(device);
        self.w_v.to_device(device);
        self.w_o.to_device(device);
        self.rope.to_device(device);
    }
}

// train 路径：repeat_kv（依赖 Tensor ops，可导）
// x: [B, H_kv, S, D] -> [B, H, S, D]

pub fn repeat_kv(x: Tensor, n_rep: usize) -> Tensor {
    assert!(n_rep > 0, "repeat_kv expects n_rep > 0");
    let output_device = x.device();
    let build_graph = !is_no_grad() && x.requires_grad();
    assert_native_device_support(output_device, "repeat_kv", output_device == Device::Cuda);

    let shape = x.shape_vec();
    assert_eq!(shape.len(), 4, "repeat_kv expects input [B,H_kv,S,D]");
    let (b, n_kv, s, d) = (shape[0], shape[1], shape[2], shape[3]);
    let out_shape = vec![b, n_kv * n_rep, s, d];

    if !build_graph {
        if output_device == Device::Cuda && x.len() > 0 {
            if x.dtype() == DType::F32 {
                let cuda_out = x.with_cuda_f32_buffer(|input_buf| {
                    cuda::repeat_kv_f32_buffer(input_buf, b, n_kv, s, d, n_rep)
                });
                if let Ok(buffer) = cuda_out {
                    return Tensor::from_cuda_f32_buffer_no_host(&out_shape, buffer, output_device);
                }
            }

            let cuda_out = x.with_cuda_f32_buffer(|input_buf| {
                cuda::repeat_kv_f32(input_buf, b, n_kv, s, d, n_rep)
            });
            if let Ok((buffer, out)) = cuda_out {
                if x.dtype() == DType::I8 {
                    let scale = match x.native_storage_owned() {
                        TensorStorageOwned::I8(_, scale) => scale,
                        TensorStorageOwned::F32(_)
                        | TensorStorageOwned::F16(_)
                        | TensorStorageOwned::BF16(_) => unreachable!("checked i8 dtype above"),
                    };
                    let raw = out
                        .iter()
                        .map(|&v| (v / scale).round().clamp(-127.0, 127.0) as i8)
                        .collect::<Vec<_>>();
                    let out = Array::from_shape_vec(IxDyn(&out_shape), raw)
                        .expect("CUDA repeat_kv i8 output shape build failed")
                        .into_dyn()
                        .into_shared();
                    let tensor =
                        Tensor::from_shared_i8_no_grad_with_device(out, scale, output_device);
                    tensor.set_cuda_f32_buffer_inplace(buffer);
                    return tensor;
                }

                let out = Array::from_shape_vec(IxDyn(&out_shape), out)
                    .expect("CUDA repeat_kv output shape build failed")
                    .into_dyn();
                return Tensor::from_f32_data_no_grad_with_device_dtype_and_cuda_buffer(
                    out,
                    x.dtype(),
                    output_device,
                    Some(buffer),
                );
            }
        }

        return match x.native_storage_owned() {
            TensorStorageOwned::F32(data) => {
                let expanded = data
                    .into_shape((b, n_kv, 1, s, d))
                    .expect("Failed to expand KV shape");
                let broadcasted = expanded
                    .broadcast((b, n_kv, n_rep, s, d))
                    .expect("Failed to broadcast KV");
                let res = broadcasted
                    .to_owned()
                    .into_shape((b, n_kv * n_rep, s, d))
                    .expect("Failed to flatten repeated KV heads")
                    .into_dyn()
                    .into_shared();
                Tensor::from_shared_f32_no_grad_with_device(res, output_device)
            }
            TensorStorageOwned::F16(data) => {
                let expanded = data
                    .into_shape((b, n_kv, 1, s, d))
                    .expect("Failed to expand KV shape");
                let broadcasted = expanded
                    .broadcast((b, n_kv, n_rep, s, d))
                    .expect("Failed to broadcast KV");
                let res = broadcasted
                    .to_owned()
                    .into_shape((b, n_kv * n_rep, s, d))
                    .expect("Failed to flatten repeated KV heads")
                    .into_dyn()
                    .into_shared();
                Tensor::from_shared_f16_no_grad_with_device(res, output_device)
            }
            TensorStorageOwned::BF16(data) => {
                let expanded = data
                    .into_shape((b, n_kv, 1, s, d))
                    .expect("Failed to expand KV shape");
                let broadcasted = expanded
                    .broadcast((b, n_kv, n_rep, s, d))
                    .expect("Failed to broadcast KV");
                let res = broadcasted
                    .to_owned()
                    .into_shape((b, n_kv * n_rep, s, d))
                    .expect("Failed to flatten repeated KV heads")
                    .into_dyn()
                    .into_shared();
                Tensor::from_shared_bf16_no_grad_with_device(res, output_device)
            }
            TensorStorageOwned::I8(data, scale) => {
                let expanded = data
                    .into_shape((b, n_kv, 1, s, d))
                    .expect("Failed to expand KV shape");
                let broadcasted = expanded
                    .broadcast((b, n_kv, n_rep, s, d))
                    .expect("Failed to broadcast KV");
                let res = broadcasted
                    .to_owned()
                    .into_shape((b, n_kv * n_rep, s, d))
                    .expect("Failed to flatten repeated KV heads")
                    .into_dyn()
                    .into_shared();
                Tensor::from_shared_i8_no_grad_with_device(res, scale, output_device)
            }
        };
    }

    if output_device == Device::Cuda && x.len() > 0 {
        let cuda_out = x.with_cuda_f32_buffer(|input_buf| {
            cuda::repeat_kv_f32_buffer(input_buf, b, n_kv, s, d, n_rep)
        });
        match cuda_out {
            Ok(buffer) => {
                let x_clone = x.clone();
                let output_self = Rc::new(RefCell::new(None::<Tensor>));
                let output_self_for_backward = output_self.clone();
                let tensor = Tensor(Rc::new(RefCell::new(TensorData {
                    data: Array::zeros(IxDyn(&out_shape)).into_shared(),
                    f16_data: None,
                    bf16_data: None,
                    i8_data: None,
                    cuda_f32_data: Some(buffer),
                    i8_scale: None,
                    has_f32_data: false,
                    storage_dtype: crate::precision::DType::F32,
                    cache_dirty: false,
                    is_parameter: false,
                    grad: None,
                    cuda_f32_grad: None,
                    parents: vec![x.clone()],
                    backward_op: Some(std::rc::Rc::new(move |grad| {
                        let cuda_grad = if let Some(grad_buffer) = output_self_for_backward
                            .borrow()
                            .as_ref()
                            .and_then(|output| output.cloned_cuda_f32_grad())
                            .filter(|grad_buffer| grad_buffer.len() == grad.len())
                        {
                            if is_strict_device_execution() {
                                match cuda::repeat_kv_backward_f32_buffer(
                                    &grad_buffer,
                                    b,
                                    n_kv,
                                    s,
                                    d,
                                    n_rep,
                                ) {
                                    Ok(grad_buffer) => {
                                        x_clone.add_cuda_grad_buffer_only(grad_buffer);
                                        return;
                                    }
                                    Err(err) => {
                                        panic!("CUDA repeat_kv backward failed: {err}");
                                    }
                                }
                            }
                            cuda::repeat_kv_backward_f32(&grad_buffer, b, n_kv, s, d, n_rep)
                        } else {
                            let grad_host = grad.iter().copied().collect::<Vec<_>>();
                            if is_strict_device_execution() {
                                match cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                    cuda::repeat_kv_backward_f32_buffer(
                                        &grad_buf, b, n_kv, s, d, n_rep,
                                    )
                                }) {
                                    Ok(grad_buffer) => {
                                        x_clone.add_cuda_grad_buffer_only(grad_buffer);
                                        return;
                                    }
                                    Err(err) => {
                                        panic!("CUDA repeat_kv backward failed: {err}");
                                    }
                                }
                            }
                            cuda::upload_f32(&grad_host).and_then(|grad_buf| {
                                cuda::repeat_kv_backward_f32(&grad_buf, b, n_kv, s, d, n_rep)
                            })
                        };
                        match cuda_grad {
                            Ok((grad_buffer, grad_host)) => {
                                let d_x = Array::from_shape_vec(IxDyn(&shape), grad_host)
                                    .expect("CUDA repeat_kv grad shape build failed")
                                    .into_dyn();
                                x_clone.add_grad_with_cuda_buffer(d_x, Some(grad_buffer));
                            }
                            Err(err) => {
                                panic!("CUDA repeat_kv backward failed: {err}");
                            }
                        }
                    })),
                    requires_grad: true,
                    device: output_device,
                })));
                *output_self.borrow_mut() = Some(tensor.clone());
                return tensor;
            }
            Err(err) => {
                assert!(
                    !is_strict_device_execution(),
                    "repeat_kv CUDA forward failed while strict device execution is enabled: {err}"
                );
            }
        }
    }

    let data_ref = x.data_ref();
    let contig_data = data_ref.as_standard_layout();

    let expanded = contig_data
        .into_shape((b, n_kv, 1, s, d))
        .expect("Failed to expand KV shape");

    let broadcasted = expanded
        .broadcast((b, n_kv, n_rep, s, d))
        .expect("Failed to broadcast KV");

    let res = broadcasted
        .to_owned()
        .into_shape((b, n_kv * n_rep, s, d))
        .expect("Failed to flatten repeated KV heads");

    let x_clone = x.clone();
    Tensor(Rc::new(RefCell::new(TensorData {
        data: res.into_dyn().into_shared(),
        f16_data: None,
        bf16_data: None,
        i8_data: None,
        cuda_f32_data: None,
        i8_scale: None,
        has_f32_data: true,
        storage_dtype: crate::precision::DType::F32,
        cache_dirty: false,
        is_parameter: false,
        grad: None,
        cuda_f32_grad: None,
        parents: vec![x.clone()],
        backward_op: Some(std::rc::Rc::new(move |grad| {
            let grad_4d = grad
                .view()
                .into_dimensionality::<ndarray::Ix4>()
                .expect("repeat_kv grad should be 4D");
            let mut d_x = Array4::<f32>::zeros((b, n_kv, s, d));
            for bb in 0..b {
                for hk in 0..n_kv {
                    for rep in 0..n_rep {
                        let out_h = hk * n_rep + rep;
                        for ss in 0..s {
                            for dd in 0..d {
                                d_x[[bb, hk, ss, dd]] += grad_4d[[bb, out_h, ss, dd]];
                            }
                        }
                    }
                }
            }
            x_clone.add_grad(d_x.into_dyn());
        })),
        requires_grad: true,
        device: output_device,
    })))
}

// eval 路径核心：GQA attention（不 repeat_kv）
// q: [B, H, S, D]
// k/v: [B, H_kv, L, D]
// 返回 context: [B, S, H, D]（BSHD，便于后续 reshape 到 [B,S,H*D] 视为 view）
// past_len: cache 写入前的长度（用于 causal mask 的 absolute index）

fn gqa_attention_no_repeat_bshd_view(
    q: &ndarray::ArrayView4<f32>,
    k: &ndarray::ArrayView4<f32>,
    v: &ndarray::ArrayView4<f32>,
    scale: f32,
    causal: bool,
    n_rep: usize,
    past_len: usize,
) -> Array4<f32> {
    let (b, h, s, d) = q.dim();
    let (b2, h_kv, l, d2) = k.dim();
    assert_eq!(b, b2);
    assert_eq!(d, d2);
    assert_eq!(h, h_kv * n_rep);

    let mut out = Array4::<f32>::zeros((b, s, h, d));

    // 将 out 变换为 [H,B,S,D]，对 head 维并行写入（线程间互不重叠）
    let mut out_hbsd = out.view_mut().permuted_axes([2, 0, 1, 3]);
    out_hbsd
        .outer_iter_mut()
        .into_par_iter()
        .enumerate()
        .for_each(|(hq, mut out_for_head)| {
            let hk = hq / n_rep;

            // decode(S=1) 热路径：online softmax + 直接累加 ctx，完全不需要 scores/ctx buffer
            if s == 1 {
                for bb in 0..b {
                    let q_vec = q.slice(ndarray::s![bb, hq, 0, ..]); // [D]
                    let k_mat = k.slice(ndarray::s![bb, hk, .., ..]); // [L,D]
                    let v_mat = v.slice(ndarray::s![bb, hk, .., ..]); // [L,D]

                    let q_abs = past_len; // i == 0

                    // 1) max
                    let mut maxv = f32::NEG_INFINITY;
                    for j in 0..l {
                        // causal mask
                        if causal && j > q_abs {
                            continue;
                        }
                        let kj = k_mat.slice(ndarray::s![j, ..]);
                        let mut dot = 0.0f32;
                        for i in 0..d {
                            dot += q_vec[i] * kj[i];
                        }
                        let score = dot * scale;
                        if score > maxv {
                            maxv = score;
                        }
                    }

                    // 2) sum + ctx
                    let mut sum = 0.0f32;
                    // out_for_head: [B,S,D] and S==1
                    let mut out_row = out_for_head.slice_mut(ndarray::s![bb, 0, ..]);
                    out_row.fill(0.0);

                    for j in 0..l {
                        if causal && j > q_abs {
                            continue;
                        }
                        let kj = k_mat.slice(ndarray::s![j, ..]);
                        let mut dot = 0.0f32;
                        for i in 0..d {
                            dot += q_vec[i] * kj[i];
                        }
                        let w = (dot * scale - maxv).exp();
                        sum += w;

                        for i in 0..d {
                            out_row[i] += w * v_mat[[j, i]];
                        }
                    }

                    let inv = 1.0f32 / (sum + 1e-9);
                    for i in 0..d {
                        out_row[i] *= inv;
                    }
                }
                return;
            }

            // per-thread buffer reuse
            with_attention_work_buffers(s * l, s * d, |scores_buf, ctx_buf| {
                let mut scores = ndarray::ArrayViewMut2::from_shape((s, l), scores_buf)
                    .expect("scores buffer shape mismatch");
                let mut ctx = ndarray::ArrayViewMut2::from_shape((s, d), ctx_buf)
                    .expect("ctx buffer shape mismatch");

                for bb in 0..b {
                    let q_mat = q.slice(ndarray::s![bb, hq, .., ..]); // [S,D]
                    let k_mat = k.slice(ndarray::s![bb, hk, .., ..]); // [L,D]
                    let v_mat = v.slice(ndarray::s![bb, hk, .., ..]); // [L,D]

                    scores.fill(0.0);
                    general_mat_mul(1.0, &q_mat, &k_mat.t(), 0.0, &mut scores);
                    softmax_inplace_view(&mut scores, scale, causal, past_len);

                    ctx.fill(0.0);
                    general_mat_mul(1.0, &scores, &v_mat, 0.0, &mut ctx);

                    // out_for_head: [B,S,D]
                    out_for_head.slice_mut(ndarray::s![bb, .., ..]).assign(&ctx);
                }
            });
        });

    out
}

fn softmax_inplace_view(
    scores: &mut ndarray::ArrayViewMut2<f32>,
    scale: f32,
    causal: bool,
    past_len: usize,
) {
    let (s, l) = scores.dim();

    for i in 0..s {
        // query 的 absolute index
        let q_abs = past_len + i;

        // 1) scale + causal mask
        for j in 0..l {
            let mut val = scores[(i, j)] * scale;
            if causal && j > q_abs {
                val = f32::NEG_INFINITY;
            }
            scores[(i, j)] = val;
        }

        // 2) stable softmax
        let mut maxv = f32::NEG_INFINITY;
        for j in 0..l {
            let v = scores[(i, j)];
            if v > maxv {
                maxv = v;
            }
        }
        let mut sum = 0.0f32;
        for j in 0..l {
            let e = (scores[(i, j)] - maxv).exp();
            scores[(i, j)] = e;
            sum += e;
        }
        let inv = 1.0f32 / (sum + 1e-9);
        for j in 0..l {
            scores[(i, j)] *= inv;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    #[cfg(feature = "cuda")]
    use crate::autograd::set_strict_device_execution;
    use crate::precision::{
        DType, PrecisionConfig, set_default_runtime_dtype, with_precision_config,
    };
    use ndarray::{Array, IxDyn};

    fn make_tensor(shape: &[usize], data: Vec<f32>, dtype: DType) -> Tensor {
        let t = Tensor::from_array_no_grad(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
        );
        t.cast_inplace(dtype);
        t
    }

    #[cfg(feature = "cuda")]
    fn make_grad_tensor(shape: &[usize], data: Vec<f32>) -> Tensor {
        Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(IxDyn(shape), data)
                .expect("test tensor shape mismatch")
                .into_dyn(),
            true,
        )
    }

    #[test]
    #[should_panic(expected = "n_head must be > 0")]
    fn self_attention_rejects_zero_query_heads() {
        let _ = SelfAttention::new(16, 0, 1, 8, 10000.0, true);
    }

    #[test]
    #[should_panic(expected = "n_kv_head must be > 0")]
    fn self_attention_rejects_zero_kv_heads() {
        let _ = SelfAttention::new(16, 4, 0, 8, 10000.0, true);
    }

    #[test]
    #[should_panic(expected = "n_head must be divisible by n_kv_head")]
    fn self_attention_rejects_non_divisible_gqa_ratio() {
        let _ = SelfAttention::new(12, 6, 4, 8, 10000.0, true);
    }

    #[test]
    #[should_panic(expected = "attention input must be [B,S,H]")]
    fn self_attention_rejects_non_3d_input() {
        let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
        let input = make_tensor(&[1, 8], vec![0.0; 8], DType::F32);
        no_grad(|| {
            let _ = attn.forward(input, None);
        });
    }

    #[test]
    fn no_grad_forward_keeps_bf16_input_storage() {
        let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
        let input = make_tensor(
            &[1, 2, 8],
            (0..16).map(|i| i as f32 * 0.1 - 0.5).collect(),
            DType::BF16,
        );

        no_grad(|| {
            let _ = attn.forward(input.clone(), None);
        });

        assert_eq!(input.dtype(), DType::BF16);
        input.with_storage_view(|view| match view {
            crate::autograd::TensorStorageView::BF16(_) => {}
            crate::autograd::TensorStorageView::F16(_) => {
                panic!("shape inspection should not materialize bf16 attention input")
            }
            crate::autograd::TensorStorageView::F32(_) => {
                panic!("shape inspection should not materialize bf16 attention input")
            }
        });
    }

    #[test]
    fn kv_cache_creation_follows_runtime_dtype() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let cache = KVCacheInner::new(1, 1, 4, 4);
                assert_eq!(cache.dtype, DType::BF16);
                assert!(cache.follows_global_dtype);
                cache.k.with_storage_view_preferring(
                    StoragePreference::Native,
                    |view| match view {
                        TensorStorageView::BF16(view) => assert_eq!(view.shape(), &[1, 1, 4, 4]),
                        TensorStorageView::F16(_) => {
                            panic!("kv cache should follow bf16 runtime dtype")
                        }
                        TensorStorageView::F32(_) => {
                            panic!("kv cache should follow bf16 runtime dtype")
                        }
                    },
                );
            },
        );
    }

    #[test]
    fn kv_cache_manual_cast_disables_global_following() {
        let mut cache = KVCacheInner::new(1, 1, 4, 4);
        assert!(cache.follows_global_dtype);
        cache.cast_inplace(DType::BF16);
        assert_eq!(cache.dtype, DType::BF16);
        assert!(!cache.follows_global_dtype);
        cache
            .v
            .with_storage_view_preferring(StoragePreference::Native, |view| match view {
                TensorStorageView::BF16(view) => assert_eq!(view.shape(), &[1, 1, 4, 4]),
                TensorStorageView::F16(_) => {
                    panic!("manual cast should switch cache to explicit bf16 storage")
                }
                TensorStorageView::F32(_) => {
                    panic!("manual cast should switch cache to explicit bf16 storage")
                }
            });
    }

    #[test]
    fn kv_cache_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let cache = KVCacheInner::new_with_dtype(1, 1, 4, 4, DType::F32);
                assert_eq!(cache.dtype, DType::F32);
                assert!(!cache.follows_global_dtype);
                cache.k.with_storage_view_preferring(
                    StoragePreference::Native,
                    |view| match view {
                        TensorStorageView::F32(view) => assert_eq!(view.shape(), &[1, 1, 4, 4]),
                        TensorStorageView::F16(_) => {
                            panic!(
                                "explicit kv cache dtype should override global bf16 runtime dtype"
                            )
                        }
                        TensorStorageView::BF16(_) => {
                            panic!(
                                "explicit kv cache dtype should override global bf16 runtime dtype"
                            )
                        }
                    },
                );
            },
        );
    }

    #[test]
    fn self_attention_allows_i8_activation_dtype_with_float_kv_cache() {
        let attn = SelfAttention::new_with_runtime_dtypes(
            8,
            2,
            1,
            8,
            10000.0,
            true,
            DType::I8,
            DType::I8,
            DType::BF16,
        );
        assert_eq!(attn.activation_dtype(), DType::I8);
        assert_eq!(attn.kv_cache_dtype(), DType::BF16);
    }

    #[test]
    #[should_panic(expected = "KV cache batch size must be > 0")]
    fn kv_cache_rejects_zero_batch() {
        let _ = KVCacheInner::new(0, 1, 4, 4);
    }

    #[test]
    #[should_panic(expected = "SelfAttention KV cache shape mismatch")]
    fn self_attention_rejects_cache_shape_mismatch() {
        let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
        let input = make_tensor(&[1, 1, 8], vec![0.0; 8], DType::F32);
        let cache: KVCache = Rc::new(RefCell::new(KVCacheInner::new(2, 1, 8, 4)));
        no_grad(|| {
            let _ = attn.forward(input, Some(cache));
        });
    }

    #[test]
    #[should_panic(expected = "SelfAttention KV cache current length out of bounds")]
    fn self_attention_rejects_cache_len_overflow() {
        let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
        let input = make_tensor(&[1, 1, 8], vec![0.0; 8], DType::F32);
        let mut cache = KVCacheInner::new(1, 1, 8, 4);
        cache.len = 9;
        let cache: KVCache = Rc::new(RefCell::new(cache));
        no_grad(|| {
            let _ = attn.forward(input, Some(cache));
        });
    }

    #[test]
    fn self_attention_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::F32);
                for weight in [
                    &attn.w_q.weight,
                    &attn.w_k.weight,
                    &attn.w_v.weight,
                    &attn.w_o.weight,
                ] {
                    assert_eq!(weight.dtype(), DType::F32);
                }
                assert_eq!(attn.rope.cache_dtype(), DType::F32);
            },
        );
    }

    #[test]
    fn self_attention_default_construction_splits_parameter_and_runtime_defaults() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
                for weight in [
                    &attn.w_q.weight,
                    &attn.w_k.weight,
                    &attn.w_v.weight,
                    &attn.w_o.weight,
                ] {
                    assert_eq!(weight.dtype(), DType::F32);
                }
                assert_eq!(attn.rope.cache_dtype(), DType::BF16);
            },
        );
    }

    #[test]
    fn self_attention_default_construction_captures_runtime_dtype_for_future_cache_creation() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let attn = SelfAttention::new(8, 2, 1, 8, 10000.0, true);
                set_default_runtime_dtype(DType::F32);
                let input = make_tensor(&[1, 1, 8], vec![0.0; 8], DType::F32);
                let (_out, cache) = no_grad(|| attn.forward(input, None));
                let cache = cache.expect("no_grad attention should create cache");
                assert_eq!(cache.borrow().dtype, DType::BF16);
            },
        );
    }

    #[test]
    fn repeat_kv_no_grad_preserves_bf16_input_dtype() {
        let input = make_tensor(
            &[1, 2, 2, 2],
            vec![1.0, 2.0, -1.0, -2.0, 3.0, 4.0, -3.0, -4.0],
            DType::BF16,
        );
        let out = no_grad(|| repeat_kv(input, 2));
        assert_eq!(out.shape_vec(), vec![1, 4, 2, 2]);
        assert_eq!(out.dtype(), DType::BF16);
    }

    #[test]
    fn repeat_kv_no_grad_preserves_i8_input_dtype() {
        let input = make_tensor(&[1, 2, 1, 2], vec![1.0, -2.0, 3.0, -4.0], DType::I8);
        let out = no_grad(|| repeat_kv(input, 3));
        assert_eq!(out.shape_vec(), vec![1, 6, 1, 2]);
        assert_eq!(out.dtype(), DType::I8);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_repeat_kv_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(
            &[1, 2, 2, 3],
            vec![
                1.0, 2.0, 3.0, -1.0, -2.0, -3.0, 4.0, 5.0, 6.0, -4.0, -5.0, -6.0,
            ],
            DType::BF16,
        )
        .to_cuda();

        set_strict_device_execution(true);
        let cuda_out = no_grad(|| repeat_kv(input.clone(), 2));
        set_strict_device_execution(false);

        let cpu_out = no_grad(|| repeat_kv(input.to_cpu(), 2));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.dtype(), DType::BF16);
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 2e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_empty_repeat_kv_no_grad_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(&[1, 2, 0, 3], vec![], DType::BF16).to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let out = no_grad(|| repeat_kv(input, 2));
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::BF16);
        assert_eq!(out.shape_vec(), vec![1, 4, 0, 3]);
        assert_eq!(out.len(), 0);
    }

    #[test]
    fn repeat_kv_backward_accumulates_repeated_heads_on_cpu() {
        let input = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[1, 2, 2, 2]),
                vec![1.0, -1.0, 2.0, -2.0, 0.5, 1.5, -0.5, -1.5],
            )
            .expect("input shape mismatch")
            .into_dyn(),
            true,
        );
        let coeff = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[1, 4, 2, 2]),
                (0..16).map(|v| v as f32 * 0.25 - 1.0).collect(),
            )
            .expect("coeff shape mismatch")
            .into_dyn(),
            false,
        );

        let out = repeat_kv(input.clone(), 2);
        let loss = crate::ops::arithmetic::sum(&(&out * &coeff));
        loss.backward();

        let grad = input.grad().expect("repeat_kv input grad");
        let vals = grad.iter().copied().collect::<Vec<_>>();
        let coeff_vals = coeff.data_ref().iter().copied().collect::<Vec<_>>();
        let mut expected = vec![0.0f32; 8];
        for hk in 0..2 {
            for rep in 0..2 {
                for ss in 0..2 {
                    for dd in 0..2 {
                        let src = ((hk * 2 + rep) * 2 + ss) * 2 + dd;
                        let dst = (hk * 2 + ss) * 2 + dd;
                        expected[dst] += coeff_vals[src];
                    }
                }
            }
        }
        assert_eq!(vals, expected);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_repeat_kv_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[1, 2, 2, 2]),
                vec![1.0, -1.0, 2.0, -2.0, 0.5, 1.5, -0.5, -1.5],
            )
            .expect("input shape mismatch")
            .into_dyn(),
            true,
        );
        let coeff_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[1, 4, 2, 2]),
                (0..16).map(|v| v as f32 * 0.25 - 1.0).collect(),
            )
            .expect("coeff shape mismatch")
            .into_dyn(),
            false,
        );
        let input_cuda = input_cpu.to_cuda();
        let coeff_cuda = coeff_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let cuda_out = repeat_kv(input_cuda.clone(), 2);
        let cuda_loss = crate::ops::arithmetic::sum(&(&cuda_out * &coeff_cuda));
        cuda_loss.backward();
        assert!(input_cuda.cloned_cuda_f32_grad().is_some());
        assert!(!input_cuda.has_host_grad());
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let cpu_out = repeat_kv(input_cpu.clone(), 2);
        let cpu_loss = crate::ops::arithmetic::sum(&(&cpu_out * &coeff_cpu));
        cpu_loss.backward();

        let cuda_grad = input_cuda.grad().expect("cuda repeat_kv grad");
        let cpu_grad = input_cpu.grad().expect("cpu repeat_kv grad");
        for (got, expect) in cuda_grad.iter().zip(cpu_grad.iter()) {
            assert!((got - expect).abs() < 1e-5, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_repeat_kv_preserves_i8_dtype_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let input = make_tensor(&[1, 2, 1, 2], vec![1.0, -2.0, 3.0, -4.0], DType::I8).to_cuda();

        set_strict_device_execution(true);
        let out = no_grad(|| repeat_kv(input, 2));
        set_strict_device_execution(false);

        assert!(out.is_cuda());
        assert_eq!(out.dtype(), DType::I8);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_self_attention_training_backward_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::F32);
        let attn_ref = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::F32);

        for ((dst, src), shape, scale) in [
            (
                (&attn.w_q.weight, &attn_ref.w_q.weight),
                vec![8, 8],
                0.03125f32,
            ),
            (
                (&attn.w_k.weight, &attn_ref.w_k.weight),
                vec![4, 8],
                0.0275f32,
            ),
            (
                (&attn.w_v.weight, &attn_ref.w_v.weight),
                vec![4, 8],
                -0.021f32,
            ),
            (
                (&attn.w_o.weight, &attn_ref.w_o.weight),
                vec![8, 8],
                0.01875f32,
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * scale) - 0.25)
                .collect::<Vec<_>>();
            let arr = Array::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::F32);
            src.set_array_f32_with_dtype(arr, DType::F32);
        }

        attn.to_cuda();
        let input_cpu = make_grad_tensor(
            &[1, 3, 8],
            (0..24).map(|i| (i as f32 * 0.0625) - 0.4).collect(),
        );
        let coeff_cpu = Tensor::from_data_with_grad_flag(
            Array::from_shape_vec(
                IxDyn(&[1, 3, 8]),
                (0..24).map(|i| (i as f32 * 0.0375) - 0.2).collect(),
            )
            .expect("coeff shape mismatch")
            .into_dyn(),
            false,
        );
        let input_cuda = input_cpu.to_cuda();
        let coeff_cuda = coeff_cpu.to_cuda();

        crate::ops::cuda::set_enabled(true);
        set_strict_device_execution(true);
        let (cuda_out, cuda_cache) = attn.forward(input_cuda.clone(), None);
        assert!(cuda_out.is_cuda());
        assert!(cuda_cache.is_none());
        let cuda_loss = crate::ops::arithmetic::sum(&(&cuda_out * &coeff_cuda));
        cuda_loss.backward();
        assert!(input_cuda.cloned_cuda_f32_grad().is_some());
        assert!(!input_cuda.has_host_grad());
        for param in attn.parameters() {
            assert!(param.cloned_cuda_f32_grad().is_some());
            assert!(!param.has_host_grad());
        }
        set_strict_device_execution(false);
        crate::ops::cuda::set_enabled(false);

        let (cpu_out, cpu_cache) = attn_ref.forward(input_cpu.clone(), None);
        assert!(cpu_cache.is_none());
        let cpu_loss = crate::ops::arithmetic::sum(&(&cpu_out * &coeff_cpu));
        cpu_loss.backward();

        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 1e-4, "got {got}, expect {expect}");
        }
        let cuda_input_grad = input_cuda.grad().expect("cuda attention input grad");
        let cpu_input_grad = input_cpu.grad().expect("cpu attention input grad");
        for (got, expect) in cuda_input_grad.iter().zip(cpu_input_grad.iter()) {
            assert!((got - expect).abs() < 1e-3, "got {got}, expect {expect}");
        }
        for (cuda_param, cpu_param) in attn.parameters().iter().zip(attn_ref.parameters().iter()) {
            let cuda_grad = cuda_param.grad().expect("cuda attention parameter grad");
            let cpu_grad = cpu_param.grad().expect("cpu attention parameter grad");
            for (got, expect) in cuda_grad.iter().zip(cpu_grad.iter()) {
                assert!((got - expect).abs() < 1e-3, "got {got}, expect {expect}");
            }
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_self_attention_decode_output_projection_stays_on_cuda_and_matches_cpu() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);
        let attn_ref = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);

        for ((dst, src), shape, scale) in [
            (
                (&attn.w_q.weight, &attn_ref.w_q.weight),
                vec![8, 8],
                0.03125f32,
            ),
            (
                (&attn.w_k.weight, &attn_ref.w_k.weight),
                vec![4, 8],
                0.0275f32,
            ),
            (
                (&attn.w_v.weight, &attn_ref.w_v.weight),
                vec![4, 8],
                -0.021f32,
            ),
            (
                (&attn.w_o.weight, &attn_ref.w_o.weight),
                vec![8, 8],
                0.01875f32,
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * scale) - 0.25)
                .collect::<Vec<_>>();
            let arr = Array::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::BF16);
            src.set_array_f32_with_dtype(arr, DType::BF16);
        }

        attn.to_cuda();
        let input = make_tensor(
            &[1, 1, 8],
            (0..8).map(|i| (i as f32 * 0.125) - 0.25).collect(),
            DType::BF16,
        )
        .to_cuda();

        set_strict_device_execution(true);
        let (cuda_out, _) = no_grad(|| attn.forward(input.clone(), None));
        set_strict_device_execution(false);

        let (cpu_out, _) = no_grad(|| attn_ref.forward(input.to_cpu(), None));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 3e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_self_attention_decode_second_step_matches_cpu_reference() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);
        let attn_ref = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);

        for ((dst, src), shape, scale) in [
            (
                (&attn.w_q.weight, &attn_ref.w_q.weight),
                vec![8, 8],
                0.03125f32,
            ),
            (
                (&attn.w_k.weight, &attn_ref.w_k.weight),
                vec![4, 8],
                0.0275f32,
            ),
            (
                (&attn.w_v.weight, &attn_ref.w_v.weight),
                vec![4, 8],
                -0.021f32,
            ),
            (
                (&attn.w_o.weight, &attn_ref.w_o.weight),
                vec![8, 8],
                0.01875f32,
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * scale) - 0.25)
                .collect::<Vec<_>>();
            let arr = Array::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::BF16);
            src.set_array_f32_with_dtype(arr, DType::BF16);
        }

        attn.to_cuda();
        let cuda_cache: KVCache = Rc::new(RefCell::new(KVCacheInner::new_with_dtype(
            1,
            1,
            8,
            4,
            DType::BF16,
        )));
        cuda_cache.borrow_mut().to_device_inplace(Device::Cuda);
        let cpu_cache: KVCache = Rc::new(RefCell::new(KVCacheInner::new_with_dtype(
            1,
            1,
            8,
            4,
            DType::BF16,
        )));

        let step1 = make_tensor(
            &[1, 1, 8],
            (0..8).map(|i| (i as f32 * 0.125) - 0.25).collect(),
            DType::BF16,
        );
        let step2 = make_tensor(
            &[1, 1, 8],
            (0..8).map(|i| (i as f32 * 0.09375) + 0.1).collect(),
            DType::BF16,
        );

        set_strict_device_execution(true);
        let (_cuda_out1, cuda_cache) = no_grad(|| attn.forward(step1.to_cuda(), Some(cuda_cache)));
        let cuda_cache = cuda_cache.expect("cuda cache after step1");
        let (cuda_out2, cuda_cache) = no_grad(|| attn.forward(step2.to_cuda(), Some(cuda_cache)));
        let cuda_cache = cuda_cache.expect("cuda cache after step2");
        set_strict_device_execution(false);

        let (_cpu_out1, cpu_cache) = no_grad(|| attn_ref.forward(step1, Some(cpu_cache)));
        let cpu_cache = cpu_cache.expect("cpu cache after step1");
        let (cpu_out2, cpu_cache) = no_grad(|| attn_ref.forward(step2, Some(cpu_cache)));
        let cpu_cache = cpu_cache.expect("cpu cache after step2");

        assert!(cuda_out2.is_cuda());
        assert_eq!(cuda_cache.borrow().len, 2);
        assert_eq!(cpu_cache.borrow().len, 2);
        assert_eq!(cuda_out2.shape_vec(), cpu_out2.shape_vec());
        for (got, expect) in cuda_out2.data_ref().iter().zip(cpu_out2.data_ref().iter()) {
            assert!((got - expect).abs() < 3e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_self_attention_prefill_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);
        let attn_ref = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);

        for ((dst, src), shape, scale) in [
            (
                (&attn.w_q.weight, &attn_ref.w_q.weight),
                vec![8, 8],
                0.03125f32,
            ),
            (
                (&attn.w_k.weight, &attn_ref.w_k.weight),
                vec![4, 8],
                0.0275f32,
            ),
            (
                (&attn.w_v.weight, &attn_ref.w_v.weight),
                vec![4, 8],
                -0.021f32,
            ),
            (
                (&attn.w_o.weight, &attn_ref.w_o.weight),
                vec![8, 8],
                0.01875f32,
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * scale) - 0.25)
                .collect::<Vec<_>>();
            let arr = Array::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::BF16);
            src.set_array_f32_with_dtype(arr, DType::BF16);
        }

        attn.to_cuda();
        let input = make_tensor(
            &[1, 3, 8],
            (0..24).map(|i| (i as f32 * 0.0625) - 0.4).collect(),
            DType::BF16,
        );

        set_strict_device_execution(true);
        let (cuda_out, cuda_cache) = no_grad(|| attn.forward(input.to_cuda(), None));
        set_strict_device_execution(false);

        let (cpu_out, cpu_cache) = no_grad(|| attn_ref.forward(input, None));
        let cuda_cache = cuda_cache.expect("cuda cache after prefill");
        let cpu_cache = cpu_cache.expect("cpu cache after prefill");

        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        assert_eq!(cuda_cache.borrow().len, 3);
        assert_eq!(cpu_cache.borrow().len, 3);
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 6e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_self_attention_multi_token_continuation_matches_cpu_reference_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);
        let attn_ref = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);

        for ((dst, src), shape, scale) in [
            (
                (&attn.w_q.weight, &attn_ref.w_q.weight),
                vec![8, 8],
                0.03125f32,
            ),
            (
                (&attn.w_k.weight, &attn_ref.w_k.weight),
                vec![4, 8],
                0.0275f32,
            ),
            (
                (&attn.w_v.weight, &attn_ref.w_v.weight),
                vec![4, 8],
                -0.021f32,
            ),
            (
                (&attn.w_o.weight, &attn_ref.w_o.weight),
                vec![8, 8],
                0.01875f32,
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * scale) - 0.25)
                .collect::<Vec<_>>();
            let arr = Array::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::BF16);
            src.set_array_f32_with_dtype(arr, DType::BF16);
        }

        attn.to_cuda();
        let prefill = make_tensor(
            &[1, 2, 8],
            (0..16).map(|i| (i as f32 * 0.0625) - 0.4).collect(),
            DType::BF16,
        );
        let continuation = make_tensor(
            &[1, 2, 8],
            (0..16).map(|i| (i as f32 * 0.0575) + 0.15).collect(),
            DType::BF16,
        );

        set_strict_device_execution(true);
        let (_cuda_prefill_out, cuda_cache) = no_grad(|| attn.forward(prefill.to_cuda(), None));
        let cuda_cache = cuda_cache.expect("cuda cache after prefill");
        let (cuda_out, cuda_cache) =
            no_grad(|| attn.forward(continuation.to_cuda(), Some(cuda_cache)));
        let cuda_cache = cuda_cache.expect("cuda cache after continuation");
        set_strict_device_execution(false);

        let (_cpu_prefill_out, cpu_cache) = no_grad(|| attn_ref.forward(prefill, None));
        let cpu_cache = cpu_cache.expect("cpu cache after prefill");
        let (cpu_out, cpu_cache) = no_grad(|| attn_ref.forward(continuation, Some(cpu_cache)));
        let cpu_cache = cpu_cache.expect("cpu cache after continuation");

        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_cache.borrow().len, 4);
        assert_eq!(cpu_cache.borrow().len, 4);
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 7e-2, "got {got}, expect {expect}");
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn cuda_kv_cache_writes_preserve_resident_buffers_across_decode_steps() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let attn = SelfAttention::new_with_dtype(8, 2, 1, 8, 10000.0, true, DType::BF16);
        attn.to_cuda();
        let cache: KVCache = Rc::new(RefCell::new(KVCacheInner::new_with_dtype(
            1,
            1,
            8,
            4,
            DType::BF16,
        )));
        cache.borrow().k.to_cuda_inplace();
        cache.borrow().v.to_cuda_inplace();

        let step1 = make_tensor(
            &[1, 1, 8],
            (0..8).map(|i| (i as f32 * 0.125) - 0.25).collect(),
            DType::BF16,
        )
        .to_cuda();
        let step2 = make_tensor(
            &[1, 1, 8],
            (0..8).map(|i| (i as f32 * 0.09375) + 0.1).collect(),
            DType::BF16,
        )
        .to_cuda();

        set_strict_device_execution(true);
        let (_out1, cache) = no_grad(|| attn.forward(step1, Some(cache)));
        let cache = cache.expect("decode step 1 should return KV cache");
        assert!(cache.borrow().k.cloned_cuda_f32_buffer().is_some());
        assert!(cache.borrow().v.cloned_cuda_f32_buffer().is_some());
        let (_out2, cache) = no_grad(|| attn.forward(step2, Some(cache)));
        let cache = cache.expect("decode step 2 should return KV cache");
        set_strict_device_execution(false);

        assert_eq!(cache.borrow().len, 2);
        assert!(cache.borrow().k.cloned_cuda_f32_buffer().is_some());
        assert!(cache.borrow().v.cloned_cuda_f32_buffer().is_some());
    }
}
