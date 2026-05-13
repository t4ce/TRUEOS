use alloc::collections::VecDeque;
use alloc::format;
use alloc::string::{String as AllocString, ToString};
use alloc::vec::Vec;

use ::lumen::autograd::{Tensor, TensorRawData, no_grad};
use ::lumen::init::{ParameterInitMode, with_parameter_init_mode};
use ::lumen::layers::KVCache;
use ::lumen::models::{LlamaConfig, LlamaModel};
use ::lumen::precision::DType;
use embassy_sync::signal::Signal;
use embassy_time::{Duration as EmbassyDuration, Timer};

pub(crate) mod avx2_fma_sse2_help;
pub(crate) mod burn_baba;
pub(crate) mod burn_baby;
pub(crate) mod cgp;
pub(crate) mod gpu_shadow;
pub(crate) mod lumen_net;
pub(crate) mod lumen_service;

use crate::shell2::cmds::bench::{
    bench_cancel_requested, bench_session_finish, bps_from_progress, elapsed_ms_since,
    format_bytes, format_metric_units, format_speed, online_background_worker_slots,
};
use crate::shell2::{MatrixTarget, print_matrix_target_line, set_matrix_target_active};

const LUMEN_WEIGHTS_PATH: &str = "model.safetensors";
const LUMEN_TOKENIZER_PATH: &str = "tokenizer.json";
const LUMEN_SAFETENSORS_MAX_HEADER_BYTES: usize = 64 * 1024 * 1024;
const LUMEN_KERNEL_PROBE_ROWS: usize = 16;
const LUMEN_KERNEL_PROBE_ITERS: usize = 256;
const LUMEN_STATIC_HI_PROMPT: &str = "Hi";
const LUMEN_REAL_HI_LM_HEAD_CHUNK_ROWS: usize = 512;
const LUMEN_AP_CHUNKS_PER_WORKER: usize = 4;
const LUMEN_AP_MIN_CHUNK_ROWS: usize = 16;
const LUMEN_AP_MAX_CHUNK_ROWS: usize = 256;
const LUMEN_RUNTIME_MAX_SEQ_LEN: usize = 512;
const LUMEN_RUNTIME_MAX_NEW_TOKENS: usize = 48;
const LUMEN_RUNTIME_SOFT_STOP_TOKENS: usize = 24;
const LUMEN_RUNTIME_MIN_SENTENCE_TOKENS: usize = 12;
const LUMEN_RUNTIME_STREAM_TOKENS: usize = 5;
const LUMEN_RUNTIME_MAX_ANSWER_CHARS: usize = 512;
const LUMEN_RUNTIME_SELFTEST_PROMPT: &str = "Hello Lumen, how are you doing today";
const LUMEN_RUNTIME_PROGRESS_TENSORS: usize = 16;
const LUMEN_RUNTIME_EARLY_PROGRESS_TENSORS: usize = 2;
const LUMEN_RUNTIME_HEAP_EXTRA_BYTES: usize = 512 * 1024 * 1024;
const LUMEN_PREFLIGHT_WAIT_MS: u64 = 75_000;
const LUMEN_PREFLIGHT_POLL_MS: u64 = 250;
const LUMEN_PREFLIGHT_LOG_MS: u64 = 2_000;
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
    pub(crate) prompt: AllocString,
    pub(crate) statement: Option<AllocString>,
}

static LUMEN_INTERACTIVE_SESSIONS: spin::Mutex<Vec<LumenInteractiveSession>> =
    spin::Mutex::new(Vec::new());
static LUMEN_PROMPT_SIGNAL: Signal<crate::wait::EmbassySpinRawMutex, u64> = Signal::new();
static LUMEN_INFER_MAILBOX: spin::Mutex<LumenInferMailbox> =
    spin::Mutex::new(LumenInferMailbox::new());
static LUMEN_INFER_REQUEST_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();
static LUMEN_INFER_RESULT_WAIT: crate::wait::WaitQueue = crate::wait::WaitQueue::new();

#[inline]
fn lumen_cooperate() {
    crate::time::poll();
    crate::smp::poll();
}

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
    let mut sessions = LUMEN_INTERACTIVE_SESSIONS.lock();
    let session = sessions
        .iter_mut()
        .find(|session| session.id == session_id)?;
    let request = session.prompts.pop_front()?;
    crate::log!(
        "lumen: prompt dequeue session={} remaining={} bytes={}\n",
        session_id,
        session.prompts.len(),
        request.prompt.len()
    );
    Some(request)
}

pub(crate) fn push_lumen_chat_prompt(
    session_id: u64,
    prompt: &str,
    statement: Option<&str>,
) -> bool {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return true;
    }
    let queued = {
        let mut sessions = LUMEN_INTERACTIVE_SESSIONS.lock();
        let Some(session) = sessions.iter_mut().find(|session| session.id == session_id) else {
            crate::log!("lumen: chat prompt enqueue missed session={}\n", session_id);
            return false;
        };
        session.prompts.push_back(LumenPromptRequest {
            prompt: AllocString::from(prompt),
            statement: statement.map(AllocString::from),
        });
        session.prompts.len()
    };
    crate::log!(
        "lumen: prompt enqueue session={} queued={} bytes={}\n",
        session_id,
        queued,
        prompt.len()
    );
    LUMEN_PROMPT_SIGNAL.signal(session_id);
    true
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
    format!(
        "System: You are Lumen, a friendly local chat assistant. Reply directly and briefly.\nUser: {}\nAssistant:",
        user.trim()
    )
}

fn lumen_chat_next_turn_prompt(user: &str) -> AllocString {
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

fn lumen_turn_marker_end(answer: &str) -> usize {
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
    end
}

fn trim_lumen_turn_markers(answer: &str) -> AllocString {
    answer[..lumen_turn_marker_end(answer)].trim().to_string()
}

fn lumen_answer_has_sentence_end(answer: &str) -> bool {
    let trimmed = answer.trim_end();
    let trimmed = trimmed.trim_end_matches(['"', '\'', ')', ']', '}']);
    trimmed
        .chars()
        .next_back()
        .map(|ch| matches!(ch, '.' | '?' | '!'))
        .unwrap_or(false)
}

fn lumen_should_stop_reasonably(answer: &str, generated_tokens: usize) -> bool {
    if answer.len() >= LUMEN_RUNTIME_MAX_ANSWER_CHARS {
        return true;
    }
    if generated_tokens >= LUMEN_RUNTIME_SOFT_STOP_TOKENS && lumen_answer_has_sentence_end(answer) {
        return true;
    }
    generated_tokens >= LUMEN_RUNTIME_MIN_SENTENCE_TOKENS
        && answer.len() >= 80
        && lumen_answer_has_sentence_end(answer)
}

struct LumenGenerateReport {
    answer: AllocString,
    prompt_tokens: usize,
    generated_tokens: usize,
    first_token_ms: u64,
    streamed: bool,
}

struct LumenInferenceRequest {
    id: u64,
    session_id: u64,
    prompt: AllocString,
    statement: Option<AllocString>,
}

struct LumenInferenceResult {
    id: u64,
    session_id: u64,
    result: Result<LumenGenerateReport, AllocString>,
}

struct LumenInferMailbox {
    pending: Option<LumenInferenceRequest>,
    result: Option<LumenInferenceResult>,
    busy: bool,
    next_job_id: u64,
}

impl LumenInferMailbox {
    const fn new() -> Self {
        Self {
            pending: None,
            result: None,
            busy: false,
            next_job_id: 1,
        }
    }
}

enum LumenInferEngine {
    Local {
        model: LlamaModel,
        chat_state: LumenChatState,
    },
    Worker,
}

struct ExclusiveAp2LumenModel {
    model: LlamaModel,
}

struct ExclusiveAp2LumenRuntime {
    model: LlamaModel,
    chat_state: LumenChatState,
}

// Safety: LlamaModel tensors are Rc/RefCell-backed, so the model is not Send in
// the general case. This wrapper exists only for the one-time BSP -> AP2+
// handoff: BSP constructs/loads the model, wraps it exactly once, then moves it
// into lumen_inference_worker_task. The wrapper is not Clone, and after this
// move BSP must not borrow, dereference, or otherwise touch the model or its
// Rc/RefCell internals. The AP2+ worker owns the model, KV caches, and scratch
// state until the worker exits. Burn-baby AP jobs may still receive raw weight
// memory chunks for numeric kernels; they never own or access this LlamaModel.
unsafe impl Send for ExclusiveAp2LumenModel {}
unsafe impl Send for ExclusiveAp2LumenRuntime {}

impl ExclusiveAp2LumenModel {
    fn new(model: LlamaModel) -> Self {
        Self { model }
    }

    fn into_runtime(self) -> ExclusiveAp2LumenRuntime {
        let model = self.model;
        let chat_state = LumenChatState::new(&model);
        ExclusiveAp2LumenRuntime { model, chat_state }
    }
}

struct LumenChatState {
    kv_caches: Vec<KVCache>,
    all_tokens: Vec<usize>,
    first_turn: bool,
}

impl LumenChatState {
    fn new(model: &LlamaModel) -> Self {
        let mut kv_caches = model.init_kv_caches(1);
        model.reset_kv_caches(&mut kv_caches);
        Self {
            kv_caches,
            all_tokens: Vec::new(),
            first_turn: true,
        }
    }

    fn reset(&mut self, model: &LlamaModel) {
        self.all_tokens.clear();
        model.reset_kv_caches(&mut self.kv_caches);
        self.first_turn = true;
    }

    fn cache_len(&self) -> usize {
        self.kv_caches
            .first()
            .map(|cache| cache.borrow().len)
            .unwrap_or(0)
    }
}

fn format_tokens_per_second(tokens: usize, elapsed_ms: u64) -> AllocString {
    if tokens == 0 || elapsed_ms == 0 {
        return AllocString::from("0 tok/s");
    }
    format!("{:.2} tok/s", tokens as f64 * 1000.0 / elapsed_ms as f64)
}

fn generate_lumen_answer(
    model: &LlamaModel,
    state: &mut LumenChatState,
    tokenizer_json: &serde_json::Value,
    vocab_entries: &[(AllocString, usize)],
    stop_ids: &[usize],
    prompt: &str,
    statement: Option<&str>,
) -> Result<LumenGenerateReport, AllocString> {
    let chat_prompt = if state.first_turn {
        lumen_chat_prompt(prompt)
    } else {
        lumen_chat_next_turn_prompt(prompt)
    };
    let prompt_tokens = encode_prompt_lossy(chat_prompt.as_str(), vocab_entries);
    if prompt_tokens.is_empty() {
        return Err(AllocString::from("tokenizer produced no tokens"));
    }
    if state.cache_len() == 0
        && prompt_tokens
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

    let mut prompt_tokens = prompt_tokens;
    let projected_len = state
        .cache_len()
        .saturating_add(prompt_tokens.len())
        .saturating_add(LUMEN_RUNTIME_MAX_NEW_TOKENS)
        .saturating_add(8);
    if state.cache_len() != 0 && projected_len >= LUMEN_RUNTIME_MAX_SEQ_LEN {
        let old_cache_len = state.cache_len();
        crate::log!(
            "lumen: context full; resetting conversation cache_len={} remembered_tokens={} new_prompt_tokens={} max_new_tokens={} max_seq_len={} action=new-conversation\n",
            old_cache_len,
            state.all_tokens.len(),
            prompt_tokens.len(),
            LUMEN_RUNTIME_MAX_NEW_TOKENS,
            LUMEN_RUNTIME_MAX_SEQ_LEN
        );
        crate::lumen::lumen_service::submit_chat_answer(
            "context size full, starting a new conversation.",
        );
        state.reset(model);

        let fresh_prompt = lumen_chat_prompt(prompt);
        prompt_tokens = encode_prompt_lossy(fresh_prompt.as_str(), vocab_entries);
        if prompt_tokens.is_empty() {
            return Err(AllocString::from("tokenizer produced no tokens"));
        }
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

    let mut answer = AllocString::new();
    let mut generated = 0usize;
    let mut first_token_ms = 0u64;
    let stream_statement = statement.map(AllocString::from);
    let mut streamed_answer_len = 0usize;
    let mut streamed = false;
    let generate_start = embassy_time_driver::now();
    let prefill_start_len = state.cache_len();
    let _bf16_prompt_context = crate::lumen::burn_baby::enter_lumen_prompt_bf16_context();
    state.all_tokens.extend_from_slice(&prompt_tokens);
    crate::log!(
        "lumen: generation start prompt_tokens={} context_tokens={} max_new_tokens={} max_seq_len={} prefill_mode=incremental-decode-ap cgp_backend=local-gpgpu-burn-baby cgp_role=proof-only note=first-token-is-prompt-ingest\n",
        prompt_tokens.len(),
        prefill_start_len,
        LUMEN_RUNTIME_MAX_NEW_TOKENS,
        LUMEN_RUNTIME_MAX_SEQ_LEN
    );
    lumen_cooperate();

    no_grad(|| {
        let first_token_start = embassy_time_driver::now();
        let mut next_token = 0usize;
        for (idx, prompt_token) in prompt_tokens.iter().copied().enumerate() {
            lumen_cooperate();
            let input = tensor_from_token_ids(&[prompt_token])?;
            next_token = model.forward_last_argmax(input, &mut state.kv_caches, 0);
            lumen_cooperate();
            let consumed = idx + 1;
            if consumed == 1 || consumed == prompt_tokens.len() || consumed.is_multiple_of(4) {
                crate::log!(
                    "lumen: prefill progress consumed={} total={} next_token={} elapsed={}ms\n",
                    consumed,
                    prompt_tokens.len(),
                    next_token,
                    elapsed_ms_since(generate_start)
                );
            }
        }
        first_token_ms = elapsed_ms_since(first_token_start);
        crate::log!(
            "lumen: first-token done token_id={} first_token={}ms total={}ms prefill_mode=incremental-decode-ap\n",
            next_token,
            first_token_ms,
            elapsed_ms_since(generate_start)
        );

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
            state.all_tokens.push(next_token);
            generated = generated.saturating_add(1);
            if generated == 1 || generated.is_multiple_of(4) {
                crate::log!(
                    "lumen: token progress generated={} last_token={} elapsed={}ms\n",
                    generated,
                    next_token,
                    elapsed_ms_since(generate_start)
                );
            }

            let stop_after_emit = answer.contains("\nUser:")
                || answer.contains("\nAssistant:")
                || lumen_should_stop_reasonably(answer.as_str(), generated);
            let cache_nearly_full = state
                .kv_caches
                .first()
                .map(|cache| cache.borrow().len.saturating_add(2) >= LUMEN_RUNTIME_MAX_SEQ_LEN)
                .unwrap_or(false);
            if !stop_after_emit
                && !cache_nearly_full
                && generated.is_multiple_of(LUMEN_RUNTIME_STREAM_TOKENS)
                && answer.len() > streamed_answer_len
            {
                if let Some(stream_statement) = stream_statement.as_deref() {
                    crate::lumen::lumen_service::submit_chat_statement_delta(
                        stream_statement,
                        &answer[streamed_answer_len..],
                    );
                    streamed_answer_len = answer.len();
                    streamed = true;
                }
            }
            if stop_after_emit || cache_nearly_full {
                if !cache_nearly_full {
                    lumen_cooperate();
                    let input = tensor_from_token_ids(&[next_token])?;
                    let _ = model.forward_last_argmax(input, &mut state.kv_caches, 0);
                    lumen_cooperate();
                }
                break;
            }

            lumen_cooperate();
            let input = tensor_from_token_ids(&[next_token])?;
            next_token = model.forward_last_argmax(input, &mut state.kv_caches, 0);
            lumen_cooperate();
        }

        crate::log!(
            "lumen: generation done generated={} elapsed={}ms\n",
            generated,
            elapsed_ms_since(generate_start)
        );

        Ok::<(), AllocString>(())
    })?;

    state.first_turn = false;
    let visible_answer_end = lumen_turn_marker_end(answer.as_str());
    let final_answer = trim_lumen_turn_markers(answer.as_str());
    if streamed_answer_len == 0 {
        if !final_answer.is_empty() {
            if let Some(stream_statement) = stream_statement.as_deref() {
                crate::lumen::lumen_service::submit_chat_statement_delta(
                    stream_statement,
                    final_answer.as_str(),
                );
                streamed = true;
            }
        }
    } else if visible_answer_end > streamed_answer_len {
        if let Some(stream_statement) = stream_statement.as_deref() {
            crate::lumen::lumen_service::submit_chat_statement_delta(
                stream_statement,
                &answer[streamed_answer_len..visible_answer_end],
            );
            streamed = true;
        }
    }

    Ok(LumenGenerateReport {
        answer: final_answer,
        prompt_tokens: prompt_tokens.len(),
        generated_tokens: generated,
        first_token_ms,
        streamed,
    })
}

fn submit_lumen_inference(
    session_id: u64,
    prompt: &str,
    statement: Option<&str>,
) -> Result<u64, AllocString> {
    let mut mailbox = LUMEN_INFER_MAILBOX.lock();
    if mailbox.busy || mailbox.pending.is_some() || mailbox.result.is_some() {
        return Err(AllocString::from("inference mailbox busy"));
    }
    let id = mailbox.next_job_id;
    mailbox.next_job_id = mailbox.next_job_id.saturating_add(1).max(1);
    mailbox.pending = Some(LumenInferenceRequest {
        id,
        session_id,
        prompt: AllocString::from(prompt),
        statement: statement.map(AllocString::from),
    });
    mailbox.busy = true;
    drop(mailbox);
    LUMEN_INFER_REQUEST_WAIT.notify_one();
    Ok(id)
}

fn take_lumen_inference_result(id: u64) -> Option<Result<LumenGenerateReport, AllocString>> {
    let mut mailbox = LUMEN_INFER_MAILBOX.lock();
    if mailbox.result.as_ref().map(|result| result.id) != Some(id) {
        return None;
    }
    mailbox.result.take().map(|result| result.result)
}

fn cleanup_lumen_inference_mailbox(session_id: u64, reason: &str) {
    let (removed_requests, removed_results, cleared_busy) = {
        let mut mailbox = LUMEN_INFER_MAILBOX.lock();
        let removed_requests = usize::from(
            mailbox
                .pending
                .as_ref()
                .map(|request| request.session_id == session_id)
                .unwrap_or(false),
        );
        if removed_requests != 0 {
            mailbox.pending = None;
        }
        let removed_results = usize::from(
            mailbox
                .result
                .as_ref()
                .map(|result| result.session_id == session_id)
                .unwrap_or(false),
        );
        if removed_results != 0 {
            mailbox.result = None;
        }
        let cleared_busy = mailbox.busy && (removed_requests != 0 || removed_results != 0);
        if cleared_busy {
            mailbox.busy = false;
        }
        (removed_requests, removed_results, cleared_busy)
    };
    let request_waiters = LUMEN_INFER_REQUEST_WAIT.notify_all();
    let result_waiters = LUMEN_INFER_RESULT_WAIT.notify_all();
    crate::log!(
        "lumen: inference mailbox cleanup session={} reason={} removed_requests={} removed_results={} cleared_busy={} woke_request_waiters={} woke_result_waiters={}\n",
        session_id,
        reason,
        removed_requests,
        removed_results,
        cleared_busy,
        request_waiters,
        result_waiters
    );
}

async fn wait_lumen_inference_result(
    session_id: u64,
    id: u64,
) -> Result<LumenGenerateReport, AllocString> {
    loop {
        if let Some(result) = take_lumen_inference_result(id) {
            return result;
        }
        if bench_cancel_requested(session_id) {
            cleanup_lumen_inference_mailbox(session_id, "session-cancel-wait");
            return Err(AllocString::from("cancelled"));
        }
        LUMEN_INFER_RESULT_WAIT.wait_for_event_timeout(1_000).await;
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn lumen_inference_worker_task(
    session_id: u64,
    model: ExclusiveAp2LumenModel,
    tokenizer_json: serde_json::Value,
    vocab_entries: Vec<(AllocString, usize)>,
    stop_ids: Vec<usize>,
) {
    let cpu_slot = crate::percpu::current_slot();
    let lapic_id = crate::percpu::current_lapic_id_via_cpuid();
    crate::log!(
        "lumen: AP2+ inference worker start cpu_slot={} lapic={} session={} ownership=exclusive-model-kv-scratch\n",
        cpu_slot,
        lapic_id,
        session_id
    );
    let mut runtime = model.into_runtime();
    loop {
        let request = {
            let mut mailbox = LUMEN_INFER_MAILBOX.lock();
            if mailbox
                .pending
                .as_ref()
                .map(|request| request.session_id == session_id)
                .unwrap_or(false)
            {
                mailbox.pending.take()
            } else {
                None
            }
        };
        let Some(request) = request else {
            if bench_cancel_requested(session_id) {
                cleanup_lumen_inference_mailbox(session_id, "worker-cancel");
                crate::log!(
                    "lumen: AP2+ inference worker exit cpu_slot={} lapic={} session={} reason=cancel ownership=released\n",
                    cpu_slot,
                    lapic_id,
                    session_id
                );
                return;
            }
            LUMEN_INFER_REQUEST_WAIT.wait_for_event_timeout(1_000).await;
            continue;
        };
        let result = generate_lumen_answer(
            &runtime.model,
            &mut runtime.chat_state,
            &tokenizer_json,
            vocab_entries.as_slice(),
            stop_ids.as_slice(),
            request.prompt.as_str(),
            request.statement.as_deref(),
        );
        {
            let mut mailbox = LUMEN_INFER_MAILBOX.lock();
            mailbox.result = Some(LumenInferenceResult {
                id: request.id,
                session_id,
                result,
            });
            mailbox.busy = false;
        }
        LUMEN_INFER_RESULT_WAIT.notify_all();
    }
}

fn safetensor_dtype_nbytes(dtype: &str) -> Option<usize> {
    match dtype {
        "F32" => Some(4),
        "F16" | "BF16" => Some(2),
        "I8" | "U8" | "BOOL" => Some(1),
        _ => None,
    }
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

fn units_per_second_from_ticks(units: u64, elapsed_ticks: u64) -> u64 {
    if elapsed_ticks == 0 {
        0
    } else {
        units.saturating_mul(embassy_time_driver::TICK_HZ) / elapsed_ticks
    }
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
        crate::lumen::burn_baby::ComputeStats,
        crate::lumen::burn_baby::ComputeStats,
    ),
    crate::lumen::burn_baby::ComputeError,
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
        crate::lumen::burn_baby::ComputeStats,
        crate::lumen::burn_baby::ComputeStats,
    ),
    crate::lumen::burn_baby::ComputeError,
> {
    let mut out = Vec::new();
    out.resize(rows, 0.0f32);

    let stats_before = crate::lumen::burn_baby::stats();
    let start = embassy_time_driver::now();
    for _ in 0..iters {
        crate::lumen::burn_baby::matvec_rowmajor_bf16(
            hidden,
            rows_bf16,
            rows,
            k_dim,
            out.as_mut_slice(),
            chunk_rows,
        )?;
    }
    let elapsed_ticks = embassy_time_driver::now().saturating_sub(start);
    let stats_after = crate::lumen::burn_baby::stats();
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
    phase_ms: LumenRuntimeLoadPhaseMs,
}

#[derive(Clone, Copy, Default)]
struct LumenRuntimeLoadPhaseMs {
    alloc: u64,
    read: u64,
    decode: u64,
    import: u64,
    manifest: u64,
}

impl LumenRuntimeLoadPhaseMs {
    fn total(self) -> u64 {
        self.alloc
            .saturating_add(self.read)
            .saturating_add(self.decode)
            .saturating_add(self.import)
            .saturating_add(self.manifest)
    }

    fn saturating_sub(self, previous: Self) -> Self {
        Self {
            alloc: self.alloc.saturating_sub(previous.alloc),
            read: self.read.saturating_sub(previous.read),
            decode: self.decode.saturating_sub(previous.decode),
            import: self.import.saturating_sub(previous.import),
            manifest: self.manifest.saturating_sub(previous.manifest),
        }
    }
}

fn lumen_preflight_retryable_error(err: crate::disc::block::Error) -> bool {
    matches!(
        err,
        crate::disc::block::Error::NotReady
            | crate::disc::block::Error::Timeout
            | crate::disc::block::Error::Io
    )
}

async fn wait_lumen_file_info(
    disk: crate::disc::block::DeviceHandle,
    path: &str,
    min_bytes: u64,
    session_id: u64,
) -> Result<crate::r::fs::trueosfs::FileInfo, AllocString> {
    let start = embassy_time_driver::now();
    let mut last_log_ms = 0u64;

    loop {
        if bench_cancel_requested(session_id) {
            return Err(format!("cancelled while waiting for {}", path));
        }

        let last_status = match crate::r::fs::trueosfs::file_info_async(disk, path).await {
            Ok(Some(info)) if info.data_len >= min_bytes => return Ok(info),
            Ok(Some(info)) => format!("incomplete bytes={} need>={}", info.data_len, min_bytes),
            Ok(None) => AllocString::from("missing"),
            Err(err) if lumen_preflight_retryable_error(err) => format!("err={:?}", err),
            Err(err) => {
                return Err(format!("probe failed path={} err={:?}", path, err));
            }
        };

        let waited_ms = elapsed_ms_since(start);
        if waited_ms >= LUMEN_PREFLIGHT_WAIT_MS {
            return Err(format!(
                "timeout waiting path={} waited={}ms status={}",
                path, waited_ms, last_status
            ));
        }

        if waited_ms.saturating_sub(last_log_ms) >= LUMEN_PREFLIGHT_LOG_MS {
            last_log_ms = waited_ms;
            crate::log!(
                "bench lumen: waiting for TRUEOSFS file path={} waited={}ms status={}\n",
                path,
                waited_ms,
                last_status
            );
        }

        Timer::after(EmbassyDuration::from_millis(LUMEN_PREFLIGHT_POLL_MS)).await;
    }
}

async fn wait_lumen_root_handle(
    session_id: u64,
) -> Result<crate::disc::block::DeviceHandle, AllocString> {
    let start = embassy_time_driver::now();
    let mut last_log_ms = 0u64;

    loop {
        if bench_cancel_requested(session_id) {
            return Err(AllocString::from("cancelled while waiting for TRUEOSFS root"));
        }

        if let Some(disk) = crate::r::fs::trueosfs::primary_root_handle() {
            return Ok(disk);
        }

        let waited_ms = elapsed_ms_since(start);
        if waited_ms >= LUMEN_PREFLIGHT_WAIT_MS {
            return Err(format!("timeout waiting TRUEOSFS root waited={}ms", waited_ms));
        }

        if waited_ms.saturating_sub(last_log_ms) >= LUMEN_PREFLIGHT_LOG_MS {
            last_log_ms = waited_ms;
            crate::log!("bench lumen: waiting for TRUEOSFS root waited={}ms\n", waited_ms);
        }

        Timer::after(EmbassyDuration::from_millis(LUMEN_PREFLIGHT_POLL_MS)).await;
    }
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
    let mut phase_ms = LumenRuntimeLoadPhaseMs::default();
    let mut last_progress_phase_ms = phase_ms;
    crate::lumen::lumen_net::begin_matrix_manifest_load();
    let mut weights_reader =
        match crate::r::stream::TrueosFsObjectReader::open(disk, LUMEN_WEIGHTS_PATH).await {
            Ok(Some(reader)) => reader,
            Ok(None) => {
                return Err(format!("{} unavailable during weight load", LUMEN_WEIGHTS_PATH));
            }
            Err(err) => return Err(format!("{} open failed err={:?}", LUMEN_WEIGHTS_PATH, err)),
        };
    crate::log!(
        "bench lumen: runtime-hi vlayer-read open path={} bytes={}\n",
        LUMEN_WEIGHTS_PATH,
        weights_reader.total_len()
    );

    macro_rules! fail_load {
        ($err:expr) => {{
            let _ = weights_reader.close();
            return Err($err);
        }};
    }

    for (_, name, tensor) in ordered {
        if bench_cancel_requested(session_id) {
            fail_load!(AllocString::from("cancelled"));
        }

        let Some(value) = obj.get(name) else {
            missing_tensors = missing_tensors.saturating_add(1);
            continue;
        };
        let dtype = value.get("dtype").and_then(|v| v.as_str()).unwrap_or("?");
        let shape = match safetensor_shape(value) {
            Some(shape) => shape,
            None => fail_load!(format!("{} missing safetensors shape", name)),
        };
        if shape != tensor.shape_vec() {
            fail_load!(format!(
                "{} shape mismatch safetensors={:?} model={:?}",
                name,
                shape,
                tensor.shape_vec()
            ));
        }
        let Some(dtype_bytes) = safetensor_dtype_nbytes(dtype) else {
            fail_load!(format!("{} unsupported safetensors dtype={}", name, dtype));
        };
        let Some(loc) = safetensor_location(header_len, value) else {
            fail_load!(format!("{} missing safetensors offsets", name));
        };
        let elem_count = match shape
            .iter()
            .copied()
            .try_fold(1usize, |acc, dim| acc.checked_mul(dim))
        {
            Some(count) => count,
            None => fail_load!(format!("{} element count overflow", name)),
        };
        let byte_count = match elem_count.checked_mul(dtype_bytes) {
            Some(count) => count,
            None => fail_load!(format!("{} byte count overflow", name)),
        };
        if loc.data_start.saturating_add(byte_count) > loc.data_end {
            fail_load!(format!("{} safetensors range invalid", name));
        }

        let alloc_start = embassy_time_driver::now();
        let mut bytes = vec![0u8; byte_count];
        phase_ms.alloc = phase_ms.alloc.saturating_add(elapsed_ms_since(alloc_start));

        let read_start = embassy_time_driver::now();
        match weights_reader
            .read_exact_at(loc.file_offset, bytes.as_mut_slice())
            .await
        {
            Ok(true) => {}
            Ok(false) => fail_load!(format!("{} disappeared during weight load", name)),
            Err(err) => fail_load!(format!("{} read failed err={:?}", name, err)),
        }
        phase_ms.read = phase_ms.read.saturating_add(elapsed_ms_since(read_start));

        let decode_start = embassy_time_driver::now();
        let raw = match dtype {
            "BF16" => (DType::BF16, TensorRawData::BF16LeBytes(bytes)),
            "F16" => (DType::F16, TensorRawData::F16LeBytes(bytes)),
            "F32" => {
                let mut values = Vec::with_capacity(bytes.len() / 4);
                for chunk in bytes.chunks_exact(4) {
                    values.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
                }
                (DType::F32, TensorRawData::F32(values))
            }
            other => fail_load!(format!("{} unsupported runtime import dtype={}", name, other)),
        };
        phase_ms.decode = phase_ms
            .decode
            .saturating_add(elapsed_ms_since(decode_start));

        let import_start = embassy_time_driver::now();
        if let Err(err) = tensor.import_raw(shape.clone(), raw.0, raw.1) {
            fail_load!(format!("{} {} import failed: {}", name, dtype, err));
        }
        phase_ms.import = phase_ms
            .import
            .saturating_add(elapsed_ms_since(import_start));

        let manifest_start = embassy_time_driver::now();
        if let Some((ptr, len)) = tensor.bf16_storage_ptr_len_bytes() {
            crate::lumen::lumen_net::register_loaded_matrix(name, dtype, &shape, ptr as usize, len);
        }
        phase_ms.manifest = phase_ms
            .manifest
            .saturating_add(elapsed_ms_since(manifest_start));

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
            let step_phase_ms = phase_ms.saturating_sub(last_progress_phase_ms);
            crate::log!(
                "bench lumen: runtime-hi loading tensors={} tensor={} tensor_bytes={} bytes={} elapsed={}ms speed={} step_bytes={} step_ms={} step_speed={} phase_ms=alloc:{} read:{} decode:{} import:{} manifest:{} step_phase_ms=alloc:{} read:{} decode:{} import:{} manifest:{}\n",
                loaded_tensors,
                name,
                format_bytes(byte_count as u64),
                format_bytes(loaded_bytes),
                elapsed_ms,
                format_speed(bps_from_progress(loaded_bytes, elapsed_ms)),
                format_bytes(step_bytes),
                step_ms,
                format_speed(bps_from_progress(step_bytes, step_ms)),
                phase_ms.alloc,
                phase_ms.read,
                phase_ms.decode,
                phase_ms.import,
                phase_ms.manifest,
                step_phase_ms.alloc,
                step_phase_ms.read,
                step_phase_ms.decode,
                step_phase_ms.import,
                step_phase_ms.manifest
            );
            last_progress_tick = embassy_time_driver::now();
            last_progress_bytes = loaded_bytes;
            last_progress_phase_ms = phase_ms;
            Timer::after(EmbassyDuration::from_millis(1)).await;
        } else if loaded_tensors % 3 == 0 {
            Timer::after(EmbassyDuration::from_millis(1)).await;
        }
    }

    let _ = weights_reader.close();
    crate::log!(
        "bench lumen: runtime-hi vlayer-read close path={} loaded_bytes={}\n",
        LUMEN_WEIGHTS_PATH,
        loaded_bytes
    );

    Ok(LumenRuntimeLoadReport {
        loaded_tensors,
        loaded_bytes,
        missing_tensors,
        phase_ms,
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

        let disk = match wait_lumen_root_handle(session_id).await {
            Ok(disk) => disk,
            Err(err) => {
                log(format!("bench lumen: skipped; {}", err).as_str());
                return;
            }
        };

        let weights_info = match wait_lumen_file_info(disk, LUMEN_WEIGHTS_PATH, 16, session_id).await
        {
            Ok(info) => info,
            Err(err) => {
                log(format!("bench lumen: skipped; weights {}", err).as_str());
                return;
            }
        };

        let tokenizer_info =
            match wait_lumen_file_info(disk, LUMEN_TOKENIZER_PATH, 1, session_id).await {
                Ok(info) => info,
                Err(err) => {
                    log(format!("bench lumen: skipped; tokenizer {}", err).as_str());
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
                ::lumen::backend::default_backend_name(),
                ::lumen::backend::trueos_parallel_contract(),
                ::lumen::ops::fp_kernels::active_float_backend_name(),
                ::lumen::ops::int8_kernels::active_int8_backend_name(),
                ::lumen::arch::trueos_cpu_backend_compiled(),
                ::lumen::arch::x86_fp_kernel_runtime_available(),
                ::lumen::arch::x86_i8_kernel_runtime_available(),
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
            let slot_counts_before = crate::lumen::burn_baby::poll_counts_for_slots(&slots_for_counts);
            match run_bf16_compute_lane_probe(
                &compute_rows_bytes[..compute_bytes],
                compute_rows,
                k_dim,
                chunk_rows,
                case.iters,
            ) {
                Ok((checksum, top, ops_per_s, stats_before, stats_after)) => {
                    let slot_counts_after = crate::lumen::burn_baby::poll_counts_for_slots(&slots_for_counts);
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
        let stats_before = crate::lumen::burn_baby::stats();
        let slot_counts_before = crate::lumen::burn_baby::poll_counts_for_slots(&slots_for_counts);
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
        let stats_after = crate::lumen::burn_baby::stats();
        let slot_counts_after = crate::lumen::burn_baby::poll_counts_for_slots(&slots_for_counts);
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
                "bench lumen: runtime start parameter_dtype=bf16 runtime_dtype=bf16 max_seq_len={} max_new_tokens={} soft_stop_tokens={} min_sentence_tokens={} tokenizer_read={}ms heap_free={} heap_total={} note=interactive-until-q",
                LUMEN_RUNTIME_MAX_SEQ_LEN,
                LUMEN_RUNTIME_MAX_NEW_TOKENS,
                LUMEN_RUNTIME_SOFT_STOP_TOKENS,
                LUMEN_RUNTIME_MIN_SENTENCE_TOKENS,
                tokenizer_ms,
                format_bytes(heap.free_bytes as u64),
                format_bytes(heap.usable_total as u64)
            )
            .as_str(),
        );
        let runtime_start = embassy_time_driver::now();
        let config = lumen_model_config(LUMEN_RUNTIME_MAX_SEQ_LEN);
        log(
            format!(
                "bench lumen: runtime model construct start mode=placeholder safetensor_entries={} layers={} hidden={} vocab={}",
                obj.len(),
                config.num_hidden_layers,
                config.hidden_size,
                config.vocab_size
            )
            .as_str(),
        );
        let model_construct_start = embassy_time_driver::now();
        let model = with_parameter_init_mode(ParameterInitMode::Placeholder, || {
            LlamaModel::new_with_runtime_dtypes(
                config.clone(),
                DType::BF16,
                DType::BF16,
                DType::BF16,
            )
        });
        let model_construct_ms = elapsed_ms_since(model_construct_start);
        log(
            format!(
                "bench lumen: runtime model construct done elapsed={}ms parameters={} next=weight-load",
                model_construct_ms,
                model.named_parameters().len()
            )
            .as_str(),
        );

        let load_start = embassy_time_driver::now();
        log(
            format!(
                "bench lumen: runtime weight load begin path={} safetensor_entries={} header_len={} session={}",
                LUMEN_WEIGHTS_PATH,
                obj.len(),
                header_len,
                session_id
            )
            .as_str(),
        );
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
                "bench lumen: runtime loaded load={}ms load_speed={} total={}ms tensors={} missing={} bytes={} phase_ms=alloc:{} read:{} decode:{} import:{} manifest:{} accounted:{} max_new_tokens={} note=interactive-until-q",
                load_ms,
                format_speed(bps_from_progress(load_report.loaded_bytes, load_ms)),
                total_ms,
                load_report.loaded_tensors,
                load_report.missing_tensors,
                format_bytes(load_report.loaded_bytes),
                load_report.phase_ms.alloc,
                load_report.phase_ms.read,
                load_report.phase_ms.decode,
                load_report.phase_ms.import,
                load_report.phase_ms.manifest,
                load_report.phase_ms.total(),
                LUMEN_RUNTIME_MAX_NEW_TOKENS
            )
            .as_str(),
        );
        let vocab_entries = tokenizer_vocab_entries(&tokenizer_json);
        let stop_ids = tokenizer_stop_ids(&tokenizer_json);
        let mut infer_engine = match crate::workers::pick_background_spawner_with_slot() {
            Some((slot, kind, spawner)) => {
                let bsp_cpu_slot = crate::percpu::current_slot();
                let bsp_lapic_id = crate::percpu::current_lapic_id_via_cpuid();
                crate::log!(
                    "lumen: BSP transferring exclusive LlamaModel ownership to AP2+ worker target_slot={} kind={} session={} bsp_cpu_slot={} bsp_lapic={} invariant=bsp-model-dead-after-move\n",
                    slot,
                    kind,
                    session_id,
                    bsp_cpu_slot,
                    bsp_lapic_id
                );
                // Ownership boundary: this consumes model; Worker keeps no BSP handle.
                let exclusive_model = ExclusiveAp2LumenModel::new(model);
                crate::lumen::burn_baby::protect_service_compute_slot(slot, "lumen-inference-worker");
                match lumen_inference_worker_task(
                    session_id,
                    exclusive_model,
                    tokenizer_json.clone(),
                    vocab_entries.clone(),
                    stop_ids.clone(),
                ) {
                    Ok(token) => {
                        spawner.spawn(token);
                        crate::log!(
                            "lumen: AP2+ inference worker spawned slot={} kind={} session={} ownership=transferred-exclusive\n",
                            slot,
                            kind,
                            session_id
                        );
                        LumenInferEngine::Worker
                    }
                    Err(err) => {
                        log(
                            format!(
                                "bench lumen: skipped; inference worker spawn failed err={:?}",
                                err
                            )
                            .as_str(),
                        );
                        return;
                    }
                }
            }
            None => {
                crate::log!("lumen: no AP2+ inference worker; using local fallback\n");
                let chat_state = LumenChatState::new(&model);
                LumenInferEngine::Local { model, chat_state }
            }
        };
        let selftest_start = embassy_time_driver::now();
        let compute_before = crate::lumen::burn_baby::stats();
        crate::log!(
            "lumen: selftest prompt begin session={} prompt={:?} proof=end-to-end-after-tensor-load\n",
            session_id,
            LUMEN_RUNTIME_SELFTEST_PROMPT
        );
        let selftest_result = match &mut infer_engine {
            LumenInferEngine::Local { model, chat_state } => generate_lumen_answer(
                model,
                chat_state,
                &tokenizer_json,
                vocab_entries.as_slice(),
                stop_ids.as_slice(),
                LUMEN_RUNTIME_SELFTEST_PROMPT,
                None,
            ),
            LumenInferEngine::Worker => {
                match submit_lumen_inference(session_id, LUMEN_RUNTIME_SELFTEST_PROMPT, None) {
                    Ok(request_id) => wait_lumen_inference_result(session_id, request_id).await,
                    Err(err) => Err(err),
                }
            }
        };
        match selftest_result {
            Ok(report) => {
                let infer_ms = elapsed_ms_since(selftest_start);
                let compute_after = crate::lumen::burn_baby::stats();
                let submitted_jobs = compute_after
                    .submitted_jobs
                    .saturating_sub(compute_before.submitted_jobs);
                let completed_jobs = compute_after
                    .completed_jobs
                    .saturating_sub(compute_before.completed_jobs);
                let polled_jobs = compute_after
                    .polled_jobs
                    .saturating_sub(compute_before.polled_jobs);
                crate::log!(
                    "lumen: selftest answer prompt={:?} answer={:?} prompt_tokens={} generated_tokens={} first_token={}ms infer={}ms speed={} jobs={}/{} polled={} queued={} proof=end-to-end-ok\n",
                    LUMEN_RUNTIME_SELFTEST_PROMPT,
                    report.answer.as_str(),
                    report.prompt_tokens,
                    report.generated_tokens,
                    report.first_token_ms,
                    infer_ms,
                    format_tokens_per_second(report.generated_tokens, infer_ms),
                    completed_jobs,
                    submitted_jobs,
                    polled_jobs,
                    compute_after.queued_jobs
                );
            }
            Err(err) => {
                crate::log!(
                    "lumen: selftest failed prompt={:?} err={} proof=end-to-end-failed\n",
                    LUMEN_RUNTIME_SELFTEST_PROMPT,
                    err
                );
            }
        }
        register_lumen_interactive_session(session_id);
        crate::lumen::lumen_service::mark_online(session_id);
        print_matrix_target_line(
            &task_target,
            "lumen: selftest complete; prompt loop ready",
        );
        let mut idle_waits = 0u64;
        let mut last_wait_log_tick = embassy_time_driver::now();

        while !bench_cancel_requested(session_id) {
            let Some(request) = pop_lumen_prompt(session_id) else {
                idle_waits = idle_waits.saturating_add(1);
                let now = embassy_time_driver::now();
                if now.saturating_sub(last_wait_log_tick)
                    >= embassy_time_driver::TICK_HZ.saturating_mul(5)
                {
                    crate::log!(
                        "lumen: prompt wait idle session={} waits={} mode=signal-or-timeout\n",
                        session_id,
                        idle_waits
                    );
                    last_wait_log_tick = now;
                }
                let wake = embassy_time::with_timeout(
                    EmbassyDuration::from_millis(1_000),
                    LUMEN_PROMPT_SIGNAL.wait(),
                )
                .await;
                if let Ok(wake_session_id) = wake {
                    crate::log!(
                        "lumen: prompt wake session={} signal_session={}\n",
                        session_id,
                        wake_session_id
                    );
                }
                continue;
            };
            idle_waits = 0;
            let _ = crate::lumen::lumen_service::mark_prompt_running("dequeue");
            let offload_enabled = crate::r::net::esp::prepare_lumen_offload_for_prompt();
            crate::log!(
                "lumen: prompt offload decision session={} enabled={}\n",
                session_id,
                if offload_enabled { 1 } else { 0 }
            );

            let infer_start = embassy_time_driver::now();
            let compute_before = crate::lumen::burn_baby::stats();
            let generate_result = match &mut infer_engine {
                LumenInferEngine::Local { model, chat_state } => generate_lumen_answer(
                    model,
                    chat_state,
                    &tokenizer_json,
                    vocab_entries.as_slice(),
                    stop_ids.as_slice(),
                    request.prompt.as_str(),
                    request.statement.as_deref(),
                ),
                LumenInferEngine::Worker => {
                    match submit_lumen_inference(
                        session_id,
                        request.prompt.as_str(),
                        request.statement.as_deref(),
                    ) {
                        Ok(request_id) => wait_lumen_inference_result(session_id, request_id).await,
                        Err(err) => Err(err),
                    }
                }
            };
            match generate_result {
                Ok(report) => {
                    let infer_ms = elapsed_ms_since(infer_start);
                    let compute_after = crate::lumen::burn_baby::stats();
                    let submitted_jobs = compute_after
                        .submitted_jobs
                        .saturating_sub(compute_before.submitted_jobs);
                    let completed_jobs = compute_after
                        .completed_jobs
                        .saturating_sub(compute_before.completed_jobs);
                    let polled_jobs = compute_after
                        .polled_jobs
                        .saturating_sub(compute_before.polled_jobs);
                    crate::log!(
                        "lumen: answer stats prompt={:?} prompt_tokens={} generated_tokens={} first_token={}ms infer={}ms speed={} jobs={}/{} polled={} queued={}\n",
                        request.prompt.as_str(),
                        report.prompt_tokens,
                        report.generated_tokens,
                        report.first_token_ms,
                        infer_ms,
                        format_tokens_per_second(report.generated_tokens, infer_ms),
                        completed_jobs,
                        submitted_jobs,
                        polled_jobs,
                        compute_after.queued_jobs
                    );
                    if !report.streamed && request.statement.is_some() {
                        crate::lumen::lumen_service::submit_chat_statement_delta(
                            request.statement.as_deref().unwrap_or(""),
                            "<empty>",
                        );
                    } else if !report.streamed {
                        crate::lumen::lumen_service::submit_chat_answer(report.answer.as_str());
                    }
                    crate::lumen::lumen_service::mark_prompt_complete("answer-ready");
                }
                Err(err) => {
                    let message = format!("prompt failed: {}", err);
                    if let Some(statement) = request.statement.as_deref() {
                        crate::lumen::lumen_service::submit_chat_statement_delta(
                            statement,
                            message.as_str(),
                        );
                    } else {
                        crate::lumen::lumen_service::submit_chat_answer(message.as_str());
                    }
                    crate::lumen::lumen_service::mark_prompt_complete("answer-error");
                }
            }
        }
        crate::lumen::lumen_service::mark_prompt_complete("session-end");
        crate::lumen::lumen_service::mark_offline(session_id);
        unregister_lumen_interactive_session(session_id);
        cleanup_lumen_inference_mailbox(session_id, "session-end");

        Timer::after(EmbassyDuration::from_millis(1)).await;
    }
    .await;
    bench_session_finish(session_id);
    set_matrix_target_active(&target, false);
}
