//! VM-scoped Lumen inference capability for replicatable Blueprints.
//!
//! The Blueprint owns chat/tool policy and stores the portable mutable session
//! image in its private memory. The kernel owns immutable model assets and the
//! CPU+IGC+GuC execution lane. No GPU handle, model pointer or host allocation
//! crosses this boundary.

extern crate alloc;

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use spin::Mutex;

use crate::lumen::decode::{Lfm25DecodeInput, checkpoint_intel_igc, restore_intel_igc};

const MAX_SYSTEM_BYTES: usize = 8 * 1024;
const MAX_PROMPT_BYTES: usize = 4 * 1024;
const MAX_SPIRIT_RESPONSE_BYTES: usize = 4 * 1024;
const MAX_REPLY_TOKENS: usize = 48;
// A mature 16K-token session is roughly 192 MiB (KV dominates), while the
// prefilled tool/personality template is only a small fraction of that. Keep
// the transport ceiling large enough for the model's full context instead of
// making the prototype silently stop being migratable after ~2.6K tokens.
const MAX_SESSION_IMAGE_BYTES: usize = 256 * 1024 * 1024;
const WORKER_POLL_MS: u64 = 5;
const TASK_POOL_SIZE: usize = 4;

const ERROR_BAD_OWNER: i32 = -1;
const ERROR_BAD_STATE: i32 = -2;
const ERROR_BAD_INPUT: i32 = -3;
const ERROR_BUSY: i32 = -4;
const ERROR_UNAVAILABLE: i32 = -5;
const ERROR_INFERENCE: i32 = -6;
const ERROR_TRANSPORT: i32 = -7;

type LfmModule =
    crate::lumen::decode::Lfm25Decode<crate::r::lfm25_hybrid_cpu_backend::IntelIgcAotDecodeBackend>;

enum LumenRequest {
    TemplateOpen(String),
    Prompt {
        turn: u64,
        tail: [u32; 2],
        tail_len: usize,
        prompt: String,
    },
    Checkpoint,
    Restore(Vec<u8>),
    Close,
}

struct LumenSlot {
    phase: u32,
    error: i32,
    position: u32,
    request: Option<LumenRequest>,
    worker_active: bool,
    reply: Vec<u8>,
    reply_tail: [u32; 2],
    reply_tail_len: usize,
    checkpoint: Vec<u8>,
    restore: Vec<u8>,
    restore_written: usize,
}

impl LumenSlot {
    const fn new() -> Self {
        Self {
            phase: v::bp_abi::LUMEN_PHASE_IDLE,
            error: 0,
            position: 0,
            request: None,
            worker_active: false,
            reply: Vec::new(),
            reply_tail: [0; 2],
            reply_tail_len: 0,
            checkpoint: Vec::new(),
            restore: Vec::new(),
            restore_written: 0,
        }
    }

    fn status(&self) -> v::bp_abi::TrueosLumenStatus {
        v::bp_abi::TrueosLumenStatus {
            phase: self.phase,
            error: self.error,
            position: self.position,
            reply_len: self.reply.len().min(u32::MAX as usize) as u32,
            checkpoint_len: self.checkpoint.len() as u64,
            reply_tail: self.reply_tail,
            reply_tail_len: self.reply_tail_len as u32,
            reserved: 0,
        }
    }

    fn fail(&mut self, error: i32) {
        self.phase = v::bp_abi::LUMEN_PHASE_ERROR;
        self.error = error;
        self.worker_active = false;
        self.request = None;
    }

    fn reset(&mut self) {
        self.phase = v::bp_abi::LUMEN_PHASE_IDLE;
        self.error = 0;
        self.position = 0;
        self.request = None;
        self.worker_active = false;
        self.reply.clear();
        self.reply_tail = [0; 2];
        self.reply_tail_len = 0;
        self.checkpoint.clear();
        self.restore.clear();
        self.restore_written = 0;
    }
}

static LUMEN_SLOTS: [Mutex<LumenSlot>; crate::allcaps::hv::VM_ID_LIMIT] =
    [const { Mutex::new(LumenSlot::new()) }; crate::allcaps::hv::VM_ID_LIMIT];

fn slot(owner: u8) -> Option<&'static Mutex<LumenSlot>> {
    LUMEN_SLOTS.get(owner as usize)
}

fn spawn_worker(owner: u8) -> bool {
    let Some((_worker_slot, _core_kind, spawner)) =
        crate::workers::pick_perf_background_spawner_with_slot()
    else {
        return false;
    };
    match lumen_blueprint_worker(owner) {
        Ok(task) => {
            spawner.spawn(task);
            true
        }
        Err(_) => false,
    }
}

pub(crate) fn template_open(owner: u8, system: &[u8]) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    if system.is_empty() || system.len() > MAX_SYSTEM_BYTES {
        return ERROR_BAD_INPUT;
    }
    let Ok(system) = core::str::from_utf8(system) else {
        return ERROR_BAD_INPUT;
    };
    let mut state = slot.lock();
    if state.worker_active
        || !matches!(state.phase, v::bp_abi::LUMEN_PHASE_IDLE | v::bp_abi::LUMEN_PHASE_ERROR)
    {
        return ERROR_BUSY;
    }
    state.reset();
    state.phase = v::bp_abi::LUMEN_PHASE_OPENING;
    state.worker_active = true;
    state.request = Some(LumenRequest::TemplateOpen(system.to_string()));
    drop(state);
    if spawn_worker(owner) {
        0
    } else {
        slot.lock().fail(ERROR_UNAVAILABLE);
        ERROR_UNAVAILABLE
    }
}

pub(crate) fn prompt_submit(owner: u8, turn: u64, tail: &[u32], prompt: &[u8]) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    if prompt.is_empty() || prompt.len() > MAX_PROMPT_BYTES || tail.len() > 2 {
        return ERROR_BAD_INPUT;
    }
    let Ok(prompt) = core::str::from_utf8(prompt) else {
        return ERROR_BAD_INPUT;
    };
    if (turn == 0 && !tail.is_empty()) || (turn != 0 && tail.is_empty()) {
        return ERROR_BAD_INPUT;
    }
    let mut state = slot.lock();
    if !state.worker_active || state.phase != v::bp_abi::LUMEN_PHASE_READY {
        return ERROR_BUSY;
    }
    let mut stored_tail = [0u32; 2];
    stored_tail[..tail.len()].copy_from_slice(tail);
    state.reply.clear();
    state.reply_tail = [0; 2];
    state.reply_tail_len = 0;
    state.phase = v::bp_abi::LUMEN_PHASE_RUNNING;
    state.request = Some(LumenRequest::Prompt {
        turn,
        tail: stored_tail,
        tail_len: tail.len(),
        prompt: prompt.to_string(),
    });
    0
}

pub(crate) fn status(owner: u8) -> Option<v::bp_abi::TrueosLumenStatus> {
    Some(slot(owner)?.lock().status())
}

pub(crate) fn reply_read(owner: u8, out: &mut [u8]) -> isize {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER as isize;
    };
    let mut state = slot.lock();
    if state.phase != v::bp_abi::LUMEN_PHASE_REPLY_READY || out.len() < state.reply.len() {
        return ERROR_BAD_STATE as isize;
    }
    let len = state.reply.len();
    out[..len].copy_from_slice(&state.reply);
    state.reply.clear();
    state.phase = v::bp_abi::LUMEN_PHASE_READY;
    len as isize
}

pub(crate) fn checkpoint_request(owner: u8) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    let mut state = slot.lock();
    if !state.worker_active || state.phase != v::bp_abi::LUMEN_PHASE_READY {
        return ERROR_BUSY;
    }
    state.checkpoint.clear();
    state.phase = v::bp_abi::LUMEN_PHASE_CHECKPOINTING;
    state.request = Some(LumenRequest::Checkpoint);
    0
}

pub(crate) fn checkpoint_read(owner: u8, offset: usize, out: &mut [u8]) -> isize {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER as isize;
    };
    let state = slot.lock();
    if state.phase != v::bp_abi::LUMEN_PHASE_CHECKPOINT_READY {
        return ERROR_BAD_STATE as isize;
    }
    let Some(bytes) = state.checkpoint.get(offset..) else {
        return ERROR_BAD_INPUT as isize;
    };
    let len = bytes.len().min(out.len());
    out[..len].copy_from_slice(&bytes[..len]);
    len as isize
}

pub(crate) fn restore_begin(owner: u8, total_len: usize) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    if total_len == 0 || total_len > MAX_SESSION_IMAGE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let mut state = slot.lock();
    if state.worker_active
        || !matches!(
            state.phase,
            v::bp_abi::LUMEN_PHASE_IDLE
                | v::bp_abi::LUMEN_PHASE_CHECKPOINT_READY
                | v::bp_abi::LUMEN_PHASE_ERROR
        )
    {
        return ERROR_BUSY;
    }
    state.reset();
    if state.restore.try_reserve_exact(total_len).is_err() {
        state.fail(ERROR_UNAVAILABLE);
        return ERROR_UNAVAILABLE;
    }
    state.restore.resize(total_len, 0);
    state.restore_written = 0;
    state.phase = v::bp_abi::LUMEN_PHASE_RESTORE_UPLOAD;
    0
}

pub(crate) fn restore_write(owner: u8, offset: usize, data: &[u8]) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    let mut state = slot.lock();
    if state.phase != v::bp_abi::LUMEN_PHASE_RESTORE_UPLOAD
        || offset != state.restore_written
        || offset
            .checked_add(data.len())
            .is_none_or(|end| end > state.restore.len())
    {
        return ERROR_BAD_INPUT;
    }
    let end = offset + data.len();
    state.restore[offset..end].copy_from_slice(data);
    state.restore_written = end;
    0
}

pub(crate) fn restore_commit(owner: u8) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    let mut state = slot.lock();
    if state.phase != v::bp_abi::LUMEN_PHASE_RESTORE_UPLOAD
        || state.restore_written != state.restore.len()
    {
        return ERROR_BAD_STATE;
    }
    let image = core::mem::take(&mut state.restore);
    state.restore_written = 0;
    state.phase = v::bp_abi::LUMEN_PHASE_RESTORING;
    state.worker_active = true;
    state.request = Some(LumenRequest::Restore(image));
    drop(state);
    if spawn_worker(owner) {
        0
    } else {
        slot.lock().fail(ERROR_UNAVAILABLE);
        ERROR_UNAVAILABLE
    }
}

pub(crate) fn close(owner: u8) -> i32 {
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    let mut state = slot.lock();
    if state.worker_active {
        if matches!(
            state.phase,
            v::bp_abi::LUMEN_PHASE_RUNNING
                | v::bp_abi::LUMEN_PHASE_OPENING
                | v::bp_abi::LUMEN_PHASE_RESTORING
                | v::bp_abi::LUMEN_PHASE_CHECKPOINTING
        ) {
            return ERROR_BUSY;
        }
        state.request = Some(LumenRequest::Close);
        return 0;
    }
    state.reset();
    0
}

pub(crate) fn spirit_response_present(owner: u8, turn: u64, text: &[u8]) -> i32 {
    if turn == 0 || text.is_empty() || text.len() > MAX_SPIRIT_RESPONSE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let Some(slot) = slot(owner) else {
        return ERROR_BAD_OWNER;
    };
    let state = slot.lock();
    if !state.worker_active || state.phase != v::bp_abi::LUMEN_PHASE_READY {
        return ERROR_BAD_STATE;
    }
    drop(state);
    let Ok(text) = core::str::from_utf8(text) else {
        return ERROR_BAD_INPUT;
    };
    if text.trim().is_empty() {
        return ERROR_BAD_INPUT;
    }
    if crate::spirit::enqueue_reasoning_response(turn, text) {
        0
    } else {
        ERROR_UNAVAILABLE
    }
}

#[embassy_executor::task(pool_size = TASK_POOL_SIZE)]
async fn lumen_blueprint_worker(owner: u8) {
    let initial = slot(owner).and_then(|slot| slot.lock().request.take());
    let Some(initial) = initial else {
        if let Some(slot) = slot(owner) {
            slot.lock().fail(ERROR_BAD_STATE);
        }
        return;
    };

    let tokenizer = match crate::r::lfm25_tokenizer::load().await {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            crate::log_warn!(
                target: "r";
                "lumen-bp: tokenizer open failed owner={} error={error:?}\n",
                owner,
            );
            slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
            return;
        }
    };

    let module = match initial {
        LumenRequest::TemplateOpen(system) => {
            let module = match crate::lumen::decode::open_intel_igc().await {
                Ok(module) => module,
                Err(error) => {
                    crate::log_warn!(
                        target: "r";
                        "lumen-bp: model open failed owner={} error={error:?}\n",
                        owner,
                    );
                    slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
                    return;
                }
            };
            let tokens = match tokenizer.encode_system_prefix(&system) {
                Ok(tokens) => tokens,
                Err(error) => {
                    crate::log_warn!(
                        target: "r";
                        "lumen-bp: system tokenize failed owner={} error={error:?}\n",
                        owner,
                    );
                    slot(owner).unwrap().lock().fail(ERROR_BAD_INPUT);
                    return;
                }
            };
            for token in tokens {
                if let Err(error) = module.prefill_token(Lfm25DecodeInput::new(token)).await {
                    crate::log_warn!(
                        target: "r";
                        "lumen-bp: template prefill failed owner={} error={error:?}\n",
                        owner,
                    );
                    slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
                    return;
                }
            }
            module
        }
        LumenRequest::Restore(image) => match restore_intel_igc(&image).await {
            Ok(module) => module,
            Err(error) => {
                crate::log_warn!(
                    target: "r";
                    "lumen-bp: restore failed owner={} error={error:?}\n",
                    owner,
                );
                slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
                return;
            }
        },
        _ => {
            slot(owner).unwrap().lock().fail(ERROR_BAD_STATE);
            return;
        }
    };

    {
        let mut state = slot(owner).unwrap().lock();
        state.position = module.try_state().map_or(0, |state| state.position);
        state.phase = v::bp_abi::LUMEN_PHASE_READY;
        state.error = 0;
    }
    crate::log_info!(
        target: "r";
        "lumen-bp: capability ready owner={} position={} assets=kernel-shared state=blueprint-checkpointable\n",
        owner,
        module.try_state().map_or(0, |state| state.position),
    );

    loop {
        let request = slot(owner).and_then(|slot| slot.lock().request.take());
        match request {
            Some(LumenRequest::Prompt {
                turn,
                tail,
                tail_len,
                prompt,
            }) => match run_prompt(&tokenizer, &module, turn, &tail[..tail_len], &prompt).await {
                Ok(reply) => {
                    let mut state = slot(owner).unwrap().lock();
                    state.reply = reply.text;
                    state.reply_tail = reply.tail;
                    state.reply_tail_len = reply.tail_len;
                    state.position = module.try_state().map_or(0, |state| state.position);
                    state.phase = v::bp_abi::LUMEN_PHASE_REPLY_READY;
                }
                Err(()) => {
                    slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
                    return;
                }
            },
            Some(LumenRequest::Checkpoint) => {
                let position = module.try_state().map_or(0, |state| state.position);
                match checkpoint_intel_igc(module) {
                    Ok(image) => {
                        let bytes = image.len();
                        let mut state = slot(owner).unwrap().lock();
                        state.position = position;
                        state.checkpoint = image;
                        state.worker_active = false;
                        state.phase = v::bp_abi::LUMEN_PHASE_CHECKPOINT_READY;
                        crate::log_info!(
                            target: "r";
                            "lumen-bp: capability checkpointed owner={} position={} bytes={} action=release-gpu-session\n",
                            owner,
                            position,
                            bytes,
                        );
                    }
                    Err(error) => {
                        crate::log_warn!(
                            target: "r";
                            "lumen-bp: checkpoint failed owner={} error={error:?}\n",
                            owner,
                        );
                        slot(owner).unwrap().lock().fail(ERROR_INFERENCE);
                    }
                }
                return;
            }
            Some(LumenRequest::Close) => {
                slot(owner).unwrap().lock().reset();
                return;
            }
            Some(LumenRequest::TemplateOpen(_) | LumenRequest::Restore(_)) => {
                slot(owner).unwrap().lock().fail(ERROR_BAD_STATE);
                return;
            }
            None => Timer::after(Duration::from_millis(WORKER_POLL_MS)).await,
        }
    }
}

struct PromptReply {
    text: Vec<u8>,
    tail: [u32; 2],
    tail_len: usize,
}

async fn run_prompt(
    tokenizer: &trueos_lfm25_cpu::Lfm25Tokenizer,
    module: &LfmModule,
    turn: u64,
    tail: &[u32],
    prompt: &str,
) -> Result<PromptReply, ()> {
    let reasoning = crate::r::ai_activity::begin_reasoning(
        crate::r::ai_activity::AiActivitySource::Lumen,
        turn.saturating_add(1),
    );
    let _lumen_gt_boost = crate::intel::begin_lumen_gt_boost();
    let mut prompt_tokens = if turn == 0 {
        tokenizer
            .encode_user_after_system_prefix(prompt)
            .map_err(|_| ())?
    } else {
        let suffix = tokenizer
            .encode_followup_user_turn(prompt)
            .map_err(|_| ())?;
        let mut tokens = Vec::new();
        tokens
            .try_reserve_exact(tail.len() + suffix.len())
            .map_err(|_| ())?;
        tokens.extend_from_slice(tail);
        tokens.extend(suffix);
        tokens
    };
    if prompt_tokens.is_empty() {
        return Err(());
    }
    let mut next_token = None;
    for (index, token) in prompt_tokens.iter().copied().enumerate() {
        if index + 1 == prompt_tokens.len() {
            next_token = Some(
                module
                    .decode_token(Lfm25DecodeInput::new(token))
                    .await
                    .map_err(|_| ())?
                    .token,
            );
        } else {
            module
                .prefill_token(Lfm25DecodeInput::new(token))
                .await
                .map_err(|_| ())?;
        }
    }
    prompt_tokens.clear();

    let mut generated = Vec::new();
    let mut stopped = false;
    for index in 0..MAX_REPLY_TOKENS {
        let token = next_token.ok_or(())?;
        if tokenizer.is_stop(token) {
            stopped = true;
            break;
        }
        generated.push(token);
        if index + 1 == MAX_REPLY_TOKENS {
            break;
        }
        next_token = Some(
            module
                .decode_token(Lfm25DecodeInput::new(token))
                .await
                .map_err(|_| ())?
                .token,
        );
    }
    let text = tokenizer.decode(&generated, false).map_err(|_| ())?;
    let mut result_tail = [0u32; 2];
    let tail_len = if stopped {
        result_tail[0] = next_token.ok_or(())?;
        1
    } else {
        result_tail[0] = *generated.last().ok_or(())?;
        result_tail[1] = tokenizer.im_end_id();
        2
    };
    let reply = PromptReply {
        text,
        tail: result_tail,
        tail_len,
    };
    reasoning.finish();
    Ok(reply)
}

fn current_direct_owner() -> Option<u8> {
    crate::hv::current_guest_execution_context_vm_id()
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_template_open(
    system_ptr: *const u8,
    system_len: usize,
) -> i32 {
    if system_ptr.is_null() || system_len == 0 || system_len > MAX_SYSTEM_BYTES {
        return ERROR_BAD_INPUT;
    }
    let system = unsafe { core::slice::from_raw_parts(system_ptr, system_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_TEMPLATE_OPEN,
            0,
            0,
            system,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(|owner| template_open(owner, system))
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_prompt_submit(
    turn: u64,
    tail_ptr: *const u32,
    tail_len: usize,
    prompt_ptr: *const u8,
    prompt_len: usize,
) -> i32 {
    if prompt_ptr.is_null()
        || prompt_len == 0
        || prompt_len > MAX_PROMPT_BYTES
        || tail_len > 2
        || (tail_len != 0 && tail_ptr.is_null())
    {
        return ERROR_BAD_INPUT;
    }
    let tail = if tail_len == 0 {
        &[]
    } else {
        unsafe { core::slice::from_raw_parts(tail_ptr, tail_len) }
    };
    let prompt = unsafe { core::slice::from_raw_parts(prompt_ptr, prompt_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut payload = Vec::new();
        if payload
            .try_reserve_exact(4 + tail_len * 4 + prompt_len)
            .is_err()
        {
            return ERROR_UNAVAILABLE;
        }
        payload.extend_from_slice(&(tail_len as u32).to_le_bytes());
        for token in tail {
            payload.extend_from_slice(&token.to_le_bytes());
        }
        payload.extend_from_slice(prompt);
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_PROMPT_SUBMIT,
            turn,
            0,
            &payload,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(|owner| prompt_submit(owner, turn, tail, prompt))
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_status(out: *mut v::bp_abi::TrueosLumenStatus) -> i32 {
    if out.is_null() {
        return ERROR_BAD_INPUT;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let bytes = unsafe {
            core::slice::from_raw_parts_mut(
                out.cast::<u8>(),
                core::mem::size_of::<v::bp_abi::TrueosLumenStatus>(),
            )
        };
        let (status, _) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_STATUS,
            0,
            0,
            &[],
            bytes,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            0
        } else {
            ERROR_TRANSPORT
        };
    }
    let Some(status) = current_direct_owner().and_then(self::status) else {
        return ERROR_BAD_OWNER;
    };
    unsafe { out.write(status) };
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_reply_read(out_ptr: *mut u8, out_cap: usize) -> isize {
    if out_ptr.is_null() {
        return ERROR_BAD_INPUT as isize;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_REPLY_READ,
            out_cap as u64,
            0,
            &[],
            out,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as isize
        } else {
            ERROR_TRANSPORT as isize
        };
    }
    current_direct_owner()
        .map(|owner| reply_read(owner, out))
        .unwrap_or(ERROR_BAD_OWNER as isize)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_checkpoint_request() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_LUMEN_CHECKPOINT_REQUEST, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(checkpoint_request)
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_checkpoint_read(
    offset: usize,
    out_ptr: *mut u8,
    out_cap: usize,
) -> isize {
    if out_ptr.is_null() {
        return ERROR_BAD_INPUT as isize;
    }
    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr, out_cap) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_CHECKPOINT_READ,
            offset as u64,
            out_cap as u64,
            &[],
            out,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as isize
        } else {
            ERROR_TRANSPORT as isize
        };
    }
    current_direct_owner()
        .map(|owner| checkpoint_read(owner, offset, out))
        .unwrap_or(ERROR_BAD_OWNER as isize)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_restore_begin(total_len: usize) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_LUMEN_RESTORE_BEGIN,
            total_len as u64,
            0,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(|owner| restore_begin(owner, total_len))
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_lumen_restore_write(
    offset: usize,
    data_ptr: *const u8,
    data_len: usize,
) -> i32 {
    if data_ptr.is_null() && data_len != 0 {
        return ERROR_BAD_INPUT;
    }
    let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, result) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_LUMEN_RESTORE_WRITE,
            offset as u64,
            0,
            data,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            result as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(|owner| restore_write(owner, offset, data))
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_restore_commit() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) =
            trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_LUMEN_RESTORE_COMMIT, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(restore_commit)
        .unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_lumen_close() -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(trueos_vm::vmcall::OP_BP_LUMEN_CLOSE, 0, 0);
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner().map(close).unwrap_or(ERROR_BAD_OWNER)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_spirit_emotion_play(
    idea_ptr: *const u8,
    idea_len: usize,
) -> i32 {
    if idea_ptr.is_null() || idea_len == 0 || idea_len > 16 {
        return ERROR_BAD_INPUT;
    }
    let idea = unsafe { core::slice::from_raw_parts(idea_ptr, idea_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SPIRIT_EMOTION_PLAY,
            0,
            0,
            idea,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    let Ok(idea) = core::str::from_utf8(idea) else {
        return ERROR_BAD_INPUT;
    };
    crate::spirit::enqueue_emotion_words(&[idea])
        .map(|_| 0)
        .unwrap_or(ERROR_UNAVAILABLE)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_spirit_response_present(
    turn: u64,
    text_ptr: *const u8,
    text_len: usize,
) -> i32 {
    if text_ptr.is_null() || text_len == 0 || text_len > MAX_SPIRIT_RESPONSE_BYTES {
        return ERROR_BAD_INPUT;
    }
    let text = unsafe { core::slice::from_raw_parts(text_ptr, text_len) };
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_SPIRIT_RESPONSE_PRESENT,
            turn,
            0,
            text,
            &mut [],
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    current_direct_owner()
        .map(|owner| spirit_response_present(owner, turn, text))
        .unwrap_or(ERROR_BAD_OWNER)
}

pub(crate) fn spirit_move(x_normalized: f32, y_normalized: f32) -> i32 {
    if !x_normalized.is_finite()
        || !y_normalized.is_finite()
        || !(0.0..=1.0).contains(&x_normalized)
        || !(0.0..=1.0).contains(&y_normalized)
    {
        return ERROR_BAD_INPUT;
    }
    crate::spirit::move_spirit_to(x_normalized as f64, y_normalized as f64)
        .map(|_| 0)
        .unwrap_or(ERROR_UNAVAILABLE)
}

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_spirit_move(x_normalized: f32, y_normalized: f32) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let (status, data) = trueos_vm::vmcall::call(
            trueos_vm::vmcall::OP_BP_SPIRIT_MOVE,
            x_normalized.to_bits() as u64,
            y_normalized.to_bits() as u64,
        );
        return if status == trueos_vm::vmcall::STATUS_OK {
            data as i64 as i32
        } else {
            ERROR_TRANSPORT
        };
    }
    spirit_move(x_normalized, y_normalized)
}
