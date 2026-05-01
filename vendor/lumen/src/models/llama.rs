use crate::autograd::{
    KernelRouteClass, StoragePreference, Tensor, TensorStorageOwned, TensorStorageView, is_no_grad,
    is_strict_device_execution,
};
use crate::layers::attention::self_attention::KVCacheInner;
use crate::layers::{Embedding, KVCache, Linear, RMSNorm, SelfAttention, SiLU};
use crate::module::Module;
use crate::ops::cuda;
use crate::ops::fused::{fused_gate_up_silu_infer, fused_gate_up_silu_infer_into};
use crate::ops::matmul::{SliceRef, matvec_argmax_rowmajor_parallel_mixed};
use crate::ops::shape::slice_last_dim;
use crate::precision::{DType, default_activation_dtype, default_kv_cache_dtype};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

thread_local! {
    static MLP_INTER_BUF: RefCell<Vec<f32>> = RefCell::new(Vec::new());
}

fn with_mlp_inter_buffer<R>(len: usize, f: impl FnOnce(&mut [f32]) -> R) -> R {
    MLP_INTER_BUF.with(|buf| {
        if let Ok(mut buf) = buf.try_borrow_mut() {
            if buf.len() < len {
                buf.resize(len, 0.0);
            }
            return f(&mut buf[..len]);
        }

        let mut fallback = vec![0.0f32; len];
        f(&mut fallback)
    })
}

#[inline]
fn slice_ref_dtype(slice: SliceRef<'_>) -> DType {
    match slice {
        SliceRef::F32(_) => DType::F32,
        SliceRef::F16(_) => DType::F16,
        SliceRef::BF16(_) => DType::BF16,
        SliceRef::I8(_, _) => DType::I8,
    }
}

fn for_each_last_hidden_token_as_slice_ref(
    hidden: &Tensor,
    mut f: impl FnMut(usize, SliceRef<'_>),
) {
    let shape = hidden.shape_vec();
    assert_eq!(shape.len(), 3, "hidden states must be [B,S,H]");
    let (batch, seq_len, hidden_size) = (shape[0], shape[1], shape[2]);
    assert!(seq_len > 0, "hidden sequence length must be > 0");

    if hidden.dtype() == DType::I8 {
        return match hidden.native_storage_owned() {
            TensorStorageOwned::I8(hidden_data, scale) => {
                let hidden3 = hidden_data
                    .view()
                    .into_dimensionality::<ndarray::Ix3>()
                    .expect("hidden states must be [B,S,H]");
                for bb in 0..batch {
                    let last = hidden3.slice(ndarray::s![bb, seq_len - 1, ..]);
                    if let Some(hidden_slice) = last.as_slice() {
                        f(bb, SliceRef::I8(hidden_slice, scale));
                    } else {
                        let hidden_owned = last.iter().copied().collect::<Vec<_>>();
                        f(bb, SliceRef::I8(hidden_owned.as_slice(), scale));
                    }
                }
            }
            TensorStorageOwned::F32(_)
            | TensorStorageOwned::F16(_)
            | TensorStorageOwned::BF16(_) => {
                unreachable!("checked i8 hidden storage above")
            }
        };
    }

    hidden.with_storage_view_preferring(
        StoragePreference::Native,
        |hidden_view| match hidden_view {
            TensorStorageView::F32(hidden_view) => {
                let hidden3 = hidden_view
                    .into_dimensionality::<ndarray::Ix3>()
                    .expect("hidden states must be [B,S,H]");
                for bb in 0..batch {
                    let last = hidden3.slice(ndarray::s![bb, seq_len - 1, ..]);
                    debug_assert_eq!(last.len(), hidden_size);
                    if let Some(hidden_slice) = last.as_slice() {
                        f(bb, SliceRef::F32(hidden_slice));
                    } else {
                        let hidden_owned = last.iter().copied().collect::<Vec<_>>();
                        f(bb, SliceRef::F32(hidden_owned.as_slice()));
                    }
                }
            }
            TensorStorageView::F16(hidden_view) => {
                let hidden3 = hidden_view
                    .into_dimensionality::<ndarray::Ix3>()
                    .expect("hidden states must be [B,S,H]");
                for bb in 0..batch {
                    let last = hidden3.slice(ndarray::s![bb, seq_len - 1, ..]);
                    debug_assert_eq!(last.len(), hidden_size);
                    if let Some(hidden_slice) = last.as_slice() {
                        f(bb, SliceRef::F16(hidden_slice));
                    } else {
                        let hidden_owned = last.iter().copied().collect::<Vec<_>>();
                        f(bb, SliceRef::F16(hidden_owned.as_slice()));
                    }
                }
            }
            TensorStorageView::BF16(hidden_view) => {
                let hidden3 = hidden_view
                    .into_dimensionality::<ndarray::Ix3>()
                    .expect("hidden states must be [B,S,H]");
                for bb in 0..batch {
                    let last = hidden3.slice(ndarray::s![bb, seq_len - 1, ..]);
                    debug_assert_eq!(last.len(), hidden_size);
                    if let Some(hidden_slice) = last.as_slice() {
                        f(bb, SliceRef::BF16(hidden_slice));
                    } else {
                        let hidden_owned = last.iter().copied().collect::<Vec<_>>();
                        f(bb, SliceRef::BF16(hidden_owned.as_slice()));
                    }
                }
            }
        },
    )
}

fn last_hidden_token_tensor(hidden: Tensor) -> Tensor {
    let shape = hidden.shape_vec();
    assert_eq!(shape.len(), 3, "hidden states must be [B,S,H]");
    let seq_len = shape[1];
    if seq_len == 1 {
        return hidden;
    }
    if hidden.is_cuda() {
        let transposed = hidden.permute(vec![0, 2, 1]);
        let last = slice_last_dim(&transposed, seq_len - 1, seq_len);
        return last.permute(vec![0, 2, 1]);
    }

    match hidden.native_storage_owned() {
        TensorStorageOwned::F32(data) => {
            let hidden3 = data
                .view()
                .into_dimensionality::<ndarray::Ix3>()
                .expect("hidden states must be [B,S,H]");
            let last = hidden3
                .slice(ndarray::s![.., seq_len - 1..seq_len, ..])
                .to_owned()
                .into_dyn();
            Tensor::from_array_no_grad(last)
        }
        TensorStorageOwned::F16(data) => {
            let hidden3 = data
                .view()
                .into_dimensionality::<ndarray::Ix3>()
                .expect("hidden states must be [B,S,H]");
            let last = hidden3
                .slice(ndarray::s![.., seq_len - 1..seq_len, ..])
                .to_owned()
                .into_dyn()
                .into_shared();
            Tensor::from_f16_data_no_grad(last)
        }
        TensorStorageOwned::BF16(data) => {
            let hidden3 = data
                .view()
                .into_dimensionality::<ndarray::Ix3>()
                .expect("hidden states must be [B,S,H]");
            let last = hidden3
                .slice(ndarray::s![.., seq_len - 1..seq_len, ..])
                .to_owned()
                .into_dyn()
                .into_shared();
            Tensor::from_bf16_data_no_grad(last)
        }
        TensorStorageOwned::I8(data, scale) => {
            let hidden3 = data
                .view()
                .into_dimensionality::<ndarray::Ix3>()
                .expect("hidden states must be [B,S,H]");
            let last = hidden3
                .slice(ndarray::s![.., seq_len - 1..seq_len, ..])
                .to_owned()
                .into_dyn()
                .into_shared();
            Tensor::from_i8_data_no_grad(last, scale)
        }
    }
}

// Llama 配置参数
#[derive(Clone, Debug)]
pub struct LlamaConfig {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub intermediate_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub num_key_value_heads: usize, // 支持 GQA
    pub rms_norm_eps: f32,
    pub max_seq_len: usize,
    pub rope_theta: f32,
}

impl Default for LlamaConfig {
    fn default() -> Self {
        Self {
            vocab_size: 32000,
            hidden_size: 2048,
            intermediate_size: 5632,
            num_hidden_layers: 22,
            num_attention_heads: 32,
            num_key_value_heads: 4, // TinyLlama 1.1B 其实是 32 (MHA)，但 Qwen 是 GQA
            rms_norm_eps: 1e-5,
            max_seq_len: 2048,
            rope_theta: 10000.0,
        }
    }
}

impl LlamaConfig {
    fn validate(&self) {
        assert!(self.vocab_size > 0, "vocab_size must be > 0");
        assert!(self.hidden_size > 0, "hidden_size must be > 0");
        assert!(self.intermediate_size > 0, "intermediate_size must be > 0");
        assert!(
            self.num_attention_heads > 0,
            "num_attention_heads must be > 0"
        );
        assert!(
            self.num_key_value_heads > 0,
            "num_key_value_heads must be > 0"
        );
        assert!(self.max_seq_len > 0, "max_seq_len must be > 0");
        assert!(
            self.rms_norm_eps > 0.0,
            "rms_norm_eps must be > 0, got {}",
            self.rms_norm_eps
        );
        assert_eq!(
            self.hidden_size % self.num_attention_heads,
            0,
            "hidden_size must be divisible by num_attention_heads"
        );
        assert_eq!(
            self.num_attention_heads % self.num_key_value_heads,
            0,
            "num_attention_heads must be divisible by num_key_value_heads"
        );
    }
}

// Llama MLP 层 (SwiGLU)
// 公式: down(act(gate(x)) * up(x))
struct LlamaMLP {
    gate_proj: Linear,
    up_proj: Linear,
    down_proj: Linear,
    act: SiLU,
}

impl LlamaMLP {
    fn new(config: &LlamaConfig) -> Self {
        Self {
            gate_proj: Linear::new_no_bias(config.hidden_size, config.intermediate_size),
            up_proj: Linear::new_no_bias(config.hidden_size, config.intermediate_size),
            down_proj: Linear::new_no_bias(config.intermediate_size, config.hidden_size),
            act: SiLU::new(),
        }
    }

    fn new_with_dtype(config: &LlamaConfig, dtype: DType) -> Self {
        Self {
            // Llama 官方通常没有 bias，使用 new_no_bias
            gate_proj: Linear::new_no_bias_with_dtype(
                config.hidden_size,
                config.intermediate_size,
                dtype,
            ),
            up_proj: Linear::new_no_bias_with_dtype(
                config.hidden_size,
                config.intermediate_size,
                dtype,
            ),
            down_proj: Linear::new_no_bias_with_dtype(
                config.intermediate_size,
                config.hidden_size,
                dtype,
            ),
            act: SiLU::new(),
        }
    }

    #[inline]
    fn should_use_fused_gate_up(x: &Tensor) -> bool {
        if !is_no_grad() {
            return false;
        }
        let shape = x.shape_vec();
        if shape.is_empty() {
            return false;
        }
        let k_dim = *shape.last().expect("MLP input must have last dim");
        if k_dim == 0 {
            return false;
        }
        let rows = x.len() / k_dim;
        rows == 1
    }

    fn forward(&self, x: Tensor) -> Tensor {
        if Self::should_use_fused_gate_up(&x) {
            if x.is_cuda() {
                let inter =
                    fused_gate_up_silu_infer(&x, &self.gate_proj.weight, &self.up_proj.weight);
                return self.down_proj.forward(inter);
            }
            let inter_dim = self.down_proj.in_features;
            return with_mlp_inter_buffer(inter_dim, |inter| {
                {
                    fused_gate_up_silu_infer_into(
                        &x,
                        &self.gate_proj.weight,
                        &self.up_proj.weight,
                        inter,
                    );
                }
                self.down_proj.forward_decode_slice_no_bias(inter)
            });
        }

        let gate = { self.gate_proj.forward(x.clone()) };
        let gate_act = { self.act.forward(gate) };
        let up = { self.up_proj.forward(x) };
        let fused = { gate_act * up };
        self.down_proj.forward(fused)
    }
}

// NOTE:
// llama.rs 之前自带了一套 LlamaAttention（repeat_kv + 显式 score/prob Tensor + KVCache::get_view 分配）。
// 这里直接复用 self_attention.rs 的实现：
// - eval/no_grad: 支持 KV cache 预分配、decode(S=1) online-softmax 热路径、GQA 不 repeat_kv。
// - train: 走可导的标准路径（fused_softmax + batch_matmul）。
//
// 因此：
// - LlamaDecoderLayer::self_attn 改为 SelfAttention
// - Cache 类型改为 layers::KVCache（Rc<RefCell<KVCacheInner>>）

// Llama Decoder Block
struct LlamaDecoderLayer {
    self_attn: SelfAttention,
    mlp: LlamaMLP,
    input_layernorm: RMSNorm,
    post_attention_layernorm: RMSNorm,
}

impl LlamaDecoderLayer {
    fn new(config: &LlamaConfig) -> Self {
        Self {
            self_attn: SelfAttention::new(
                config.hidden_size,
                config.num_attention_heads,
                config.num_key_value_heads,
                config.max_seq_len,
                config.rope_theta,
                true, // causal
            ),
            mlp: LlamaMLP::new(config),
            input_layernorm: RMSNorm::new(config.hidden_size, config.rms_norm_eps),
            post_attention_layernorm: RMSNorm::new(config.hidden_size, config.rms_norm_eps),
        }
    }

    fn new_with_dtype(config: &LlamaConfig, dtype: DType) -> Self {
        Self {
            self_attn: SelfAttention::new_with_dtype(
                config.hidden_size,
                config.num_attention_heads,
                config.num_key_value_heads,
                config.max_seq_len,
                config.rope_theta,
                true, // causal
                dtype,
            ),
            mlp: LlamaMLP::new_with_dtype(config, dtype),
            input_layernorm: RMSNorm::new_with_dtype(
                config.hidden_size,
                config.rms_norm_eps,
                dtype,
            ),
            post_attention_layernorm: RMSNorm::new_with_dtype(
                config.hidden_size,
                config.rms_norm_eps,
                dtype,
            ),
        }
    }

    fn new_with_dtypes(
        config: &LlamaConfig,
        parameter_dtype: DType,
        activation_dtype: DType,
        kv_cache_dtype: DType,
    ) -> Self {
        Self {
            self_attn: SelfAttention::new_with_runtime_dtypes(
                config.hidden_size,
                config.num_attention_heads,
                config.num_key_value_heads,
                config.max_seq_len,
                config.rope_theta,
                true, // causal
                parameter_dtype,
                activation_dtype,
                kv_cache_dtype,
            ),
            mlp: LlamaMLP::new_with_dtype(config, parameter_dtype),
            input_layernorm: RMSNorm::new_with_dtype(
                config.hidden_size,
                config.rms_norm_eps,
                parameter_dtype,
            ),
            post_attention_layernorm: RMSNorm::new_with_dtype(
                config.hidden_size,
                config.rms_norm_eps,
                parameter_dtype,
            ),
        }
    }

    // 推理路径：传入 cache（Rc<RefCell<_>>），用于增量 decode。
    fn forward_infer(&self, x: Tensor, cache: KVCache) -> (Tensor, KVCache) {
        // Pre-Norm Architecture
        // h = x + Attention(Norm(x))
        let norm_x = self.input_layernorm.forward(x.clone());
        let (attn_out, cache_out) = self.self_attn.forward(norm_x, Some(cache));
        let cache_out = cache_out.expect("SelfAttention should return cache in eval/no_grad path");
        let h = x + attn_out;

        // out = h + MLP(Norm(h))
        let norm_h = self.post_attention_layernorm.forward(h.clone());
        let mlp_out = self.mlp.forward(norm_h);
        (h + mlp_out, cache_out)
    }

    // 训练路径：不允许传 cache（SelfAttention 会 panic）。
    fn forward_train(&self, x: Tensor) -> Tensor {
        let norm_x = self.input_layernorm.forward(x.clone());
        let (attn_out, _cache) = self.self_attn.forward(norm_x, None);
        let h = x + attn_out;
        let norm_h = self.post_attention_layernorm.forward(h.clone());
        let mlp_out = self.mlp.forward(norm_h);
        h + mlp_out
    }
}

pub struct LlamaModel {
    embed_tokens: Embedding,
    layers: Vec<LlamaDecoderLayer>,
    norm: RMSNorm,
    lm_head: Linear,
    pub config: LlamaConfig,
    activation_dtype: DType,
    kv_cache_dtype: DType,
}

impl LlamaModel {
    #[inline]
    pub fn activation_dtype(&self) -> DType {
        self.activation_dtype
    }

    #[inline]
    pub fn kv_cache_dtype(&self) -> DType {
        self.kv_cache_dtype
    }

    pub fn new(config: LlamaConfig) -> Self {
        config.validate();
        let activation_dtype = default_activation_dtype();
        let kv_cache_dtype = default_kv_cache_dtype();
        assert!(
            kv_cache_dtype.is_float(),
            "LlamaModel KV cache dtype currently only supports floating types, got {:?}",
            kv_cache_dtype
        );
        let embed_tokens = Embedding::new(config.vocab_size, config.hidden_size);

        let mut layers = Vec::new();
        for _ in 0..config.num_hidden_layers {
            layers.push(LlamaDecoderLayer::new(&config));
        }

        let norm = RMSNorm::new(config.hidden_size, config.rms_norm_eps);
        let lm_head = Linear::new_no_bias(config.hidden_size, config.vocab_size);

        Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            activation_dtype,
            kv_cache_dtype,
            config,
        }
    }

    pub fn new_with_dtype(config: LlamaConfig, dtype: DType) -> Self {
        config.validate();
        assert!(
            dtype.is_float(),
            "LlamaModel::new_with_dtype currently requires floating runtime dtype, got {:?}",
            dtype
        );
        let embed_tokens = Embedding::new_with_dtype(config.vocab_size, config.hidden_size, dtype);

        let mut layers = Vec::new();
        for _ in 0..config.num_hidden_layers {
            layers.push(LlamaDecoderLayer::new_with_dtype(&config, dtype));
        }

        let norm = RMSNorm::new_with_dtype(config.hidden_size, config.rms_norm_eps, dtype);
        let lm_head = Linear::new_no_bias_with_dtype(config.hidden_size, config.vocab_size, dtype);

        Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            activation_dtype: dtype,
            kv_cache_dtype: dtype,
            config,
        }
    }

    pub fn new_with_runtime_dtypes(
        config: LlamaConfig,
        parameter_dtype: DType,
        activation_dtype: DType,
        kv_cache_dtype: DType,
    ) -> Self {
        config.validate();
        assert!(
            kv_cache_dtype.is_float(),
            "LlamaModel KV cache dtype currently only supports floating types, got {:?}",
            kv_cache_dtype
        );
        let embed_tokens =
            Embedding::new_with_dtype(config.vocab_size, config.hidden_size, parameter_dtype);

        let mut layers = Vec::new();
        for _ in 0..config.num_hidden_layers {
            layers.push(LlamaDecoderLayer::new_with_dtypes(
                &config,
                parameter_dtype,
                activation_dtype,
                kv_cache_dtype,
            ));
        }

        let norm =
            RMSNorm::new_with_dtype(config.hidden_size, config.rms_norm_eps, parameter_dtype);
        let lm_head =
            Linear::new_no_bias_with_dtype(config.hidden_size, config.vocab_size, parameter_dtype);

        Self {
            embed_tokens,
            layers,
            norm,
            lm_head,
            config,
            activation_dtype,
            kv_cache_dtype,
        }
    }

    pub fn new_with_dtypes(
        config: LlamaConfig,
        parameter_dtype: DType,
        runtime_dtype: DType,
    ) -> Self {
        Self::new_with_runtime_dtypes(config, parameter_dtype, runtime_dtype, runtime_dtype)
    }

    /// 为推理/生成初始化每层 KV cache。
    ///
    /// SelfAttention 的 cache 内部会维护 `len`，所以推理阶段不再需要显式传 `pos`。
    pub fn init_kv_caches(&self, batch_size: usize) -> Vec<KVCache> {
        self.init_kv_caches_with_dtype(batch_size, None)
    }

    pub fn init_kv_caches_with_dtype(
        &self,
        batch_size: usize,
        dtype: Option<DType>,
    ) -> Vec<KVCache> {
        assert!(batch_size > 0, "KV cache batch size must be > 0");
        let head_dim = self.config.hidden_size / self.config.num_attention_heads;
        let h_kv = self.config.num_key_value_heads;
        let max_seq = self.config.max_seq_len;
        let cache_device = self.embed_tokens.weight.device();

        (0..self.config.num_hidden_layers)
            .map(|_| {
                let mut cache = match dtype {
                    Some(dtype) => {
                        KVCacheInner::new_with_dtype(batch_size, h_kv, max_seq, head_dim, dtype)
                    }
                    None => KVCacheInner::new_with_dtype(
                        batch_size,
                        h_kv,
                        max_seq,
                        head_dim,
                        self.kv_cache_dtype,
                    ),
                };
                cache.to_device_inplace(cache_device);
                Rc::new(RefCell::new(cache))
            })
            .collect()
    }

    /// 重置 cache（在新对话/新样本开始前调用）。
    pub fn reset_kv_caches(&self, caches: &mut [KVCache]) {
        self.assert_cache_count(caches);
        for c in caches {
            c.borrow_mut().reset();
        }
    }

    pub fn cast_kv_caches(&self, caches: &mut [KVCache], dtype: DType) {
        self.assert_cache_count(caches);
        for c in caches {
            c.borrow_mut().cast_inplace(dtype);
        }
    }

    fn assert_cache_count(&self, caches: &[KVCache]) {
        assert_eq!(
            caches.len(),
            self.layers.len(),
            "KV cache count mismatch: got {}, expected {}",
            caches.len(),
            self.layers.len()
        );
    }

    fn validate_input_ids(&self, input_ids: &Tensor, context: &str) -> usize {
        let shape = input_ids.shape_vec();
        assert_eq!(shape.len(), 2, "{} input_ids must be [B,S]", context);
        let (batch_size, seq_len) = (shape[0], shape[1]);
        assert!(batch_size > 0, "{} batch size must be > 0", context);
        assert!(seq_len > 0, "{} sequence length must be > 0", context);
        batch_size
    }

    fn validate_infer_caches(&self, caches: &[KVCache], batch_size: usize) {
        self.assert_cache_count(caches);
        for (layer_idx, (layer, cache)) in self.layers.iter().zip(caches.iter()).enumerate() {
            layer.self_attn.assert_cache_compatible(
                cache,
                batch_size,
                &format!("Llama layer {} KV cache", layer_idx),
            );
        }
    }

    fn forward_hidden_infer(&self, input_ids: Tensor, caches: &mut Vec<KVCache>) -> Tensor {
        let batch_size = self.validate_input_ids(&input_ids, "inference");
        self.validate_infer_caches(caches, batch_size);

        // Embedding: [B,S] -> [B,S,H]
        let mut x = self.embed_tokens.forward(&input_ids);

        // Decoder Layers
        for (i, layer) in self.layers.iter().enumerate() {
            let cache_in = caches[i].clone();
            let (y, cache_out) = layer.forward_infer(x, cache_in);
            caches[i] = cache_out;
            x = y;
        }

        self.norm.forward(x)
    }

    fn lm_head_argmax_batch_from_last_hidden(&self, hidden: &Tensor) -> Vec<usize> {
        assert!(is_no_grad(), "forward_last_argmax is inference-only");

        let hidden_shape = hidden.shape_vec();
        assert_eq!(hidden_shape.len(), 3, "hidden states must be [B,S,H]");
        let (b, s, h) = (hidden_shape[0], hidden_shape[1], hidden_shape[2]);
        assert!(s >= 1, "sequence length must be >= 1");
        assert!(
            hidden.is_cuda() == self.lm_head.weight.is_cuda(),
            "lm_head argmax requires hidden states and lm_head weight on the same device; move both to CUDA or both to CPU"
        );
        if hidden.is_cuda() {
            let weight_shape = self.lm_head.weight.shape_vec();
            assert_eq!(weight_shape.len(), 2, "lm_head weight must be [V,H]");
            let (vocab, in_features) = (weight_shape[0], weight_shape[1]);
            assert_eq!(in_features, h, "lm_head in_features mismatch");
            let last_hidden = last_hidden_token_tensor(hidden.clone());
            if let (Some(hidden_buf), Some(weight_buf)) = (
                last_hidden.cloned_cuda_f32_buffer(),
                self.lm_head.weight.cloned_cuda_f32_buffer(),
            ) {
                match cuda::matvec_argmax_f32(&hidden_buf, &weight_buf, b, vocab, h) {
                    Ok(out) => return out,
                    Err(err) if is_strict_device_execution() => {
                        panic!("CUDA lm_head argmax failed: {}", err);
                    }
                    Err(_) => {}
                }
            } else if is_strict_device_execution() {
                panic!("CUDA lm_head argmax requires resident CUDA data");
            }
        }
        let mut out = vec![0usize; b];
        if self.lm_head.weight.dtype() == DType::I8 {
            let weight_owned = self.lm_head.weight.native_storage_owned();
            for_each_last_hidden_token_as_slice_ref(hidden, |bb, hidden_slice| {
                out[bb] = match &weight_owned {
                    TensorStorageOwned::I8(weight_data, scale) => {
                        let weight2 = weight_data
                            .view()
                            .into_dimensionality::<ndarray::Ix2>()
                            .expect("lm_head weight must be [V,H]");
                        let (vocab, in_features) = weight2.dim();
                        let weight_slice = SliceRef::I8(
                            weight2
                                .as_slice()
                                .expect("lm_head weight must be contiguous row-major"),
                            *scale,
                        );
                        assert_eq!(in_features, h, "lm_head in_features mismatch");
                        matvec_argmax_rowmajor_parallel_mixed(
                            hidden_slice,
                            weight_slice,
                            vocab,
                            in_features,
                        )
                    }
                    TensorStorageOwned::F32(_)
                    | TensorStorageOwned::F16(_)
                    | TensorStorageOwned::BF16(_) => unreachable!("checked I8 weight above"),
                };
            });
            return out;
        }

        for_each_last_hidden_token_as_slice_ref(hidden, |bb, hidden_slice| {
            let hidden_dtype = slice_ref_dtype(hidden_slice);
            out[bb] = self
                .lm_head
                .weight
                .with_storage_view_for_input_dtype_and_route(
                    hidden_dtype,
                    KernelRouteClass::Argmax,
                    |weight_view| {
                        macro_rules! run_argmax {
                            ($weight_slice:expr, $vocab:expr, $in_features:expr) => {{
                                assert_eq!($in_features, h, "lm_head in_features mismatch");
                                matvec_argmax_rowmajor_parallel_mixed(
                                    hidden_slice,
                                    $weight_slice,
                                    $vocab,
                                    $in_features,
                                )
                            }};
                        }

                        match weight_view {
                            TensorStorageView::F32(weight_view) => {
                                let weight2 = weight_view
                                    .into_dimensionality::<ndarray::Ix2>()
                                    .expect("lm_head weight must be [V,H]");
                                let (vocab, in_features) = weight2.dim();
                                let weight_slice = SliceRef::F32(
                                    weight2
                                        .as_slice()
                                        .expect("lm_head weight must be contiguous row-major"),
                                );
                                run_argmax!(weight_slice, vocab, in_features)
                            }
                            TensorStorageView::F16(weight_view) => {
                                let weight2 = weight_view
                                    .into_dimensionality::<ndarray::Ix2>()
                                    .expect("lm_head weight must be [V,H]");
                                let (vocab, in_features) = weight2.dim();
                                let weight_slice = SliceRef::F16(
                                    weight2
                                        .as_slice()
                                        .expect("lm_head weight must be contiguous row-major"),
                                );
                                run_argmax!(weight_slice, vocab, in_features)
                            }
                            TensorStorageView::BF16(weight_view) => {
                                let weight2 = weight_view
                                    .into_dimensionality::<ndarray::Ix2>()
                                    .expect("lm_head weight must be [V,H]");
                                let (vocab, in_features) = weight2.dim();
                                let weight_slice = SliceRef::BF16(
                                    weight2
                                        .as_slice()
                                        .expect("lm_head weight must be contiguous row-major"),
                                );
                                run_argmax!(weight_slice, vocab, in_features)
                            }
                        }
                    },
                );
        });
        out
    }

    fn lm_head_argmax_from_last_hidden(&self, hidden: &Tensor) -> usize {
        let out = self.lm_head_argmax_batch_from_last_hidden(hidden);
        assert_eq!(
            out.len(),
            1,
            "forward_last_argmax returns a single token; use forward_last_argmax_batch for batch inputs"
        );
        out[0]
    }

    /// 推理/生成（需要 caches）。
    ///
    /// `pos` 参数为了兼容旧调用方保留，但会被忽略：长度由 cache 内部维护。
    pub fn forward(&self, input_ids: Tensor, caches: &mut Vec<KVCache>, _pos: usize) -> Tensor {
        let x = self.forward_hidden_infer(input_ids, caches);

        self.lm_head.forward(x)
    }

    /// 生成/benchmark 专用：只返回最后一个位置的 logits。
    ///
    /// - prefill(S>1) 时，避免对整段序列都跑 lm_head
    /// - decode(S=1) 时，等价于普通 forward
    pub fn forward_last_logits(
        &self,
        input_ids: Tensor,
        caches: &mut Vec<KVCache>,
        _pos: usize,
    ) -> Tensor {
        let x = self.forward_hidden_infer(input_ids, caches);

        let x_shape = x.shape_vec();
        assert_eq!(x_shape.len(), 3, "hidden states must be [B,S,H]");
        let (b, h) = (x_shape[0], x_shape[2]);

        let last_hidden = last_hidden_token_tensor(x);

        debug_assert_eq!(last_hidden.shape_vec(), vec![b, 1, h]);

        self.lm_head.forward(last_hidden)
    }

    /// 生成/benchmark 热路径：直接返回最后一个位置的 greedy argmax token。
    ///
    /// - decode(S=1) 时，避免物化 [1,1,V] logits Tensor
    /// - prefill(S>1) 时，也只扫描最后一个位置对应的 hidden
    pub fn forward_last_argmax(
        &self,
        input_ids: Tensor,
        caches: &mut Vec<KVCache>,
        _pos: usize,
    ) -> usize {
        let x = self.forward_hidden_infer(input_ids, caches);
        self.lm_head_argmax_from_last_hidden(&x)
    }

    pub fn forward_last_argmax_batch(
        &self,
        input_ids: Tensor,
        caches: &mut Vec<KVCache>,
        _pos: usize,
    ) -> Vec<usize> {
        let x = self.forward_hidden_infer(input_ids, caches);
        self.lm_head_argmax_batch_from_last_hidden(&x)
    }

    /// 训练（不使用 cache，支持 autograd）。
    pub fn forward_train(&self, input_ids: Tensor) -> Tensor {
        self.validate_input_ids(&input_ids, "training");
        let mut x = self.embed_tokens.forward(&input_ids);
        for layer in self.layers.iter() {
            x = layer.forward_train(x);
        }
        x = self.norm.forward(x);
        self.lm_head.forward(x)
    }

    pub fn named_parameters(&self) -> HashMap<String, Tensor> {
        let mut params = HashMap::new();

        // Embedding
        params.insert(
            "model.embed_tokens.weight".to_string(),
            self.embed_tokens.weight.clone(),
        );

        // Layers
        for (i, layer) in self.layers.iter().enumerate() {
            let prefix = format!("model.layers.{}", i);

            // Self Attention
            params.insert(
                format!("{}.self_attn.q_proj.weight", prefix),
                layer.self_attn.w_q.weight.clone(),
            );
            params.insert(
                format!("{}.self_attn.k_proj.weight", prefix),
                layer.self_attn.w_k.weight.clone(),
            );
            params.insert(
                format!("{}.self_attn.v_proj.weight", prefix),
                layer.self_attn.w_v.weight.clone(),
            );
            params.insert(
                format!("{}.self_attn.o_proj.weight", prefix),
                layer.self_attn.w_o.weight.clone(),
            );

            // MLP
            params.insert(
                format!("{}.mlp.gate_proj.weight", prefix),
                layer.mlp.gate_proj.weight.clone(),
            );
            params.insert(
                format!("{}.mlp.up_proj.weight", prefix),
                layer.mlp.up_proj.weight.clone(),
            );
            params.insert(
                format!("{}.mlp.down_proj.weight", prefix),
                layer.mlp.down_proj.weight.clone(),
            );

            // Layernorms
            params.insert(
                format!("{}.input_layernorm.weight", prefix),
                layer.input_layernorm.weight.clone(),
            );
            params.insert(
                format!("{}.post_attention_layernorm.weight", prefix),
                layer.post_attention_layernorm.weight.clone(),
            );
        }

        // Final Norm & Head
        params.insert("model.norm.weight".to_string(), self.norm.weight.clone());
        params.insert("lm_head.weight".to_string(), self.lm_head.weight.clone());

        params
    }
}

impl Module for LlamaModel {
    fn forward(&self, input: Tensor) -> Tensor {
        // 训练/全序列前向：不使用 KV cache（可导）。
        self.forward_train(input)
    }

    fn parameters(&self) -> Vec<Tensor> {
        // 收集所有参数
        let mut params = vec![self.embed_tokens.weight.clone()];
        for layer in &self.layers {
            params.extend(layer.self_attn.w_q.parameters());
            params.extend(layer.self_attn.w_k.parameters());
            params.extend(layer.self_attn.w_v.parameters());
            params.extend(layer.self_attn.w_o.parameters());
            params.extend(layer.mlp.gate_proj.parameters());
            params.extend(layer.mlp.up_proj.parameters());
            params.extend(layer.mlp.down_proj.parameters());
            params.push(layer.input_layernorm.weight.clone());
            params.push(layer.post_attention_layernorm.weight.clone());
        }
        params.push(self.norm.weight.clone());
        params.push(self.lm_head.weight.clone());
        params
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autograd::no_grad;
    #[cfg(feature = "cuda")]
    use crate::autograd::set_strict_device_execution;
    #[cfg(feature = "cuda")]
    use crate::loss::CrossEntropyLoss;
    use crate::precision::{
        PrecisionConfig, set_default_runtime_dtype, with_precision_config,
        with_runtime_component_dtypes,
    };
    use ndarray::ArrayD;
    use ndarray::IxDyn;

    fn test_config() -> LlamaConfig {
        LlamaConfig {
            vocab_size: 32,
            hidden_size: 8,
            intermediate_size: 16,
            num_hidden_layers: 1,
            num_attention_heads: 2,
            num_key_value_heads: 1,
            rms_norm_eps: 1e-5,
            max_seq_len: 8,
            rope_theta: 10000.0,
        }
    }

    fn input_ids(shape: &[usize], ids: Vec<f32>) -> Tensor {
        Tensor::from_array_no_grad(
            ArrayD::from_shape_vec(IxDyn(shape), ids).expect("input_ids shape"),
        )
    }

    #[cfg(feature = "cuda")]
    fn one_hot_targets(rows: usize, cols: usize) -> Tensor {
        let mut data = vec![0.0; rows * cols];
        for row in 0..rows {
            data[row * cols + (row * 5 + 3) % cols] = 1.0;
        }
        Tensor::from_array_no_grad(
            ArrayD::from_shape_vec(IxDyn(&[rows, cols]), data).expect("target shape"),
        )
    }

    #[test]
    fn llama_model_explicit_dtype_overrides_global_default() {
        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let model = LlamaModel::new_with_dtype(test_config(), DType::F32);
                for param in model.parameters() {
                    assert_eq!(param.dtype(), DType::F32);
                }
            },
        );
    }

    #[test]
    fn llama_model_default_construction_captures_runtime_dtype_for_future_caches() {
        let config = test_config();

        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::BF16,
                allow_parameter_dtype_copies: false,
            },
            || {
                let model = LlamaModel::new(config.clone());
                set_default_runtime_dtype(DType::F32);
                let caches = model.init_kv_caches(1);
                assert_eq!(caches[0].borrow().dtype, DType::BF16);
                for param in model.parameters() {
                    assert_eq!(param.dtype(), DType::F32);
                }
            },
        );
    }

    #[test]
    fn llama_model_explicit_dtypes_override_global_defaults() {
        let config = test_config();

        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::BF16,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                let model = LlamaModel::new_with_dtypes(config, DType::F32, DType::BF16);
                for param in model.parameters() {
                    assert_eq!(param.dtype(), DType::F32);
                }
                let caches = model.init_kv_caches(1);
                assert_eq!(caches[0].borrow().dtype, DType::BF16);
            },
        );
    }

    #[test]
    fn llama_model_default_construction_can_override_activation_and_kv_cache_dtypes_independently()
    {
        let config = test_config();

        with_precision_config(
            PrecisionConfig {
                parameter_dtype: DType::F32,
                runtime_dtype: DType::F32,
                allow_parameter_dtype_copies: false,
            },
            || {
                with_runtime_component_dtypes(Some(DType::BF16), Some(DType::F16), || {
                    let model = LlamaModel::new(config.clone());
                    assert_eq!(model.activation_dtype(), DType::BF16);
                    assert_eq!(model.kv_cache_dtype(), DType::F16);

                    let caches = model.init_kv_caches(1);
                    assert_eq!(caches[0].borrow().dtype, DType::F16);
                    for param in model.parameters() {
                        assert_eq!(param.dtype(), DType::F32);
                    }
                });
            },
        );
    }

    #[test]
    fn last_hidden_token_tensor_preserves_bf16_storage() {
        no_grad(|| {
            let hidden = Tensor::new_with_dtype(
                ArrayD::from_shape_vec(
                    IxDyn(&[1, 3, 4]),
                    vec![
                        0.1, 0.2, 0.3, 0.4, 1.0, 1.1, 1.2, 1.3, -0.5, -0.4, -0.3, -0.2,
                    ],
                )
                .expect("hidden shape"),
                DType::BF16,
            );
            let last = last_hidden_token_tensor(hidden);
            assert_eq!(last.shape_vec(), vec![1, 1, 4]);
            match last.native_storage_owned() {
                TensorStorageOwned::BF16(data) => {
                    let values = data.iter().map(|v| v.to_f32()).collect::<Vec<_>>();
                    assert_eq!(values.len(), 4);
                    assert!((values[0] + 0.5).abs() < 0.02);
                    assert!((values[3] + 0.2).abs() < 0.02);
                }
                TensorStorageOwned::F32(_) => {
                    panic!("last hidden token should not materialize f32")
                }
                TensorStorageOwned::F16(_) => panic!("last hidden token should stay bf16"),
                TensorStorageOwned::I8(_, _) => panic!("last hidden token should stay bf16"),
            }
        });
    }

    #[test]
    fn llama_model_allows_i8_activation_dtype_with_float_kv_cache() {
        let model =
            LlamaModel::new_with_runtime_dtypes(test_config(), DType::I8, DType::I8, DType::BF16);
        assert_eq!(model.activation_dtype(), DType::I8);
        assert_eq!(model.kv_cache_dtype(), DType::BF16);
    }

    #[test]
    #[should_panic(expected = "vocab_size must be > 0")]
    fn llama_model_rejects_invalid_config() {
        let mut config = test_config();
        config.vocab_size = 0;
        let _ = LlamaModel::new(config);
    }

    #[test]
    #[should_panic(expected = "inference input_ids must be [B,S]")]
    fn llama_model_rejects_non_matrix_inference_ids() {
        let model = LlamaModel::new(test_config());
        let mut caches = model.init_kv_caches(1);
        no_grad(|| {
            let _ = model.forward(input_ids(&[1], vec![0.0]), &mut caches, 0);
        });
    }

    #[test]
    #[should_panic(expected = "Llama layer 0 KV cache shape mismatch")]
    fn llama_model_rejects_cache_batch_mismatch() {
        let model = LlamaModel::new(test_config());
        let mut caches = model.init_kv_caches(2);
        no_grad(|| {
            let _ = model.forward(input_ids(&[1, 1], vec![0.0]), &mut caches, 0);
        });
    }

    #[test]
    fn forward_last_argmax_batch_matches_logits_argmax() {
        let model = LlamaModel::new_with_dtype(test_config(), DType::BF16);
        let input = input_ids(&[2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
        let mut logits_caches = model.init_kv_caches(2);
        let mut argmax_caches = model.init_kv_caches(2);

        let logits = no_grad(|| model.forward_last_logits(input.clone(), &mut logits_caches, 0));
        let got = no_grad(|| model.forward_last_argmax_batch(input, &mut argmax_caches, 0));
        let logits_vals = logits.data_ref().iter().copied().collect::<Vec<_>>();
        let vocab = test_config().vocab_size;
        let expected = logits_vals
            .chunks(vocab)
            .map(|row| {
                row.iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(idx, _)| idx)
                    .expect("logits row must not be empty")
            })
            .collect::<Vec<_>>();

        assert_eq!(got, expected);
    }

    #[test]
    fn forward_last_argmax_single_batch_keeps_old_return_type() {
        let model = LlamaModel::new_with_dtype(test_config(), DType::F16);
        let input = input_ids(&[1, 3], vec![1.0, 2.0, 3.0]);
        let mut batch_caches = model.init_kv_caches(1);
        let mut single_caches = model.init_kv_caches(1);

        let batch =
            no_grad(|| model.forward_last_argmax_batch(input.clone(), &mut batch_caches, 0));
        let single = no_grad(|| model.forward_last_argmax(input, &mut single_caches, 0));

        assert_eq!(batch, vec![single]);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_forward_last_argmax_batch_matches_logits_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let config = test_config();
        let model = LlamaModel::new_with_dtype(config.clone(), DType::BF16);
        model.to_cuda();
        let input = input_ids(&[2, 3], vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).to_cuda();
        let mut logits_caches = model.init_kv_caches(2);
        let mut argmax_caches = model.init_kv_caches(2);

        set_strict_device_execution(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            no_grad(|| {
                let logits = model.forward_last_logits(input.clone(), &mut logits_caches, 0);
                let got = model.forward_last_argmax_batch(input, &mut argmax_caches, 0);
                (logits, got)
            })
        }));
        set_strict_device_execution(false);
        let (logits, got) = result.unwrap();

        assert!(logits.is_cuda());
        let logits_vals = logits.data_ref().iter().copied().collect::<Vec<_>>();
        let expected = logits_vals
            .chunks(config.vocab_size)
            .map(|row| {
                row.iter()
                    .enumerate()
                    .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(idx, _)| idx)
                    .expect("logits row must not be empty")
            })
            .collect::<Vec<_>>();

        assert_eq!(got, expected);
    }

    #[test]
    #[should_panic(expected = "KV cache count mismatch")]
    fn llama_model_reset_rejects_cache_count_mismatch() {
        let model = LlamaModel::new(test_config());
        let mut caches = model.init_kv_caches(1);
        caches.pop();
        model.reset_kv_caches(&mut caches);
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_model_init_kv_caches_follow_model_device() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let model = LlamaModel::new_with_dtype(test_config(), DType::BF16);
        model.to_cuda();
        let caches = model.init_kv_caches(1);
        assert!(caches[0].borrow().k.is_cuda());
        assert!(caches[0].borrow().v.is_cuda());
    }

    #[cfg(feature = "cuda")]
    fn assert_llama_forward_last_logits_stays_on_cuda(dtype: DType) {
        let model = LlamaModel::new_with_dtype(test_config(), dtype);
        model.to_cuda();
        let mut caches = model.init_kv_caches(1);
        let input_cuda = input_ids(&[1, 4], vec![1.0, 2.0, 3.0, 4.0]).to_cuda();

        set_strict_device_execution(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            no_grad(|| model.forward_last_logits(input_cuda, &mut caches, 0))
        }));
        set_strict_device_execution(false);
        let logits = result.unwrap();

        assert!(logits.is_cuda());
        assert_eq!(logits.shape_vec(), vec![1, 1, test_config().vocab_size]);
        assert!(logits.cloned_cuda_f32_buffer().is_some());
        assert_eq!(logits.dtype(), dtype);
        assert!(caches[0].borrow().k.is_cuda());
        assert!(caches[0].borrow().v.is_cuda());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_forward_last_logits_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        for dtype in [DType::F32, DType::F16, DType::BF16] {
            assert_llama_forward_last_logits_stays_on_cuda(dtype);
        }
    }

    #[cfg(feature = "cuda")]
    fn assert_llama_prefill_then_decode_stays_on_cuda(dtype: DType) {
        let model = LlamaModel::new_with_dtype(test_config(), dtype);
        model.to_cuda();
        let mut caches = model.init_kv_caches(1);
        let prefill_cuda = input_ids(&[1, 4], vec![1.0, 2.0, 3.0, 4.0]).to_cuda();
        let decode_cuda = input_ids(&[1, 1], vec![5.0]).to_cuda();

        set_strict_device_execution(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            no_grad(|| {
                let prefill_logits = model.forward_last_logits(prefill_cuda, &mut caches, 0);
                let decode_logits = model.forward_last_logits(decode_cuda, &mut caches, 4);
                (prefill_logits, decode_logits)
            })
        }));
        set_strict_device_execution(false);
        let (prefill_logits, decode_logits) = result.unwrap();

        assert!(prefill_logits.is_cuda());
        assert!(decode_logits.is_cuda());
        assert_eq!(prefill_logits.dtype(), dtype);
        assert_eq!(decode_logits.dtype(), dtype);
        assert_eq!(
            prefill_logits.shape_vec(),
            vec![1, 1, test_config().vocab_size]
        );
        assert_eq!(
            decode_logits.shape_vec(),
            vec![1, 1, test_config().vocab_size]
        );
        assert!(prefill_logits.cloned_cuda_f32_buffer().is_some());
        assert!(decode_logits.cloned_cuda_f32_buffer().is_some());
        assert!(caches[0].borrow().k.is_cuda());
        assert!(caches[0].borrow().v.is_cuda());
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_prefill_then_decode_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        for dtype in [DType::F32, DType::F16, DType::BF16] {
            assert_llama_prefill_then_decode_stays_on_cuda(dtype);
        }
    }

    #[cfg(feature = "cuda")]
    fn assert_llama_model_training_backward_stays_on_cuda(dtype: DType) {
        let config = test_config();
        let model = LlamaModel::new_with_dtype(config.clone(), dtype);
        model.to_cuda();
        let input_cuda = input_ids(&[1, 4], vec![1.0, 2.0, 3.0, 4.0]).to_cuda();
        let targets_cuda = one_hot_targets(4, config.vocab_size).to_cuda();

        set_strict_device_execution(true);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let logits = model.forward_train(input_cuda);
            assert!(logits.is_cuda());
            let loss = CrossEntropyLoss::apply(
                &logits.reshape(vec![-1, config.vocab_size as i32]),
                &targets_cuda,
            );
            loss.backward();
        }));
        set_strict_device_execution(false);
        result.unwrap();

        let params = model.parameters();
        assert!(!params.is_empty());
        for param in params {
            assert!(param.cloned_cuda_f32_grad().is_some());
            assert!(!param.has_host_grad());
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_model_training_backward_stays_on_cuda_in_strict_mode() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        for dtype in [DType::F32, DType::F16, DType::BF16] {
            assert_llama_model_training_backward_stays_on_cuda(dtype);
        }
    }

    #[cfg(feature = "cuda")]
    #[test]
    fn llama_mlp_decode_fast_path_stays_on_cuda_and_matches_cpu() {
        if !crate::ops::cuda::is_available() {
            return;
        }

        let config = test_config();
        let mlp = LlamaMLP::new_with_dtype(&config, DType::BF16);
        let mlp_ref = LlamaMLP::new_with_dtype(&config, DType::BF16);

        for ((dst, src), shape) in [
            (
                (&mlp.gate_proj.weight, &mlp_ref.gate_proj.weight),
                vec![config.intermediate_size, config.hidden_size],
            ),
            (
                (&mlp.up_proj.weight, &mlp_ref.up_proj.weight),
                vec![config.intermediate_size, config.hidden_size],
            ),
            (
                (&mlp.down_proj.weight, &mlp_ref.down_proj.weight),
                vec![config.hidden_size, config.intermediate_size],
            ),
        ] {
            let data = (0..shape.iter().product::<usize>())
                .map(|i| (i as f32 * 0.03125) - 0.5)
                .collect::<Vec<_>>();
            let arr = ArrayD::from_shape_vec(IxDyn(&shape), data).expect("weight shape");
            dst.set_array_f32_with_dtype(arr.clone(), DType::BF16);
            src.set_array_f32_with_dtype(arr, DType::BF16);
        }

        mlp.gate_proj.weight.to_cuda_inplace();
        mlp.up_proj.weight.to_cuda_inplace();
        mlp.down_proj.weight.to_cuda_inplace();

        let input = Tensor::new_with_dtype(
            ArrayD::from_shape_vec(
                IxDyn(&[1, 1, config.hidden_size]),
                (0..config.hidden_size)
                    .map(|i| (i as f32 * 0.125) - 0.25)
                    .collect(),
            )
            .expect("input shape"),
            DType::BF16,
        )
        .to_cuda();

        crate::autograd::set_strict_device_execution(true);
        let cuda_out = no_grad(|| mlp.forward(input.clone()));
        crate::autograd::set_strict_device_execution(false);

        let cpu_out = no_grad(|| mlp_ref.forward(input.to_cpu()));
        assert!(cuda_out.is_cuda());
        assert_eq!(cuda_out.shape_vec(), cpu_out.shape_vec());
        for (got, expect) in cuda_out.data_ref().iter().zip(cpu_out.data_ref().iter()) {
            assert!((got - expect).abs() < 3e-2, "got {got}, expect {expect}");
        }
    }
}
