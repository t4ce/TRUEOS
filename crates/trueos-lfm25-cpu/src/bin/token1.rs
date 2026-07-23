use std::fs;
use std::path::Path;

use trueos_fpga_abi::lfm25::{self, LayerKind, NativeTensorDescriptor, TensorRole};
use trueos_fpga_abi::lfm25_decode::{self, LayerStateSlot};
use trueos_lfm25_cpu as cpu;

const HIDDEN: usize = lfm25::MODEL_HIDDEN_SIZE as usize;
const HEADS: usize = lfm25::MODEL_ATTENTION_HEADS as usize;
const KV_HEADS: usize = lfm25::MODEL_KV_HEADS as usize;
const HEAD_DIM: usize = lfm25::MODEL_HEAD_DIMENSION as usize;
const FFN: usize = lfm25::MODEL_FEED_FORWARD_SIZE as usize;

fn main() -> Result<(), String> {
    let mut arguments = std::env::args();
    let program = arguments.next().unwrap_or_else(|| "token1".into());
    let native_path = arguments
        .next()
        .ok_or_else(|| format!("usage: {program} NATIVE.truega.bin TOKEN1.golden.bin"))?;
    let golden_path = arguments
        .next()
        .ok_or_else(|| format!("usage: {program} NATIVE.truega.bin TOKEN1.golden.bin"))?;
    if arguments.next().is_some() {
        return Err(format!("usage: {program} NATIVE.truega.bin TOKEN1.golden.bin"));
    }

    let native = fs::read(&native_path).map_err(|error| format!("read {native_path}: {error}"))?;
    if native.len() != lfm25::PINNED_NATIVE_IMAGE_BYTES as usize {
        return Err(format!(
            "native image bytes={} expected={}",
            native.len(),
            lfm25::PINNED_NATIVE_IMAGE_BYTES
        ));
    }
    let golden = read_golden(Path::new(&golden_path))?;
    if golden.len() != lfm25_decode::OPS_PER_TOKEN {
        return Err(format!(
            "golden checkpoints={} expected={}",
            golden.len(),
            lfm25_decode::OPS_PER_TOKEN
        ));
    }

    let mut checkpoint = 0usize;
    let embedding = descriptor(None, TensorRole::TokenEmbedding)?;
    let embedding_bytes = tensor(&native, embedding)?;
    let row_bytes = cpu::q8_row_bytes(HIDDEN).map_err(kernel)?;
    let token = lfm25_decode::TOKEN1_DECODE_INPUT_TOKEN as usize;
    let mut hidden = vec![0.0f32; HIDDEN];
    cpu::dequantize_q8_row(
        &embedding_bytes[token * row_bytes..(token + 1) * row_bytes],
        &mut hidden,
    )
    .map_err(kernel)?;
    compare(&golden, checkpoint, &hidden)?;
    checkpoint += 1;

    let mut shortconv = vec![vec![[0.0f32; 2]; HIDDEN]; lfm25_decode::SHORTCONV_STATE_COUNT];

    for layer in 0..lfm25::MODEL_LAYER_COUNT as u8 {
        let operator_residual = hidden.clone();
        let operator_weights =
            bf16_tensor(&native, descriptor(Some(layer), TensorRole::OperatorNorm)?)?;
        let normalized = cpu::rms_norm(&hidden, &operator_weights).map_err(kernel)?;
        compare(&golden, checkpoint, &normalized)?;
        checkpoint += 1;

        let branch = match lfm25::LAYER_SCHEDULE[layer as usize] {
            LayerKind::ShortConv => {
                let slot = match lfm25_decode::state_slot_for_layer(layer) {
                    Some(LayerStateSlot::ShortConv(slot)) => slot as usize,
                    _ => return Err(format!("layer {layer} shortconv state contract")),
                };
                shortconv_layer(&native, layer, &normalized, &mut shortconv[slot])?
            }
            LayerKind::Attention => attention_position_zero(&native, layer, &normalized)?,
        };
        compare(&golden, checkpoint, &branch)?;
        checkpoint += 1;

        hidden = cpu::add(&operator_residual, &branch).map_err(kernel)?;
        compare(&golden, checkpoint, &hidden)?;
        checkpoint += 1;

        let ffn_residual = hidden.clone();
        let ffn_weights = bf16_tensor(&native, descriptor(Some(layer), TensorRole::FfnNorm)?)?;
        let ffn_input = cpu::rms_norm(&hidden, &ffn_weights).map_err(kernel)?;
        compare(&golden, checkpoint, &ffn_input)?;
        checkpoint += 1;

        let gate = project(&native, descriptor(Some(layer), TensorRole::FfnGate)?, &ffn_input)?;
        let up = project(&native, descriptor(Some(layer), TensorRole::FfnUp)?, &ffn_input)?;
        if gate.len() != FFN || up.len() != FFN {
            return Err(format!("layer {layer} FFN shape"));
        }
        let activated: Vec<f32> = gate
            .iter()
            .zip(&up)
            .map(|(&gate, &up)| gate / (1.0 + libm::expf(-gate)) * up)
            .collect();
        let ffn_output =
            project(&native, descriptor(Some(layer), TensorRole::FfnDown)?, &activated)?;
        compare(&golden, checkpoint, &ffn_output)?;
        checkpoint += 1;

        hidden = cpu::add(&ffn_residual, &ffn_output).map_err(kernel)?;
        compare(&golden, checkpoint, &hidden)?;
        checkpoint += 1;
    }

    let final_weights = bf16_tensor(&native, descriptor(None, TensorRole::TokenEmbeddingNorm)?)?;
    let normalized = cpu::rms_norm(&hidden, &final_weights).map_err(kernel)?;
    compare(&golden, checkpoint, &normalized)?;
    checkpoint += 1;

    let logits = cpu::q8_matrix_vector_quantized(
        embedding_bytes,
        lfm25::MODEL_VOCABULARY_SIZE as usize,
        HIDDEN,
        &normalized,
    )
    .map_err(kernel)?;
    compare(&golden, checkpoint, &logits)?;
    checkpoint += 1;

    let (output_token, output_score) = logits
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
        .ok_or_else(|| "empty logits".to_string())?;
    println!(
        "PASS cpu token1 checkpoints={} output_token={} expected_token={} score={:.9}",
        checkpoint,
        output_token,
        lfm25_decode::TOKEN1_DECODE_OUTPUT_TOKEN,
        output_score
    );
    Ok(())
}

fn shortconv_layer(
    native: &[u8],
    layer: u8,
    input: &[f32],
    state: &mut [[f32; 2]],
) -> Result<Vec<f32>, String> {
    let projected = project(native, descriptor(Some(layer), TensorRole::ShortConvInput)?, input)?;
    if projected.len() != 3 * HIDDEN || state.len() != HIDDEN {
        return Err(format!("layer {layer} shortconv input shape"));
    }
    let kernel = bf16_tensor(native, descriptor(Some(layer), TensorRole::ShortConvKernel)?)?;
    if kernel.len() != 3 * HIDDEN {
        return Err(format!("layer {layer} shortconv kernel shape"));
    }
    let (b, rest) = projected.split_at(HIDDEN);
    let (c, x) = rest.split_at(HIDDEN);
    let mut mixed = Vec::with_capacity(HIDDEN);
    for channel in 0..HIDDEN {
        let kernel_base = channel * 3;
        let (output, oldest, newest) = cpu::shortconv_channel(
            b[channel],
            c[channel],
            x[channel],
            state[channel][0],
            state[channel][1],
            [
                kernel[kernel_base],
                kernel[kernel_base + 1],
                kernel[kernel_base + 2],
            ],
        )
        .map_err(kernel_error)?;
        state[channel] = [oldest, newest];
        mixed.push(output);
    }
    project(native, descriptor(Some(layer), TensorRole::ShortConvOutput)?, &mixed)
}

fn attention_position_zero(native: &[u8], layer: u8, input: &[f32]) -> Result<Vec<f32>, String> {
    let mut query = project(native, descriptor(Some(layer), TensorRole::Query)?, input)?;
    let mut key = project(native, descriptor(Some(layer), TensorRole::Key)?, input)?;
    let value = project(native, descriptor(Some(layer), TensorRole::Value)?, input)?;
    if query.len() != HEADS * HEAD_DIM
        || key.len() != KV_HEADS * HEAD_DIM
        || value.len() != KV_HEADS * HEAD_DIM
    {
        return Err(format!("layer {layer} attention projection shape"));
    }
    let query_norm = bf16_tensor(native, descriptor(Some(layer), TensorRole::QueryNorm)?)?;
    let key_norm = bf16_tensor(native, descriptor(Some(layer), TensorRole::KeyNorm)?)?;
    for head in query.chunks_exact_mut(HEAD_DIM) {
        cpu::rms_norm_head_in_place(head, &query_norm).map_err(kernel)?;
        cpu::rope_neox_in_place(head, 0).map_err(kernel)?;
    }
    for head in key.chunks_exact_mut(HEAD_DIM) {
        cpu::rms_norm_head_in_place(head, &key_norm).map_err(kernel)?;
        cpu::rope_neox_in_place(head, 0).map_err(kernel)?;
    }

    // At position zero every head attends to its sole cached value with
    // probability one. GQA maps two query heads to each KV head.
    let mut context = Vec::with_capacity(HIDDEN);
    for query_head in 0..HEADS {
        let kv_head = query_head * KV_HEADS / HEADS;
        context.extend_from_slice(&value[kv_head * HEAD_DIM..(kv_head + 1) * HEAD_DIM]);
    }
    project(native, descriptor(Some(layer), TensorRole::AttentionOutput)?, &context)
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

fn bf16_tensor(native: &[u8], descriptor: NativeTensorDescriptor) -> Result<Vec<f32>, String> {
    cpu::decode_bf16_vector(tensor(native, descriptor)?).map_err(kernel)
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

struct GoldenCheckpoint {
    name: String,
    values: Vec<f32>,
}

fn read_golden(path: &Path) -> Result<Vec<GoldenCheckpoint>, String> {
    let bytes = fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.len() != lfm25_decode::TOKEN1_DECODE_TRACE_BYTES as usize
        || bytes.get(..8) != Some(b"TGALDEC1")
    {
        return Err(format!("bad token trace {}", path.display()));
    }
    let count = read_u32(&bytes, 20)? as usize;
    let mut offset = 256usize;
    let mut checkpoints = Vec::with_capacity(count);
    for _ in 0..count {
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
        checkpoints.push(GoldenCheckpoint { name, values });
        offset += payload_bytes;
    }
    if offset != bytes.len() {
        return Err(format!("token trace trailing bytes={}", bytes.len() - offset));
    }
    Ok(checkpoints)
}

fn compare(golden: &[GoldenCheckpoint], index: usize, actual: &[f32]) -> Result<(), String> {
    let expected = golden
        .get(index)
        .ok_or_else(|| format!("missing golden checkpoint {index}"))?;
    if actual.len() != expected.values.len() {
        return Err(format!(
            "checkpoint {} {} elements={} expected={}",
            index,
            expected.name,
            actual.len(),
            expected.values.len()
        ));
    }
    let mut maximum = 0.0f32;
    let mut squared = 0.0f64;
    for (&actual, &expected) in actual.iter().zip(&expected.values) {
        let error = (actual - expected).abs();
        maximum = maximum.max(error);
        squared += f64::from(error) * f64::from(error);
    }
    let rmse = libm::sqrt(squared / actual.len() as f64);
    println!("checkpoint={index:02} name={} max_abs={maximum:.9e} rmse={rmse:.9e}", expected.name);
    Ok(())
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
    kernel_error(error)
}

fn kernel_error(error: cpu::Error) -> String {
    format!("CPU kernel: {error:?}")
}
