use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String as AllocString, ToString};
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};
use lumen::autograd::{Tensor, TensorRawData, no_grad};
use lumen::init::{ParameterInitMode, with_parameter_init_mode};
use lumen::models::{LlamaConfig, LlamaModel};
use lumen::precision::{
    DType, PrecisionConfig, with_precision_config, with_runtime_component_dtypes,
};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};
use super::bench::{
    bench_cancel_requested, bench_session_finish, bench_session_start, bps_from_progress,
    elapsed_ms_since, format_bytes, format_metric_units, format_speed,
    online_background_worker_slots, units_per_second_from_ticks,
};
use crate::shell2::CommandSessionInputResult;

const LUMEN_WEIGHTS_PATH: &str = crate::r::spawn_service::BOOT_LUMEN_WEIGHTS_PATH;
const LUMEN_TOKENIZER_PATH: &str = crate::r::spawn_service::BOOT_LUMEN_TOKENIZER_PATH;
const LUMEN_SAFETENSORS_MAX_HEADER_BYTES: usize = 64 * 1024 * 1024;
const LUMEN_KERNEL_PROBE_ROWS: usize = 16;
const LUMEN_KERNEL_PROBE_ITERS: usize = 256;
const LUMEN_STATIC_HI_PROMPT: &str = "Hi";
const LUMEN_REAL_HI_LM_HEAD_CHUNK_ROWS: usize = 512;
const LUMEN_AP_CHUNKS_PER_WORKER: usize = 4;
const LUMEN_AP_MIN_CHUNK_ROWS: usize = 16;
const LUMEN_AP_MAX_CHUNK_ROWS: usize = 256;
const LUMEN_RUNTIME_MAX_SEQ_LEN: usize = 384;
const LUMEN_RUNTIME_MAX_NEW_TOKENS: usize = 256;
const LUMEN_RUNTIME_PROGRESS_TENSORS: usize = 16;
const LUMEN_RUNTIME_EARLY_PROGRESS_TENSORS: usize = 2;
const LUMEN_RUNTIME_HEAP_EXTRA_BYTES: usize = 512 * 1024 * 1024;
const LUMEN_RESPONSE_LINE_CHARS: usize = 96;
const LUMEN_HI_TOKEN_CANDIDATES: [&str; 6] =
    ["Hi", "\u{2581}Hi", "hi", "\u{2581}hi", "H", "\u{2581}H"];
const LUMEN_COMPUTE_PROBE_CASES: [LumenComputeProbeCase; 3] = [
    LumenComputeProbeCase {
        label: "lm-head-1mb",
        rows: 256,
        chunk_rows: 16,
        iters: 16,
    },
    LumenComputeProbeCase {
        label: "lm-head-2mb",
        rows: 512,
        chunk_rows: 32,
        iters: 8,
    },
    LumenComputeProbeCase {
        label: "lm-head-4mb",
        rows: 1024,
        chunk_rows: 64,
        iters: 4,
    },
];

#[derive(Clone, Copy)]
struct LumenComputeProbeCase {
    label: &'static str,
    rows: usize,
    chunk_rows: usize,
    iters: usize,
}

struct LumenInteractiveSession {
    id: u64,
    prompts: VecDeque<LumenPromptRequest>,
}

#[derive(Clone)]
pub(crate) struct LumenPromptRequest {
    pub(crate) target: MatrixTarget,
    pub(crate) prompt: AllocString,
}

static LUMEN_INTERACTIVE_SESSIONS: spin::Mutex<Vec<LumenInteractiveSession>> =
    spin::Mutex::new(Vec::new());

fn register_lumen_interactive_session(session_id: u64) {
    let mut sessions = LUMEN_INTERACTIVE_SESSIONS.lock();
    if sessions.iter().any(|session| session.id == session_id) {
        return;
    }
    sessions.push(LumenInteractiveSession {
        id: session_id,
        prompts: VecDeque::new(),
    });
}

fn unregister_lumen_interactive_session(session_id: u64) {
    let mut sessions = LUMEN_INTERACTIVE_SESSIONS.lock();
    if let Some(idx) = sessions.iter().position(|session| session.id == session_id) {
        let _ = sessions.remove(idx);
    }
}

fn pop_lumen_prompt(session_id: u64) -> Option<LumenPromptRequest> {
    LUMEN_INTERACTIVE_SESSIONS
        .lock()
        .iter_mut()
        .find(|session| session.id == session_id)
        .and_then(|session| session.prompts.pop_front())
}

pub(crate) fn push_lumen_prompt(session_id: u64, target: &MatrixTarget, prompt: &str) -> bool {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return true;
    }
    let mut sessions = LUMEN_INTERACTIVE_SESSIONS.lock();
    let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) else {
        return false;
    };
    session.prompts.push_back(LumenPromptRequest {
        target: target.clone(),
        prompt: AllocString::from(prompt),
    });
    true
}

pub(crate) fn handle_lumen_session_input(
    session_id: u64,
    target: &MatrixTarget,
    submitted: &str,
) -> Option<CommandSessionInputResult> {
    let prompt = submitted.trim();
    if !LUMEN_INTERACTIVE_SESSIONS
        .lock()
        .iter()
        .any(|session| session.id == session_id)
    {
        return None;
    }
    if prompt.is_empty() {
        return Some(CommandSessionInputResult::KeepRunning);
    }

    let _ = push_lumen_prompt(session_id, target, prompt);
    print_matrix_target_line(target, "lumen: thinking...");
    Some(CommandSessionInputResult::KeepRunning)
}

pub(crate) fn submit_lumenbench(spawner: &Spawner, io: &'static dyn ShellBackend2) -> Option<u64> {
    let target = matrix_target_for_backend(io);
    let session_id = bench_session_start();

    print_matrix_target_line(
        &target,
        format!(
            "bench lumen: waiting for TRUEOSFS root weights={} tokenizer={}",
            LUMEN_WEIGHTS_PATH, LUMEN_TOKENIZER_PATH
        )
        .as_str(),
    );
    set_matrix_target_active(&target, true);
    match lumenbench_task(target.clone(), session_id) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            bench_session_finish(session_id);
            set_matrix_target_active(&target, false);
            print_shell_line(io, "bench lumen: spawn failed");
            return None;
        }
    }
    print_matrix_target_line(&target, "bench lumen: send `q` in this slot to stop");
    Some(session_id)
}

fn safetensor_shape(value: &serde_json::Value) -> Option<Vec<usize>> {
    value
        .get("shape")?
        .as_array()?
        .iter()
        .map(|v| v.as_u64().and_then(|n| usize::try_from(n).ok()))
        .collect()
}

fn safetensor_offsets(value: &serde_json::Value) -> Option<(usize, usize)> {
    let arr = value.get("data_offsets")?.as_array()?;
    let start = usize::try_from(arr.first()?.as_u64()?).ok()?;
    let end = usize::try_from(arr.get(1)?.as_u64()?).ok()?;
    Some((start, end))
}

fn find_tensor<'a>(
    obj: &'a serde_json::Map<AllocString, serde_json::Value>,
    exact: &str,
) -> Option<(&'a str, &'a serde_json::Value)> {
    obj.get_key_value(exact)
        .map(|(key, value)| (key.as_str(), value))
        .or_else(|| {
            obj.iter()
                .find(|(key, _)| key.as_str() != "__metadata__" && key.ends_with(exact))
                .map(|(key, value)| (key.as_str(), value))
        })
}

fn tensor_shape_line(shape: Option<&[usize]>) -> AllocString {
    match shape {
        Some(shape) => format!("{:?}", shape),
        None => AllocString::from("?"),
    }
}

fn bf16_to_f32(bits: u16) -> f32 {
    f32::from_bits((bits as u32) << 16)
}

fn bf16_row_to_f32(bytes: &[u8], cols: usize) -> Option<Vec<f32>> {
    if bytes.len() != cols.saturating_mul(2) {
        return None;
    }

    let mut out = Vec::with_capacity(cols);
    for col in 0..cols {
        let off = col * 2;
        let bits = u16::from_le_bytes([bytes[off], bytes[off + 1]]);
        out.push(bf16_to_f32(bits));
    }
    Some(out)
}

fn tokenizer_vocab_token_id(vocab: &serde_json::Value, token: &str) -> Option<usize> {
    if let Some(obj) = vocab.as_object() {
        return obj
            .get(token)
            .and_then(|value| value.as_u64())
            .and_then(|id| usize::try_from(id).ok());
    }

    let arr = vocab.as_array()?;
    for (idx, item) in arr.iter().enumerate() {
        if let Some(piece) = item.as_str() {
            if piece == token {
                return Some(idx);
            }
        } else if let Some(pair) = item.as_array()
            && pair.first().and_then(|value| value.as_str()) == Some(token)
        {
            return Some(idx);
        }
    }
    None
}

fn tokenizer_token_id(tokenizer: &serde_json::Value, token: &str) -> Option<usize> {
    tokenizer
        .get("model")
        .and_then(|model| model.get("vocab"))
        .and_then(|vocab| tokenizer_vocab_token_id(vocab, token))
        .or_else(|| {
            tokenizer
                .get("added_tokens")?
                .as_array()?
                .iter()
                .find_map(|item| {
                    let content = item.get("content").and_then(|value| value.as_str())?;
                    if content != token {
                        return None;
                    }
                    item.get("id")
                        .and_then(|value| value.as_u64())
                        .and_then(|id| usize::try_from(id).ok())
                })
        })
}

fn tokenizer_vocab_entries(tokenizer: &serde_json::Value) -> Vec<(AllocString, usize)> {
    let Some(vocab) = tokenizer.get("model").and_then(|model| model.get("vocab")) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    if let Some(obj) = vocab.as_object() {
        for (piece, value) in obj {
            if let Some(id) = value.as_u64().and_then(|id| usize::try_from(id).ok()) {
                out.push((AllocString::from(piece.as_str()), id));
            }
        }
    } else if let Some(arr) = vocab.as_array() {
        for (idx, item) in arr.iter().enumerate() {
            if let Some(piece) = item.as_str() {
                out.push((AllocString::from(piece), idx));
            } else if let Some(pair) = item.as_array()
                && let Some(piece) = pair.first().and_then(|value| value.as_str())
            {
                out.push((AllocString::from(piece), idx));
            }
        }
    }
    if let Some(arr) = tokenizer
        .get("added_tokens")
        .and_then(|value| value.as_array())
    {
        for item in arr {
            let Some(piece) = item.get("content").and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(id) = item
                .get("id")
                .and_then(|value| value.as_u64())
                .and_then(|id| usize::try_from(id).ok())
            else {
                continue;
            };
            if !out.iter().any(|(_, existing_id)| *existing_id == id) {
                out.push((AllocString::from(piece), id));
            }
        }
    }
    out
}

fn find_hi_token_id(tokenizer: &serde_json::Value) -> Option<(&'static str, usize)> {
    for token in LUMEN_HI_TOKEN_CANDIDATES {
        if let Some(id) = tokenizer_token_id(tokenizer, token) {
            return Some((token, id));
        }
    }
    None
}

fn tokenizer_stop_ids(tokenizer: &serde_json::Value) -> Vec<usize> {
    let mut out = Vec::new();
    for token in [
        "</s>",
        "<|end_of_text|>",
        "<|eot_id|>",
        "<|system|>",
        "<|user|>",
        "<|assistant|>",
    ] {
        if let Some(id) = tokenizer_token_id(tokenizer, token) {
            out.push(id);
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

fn tokenizer_piece_for_id(tokenizer: &serde_json::Value, token_id: usize) -> Option<&str> {
    if let Some(piece) = tokenizer
        .get("added_tokens")
        .and_then(|value| value.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|item| {
                let id = item
                    .get("id")
                    .and_then(|value| value.as_u64())
                    .and_then(|id| usize::try_from(id).ok())?;
                if id != token_id {
                    return None;
                }
                item.get("content").and_then(|value| value.as_str())
            })
        })
    {
        return Some(piece);
    }

    let vocab = tokenizer
        .get("model")
        .and_then(|model| model.get("vocab"))?;
    if let Some(obj) = vocab.as_object() {
        return obj.iter().find_map(|(piece, value)| {
            let id = value.as_u64().and_then(|id| usize::try_from(id).ok())?;
            (id == token_id).then_some(piece.as_str())
        });
    }

    let item = vocab.as_array()?.get(token_id)?;
    item.as_str().or_else(|| {
        item.as_array()
            .and_then(|pair| pair.first())
            .and_then(|piece| piece.as_str())
    })
}

fn normalize_prompt_for_vocab(prompt: &str) -> AllocString {
    let mut out = AllocString::new();
    let trimmed = prompt.trim();
    if trimmed.is_empty() {
        return out;
    }

    out.push('▁');
    for ch in trimmed.chars() {
        if ch == ' ' {
            out.push('▁');
        } else if ch == '\r' {
            continue;
        } else if ch == '\n' {
            out.push_str("<0x0A>");
        } else {
            out.push(ch);
        }
    }
    out
}

fn lumen_chat_prompt(user: &str) -> AllocString {
    format!("User: {}\nAssistant:", user.trim())
}

fn encode_prompt_lossy(prompt: &str, vocab_entries: &[(AllocString, usize)]) -> Vec<usize> {
    let normalized = normalize_prompt_for_vocab(prompt);
    let mut tokens = Vec::new();
    let mut pos = 0usize;
    while pos < normalized.len() {
        let rest = &normalized[pos..];
        let mut best: Option<(usize, usize)> = None;
        for (piece, id) in vocab_entries {
            let len = piece.len();
            if len == 0 || !rest.starts_with(piece.as_str()) {
                continue;
            }
            match best {
                Some((best_len, _)) if best_len >= len => {}
                _ => best = Some((len, *id)),
            }
        }

        if let Some((len, id)) = best {
            tokens.push(id);
            pos = pos.saturating_add(len);
        } else if let Some(ch) = rest.chars().next() {
            pos = pos.saturating_add(ch.len_utf8());
        } else {
            break;
        }
    }
    tokens
}

fn token_piece_to_text(piece: &str) -> AllocString {
    if let Some(rest) = piece.strip_prefix('▁') {
        if rest.is_empty() {
            AllocString::from(" ")
        } else {
            let mut out = AllocString::from(" ");
            out.push_str(token_piece_to_text(rest).as_str());
            out
        }
    } else if let Some(hex) = piece
        .strip_prefix("<0x")
        .and_then(|rest| rest.strip_suffix('>'))
    {
        if hex.len() == 2
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            AllocString::from(char::from(byte))
        } else {
            AllocString::from(piece)
        }
    } else {
        AllocString::from(piece)
    }
}

fn print_lumen_response(target: &MatrixTarget, answer: &str) {
    if answer.trim().is_empty() {
        print_matrix_target_line(target, "lumen: <empty>");
        return;
    }

    let mut first = true;
    for raw_line in answer.lines() {
        let mut line = raw_line.trim_end();
        if line.is_empty() {
            print_matrix_target_line(target, if first { "lumen:" } else { "" });
            first = false;
            continue;
        }

        while !line.is_empty() {
            let mut end = 0usize;
            let mut chars = 0usize;
            let mut last_space = None;
            for (idx, ch) in line.char_indices() {
                if chars >= LUMEN_RESPONSE_LINE_CHARS {
                    break;
                }
                if ch == ' ' {
                    last_space = Some(idx);
                }
                end = idx + ch.len_utf8();
                chars += 1;
            }
            if end < line.len() {
                end = last_space.filter(|idx| *idx > 0).unwrap_or(end);
            }

            let text = line[..end].trim();
            if !text.is_empty() {
                if first {
                    print_matrix_target_line(target, format!("lumen: {}", text).as_str());
                    first = false;
                } else {
                    print_matrix_target_line(target, text);
                }
            }
            line = line[end..].trim_start();
        }
    }
}

fn lumen_model_config(max_seq_len: usize) -> LlamaConfig {
    LlamaConfig {
        vocab_size: 32000,
        hidden_size: 2048,
        intermediate_size: 5632,
        num_hidden_layers: 22,
        num_attention_heads: 32,
        num_key_value_heads: 4,
        rms_norm_eps: 1e-5,
        max_seq_len,
        rope_theta: 10000.0,
    }
}

fn tensor_from_token_ids(ids: &[usize]) -> Result<Tensor, AllocString> {
    let tensor = Tensor::parameter_placeholder_with_dtype(&[1, ids.len()], DType::F32);
    let values = ids.iter().map(|&id| id as f32).collect::<Vec<_>>();
    tensor
        .import_raw(vec![1, ids.len()], DType::F32, TensorRawData::F32(values))
        .map_err(|err| format!("input tensor import failed: {}", err))?;
    Ok(tensor)
}

fn trim_lumen_turn_markers(answer: &str) -> AllocString {
    let mut end = answer.len();
    for marker in [
        "\nUser:",
        "\nAssistant:",
        "\nSystem:",
        "User:",
        "Assistant:",
        "System:",
    ] {
        if let Some(idx) = answer.find(marker) {
            end = end.min(idx);
        }
    }
    answer[..end].trim().to_string()
}

fn generate_lumen_answer(
    model: &LlamaModel,
    tokenizer_json: &serde_json::Value,
    vocab_entries: &[(AllocString, usize)],
    stop_ids: &[usize],
    prompt: &str,
) -> Result<(AllocString, usize), AllocString> {
    let chat_prompt = lumen_chat_prompt(prompt);
    let prompt_tokens = encode_prompt_lossy(chat_prompt.as_str(), vocab_entries);
    if prompt_tokens.is_empty() {
        return Err(AllocString::from("tokenizer produced no tokens"));
    }
    if prompt_tokens
        .len()
        .saturating_add(LUMEN_RUNTIME_MAX_NEW_TOKENS)
        .saturating_add(8)
        >= LUMEN_RUNTIME_MAX_SEQ_LEN
    {
        return Err(format!(
            "prompt too long tokens={} max_seq_len={}",
            prompt_tokens.len(),
            LUMEN_RUNTIME_MAX_SEQ_LEN
        ));
    }

    let mut kv_caches = model.init_kv_caches(1);
    model.reset_kv_caches(&mut kv_caches);
    let mut answer = AllocString::new();
    let mut generated = 0usize;

    no_grad(|| {
        let mut next_token = match tensor_from_token_ids(prompt_tokens.as_slice()) {
            Ok(input) => model.forward_last_argmax(input, &mut kv_caches, 0),
            Err(err) => return Err(err),
        };

        for _ in 0..LUMEN_RUNTIME_MAX_NEW_TOKENS {
            if stop_ids.binary_search(&next_token).is_ok() {
                break;
            }

            let piece = tokenizer_piece_for_id(tokenizer_json, next_token).unwrap_or("?");
            if piece.contains("<|user|>")
                || piece.contains("<|assistant|>")
                || piece.contains("<|system|>")
            {
                break;
            }
            answer.push_str(token_piece_to_text(piece).as_str());
            generated = generated.saturating_add(1);
            if answer.contains("\nUser:") || answer.contains("\nAssistant:") {
                break;
            }

            let input = tensor_from_token_ids(&[next_token])?;
            next_token = model.forward_last_argmax(input, &mut kv_caches, 0);
            if kv_caches
                .first()
                .map(|cache| cache.borrow().len.saturating_add(2) >= LUMEN_RUNTIME_MAX_SEQ_LEN)
                .unwrap_or(false)
            {
                break;
            }
        }

        Ok(())
    })?;

    Ok((trim_lumen_turn_markers(answer.as_str()), generated))
}

fn safetensor_dtype_nbytes(dtype: &str) -> Option<usize> {
    match dtype {
        "F32" => Some(4),
        "F16" | "BF16" => Some(2),
        "I8" | "U8" | "BOOL" => Some(1),
        _ => None,
    }
}

fn bytes_to_u16_vec(bytes: &[u8]) -> Vec<u16> {
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for chunk in bytes.chunks_exact(2) {
        out.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    out
}

fn lumen_probe_hidden(k_dim: usize) -> Vec<f32> {
    let mut hidden: Vec<f32> = Vec::with_capacity(k_dim);
    for idx in 0..k_dim {
        let lane = ((idx.wrapping_mul(17).wrapping_add(3)) & 0x3f) as f32;
        hidden.push((lane - 31.0) / 32.0);
    }
    hidden
}

fn checksum_f32(values: &[f32]) -> f32 {
    values
        .iter()
        .copied()
        .fold(0.0f32, |acc, value| acc + value)
}

fn top_f32(values: &[f32]) -> Option<(usize, f32)> {
    values
        .iter()
        .copied()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.total_cmp(b))
}

fn run_bf16_matvec_probe(rows_bf16: &[u8], rows: usize, k_dim: usize) -> (f32, u64) {
    let hidden = lumen_probe_hidden(k_dim);
    let mut checksum = 0.0f32;
    let start = embassy_time_driver::now();
    for _ in 0..LUMEN_KERNEL_PROBE_ITERS {
        for row in 0..rows {
            let mut acc = 0.0f32;
            let row_base = row.saturating_mul(k_dim).saturating_mul(2);
            for col in 0..k_dim {
                let off = row_base + col * 2;
                let bits = u16::from_le_bytes([rows_bf16[off], rows_bf16[off + 1]]);
                acc += hidden[col] * bf16_to_f32(bits);
            }
            checksum += acc;
        }
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start);
    let ops = (rows as u64)
        .saturating_mul(k_dim as u64)
        .saturating_mul(LUMEN_KERNEL_PROBE_ITERS as u64)
        .saturating_mul(2);
    (checksum, units_per_second_from_ticks(ops, elapsed_ticks))
}

fn run_bf16_compute_lane_probe(
    rows_bf16: &[u8],
    rows: usize,
    k_dim: usize,
    chunk_rows: usize,
    iters: usize,
) -> Result<
    (
        f32,
        Option<(usize, f32)>,
        u64,
        crate::burn_baby::ComputeStats,
        crate::burn_baby::ComputeStats,
    ),
    crate::burn_baby::ComputeError,
> {
    let hidden = lumen_probe_hidden(k_dim);
    run_bf16_compute_lane_probe_with_hidden(
        rows_bf16,
        rows,
        k_dim,
        hidden.as_slice(),
        chunk_rows,
        iters,
    )
}

fn run_bf16_compute_lane_probe_with_hidden(
    rows_bf16: &[u8],
    rows: usize,
    k_dim: usize,
    hidden: &[f32],
    chunk_rows: usize,
    iters: usize,
) -> Result<
    (
        f32,
        Option<(usize, f32)>,
        u64,
        crate::burn_baby::ComputeStats,
        crate::burn_baby::ComputeStats,
    ),
    crate::burn_baby::ComputeError,
> {
    let mut out = Vec::new();
    out.resize(rows, 0.0f32);

    let stats_before = crate::burn_baby::stats();
    let start = embassy_time_driver::now();
    for _ in 0..iters {
        crate::burn_baby::matvec_rowmajor_bf16(
            hidden,
            rows_bf16,
            rows,
            k_dim,
            out.as_mut_slice(),
            chunk_rows,
        )?;
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start);
    let stats_after = crate::burn_baby::stats();
    let ops = (rows as u64)
        .saturating_mul(k_dim as u64)
        .saturating_mul(iters as u64)
        .saturating_mul(2);
    Ok((
        checksum_f32(out.as_slice()),
        top_f32(out.as_slice()),
        units_per_second_from_ticks(ops, elapsed_ticks),
        stats_before,
        stats_after,
    ))
}

fn lumen_ap_worker_count(slots: &[u32]) -> usize {
    slots.len().max(1)
}

fn lumen_dynamic_chunk_rows(rows: usize, worker_count: usize) -> usize {
    if rows == 0 {
        return 0;
    }

    let target_chunks = worker_count
        .max(1)
        .saturating_mul(LUMEN_AP_CHUNKS_PER_WORKER);
    let chunk_rows = rows.div_ceil(target_chunks.max(1));
    chunk_rows
        .clamp(LUMEN_AP_MIN_CHUNK_ROWS.min(rows), LUMEN_AP_MAX_CHUNK_ROWS.min(rows))
        .max(1)
}

fn compute_slot_delta_line(before: &[(u32, u64)], after: &[(u32, u64)]) -> AllocString {
    let mut parts: Vec<AllocString> = Vec::new();
    for (slot, after_count) in after.iter().copied() {
        let before_count = before
            .iter()
            .find(|(before_slot, _)| *before_slot == slot)
            .map(|(_, count)| *count)
            .unwrap_or(0);
        let delta = after_count.saturating_sub(before_count);
        if delta != 0 {
            parts.push(format!("{}:{}", slot, delta));
        }
    }

    if parts.is_empty() {
        AllocString::from("[]")
    } else {
        format!("[{}]", parts.join(", "))
    }
}

struct TensorLocation {
    file_offset: u64,
    data_start: usize,
    data_end: usize,
}

fn safetensor_location(header_len: usize, value: &serde_json::Value) -> Option<TensorLocation> {
    let (data_start, data_end) = safetensor_offsets(value)?;
    let file_offset = 8usize
        .checked_add(header_len)?
        .checked_add(data_start)
        .and_then(|offset| u64::try_from(offset).ok())?;
    Some(TensorLocation {
        file_offset,
        data_start,
        data_end,
    })
}

struct LumenRuntimeLoadReport {
    loaded_tensors: usize,
    loaded_bytes: u64,
    missing_tensors: usize,
}

async fn load_lumen_model_from_trueosfs(
    disk: crate::disc::block::DeviceHandle,
    header_len: usize,
    obj: &serde_json::Map<AllocString, serde_json::Value>,
    model: &LlamaModel,
    _target: &MatrixTarget,
    session_id: u64,
) -> Result<LumenRuntimeLoadReport, AllocString> {
    let params = model.named_parameters();
    let mut ordered = params
        .iter()
        .map(|(name, tensor)| {
            let offset = obj
                .get(name)
                .and_then(|value| safetensor_offsets(value))
                .map(|(start, _)| start)
                .unwrap_or(usize::MAX);
            (offset, name.as_str(), tensor)
        })
        .collect::<Vec<_>>();
    ordered.sort_by_key(|(offset, _, _)| *offset);

    let mut loaded_tensors = 0usize;
    let mut loaded_bytes = 0u64;
    let mut missing_tensors = 0usize;
    let progress_start = embassy_time_driver::now();
    let mut last_progress_tick = progress_start;
    let mut last_progress_bytes = 0u64;
    for (_, name, tensor) in ordered {
        if bench_cancel_requested(session_id) {
            return Err(AllocString::from("cancelled"));
        }

        let Some(value) = obj.get(name) else {
            missing_tensors = missing_tensors.saturating_add(1);
            continue;
        };
        let dtype = value.get("dtype").and_then(|v| v.as_str()).unwrap_or("?");
        let shape =
            safetensor_shape(value).ok_or_else(|| format!("{} missing safetensors shape", name))?;
        if shape != tensor.shape_vec() {
            return Err(format!(
                "{} shape mismatch safetensors={:?} model={:?}",
                name,
                shape,
                tensor.shape_vec()
            ));
        }
        let Some(dtype_bytes) = safetensor_dtype_nbytes(dtype) else {
            return Err(format!("{} unsupported safetensors dtype={}", name, dtype));
        };
        let Some(loc) = safetensor_location(header_len, value) else {
            return Err(format!("{} missing safetensors offsets", name));
        };
        let elem_count = shape
            .iter()
            .copied()
            .try_fold(1usize, |acc, dim| acc.checked_mul(dim))
            .ok_or_else(|| format!("{} element count overflow", name))?;
        let byte_count = elem_count
            .checked_mul(dtype_bytes)
            .ok_or_else(|| format!("{} byte count overflow", name))?;
        if loc.data_start.saturating_add(byte_count) > loc.data_end {
            return Err(format!("{} safetensors range invalid", name));
        }

        let bytes = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            loc.file_offset,
            byte_count,
        )
        .await
        {
            Ok(Some(bytes)) if bytes.len() == byte_count => bytes,
            Ok(Some(bytes)) => {
                return Err(format!(
                    "{} short read got={} need={}",
                    name,
                    format_bytes(bytes.len() as u64),
                    format_bytes(byte_count as u64)
                ));
            }
            Ok(None) => return Err(format!("{} disappeared during weight load", name)),
            Err(err) => return Err(format!("{} read failed err={:?}", name, err)),
        };

        match dtype {
            "BF16" => tensor
                .import_raw(shape, DType::BF16, TensorRawData::BF16(bytes_to_u16_vec(&bytes)))
                .map_err(|err| format!("{} BF16 import failed: {}", name, err))?,
            "F16" => tensor
                .import_raw(shape, DType::F16, TensorRawData::F16(bytes_to_u16_vec(&bytes)))
                .map_err(|err| format!("{} F16 import failed: {}", name, err))?,
            "F32" => {
                let mut values = Vec::with_capacity(bytes.len() / 4);
                for chunk in bytes.chunks_exact(4) {
                    values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                }
                tensor
                    .import_raw(shape, DType::F32, TensorRawData::F32(values))
                    .map_err(|err| format!("{} F32 import failed: {}", name, err))?;
            }
            other => return Err(format!("{} unsupported runtime import dtype={}", name, other)),
        }

        loaded_tensors = loaded_tensors.saturating_add(1);
        loaded_bytes = loaded_bytes.saturating_add(byte_count as u64);
        let should_log_progress = loaded_tensors == 1
            || (loaded_tensors < LUMEN_RUNTIME_PROGRESS_TENSORS
                && loaded_tensors % LUMEN_RUNTIME_EARLY_PROGRESS_TENSORS == 0)
            || loaded_tensors % LUMEN_RUNTIME_PROGRESS_TENSORS == 0;
        if should_log_progress {
            let elapsed_ms = elapsed_ms_since(progress_start);
            let step_ms = elapsed_ms_since(last_progress_tick);
            let step_bytes = loaded_bytes.saturating_sub(last_progress_bytes);
            crate::log!(
                "bench lumen: runtime-hi loading tensors={} tensor={} tensor_bytes={} bytes={} elapsed={}ms speed={} step_bytes={} step_ms={} step_speed={}\n",
                loaded_tensors,
                name,
                format_bytes(byte_count as u64),
                format_bytes(loaded_bytes),
                elapsed_ms,
                format_speed(bps_from_progress(loaded_bytes, elapsed_ms)),
                format_bytes(step_bytes),
                step_ms,
                format_speed(bps_from_progress(step_bytes, step_ms))
            );
            last_progress_tick = embassy_time_driver::now();
            last_progress_bytes = loaded_bytes;
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }

    Ok(LumenRuntimeLoadReport {
        loaded_tensors,
        loaded_bytes,
        missing_tensors,
    })
}

#[embassy_executor::task(pool_size = 2)]
pub(crate) async fn lumenbench_task(target: MatrixTarget, session_id: u64) {
    run_lumen_session(target, session_id).await;
}

pub(crate) async fn run_lumen_session(target: MatrixTarget, session_id: u64) {
    let task_target = target.clone();
    async move {
        Timer::after(EmbassyDuration::from_millis(1)).await;

        let log = |line: &str| {
            crate::log!("{}\n", line);
        };

        let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() else {
            log("bench lumen: skipped; no TRUEOSFS root mounted");
            return;
        };

        let weights_info = match crate::r::fs::trueosfs::file_info_async(disk, LUMEN_WEIGHTS_PATH).await {
            Ok(Some(info)) if info.data_len >= 16 => info,
            Ok(Some(info)) => {
                log(
                    format!(
                        "bench lumen: skipped; weights incomplete path={} bytes={}",
                        LUMEN_WEIGHTS_PATH, info.data_len
                    )
                    .as_str(),
                );
                return;
            }
            Ok(None) => {
                log(
                    format!(
                        "bench lumen: skipped; missing TinyLlama weights path={} tokenizer path={}",
                        LUMEN_WEIGHTS_PATH, LUMEN_TOKENIZER_PATH
                    )
                    .as_str(),
                );
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: skipped; weights probe failed path={} err={:?}",
                        LUMEN_WEIGHTS_PATH, err
                    )
                    .as_str(),
                );
                return;
            }
        };

        let tokenizer_info =
            match crate::r::fs::trueosfs::file_info_async(disk, LUMEN_TOKENIZER_PATH).await {
                Ok(Some(info)) if info.data_len > 0 => info,
                Ok(Some(info)) => {
                    log(
                        format!(
                            "bench lumen: skipped; tokenizer empty path={} bytes={}",
                            LUMEN_TOKENIZER_PATH, info.data_len
                        )
                        .as_str(),
                    );
                    return;
                }
                Ok(None) => {
                    log(
                        format!(
                            "bench lumen: skipped; missing tokenizer path={} for LUMEN CLI contract",
                            LUMEN_TOKENIZER_PATH
                        )
                        .as_str(),
                    );
                    return;
                }
                Err(err) => {
                    log(
                        format!(
                            "bench lumen: skipped; tokenizer probe failed path={} err={:?}",
                            LUMEN_TOKENIZER_PATH, err
                        )
                        .as_str(),
                    );
                    return;
                }
            };

        let mut header_len_bytes = [0u8; 8];
        match crate::r::fs::trueosfs::file_read_range_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            0,
            &mut header_len_bytes,
        )
        .await
        {
            Ok(Some(8)) => {}
            Ok(Some(n)) => {
                log(
                    format!(
                        "bench lumen: skipped; short safetensors header read path={} bytes={}",
                        LUMEN_WEIGHTS_PATH, n
                    )
                    .as_str(),
                );
                return;
            }
            Ok(None) => {
                log(format!("bench lumen: skipped; disappeared path={}", LUMEN_WEIGHTS_PATH).as_str());
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: skipped; safetensors header read failed path={} err={:?}",
                        LUMEN_WEIGHTS_PATH, err
                    )
                    .as_str(),
                );
                return;
            }
        }

        let header_len_u64 = u64::from_le_bytes(header_len_bytes);
        let Ok(header_len) = usize::try_from(header_len_u64) else {
            log("bench lumen: skipped; safetensors header length overflows usize");
            return;
        };
        if header_len == 0
            || header_len > LUMEN_SAFETENSORS_MAX_HEADER_BYTES
            || header_len_u64.saturating_add(8) > weights_info.data_len
        {
            log(
                format!(
                    "bench lumen: skipped; invalid safetensors header_len={} file_size={}",
                    format_bytes(header_len_u64),
                    format_bytes(weights_info.data_len)
                )
                .as_str(),
            );
            return;
        }

        log(
            format!(
                "bench lumen: load-only preflight weights={} size={} header={} tokenizer={} size={}",
                LUMEN_WEIGHTS_PATH,
                format_bytes(weights_info.data_len),
                format_bytes(header_len_u64),
                LUMEN_TOKENIZER_PATH,
                format_bytes(tokenizer_info.data_len)
            )
            .as_str(),
        );
        let load_start = embassy_time_driver::now();
        let header = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            8,
            header_len,
        )
        .await
        {
            Ok(Some(bytes)) => bytes,
            Ok(None) => {
                log(format!("bench lumen: skipped; load returned missing path={}", LUMEN_WEIGHTS_PATH).as_str());
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: skipped; safetensors header load failed path={} err={:?}",
                        LUMEN_WEIGHTS_PATH, err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let load_ms = elapsed_ms_since(load_start);
        let header_value = match serde_json::from_slice::<serde_json::Value>(&header) {
            Ok(value) => value,
            Err(err) => {
                log(
                    format!(
                        "bench lumen: skipped; safetensors header JSON parse failed err={}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let Some(obj) = header_value.as_object() else {
            log("bench lumen: skipped; safetensors header root is not an object");
            return;
        };

        let tensor_count = obj.keys().filter(|key| key.as_str() != "__metadata__").count();
        let first_tensor = obj
            .iter()
            .find(|(key, _)| key.as_str() != "__metadata__");
        let mut first_line = AllocString::from("none");
        if let Some((name, value)) = first_tensor {
            let dtype = value
                .get("dtype")
                .and_then(|v| v.as_str())
                .unwrap_or("?");
            let shape = value
                .get("shape")
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| AllocString::from("?"));
            let offsets = value
                .get("data_offsets")
                .map(|v| format!("{}", v))
                .unwrap_or_else(|| AllocString::from("?"));
            first_line = format!(
                "name={} dtype={} shape={} offsets={}",
                name, dtype, shape, offsets
            );
        }

        log(
            format!(
                "bench lumen: load-only preflight ok tensors={} header_load={} elapsed={}ms first_tensor=({})",
                tensor_count,
                format_speed(bps_from_progress(header.len() as u64, load_ms)),
                load_ms,
                first_line
            )
            .as_str(),
        );
        log(
            format!(
                "bench lumen: runtime backend={} contract={} fp_backend={} int8_backend={} trueos_cpu={} x86_fp={} x86_i8={}",
                lumen::backend::default_backend_name(),
                lumen::backend::trueos_parallel_contract(),
                lumen::ops::fp_kernels::active_float_backend_name(),
                lumen::ops::int8_kernels::active_int8_backend_name(),
                lumen::arch::trueos_cpu_backend_compiled(),
                lumen::arch::x86_fp_kernel_runtime_available(),
                lumen::arch::x86_i8_kernel_runtime_available(),
            )
            .as_str(),
        );

        let lm_head = find_tensor(obj, "lm_head.weight");
        let embed = find_tensor(obj, "model.embed_tokens.weight");
        let rms = find_tensor(obj, "model.norm.weight");
        let q_proj = find_tensor(obj, "self_attn.q_proj.weight");
        let gate_proj = find_tensor(obj, "mlp.gate_proj.weight");

        let lm_shape = lm_head.and_then(|(_, value)| safetensor_shape(value));
        let embed_shape = embed.and_then(|(_, value)| safetensor_shape(value));
        let rms_shape = rms.and_then(|(_, value)| safetensor_shape(value));
        let q_shape = q_proj.and_then(|(_, value)| safetensor_shape(value));
        let gate_shape = gate_proj.and_then(|(_, value)| safetensor_shape(value));

        let vocab = lm_shape
            .as_ref()
            .and_then(|shape| shape.first().copied())
            .or_else(|| embed_shape.as_ref().and_then(|shape| shape.first().copied()));
        let hidden = lm_shape
            .as_ref()
            .and_then(|shape| shape.get(1).copied())
            .or_else(|| embed_shape.as_ref().and_then(|shape| shape.get(1).copied()));
        let layers = obj
            .keys()
            .filter_map(|key| {
                let rest = key.strip_prefix("model.layers.")?;
                let (idx, _) = rest.split_once('.')?;
                idx.parse::<usize>().ok()
            })
            .max()
            .map(|idx| idx + 1);

        let online_slots = online_background_worker_slots();
        let all_slots = crate::workers::background_worker_slots();
        log(
            format!(
                "bench lumen: model-shape vocab={} hidden={} layers={} lm_head={} embed={} rms={} q_proj={} gate_proj={} ap_online={}/{} slots={:?}",
                vocab
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| AllocString::from("?")),
                hidden
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| AllocString::from("?")),
                layers
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| AllocString::from("?")),
                tensor_shape_line(lm_shape.as_deref()),
                tensor_shape_line(embed_shape.as_deref()),
                tensor_shape_line(rms_shape.as_deref()),
                tensor_shape_line(q_shape.as_deref()),
                tensor_shape_line(gate_shape.as_deref()),
                online_slots.len(),
                all_slots.len(),
                online_slots
            )
            .as_str(),
        );

        if bench_cancel_requested(session_id) {
            log("bench lumen: stopped after preflight");
            return;
        }

        let Ok(tokenizer_len) = usize::try_from(tokenizer_info.data_len) else {
            log("bench lumen: runtime skipped; tokenizer length overflows usize");
            return;
        };
        log(
            format!(
                "bench lumen: tokenizer read start path={} bytes={}",
                LUMEN_TOKENIZER_PATH,
                format_bytes(tokenizer_len as u64)
            )
            .as_str(),
        );
        let tokenizer_start = embassy_time_driver::now();
        let mut tokenizer_bytes = vec![0u8; tokenizer_len];
        match crate::r::stream::read_trueosfs_file_range_into_logged_async(
            disk,
            LUMEN_TOKENIZER_PATH,
            0,
            tokenizer_bytes.as_mut_slice(),
            "bench lumen: tokenizer",
        )
        .await
        {
            Ok(true) => {}
            Ok(false) => {
                log("bench lumen: runtime skipped; tokenizer disappeared");
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: runtime skipped; tokenizer read failed err={:?}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let tokenizer_ms = elapsed_ms_since(tokenizer_start);
        log(
            format!(
                "bench lumen: tokenizer read done bytes={} elapsed={}ms speed={}",
                format_bytes(tokenizer_len as u64),
                tokenizer_ms,
                format_speed(bps_from_progress(tokenizer_len as u64, tokenizer_ms))
            )
            .as_str(),
        );
        let tokenizer_json = match serde_json::from_slice::<serde_json::Value>(&tokenizer_bytes) {
            Ok(value) => value,
            Err(err) => {
                log(
                    format!(
                        "bench lumen: runtime skipped; tokenizer JSON parse failed err={}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };

        if crate::allcaps::lumen::RUNTIME_DIAGNOSTIC_PROBES {
        let Some((lm_name, lm_value)) = lm_head else {
            log("bench lumen: kernel probe skipped; lm_head.weight missing");
            return;
        };
        let lm_dtype = lm_value
            .get("dtype")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let Some(lm_shape) = lm_shape.as_ref() else {
            log("bench lumen: kernel probe skipped; lm_head.weight shape missing");
            return;
        };
        if lm_dtype != "BF16" || lm_shape.len() != 2 {
            log(
                format!(
                    "bench lumen: kernel probe skipped; lm_head dtype={} shape={:?}",
                    lm_dtype, lm_shape
                )
                .as_str(),
            );
            return;
        }
        let rows = LUMEN_KERNEL_PROBE_ROWS.min(lm_shape[0]);
        let k_dim = lm_shape[1];
        let Some((data_start, data_end)) = safetensor_offsets(lm_value) else {
            log("bench lumen: kernel probe skipped; lm_head offsets missing");
            return;
        };
        let bytes_needed = rows.saturating_mul(k_dim).saturating_mul(2);
        if bytes_needed == 0 || data_start.saturating_add(bytes_needed) > data_end {
            log("bench lumen: kernel probe skipped; lm_head range invalid");
            return;
        }
        let Some(payload_offset) = 8usize
            .checked_add(header_len)
            .and_then(|base| base.checked_add(data_start))
            .and_then(|offset| u64::try_from(offset).ok())
        else {
            log("bench lumen: kernel probe skipped; lm_head file offset overflow");
            return;
        };

        let kernel_start = embassy_time_driver::now();
        let rows_bytes = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            payload_offset,
            bytes_needed,
        )
        .await
        {
            Ok(Some(bytes)) if bytes.len() == bytes_needed => bytes,
            Ok(Some(bytes)) => {
                log(
                    format!(
                        "bench lumen: kernel probe skipped; short lm_head read got={} need={}",
                        format_bytes(bytes.len() as u64),
                        format_bytes(bytes_needed as u64)
                    )
                    .as_str(),
                );
                return;
            }
            Ok(None) => {
                log("bench lumen: kernel probe skipped; weights disappeared");
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: kernel probe skipped; lm_head read failed err={:?}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let read_ms = elapsed_ms_since(kernel_start);
        let (checksum, ops_per_s) = run_bf16_matvec_probe(&rows_bytes, rows, k_dim);
        log(
            format!(
                "bench lumen: kernel probe lm_head={} rows={} hidden={} iters={} bytes={} read={}ms compute={}/s checksum={:.4}",
                lm_name,
                rows,
                k_dim,
                LUMEN_KERNEL_PROBE_ITERS,
                format_bytes(bytes_needed as u64),
                read_ms,
                format_metric_units(ops_per_s, "ops"),
                checksum
            )
            .as_str(),
        );

        if bench_cancel_requested(session_id) {
            log("bench lumen: stopped after serial kernel probe");
            return;
        }

        let max_compute_rows = LUMEN_COMPUTE_PROBE_CASES
            .iter()
            .map(|case| case.rows)
            .max()
            .unwrap_or(0)
            .min(lm_shape[0]);
        let max_compute_bytes = max_compute_rows.saturating_mul(k_dim).saturating_mul(2);
        if max_compute_rows <= rows || data_start.saturating_add(max_compute_bytes) > data_end {
            log("bench lumen: compute-lane probe skipped; lm_head range too small");
            return;
        }

        log(
            format!(
                "bench lumen: compute telemetry start lm_head={} max_rows={} hidden={} cases={} bytes={} note=no-token-decode synthetic-hidden",
                lm_name,
                max_compute_rows,
                k_dim,
                LUMEN_COMPUTE_PROBE_CASES.len(),
                format_bytes(max_compute_bytes as u64)
            )
            .as_str(),
        );

        let compute_read_start = embassy_time_driver::now();
        let compute_rows_bytes = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            payload_offset,
            max_compute_bytes,
        )
        .await
        {
            Ok(Some(bytes)) if bytes.len() == max_compute_bytes => bytes,
            Ok(Some(bytes)) => {
                log(
                    format!(
                        "bench lumen: compute-lane probe skipped; short lm_head read got={} need={}",
                        format_bytes(bytes.len() as u64),
                        format_bytes(max_compute_bytes as u64)
                    )
                    .as_str(),
                );
                return;
            }
            Ok(None) => {
                log("bench lumen: compute-lane probe skipped; weights disappeared");
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: compute-lane probe skipped; lm_head read failed err={:?}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let compute_read_ms = elapsed_ms_since(compute_read_start);

        let slots_for_counts = online_background_worker_slots();
        let ap_worker_count = lumen_ap_worker_count(&slots_for_counts);
        log(
            format!(
                "bench lumen: compute plan ap_workers={} chunks_per_worker={} chunk_rows=min{}..max{} slots={:?}",
                ap_worker_count,
                LUMEN_AP_CHUNKS_PER_WORKER,
                LUMEN_AP_MIN_CHUNK_ROWS,
                LUMEN_AP_MAX_CHUNK_ROWS,
                slots_for_counts
            )
            .as_str(),
        );
        for case in LUMEN_COMPUTE_PROBE_CASES {
            if bench_cancel_requested(session_id) {
                log("bench lumen: stopped during compute-lane probes");
                return;
            }

            let compute_rows = case.rows.min(lm_shape[0]).min(max_compute_rows);
            let compute_bytes = compute_rows.saturating_mul(k_dim).saturating_mul(2);
            if compute_rows <= rows || compute_bytes == 0 || compute_bytes > compute_rows_bytes.len() {
                continue;
            }

            let chunk_rows = lumen_dynamic_chunk_rows(compute_rows, ap_worker_count);
            let slot_counts_before = crate::burn_baby::poll_counts_for_slots(&slots_for_counts);
            match run_bf16_compute_lane_probe(
                &compute_rows_bytes[..compute_bytes],
                compute_rows,
                k_dim,
                chunk_rows,
                case.iters,
            ) {
                Ok((checksum, top, ops_per_s, stats_before, stats_after)) => {
                    let slot_counts_after = crate::burn_baby::poll_counts_for_slots(&slots_for_counts);
                    let submitted = stats_after
                        .submitted_jobs
                        .saturating_sub(stats_before.submitted_jobs);
                    let completed = stats_after
                        .completed_jobs
                        .saturating_sub(stats_before.completed_jobs);
                    let polled = stats_after
                        .polled_jobs
                        .saturating_sub(stats_before.polled_jobs);
                    let top_line = top
                        .map(|(idx, value)| format!("sample_argmax_row={}:{:.4}", idx, value))
                        .unwrap_or_else(|| AllocString::from("sample_argmax_row=?"));
                    log(
                        format!(
                            "bench lumen: compute-lane probe label={} lm_head={} rows={} hidden={} chunk_rows={} iters={} bytes={} read={}ms compute={}/s jobs={}/{} polled={} queued={} workers={} {} checksum={:.4}",
                            case.label,
                            lm_name,
                            compute_rows,
                            k_dim,
                            chunk_rows,
                            case.iters,
                            format_bytes(compute_bytes as u64),
                            compute_read_ms,
                            format_metric_units(ops_per_s, "ops"),
                            completed,
                            submitted,
                            polled,
                            stats_after.queued_jobs,
                            compute_slot_delta_line(&slot_counts_before, &slot_counts_after),
                            top_line,
                            checksum
                        )
                        .as_str(),
                    );
                }
                Err(err) => {
                    log(
                        format!(
                            "bench lumen: compute-lane probe label={} skipped; err={:?}",
                            case.label, err
                        )
                        .as_str(),
                    );
                }
            }
        }

        if bench_cancel_requested(session_id) {
            log("bench lumen: stopped before real Hi token probe");
            return;
        }

        let Some((embed_name, embed_value)) = embed else {
            log("bench lumen: real-hi skipped; model.embed_tokens.weight missing");
            return;
        };
        let embed_dtype = embed_value
            .get("dtype")
            .and_then(|v| v.as_str())
            .unwrap_or("?");
        let Some(embed_shape) = embed_shape.as_ref() else {
            log("bench lumen: real-hi skipped; embed shape missing");
            return;
        };
        if embed_dtype != "BF16" || embed_shape.len() != 2 || embed_shape[1] != k_dim {
            log(
                format!(
                    "bench lumen: real-hi skipped; embed dtype={} shape={:?} hidden={}",
                    embed_dtype, embed_shape, k_dim
                )
                .as_str(),
            );
            return;
        }

        let Some((hi_token_piece, hi_token_id)) = find_hi_token_id(&tokenizer_json) else {
            log("bench lumen: real-hi skipped; no Hi candidate token in tokenizer vocab");
            return;
        };
        if hi_token_id >= embed_shape[0] {
            log(
                format!(
                    "bench lumen: real-hi skipped; token_id={} outside embed rows={}",
                    hi_token_id, embed_shape[0]
                )
                .as_str(),
            );
            return;
        }

        let Some(embed_loc) = safetensor_location(header_len, embed_value) else {
            log("bench lumen: real-hi skipped; embed offsets missing");
            return;
        };
        let embed_row_bytes = k_dim.saturating_mul(2);
        let embed_rel = hi_token_id.saturating_mul(embed_row_bytes);
        let embed_row_end = embed_loc
            .data_start
            .saturating_add(embed_rel)
            .saturating_add(embed_row_bytes);
        if embed_row_bytes == 0 || embed_row_end > embed_loc.data_end {
            log("bench lumen: real-hi skipped; embed row range invalid");
            return;
        }
        let embed_file_offset = embed_loc.file_offset.saturating_add(embed_rel as u64);
        let embed_read_start = embassy_time_driver::now();
        let embed_row = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
            disk,
            LUMEN_WEIGHTS_PATH,
            embed_file_offset,
            embed_row_bytes,
        )
        .await
        {
            Ok(Some(bytes)) if bytes.len() == embed_row_bytes => bytes,
            Ok(Some(bytes)) => {
                log(
                    format!(
                        "bench lumen: real-hi skipped; short embed read got={} need={}",
                        format_bytes(bytes.len() as u64),
                        format_bytes(embed_row_bytes as u64)
                    )
                    .as_str(),
                );
                return;
            }
            Ok(None) => {
                log("bench lumen: real-hi skipped; weights disappeared on embed read");
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: real-hi skipped; embed read failed err={:?}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let embed_read_ms = elapsed_ms_since(embed_read_start);
        let Some(hi_hidden) = bf16_row_to_f32(embed_row.as_slice(), k_dim) else {
            log("bench lumen: real-hi skipped; embed row decode failed");
            return;
        };

        let real_hi_work_chunk_rows = lumen_dynamic_chunk_rows(
            LUMEN_REAL_HI_LM_HEAD_CHUNK_ROWS.min(lm_shape[0]),
            ap_worker_count,
        );
        log(
            format!(
                "bench lumen: real-hi start prompt={:?} token_piece={:?} token_id={} embed={} lm_head={} vocab={} hidden={} read_chunk_rows={} work_chunk_rows={} ap_workers={} tokenizer_read={}ms embed_read={}ms note=single-token-no-transformer",
                LUMEN_STATIC_HI_PROMPT,
                hi_token_piece,
                hi_token_id,
                embed_name,
                lm_name,
                lm_shape[0],
                k_dim,
                LUMEN_REAL_HI_LM_HEAD_CHUNK_ROWS,
                real_hi_work_chunk_rows,
                ap_worker_count,
                tokenizer_ms,
                embed_read_ms
            )
            .as_str(),
        );

        let real_hi_start = embassy_time_driver::now();
        let stats_before = crate::burn_baby::stats();
        let slot_counts_before = crate::burn_baby::poll_counts_for_slots(&slots_for_counts);
        let mut global_top: Option<(usize, f32)> = None;
        let mut global_checksum = 0.0f32;
        let mut scanned_rows = 0usize;
        let mut row_base = 0usize;
        while row_base < lm_shape[0] {
            if bench_cancel_requested(session_id) {
                log("bench lumen: stopped during real Hi token probe");
                return;
            }
            let chunk_rows = LUMEN_REAL_HI_LM_HEAD_CHUNK_ROWS.min(lm_shape[0] - row_base);
            let chunk_bytes = chunk_rows.saturating_mul(k_dim).saturating_mul(2);
            let chunk_rel = row_base.saturating_mul(k_dim).saturating_mul(2);
            let chunk_end = data_start
                .saturating_add(chunk_rel)
                .saturating_add(chunk_bytes);
            if chunk_bytes == 0 || chunk_end > data_end {
                log("bench lumen: real-hi stopped; lm_head chunk range invalid");
                return;
            }
            let chunk_offset = payload_offset.saturating_add(chunk_rel as u64);
            let chunk = match crate::r::stream::read_trueosfs_file_range_via_pipe_async(
                disk,
                LUMEN_WEIGHTS_PATH,
                chunk_offset,
                chunk_bytes,
            )
            .await
            {
                Ok(Some(bytes)) if bytes.len() == chunk_bytes => bytes,
                Ok(Some(bytes)) => {
                    log(
                        format!(
                            "bench lumen: real-hi stopped; short lm_head chunk got={} need={}",
                            format_bytes(bytes.len() as u64),
                            format_bytes(chunk_bytes as u64)
                        )
                        .as_str(),
                    );
                    return;
                }
                Ok(None) => {
                    log("bench lumen: real-hi stopped; weights disappeared on lm_head chunk");
                    return;
                }
                Err(err) => {
                    log(
                        format!(
                            "bench lumen: real-hi stopped; lm_head chunk read failed err={:?}",
                            err
                        )
                        .as_str(),
                    );
                    return;
                }
            };

            match run_bf16_compute_lane_probe_with_hidden(
                chunk.as_slice(),
                chunk_rows,
                k_dim,
                hi_hidden.as_slice(),
                real_hi_work_chunk_rows,
                1,
            ) {
                Ok((checksum, top, _ops_per_s, _chunk_stats_before, _chunk_stats_after)) => {
                    global_checksum += checksum;
                    if let Some((local_idx, value)) = top {
                        let global_idx = row_base + local_idx;
                        match global_top {
                            Some((_, best)) if best >= value => {}
                            _ => global_top = Some((global_idx, value)),
                        }
                    }
                }
                Err(err) => {
                    log(format!("bench lumen: real-hi stopped; compute err={:?}", err).as_str());
                    return;
                }
            }

            scanned_rows += chunk_rows;
            row_base += chunk_rows;
        }

        let elapsed_ticks = embassy_time_driver::now().saturating_sub(real_hi_start);
        let ops = (scanned_rows as u64)
            .saturating_mul(k_dim as u64)
            .saturating_mul(2);
        let ops_per_s = units_per_second_from_ticks(ops, elapsed_ticks);
        let stats_after = crate::burn_baby::stats();
        let slot_counts_after = crate::burn_baby::poll_counts_for_slots(&slots_for_counts);
        let submitted = stats_after
            .submitted_jobs
            .saturating_sub(stats_before.submitted_jobs);
        let completed = stats_after
            .completed_jobs
            .saturating_sub(stats_before.completed_jobs);
        let polled = stats_after
            .polled_jobs
            .saturating_sub(stats_before.polled_jobs);
        let top_line = global_top
            .map(|(idx, value)| format!("next_token_argmax={}:{:.4}", idx, value))
            .unwrap_or_else(|| AllocString::from("next_token_argmax=?"));
        log(
            format!(
                "bench lumen: real-hi result prompt={:?} token_piece={:?} token_id={} rows={} hidden={} compute={}/s jobs={}/{} polled={} queued={} workers={} {} checksum={:.4} note=embedding-row-to-lm-head no-transformer-no-decode",
                LUMEN_STATIC_HI_PROMPT,
                hi_token_piece,
                hi_token_id,
                scanned_rows,
                k_dim,
                format_metric_units(ops_per_s, "ops"),
                completed,
                submitted,
                polled,
                stats_after.queued_jobs,
                compute_slot_delta_line(&slot_counts_before, &slot_counts_after),
                top_line,
                global_checksum
            )
            .as_str(),
        );

        if bench_cancel_requested(session_id) {
            log("bench lumen: stopped before LUMEN runtime Hi probe");
            return;
        }
        }

        let heap = crate::allocators::heap_stats();
        let runtime_heap_need = usize::try_from(weights_info.data_len)
            .unwrap_or(usize::MAX)
            .saturating_add(LUMEN_RUNTIME_HEAP_EXTRA_BYTES);
        if heap.free_bytes < runtime_heap_need {
            log(
                format!(
                    "bench lumen: runtime-hi skipped; heap free={} largest={} total={} need~{} source={:?} note=raise-kernel-heap-or-use-streamed-model-kernels",
                    format_bytes(heap.free_bytes as u64),
                    format_bytes(heap.largest_free_block as u64),
                    format_bytes(heap.usable_total as u64),
                    format_bytes(runtime_heap_need as u64),
                    heap.source
                )
                .as_str(),
            );
            return;
        }

        log(
            format!(
                "bench lumen: runtime start parameter_dtype=bf16 runtime_dtype=bf16 max_seq_len={} max_new_tokens={} tokenizer_read={}ms heap_free={} heap_total={} note=interactive-until-q",
                LUMEN_RUNTIME_MAX_SEQ_LEN,
                LUMEN_RUNTIME_MAX_NEW_TOKENS,
                tokenizer_ms,
                format_bytes(heap.free_bytes as u64),
                format_bytes(heap.usable_total as u64)
            )
            .as_str(),
        );
        let runtime_start = embassy_time_driver::now();
        let precision = PrecisionConfig {
            parameter_dtype: DType::BF16,
            runtime_dtype: DType::BF16,
            allow_parameter_dtype_copies: true,
        };
        let config = lumen_model_config(LUMEN_RUNTIME_MAX_SEQ_LEN);
        let model = with_precision_config(precision, || {
            with_runtime_component_dtypes(Some(DType::BF16), Some(DType::BF16), || {
                with_parameter_init_mode(ParameterInitMode::Placeholder, || {
                    LlamaModel::new(config.clone())
                })
            })
        });

        let load_start = embassy_time_driver::now();
        let load_report = match load_lumen_model_from_trueosfs(
            disk,
            header_len,
            obj,
            &model,
            &task_target,
            session_id,
        )
        .await
        {
            Ok(report) => report,
            Err(err) if err == "cancelled" => {
                log("bench lumen: stopped during LUMEN runtime weight load");
                return;
            }
            Err(err) => {
                log(
                    format!(
                        "bench lumen: runtime-hi skipped; full LUMEN weight load failed err={}",
                        err
                    )
                    .as_str(),
                );
                return;
            }
        };
        let load_ms = elapsed_ms_since(load_start);

        let total_ms = elapsed_ms_since(runtime_start);
        log(
            format!(
                "bench lumen: runtime loaded load={}ms load_speed={} total={}ms tensors={} missing={} bytes={} max_new_tokens={} note=interactive-until-q",
                load_ms,
                format_speed(bps_from_progress(load_report.loaded_bytes, load_ms)),
                total_ms,
                load_report.loaded_tensors,
                load_report.missing_tensors,
                format_bytes(load_report.loaded_bytes),
                LUMEN_RUNTIME_MAX_NEW_TOKENS
            )
            .as_str(),
        );
        let vocab_entries = tokenizer_vocab_entries(&tokenizer_json);
        let stop_ids = tokenizer_stop_ids(&tokenizer_json);
        register_lumen_interactive_session(session_id);
        crate::r::lumen_service::mark_online(session_id);
        print_matrix_target_line(
            &task_target,
            "lumen: ready; type a prompt, or q to stop",
        );

        while !bench_cancel_requested(session_id) {
            let Some(request) = pop_lumen_prompt(session_id) else {
                Timer::after(EmbassyDuration::from_millis(25)).await;
                continue;
            };

            let infer_start = embassy_time_driver::now();
            match generate_lumen_answer(
                &model,
                &tokenizer_json,
                vocab_entries.as_slice(),
                stop_ids.as_slice(),
                request.prompt.as_str(),
            ) {
                Ok((answer, tokens)) => {
                    let infer_ms = elapsed_ms_since(infer_start);
                    crate::log!(
                        "bench lumen: prompt done prompt={:?} tokens={} infer={}ms\n",
                        request.prompt.as_str(),
                        tokens,
                        infer_ms
                    );
                    print_lumen_response(&request.target, answer.as_str());
                }
                Err(err) => {
                    print_matrix_target_line(
                        &request.target,
                        format!("lumen: prompt failed: {}", err).as_str(),
                    );
                }
            }
        }
        crate::r::lumen_service::mark_offline(session_id);
        unregister_lumen_interactive_session(session_id);

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}
