use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

use trueos_fpga_abi::lfm25::{self, LayerKind, NativeTensorDescriptor, TensorRole};
use trueos_fpga_abi::lfm25_decode::{self, LayerStateSlot};
use trueos_lfm25_cpu as cpu;

const HIDDEN: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
const HEADS: usize = lfm25::MODEL_ATTENTION_HEADS as usize;
const KV_HEADS: usize = lfm25::MODEL_KV_HEADS as usize;
const HEAD_DIM: usize = lfm25::MODEL_HEAD_DIMENSION as usize;
const KV_ELEMENTS: usize = KV_HEADS * HEAD_DIM;
const FFN: usize = lfm25::MODEL_FEED_FORWARD_SIZE as usize;
const ATTENTION_SLOTS: usize = 256;
const TOKENS: [u32; 10] = [1, 6, 6423, 708, 6928, 7, 708, 6, 64015, 708];
const SIDECAR_SHA256: [u8; 32] = [
    0xa6, 0x0c, 0x0d, 0x28, 0xe5, 0xe0, 0xf4, 0x83, 0x06, 0x99, 0x26, 0x0f, 0xbd, 0x9c, 0x01, 0x15,
    0x37, 0x63, 0x26, 0x1a, 0x7b, 0x13, 0x2a, 0x6b, 0x44, 0x61, 0x0d, 0x64, 0x91, 0x96, 0x09, 0xb1,
];

// This executable is the stateful end-to-end replay. Isolated CPU and FPGA
// stage bounds are enforced by their respective kernel/golden tests.
const LOCAL_CPU_STAGE_BOUND: f32 = 1.0e-5;
// The immutable row-stream Q30 accumulator's measured all-position envelope.
// The historical layer-0 sealed fixture retains its tighter 2e-6 gate.
const ISOLATED_FPGA_STAGE_BOUND: f32 = 4.0e-6;
const HIDDEN_END_TO_END_BOUND: f32 = 1.0e-3;
const INTERMEDIATE_END_TO_END_BOUND: f32 = 1.0e-2;
const LOGIT_END_TO_END_BOUND: f32 = 1.0e-2;

#[derive(Default)]
struct KvCache {
    keys: Vec<u16>,
    values: Vec<u16>,
}

struct GoldenCheckpoint {
    name: String,
    values: Vec<f32>,
}

struct GoldenTrace {
    tokens: Vec<u32>,
    output_tokens: Vec<u32>,
    checkpoints: Vec<Vec<GoldenCheckpoint>>,
}

struct TokenReplay<'a> {
    token_index: usize,
    checkpoints: &'a [GoldenCheckpoint],
    cursor: usize,
    maximum: f32,
    isolated_fpga_maximum: f32,
}

impl TokenReplay<'_> {
    fn check(&mut self, name: &str, actual: &[f32], bound: f32) -> Result<(), String> {
        let expected = self.checkpoints.get(self.cursor).ok_or_else(|| {
            format!("token={} missing checkpoint={} name={name}", self.token_index, self.cursor)
        })?;
        if expected.name != name || expected.values.len() != actual.len() {
            return Err(format!(
                "token={} checkpoint={} contract mismatch actual_name={} expected_name={} actual_elements={} expected_elements={}",
                self.token_index,
                self.cursor,
                name,
                expected.name,
                actual.len(),
                expected.values.len()
            ));
        }
        let mut maximum = 0.0f32;
        let mut maximum_index = 0usize;
        for (index, (&actual, &expected)) in actual.iter().zip(&expected.values).enumerate() {
            let error = (actual - expected).abs();
            if error > maximum {
                maximum = error;
                maximum_index = index;
            }
        }
        self.maximum = self.maximum.max(maximum);
        if maximum > bound {
            return Err(format!(
                "FAIL first_checkpoint token={} checkpoint={} name={} max_abs={maximum:.9e} bound={bound:.9e} element={} actual={:.9e} expected={:.9e}",
                self.token_index,
                self.cursor,
                name,
                maximum_index,
                actual[maximum_index],
                expected.values[maximum_index],
            ));
        }
        self.cursor += 1;
        Ok(())
    }

    fn check_isolated_fpga(&mut self, name: &str, actual: &[f32]) -> Result<(), String> {
        let expected = self.checkpoints.get(self.cursor).ok_or_else(|| {
            format!(
                "token={} missing isolated checkpoint={} name={name}",
                self.token_index, self.cursor
            )
        })?;
        if expected.name != name || expected.values.len() != actual.len() {
            return Err(format!(
                "token={} isolated checkpoint={} contract mismatch actual_name={} expected_name={} actual_elements={} expected_elements={}",
                self.token_index,
                self.cursor,
                name,
                expected.name,
                actual.len(),
                expected.values.len()
            ));
        }
        let mut maximum = 0.0f32;
        let mut maximum_index = 0usize;
        for (index, (&actual, &expected)) in actual.iter().zip(&expected.values).enumerate() {
            let error = (actual - expected).abs();
            if error > maximum {
                maximum = error;
                maximum_index = index;
            }
        }
        self.isolated_fpga_maximum = self.isolated_fpga_maximum.max(maximum);
        if maximum > ISOLATED_FPGA_STAGE_BOUND {
            return Err(format!(
                "FAIL isolated_fpga token={} checkpoint={} name={} max_abs={maximum:.9e} bound={:.9e} element={} actual={:.9e} expected={:.9e}",
                self.token_index,
                self.cursor,
                name,
                ISOLATED_FPGA_STAGE_BOUND,
                maximum_index,
                actual[maximum_index],
                expected.values[maximum_index],
            ));
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), String> {
        if self.cursor != self.checkpoints.len() {
            return Err(format!(
                "token={} consumed checkpoints={} expected={}",
                self.token_index,
                self.cursor,
                self.checkpoints.len()
            ));
        }
        Ok(())
    }
}

fn main() -> Result<(), String> {
    let mut arguments = std::env::args();
    let program = arguments
        .next()
        .unwrap_or_else(|| "lfm25-cpu-parity".into());
    let native_path = arguments
        .next()
        .ok_or_else(|| format!("usage: {program} NATIVE.truega.bin CPU-F32.bin HI.golden.bin"))?;
    let sidecar_path = arguments
        .next()
        .ok_or_else(|| format!("usage: {program} NATIVE.truega.bin CPU-F32.bin HI.golden.bin"))?;
    let golden_path = arguments
        .next()
        .ok_or_else(|| format!("usage: {program} NATIVE.truega.bin CPU-F32.bin HI.golden.bin"))?;
    if arguments.next().is_some() {
        return Err(format!("usage: {program} NATIVE.truega.bin CPU-F32.bin HI.golden.bin"));
    }

    let native = fs::read(&native_path).map_err(|error| format!("read {native_path}: {error}"))?;
    verify_bytes(
        "native image",
        &native,
        lfm25::PINNED_NATIVE_IMAGE_BYTES as usize,
        lfm25::PINNED_NATIVE_IMAGE_SHA256,
    )?;
    let sidecar_bytes =
        fs::read(&sidecar_path).map_err(|error| format!("read {sidecar_path}: {error}"))?;
    verify_bytes("F32 sidecar", &sidecar_bytes, cpu::F32_SIDECAR_BYTES, SIDECAR_SHA256)?;
    let sidecar = cpu::F32Sidecar::from_artifact(&sidecar_bytes)
        .map_err(|error| format!("F32 sidecar rejected: {error:?}"))?;
    let golden = read_golden(Path::new(&golden_path))?;
    if golden.tokens != TOKENS {
        return Err(format!("golden input tokens {:?} != {:?}", golden.tokens, TOKENS));
    }
    if golden.output_tokens.first() != Some(&1) || golden.output_tokens.last() != Some(&36_309) {
        return Err(format!(
            "golden argmax gates token1={:?} hi={:?}",
            golden.output_tokens.first(),
            golden.output_tokens.last()
        ));
    }

    let embedding = descriptor(None, TensorRole::TokenEmbedding)?;
    let embedding_bytes = tensor(&native, embedding)?;
    let row_bytes = cpu::q8_row_bytes(HIDDEN).map_err(kernel)?;
    let mut shortconv = vec![vec![[0.0f32; 2]; HIDDEN]; lfm25_decode::SHORTCONV_STATE_COUNT];
    let mut kv: Vec<KvCache> = (0..lfm25_decode::KV_CACHE_COUNT)
        .map(|_| KvCache::default())
        .collect();

    for (position, (&token, checkpoints)) in TOKENS.iter().zip(&golden.checkpoints).enumerate() {
        let mut replay = TokenReplay {
            token_index: position,
            checkpoints,
            cursor: 0,
            maximum: 0.0,
            isolated_fpga_maximum: 0.0,
        };
        let mut hidden = vec![0.0f32; HIDDEN];
        let token = token as usize;
        cpu::dequantize_q8_row(
            &embedding_bytes[token * row_bytes..(token + 1) * row_bytes],
            &mut hidden,
        )
        .map_err(kernel)?;
        replay.check("model.embed_tokens", &hidden, HIDDEN_END_TO_END_BOUND)?;

        for layer in 0..lfm25::MODEL_LAYER_COUNT as u8 {
            let operator_residual = hidden.clone();
            let operator_weights =
                f32_tensor(&sidecar, descriptor(Some(layer), TensorRole::OperatorNorm)?)?;
            let normalized = cpu::rms_norm(&hidden, operator_weights).map_err(kernel)?;
            replay.check(
                &format!("model.layers.{{}}.operator_norm-{layer}"),
                &normalized,
                HIDDEN_END_TO_END_BOUND,
            )?;

            let branch = match lfm25::LAYER_SCHEDULE[layer as usize] {
                LayerKind::ShortConv => {
                    let slot = match lfm25_decode::state_slot_for_layer(layer) {
                        Some(LayerStateSlot::ShortConv(slot)) => slot as usize,
                        _ => return Err(format!("layer {layer} shortconv state contract")),
                    };
                    shortconv_layer(
                        &native,
                        &sidecar,
                        layer,
                        &normalized,
                        &mut shortconv[slot],
                        &mut replay,
                    )?
                }
                LayerKind::Attention => {
                    let slot = match lfm25_decode::state_slot_for_layer(layer) {
                        Some(LayerStateSlot::KvCache(slot)) => slot as usize,
                        _ => return Err(format!("layer {layer} K/V state contract")),
                    };
                    attention_layer(
                        &native,
                        &sidecar,
                        layer,
                        position,
                        &normalized,
                        &mut kv[slot],
                        &mut replay,
                    )?
                }
            };

            hidden = cpu::add(&operator_residual, &branch).map_err(kernel)?;
            replay.check(
                &format!("model.layers.{{}}.operator_residual-{layer}"),
                &hidden,
                HIDDEN_END_TO_END_BOUND,
            )?;

            let ffn_residual = hidden.clone();
            let ffn_weights = f32_tensor(&sidecar, descriptor(Some(layer), TensorRole::FfnNorm)?)?;
            let ffn_input = cpu::rms_norm(&hidden, ffn_weights).map_err(kernel)?;
            replay.check(
                &format!("model.layers.{{}}.ffn_norm-{layer}"),
                &ffn_input,
                HIDDEN_END_TO_END_BOUND,
            )?;

            let reference_ffn_input = replay.checkpoints[replay.cursor - 1].values.clone();
            let isolated_up_q30 = project_fpga_q30(
                &native,
                descriptor(Some(layer), TensorRole::FfnUp)?,
                &reference_ffn_input,
            )?;
            let isolated_up = cpu::q30_to_f32(&isolated_up_q30).map_err(kernel)?;
            replay.check_isolated_fpga(&format!("ffn_up-{layer}"), &isolated_up)?;
            let up = project(&native, descriptor(Some(layer), TensorRole::FfnUp)?, &ffn_input)?;
            replay.check(&format!("ffn_up-{layer}"), &up, LOCAL_CPU_STAGE_BOUND)?;
            let isolated_gate_q30 = project_fpga_q30(
                &native,
                descriptor(Some(layer), TensorRole::FfnGate)?,
                &reference_ffn_input,
            )?;
            let isolated_gate = cpu::q30_to_f32(&isolated_gate_q30).map_err(kernel)?;
            replay.check_isolated_fpga(&format!("ffn_gate-{layer}"), &isolated_gate)?;
            let gate = project(&native, descriptor(Some(layer), TensorRole::FfnGate)?, &ffn_input)?;
            replay.check(&format!("ffn_gate-{layer}"), &gate, LOCAL_CPU_STAGE_BOUND)?;
            if gate.len() != FFN || up.len() != FFN {
                return Err(format!("layer {layer} FFN shape"));
            }
            let activated: Vec<f32> = gate
                .iter()
                .zip(&up)
                .map(|(&gate, &up)| cpu::silu_mul_f32_pinned(gate, up).map_err(kernel))
                .collect::<Result<_, _>>()?;
            replay.check(&format!("ffn_swiglu-{layer}"), &activated, LOCAL_CPU_STAGE_BOUND)?;
            let reference_activated = replay.checkpoints[replay.cursor - 1].values.clone();
            let isolated_ffn_output_q30 = project_fpga_q30(
                &native,
                descriptor(Some(layer), TensorRole::FfnDown)?,
                &reference_activated,
            )?;
            let isolated_ffn_output = cpu::q30_to_f32(&isolated_ffn_output_q30).map_err(kernel)?;
            replay.check_isolated_fpga(
                &format!("model.layers.{{}}.ffn_out-{layer}"),
                &isolated_ffn_output,
            )?;
            let ffn_output =
                project(&native, descriptor(Some(layer), TensorRole::FfnDown)?, &activated)?;
            replay.check(
                &format!("model.layers.{{}}.ffn_out-{layer}"),
                &ffn_output,
                HIDDEN_END_TO_END_BOUND,
            )?;

            hidden = cpu::add(&ffn_residual, &ffn_output).map_err(kernel)?;
            replay.check(&format!("l_out-{layer}"), &hidden, HIDDEN_END_TO_END_BOUND)?;
        }

        let final_weights =
            f32_tensor(&sidecar, descriptor(None, TensorRole::TokenEmbeddingNorm)?)?;
        let normalized = cpu::rms_norm(&hidden, final_weights).map_err(kernel)?;
        replay.check("result_norm", &normalized, HIDDEN_END_TO_END_BOUND)?;
        let logits = cpu::q8_matrix_vector_quantized(
            embedding_bytes,
            lfm25::MODEL_VOCABULARY_SIZE as usize,
            HIDDEN,
            &normalized,
        )
        .map_err(kernel)?;
        replay.check("result_output", &logits, LOGIT_END_TO_END_BOUND)?;
        replay.finish()?;

        let output_token = argmax(&logits)?;
        if output_token != golden.output_tokens[position] as usize {
            return Err(format!(
                "token={} argmax={} expected={}",
                position, output_token, golden.output_tokens[position]
            ));
        }
        println!(
            "PASS token={} input={} argmax={} checkpoints={} max_abs={:.9e} isolated_fpga_max_abs={:.9e}",
            position,
            TOKENS[position],
            output_token,
            replay.cursor,
            replay.maximum,
            replay.isolated_fpga_maximum
        );
    }
    println!("PASS hi tokens={} final_argmax=36309 decoded_prefix=Hello", TOKENS.len());
    Ok(())
}

fn shortconv_layer(
    native: &[u8],
    sidecar: &cpu::F32Sidecar,
    layer: u8,
    input: &[f32],
    state: &mut [[f32; 2]],
    replay: &mut TokenReplay<'_>,
) -> Result<Vec<f32>, String> {
    let projected = project(native, descriptor(Some(layer), TensorRole::ShortConvInput)?, input)?;
    replay.check(
        &format!("model.layers.{{}}.conv.in_proj-{layer}"),
        &projected,
        HIDDEN_END_TO_END_BOUND,
    )?;
    if projected.len() != 3 * HIDDEN || state.len() != HIDDEN {
        return Err(format!("layer {layer} shortconv input shape"));
    }
    let kernel = f32_tensor(sidecar, descriptor(Some(layer), TensorRole::ShortConvKernel)?)?;
    let (b, rest) = projected.split_at(HIDDEN);
    let (c, x) = rest.split_at(HIDDEN);
    let mut bx_trace = Vec::with_capacity(3 * HIDDEN);
    let mut state_trace = Vec::with_capacity(2 * HIDDEN);
    let mut convolution = Vec::with_capacity(HIDDEN);
    let mut mixed = Vec::with_capacity(HIDDEN);
    for channel in 0..HIDDEN {
        let bx = b[channel] * x[channel];
        let kernel_base = channel * 3;
        bx_trace.extend_from_slice(&[state[channel][0], state[channel][1], bx]);
        let value = kernel[kernel_base] * state[channel][0]
            + kernel[kernel_base + 1] * state[channel][1]
            + kernel[kernel_base + 2] * bx;
        state[channel] = [state[channel][1], bx];
        state_trace.extend_from_slice(&state[channel]);
        convolution.push(value);
        mixed.push(c[channel] * value);
    }
    replay.check(
        &format!("model.layers.{{}}.conv.bx-{layer}"),
        &bx_trace,
        HIDDEN_END_TO_END_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.conv.state-{layer}"),
        &state_trace,
        HIDDEN_END_TO_END_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.conv.conv-{layer}"),
        &convolution,
        HIDDEN_END_TO_END_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.conv.mix-{layer}"),
        &mixed,
        HIDDEN_END_TO_END_BOUND,
    )?;
    let output = project(native, descriptor(Some(layer), TensorRole::ShortConvOutput)?, &mixed)?;
    replay.check(
        &format!("model.layers.{{}}.conv.out_proj-{layer}"),
        &output,
        HIDDEN_END_TO_END_BOUND,
    )?;
    Ok(output)
}

fn attention_layer(
    native: &[u8],
    sidecar: &cpu::F32Sidecar,
    layer: u8,
    position: usize,
    input: &[f32],
    cache: &mut KvCache,
    replay: &mut TokenReplay<'_>,
) -> Result<Vec<f32>, String> {
    let mut query = project(native, descriptor(Some(layer), TensorRole::Query)?, input)?;
    let mut key = project(native, descriptor(Some(layer), TensorRole::Key)?, input)?;
    let value = project(native, descriptor(Some(layer), TensorRole::Value)?, input)?;
    replay.check(&format!("Qcur-{layer}"), &query, HIDDEN_END_TO_END_BOUND)?;
    replay.check(&format!("Kcur-{layer}"), &key, HIDDEN_END_TO_END_BOUND)?;
    replay.check(&format!("Vcur-{layer}"), &value, HIDDEN_END_TO_END_BOUND)?;

    let query_norm = f32_tensor(sidecar, descriptor(Some(layer), TensorRole::QueryNorm)?)?;
    let key_norm = f32_tensor(sidecar, descriptor(Some(layer), TensorRole::KeyNorm)?)?;
    for head in query.chunks_exact_mut(HEAD_DIM) {
        cpu::rms_norm_head_in_place(head, query_norm).map_err(kernel)?;
    }
    for head in key.chunks_exact_mut(HEAD_DIM) {
        cpu::rms_norm_head_in_place(head, key_norm).map_err(kernel)?;
    }
    replay.check(
        &format!("model.layers.{{}}.self_attn.q_layernorm-{layer}"),
        &query,
        HIDDEN_END_TO_END_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.self_attn.k_layernorm-{layer}"),
        &key,
        HIDDEN_END_TO_END_BOUND,
    )?;
    for head in query.chunks_exact_mut(HEAD_DIM) {
        cpu::rope_neox_in_place(head, position as u32).map_err(kernel)?;
    }
    for head in key.chunks_exact_mut(HEAD_DIM) {
        cpu::rope_neox_in_place(head, position as u32).map_err(kernel)?;
    }
    replay.check(
        &format!("model.layers.{{}}.self_attn.q_rope-{layer}"),
        &query,
        HIDDEN_END_TO_END_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.self_attn.k_rope-{layer}"),
        &key,
        HIDDEN_END_TO_END_BOUND,
    )?;

    let expected_cache = position * KV_ELEMENTS;
    if cache.keys.len() != expected_cache || cache.values.len() != expected_cache {
        return Err(format!("layer {layer} cache position {position}"));
    }
    for value in key {
        cache.keys.push(cpu::f16_cache_bits(value).map_err(kernel)?);
    }
    for value in value {
        cache
            .values
            .push(cpu::f16_cache_bits(value).map_err(kernel)?);
    }
    let committed_keys: Vec<f32> = cache.keys[expected_cache..]
        .iter()
        .map(|&bits| cpu::f16_cache_f32(bits))
        .collect();
    let committed_values: Vec<f32> = cache.values[expected_cache..]
        .iter()
        .map(|&bits| cpu::f16_cache_f32(bits))
        .collect();
    replay.check(
        &format!("model.layers.{{}}.self_attn.k_cache_commit-{layer}"),
        &committed_keys,
        LOCAL_CPU_STAGE_BOUND,
    )?;
    replay.check(
        &format!("model.layers.{{}}.self_attn.v_cache_commit-{layer}"),
        &committed_values,
        LOCAL_CPU_STAGE_BOUND,
    )?;

    let positions = position + 1;
    let mut context = vec![0.0f32; HIDDEN];
    let scale = 1.0 / libm::sqrtf(HEAD_DIM as f32);
    let mut scores = Vec::with_capacity(positions);
    let mut raw_scores = vec![0.0f32; ATTENTION_SLOTS * HEADS];
    let mut attention_weights = vec![0.0f32; ATTENTION_SLOTS * HEADS];
    for query_head in 0..HEADS {
        scores.clear();
        let query_values = &query[query_head * HEAD_DIM..(query_head + 1) * HEAD_DIM];
        let query_values: Vec<f32> = query_values
            .iter()
            .map(|&value| {
                cpu::f16_cache_bits(value)
                    .map(cpu::f16_cache_f32)
                    .map_err(kernel)
            })
            .collect::<Result<_, _>>()?;
        let kv_head = cpu::gqa_kv_head(query_head, HEADS, KV_HEADS).map_err(kernel)?;
        for cache_position in 0..positions {
            let key_start = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM;
            let key_values: Vec<f32> = cache.keys[key_start..key_start + HEAD_DIM]
                .iter()
                .map(|&key| cpu::f16_cache_f32(key))
                .collect();
            let dot = cpu::f32_dot_pinned(&query_values, &key_values).map_err(kernel)?;
            raw_scores[query_head * ATTENTION_SLOTS + cache_position] = dot;
            scores.push(dot * scale);
        }
        cpu::softmax_in_place(&mut scores).map_err(kernel)?;
        attention_weights[query_head * ATTENTION_SLOTS..query_head * ATTENTION_SLOTS + positions]
            .copy_from_slice(&scores);
        for dimension in 0..HEAD_DIM {
            let mut weights = vec![0.0f32; ATTENTION_SLOTS];
            let mut values = vec![0.0f32; ATTENTION_SLOTS];
            for (cache_position, &weight) in scores.iter().enumerate() {
                let index = cache_position * KV_ELEMENTS + kv_head * HEAD_DIM + dimension;
                weights[cache_position] =
                    cpu::f16_cache_f32(cpu::f16_cache_bits(weight).map_err(kernel)?);
                values[cache_position] = cpu::f16_cache_f32(cache.values[index]);
            }
            context[query_head * HEAD_DIM + dimension] =
                cpu::f32_dot_pinned(&values, &weights).map_err(kernel)?;
        }
    }
    replay.check(&format!("kq-{layer}"), &raw_scores, INTERMEDIATE_END_TO_END_BOUND)?;
    replay.check(&format!("kq_soft_max-{layer}"), &attention_weights, HIDDEN_END_TO_END_BOUND)?;
    replay.check(&format!("kqv-{layer}"), &context, HIDDEN_END_TO_END_BOUND)?;
    replay.check(&format!("kqv_out-{layer}"), &context, HIDDEN_END_TO_END_BOUND)?;
    let output = project(native, descriptor(Some(layer), TensorRole::AttentionOutput)?, &context)?;
    replay.check(
        &format!("model.layers.{{}}.self_attn.out_proj-{layer}"),
        &output,
        HIDDEN_END_TO_END_BOUND,
    )?;
    Ok(output)
}

fn project(
    native: &[u8],
    descriptor: NativeTensorDescriptor,
    input: &[f32],
) -> Result<Vec<f32>, String> {
    if descriptor.ggml_ne0 as usize != input.len() {
        return Err(format!(
            "tensor {} input={} expected={}",
            descriptor.tensor_id,
            input.len(),
            descriptor.ggml_ne0
        ));
    }
    cpu::q8_matrix_vector_quantized(
        tensor(native, descriptor)?,
        descriptor.ggml_ne1 as usize,
        descriptor.ggml_ne0 as usize,
        input,
    )
    .map_err(kernel)
}

fn project_fpga_q30(
    native: &[u8],
    descriptor: NativeTensorDescriptor,
    input: &[f32],
) -> Result<Vec<i64>, String> {
    if descriptor.ggml_ne0 as usize != input.len() {
        return Err(format!(
            "tensor {} input={} expected={}",
            descriptor.tensor_id,
            input.len(),
            descriptor.ggml_ne0
        ));
    }
    let activation = cpu::quantize_q8(input).map_err(kernel)?;
    let row_bytes = cpu::q8_row_bytes(input.len()).map_err(kernel)?;
    let matrix = tensor(native, descriptor)?;
    if matrix.len() != descriptor.ggml_ne1 as usize * row_bytes {
        return Err(format!("tensor {} matrix shape", descriptor.tensor_id));
    }
    let mut output = Vec::with_capacity(descriptor.ggml_ne1 as usize);
    for row in matrix.chunks_exact(row_bytes) {
        let mut row_q30 = 0i64;
        for (weight, activation) in row.chunks_exact(cpu::Q8_BLOCK_BYTES).zip(&activation) {
            let dot: i32 = weight[2..]
                .iter()
                .zip(&activation[2..])
                .map(|(&weight, &activation)| i32::from(weight as i8) * i32::from(activation as i8))
                .sum();
            let term = q30_term(
                dot,
                u16::from_le_bytes([activation[0], activation[1]]),
                u16::from_le_bytes([weight[0], weight[1]]),
            )?;
            row_q30 = row_q30
                .checked_add(term)
                .ok_or_else(|| "FPGA Q30 row overflow".to_string())?;
        }
        output.push(row_q30);
    }
    Ok(output)
}

fn q30_term(dot: i32, activation_scale: u16, weight_scale: u16) -> Result<i64, String> {
    let (activation_significand, activation_exponent) = half_parts(activation_scale)?;
    let (weight_significand, weight_exponent) = half_parts(weight_scale)?;
    if activation_significand == 0 || weight_significand == 0 || dot == 0 {
        return Ok(0);
    }
    let raw = i64::from(dot)
        .checked_mul(i64::from(activation_significand))
        .and_then(|value| value.checked_mul(i64::from(weight_significand)))
        .ok_or_else(|| "FPGA Q30 term overflow".to_string())?;
    let shift = activation_exponent + weight_exponent - 20;
    if shift >= 0 {
        raw.checked_shl(shift as u32)
            .ok_or_else(|| "FPGA Q30 shift overflow".to_string())
    } else {
        Ok(round_shift_right_even(raw, (-shift) as u32))
    }
}

fn half_parts(bits: u16) -> Result<(u16, i32), String> {
    if bits & 0x8000 != 0 {
        return Err("negative Q8 scale".into());
    }
    let exponent = ((bits >> 10) & 0x1f) as i32;
    let fraction = bits & 0x03ff;
    match exponent {
        0 => Ok((fraction, 1)),
        31 => Err("non-finite Q8 scale".into()),
        _ => Ok((1024 + fraction, exponent)),
    }
}

fn round_shift_right_even(value: i64, shift: u32) -> i64 {
    let negative = value < 0;
    let magnitude = value.unsigned_abs();
    if shift >= 64 {
        return 0;
    }
    let quotient = magnitude >> shift;
    let mask = if shift == 0 { 0 } else { (1u64 << shift) - 1 };
    let remainder = magnitude & mask;
    let halfway = if shift == 0 { 0 } else { 1u64 << (shift - 1) };
    let rounded =
        quotient + u64::from(remainder > halfway || (remainder == halfway && quotient & 1 != 0));
    if negative {
        -(rounded as i64)
    } else {
        rounded as i64
    }
}

fn f32_tensor(
    sidecar: &cpu::F32Sidecar,
    descriptor: NativeTensorDescriptor,
) -> Result<&[f32], String> {
    sidecar
        .tensor(descriptor.tensor_id)
        .map_err(|error| format!("F32 tensor {}: {error:?}", descriptor.tensor_id))
}

fn descriptor(layer: Option<u8>, role: TensorRole) -> Result<NativeTensorDescriptor, String> {
    let layer = layer.unwrap_or(0xff);
    lfm25::generated::TENSORS
        .iter()
        .copied()
        .find(|descriptor| descriptor.layer == layer && descriptor.role == role as u8)
        .ok_or_else(|| format!("missing tensor layer={layer} role={role:?}"))
}

fn tensor(native: &[u8], descriptor: NativeTensorDescriptor) -> Result<&[u8], String> {
    let start = descriptor.native_offset as usize;
    let end = start
        .checked_add(descriptor.native_bytes as usize)
        .ok_or_else(|| format!("tensor {} range overflow", descriptor.tensor_id))?;
    native
        .get(start..end)
        .ok_or_else(|| format!("tensor {} truncated", descriptor.tensor_id))
}

fn read_golden(path: &Path) -> Result<GoldenTrace, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.get(..8) != Some(b"TGALDE2\0")
        || read_u32(&bytes, 8)? != 2
        || read_u32(&bytes, 12)? != 256
        || bytes.get(152..184) != Some(lfm25::PINNED_GGUF_SHA256.as_slice())
        || bytes.get(184..216) != Some(lfm25::PINNED_NATIVE_IMAGE_SHA256.as_slice())
        || bytes.get(216..248) != Some(lfm25::generated::MODEL_CONTRACT_SHA256.as_slice())
    {
        return Err(format!("bad hi trace {}", path.display()));
    }
    let token_count = read_u32(&bytes, 16)? as usize;
    let checkpoints_per_token = read_u32(&bytes, 20)? as usize;
    let total_checkpoints = read_u32(&bytes, 24)? as usize;
    if token_count != TOKENS.len()
        || checkpoints_per_token == 0
        || total_checkpoints != token_count * checkpoints_per_token
    {
        return Err("hi trace count contract".to_string());
    }
    let tokens = (0..token_count)
        .map(|index| read_u32(&bytes, 32 + index * 4))
        .collect::<Result<Vec<_>, _>>()?;
    let output_tokens = (0..token_count)
        .map(|index| read_u32(&bytes, 72 + index * 4))
        .collect::<Result<Vec<_>, _>>()?;
    let mut offset = 256usize;
    let mut checkpoints = Vec::with_capacity(token_count);
    for _ in 0..token_count {
        let mut token = Vec::with_capacity(checkpoints_per_token);
        for _ in 0..checkpoints_per_token {
            let name_bytes = bytes
                .get(offset..offset + 64)
                .ok_or_else(|| "truncated checkpoint name".to_string())?;
            let name_end = name_bytes
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(name_bytes.len());
            let name = std::str::from_utf8(&name_bytes[..name_end])
                .map_err(|_| "checkpoint name is not UTF-8".to_string())?
                .to_string();
            offset += 64;
            let elements = read_u32(&bytes, offset)? as usize;
            let payload_bytes = read_u32(&bytes, offset + 4)? as usize;
            offset += 8;
            if payload_bytes != elements * 4 {
                return Err(format!("checkpoint {name} payload shape"));
            }
            let payload = bytes
                .get(offset..offset + payload_bytes)
                .ok_or_else(|| format!("checkpoint {name} truncated"))?;
            let values = payload
                .chunks_exact(4)
                .map(|word| f32::from_le_bytes(word.try_into().unwrap()))
                .collect();
            token.push(GoldenCheckpoint { name, values });
            offset += payload_bytes;
        }
        checkpoints.push(token);
    }
    if offset != bytes.len() {
        return Err(format!("hi trace trailing bytes={}", bytes.len() - offset));
    }
    Ok(GoldenTrace {
        tokens,
        output_tokens,
        checkpoints,
    })
}

fn verify_bytes(
    name: &str,
    bytes: &[u8],
    expected_bytes: usize,
    expected_hash: [u8; 32],
) -> Result<(), String> {
    if bytes.len() != expected_bytes {
        return Err(format!("{name} bytes={} expected={expected_bytes}", bytes.len()));
    }
    let observed: [u8; 32] = Sha256::digest(bytes).into();
    if observed != expected_hash {
        return Err(format!("{name} SHA-256 mismatch"));
    }
    Ok(())
}

fn argmax(values: &[f32]) -> Result<usize, String> {
    values
        .iter()
        .copied()
        .enumerate()
        .reduce(|best, candidate| {
            if candidate.1 > best.1 {
                candidate
            } else {
                best
            }
        })
        .map(|(index, _)| index)
        .ok_or_else(|| "empty logits".to_string())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or_else(|| format!("truncated u32 at {offset}"))?
            .try_into()
            .unwrap(),
    ))
}

fn kernel(error: cpu::Error) -> String {
    format!("CPU kernel: {error:?}")
}
