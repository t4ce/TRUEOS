extern crate alloc;

use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::Spawner;
use embassy_sync::signal::Signal;
use spin::{Mutex, Once};

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, claim_matrix_target_for_app_slot_selected,
    matrix_target_for_backend, matrix_target_interrupted, matrix_target_lifetime_is_live,
    matrix_target_slot_name, matrix_targets_same_live_slot, matrix_targets_same_slot_lifetime,
    output_target_for_backend, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};

const MAX_PROMPT_BYTES: usize = 512;
const MAX_REPLY_TOKENS: usize = 48;
const MAX_QUEUED_PROMPTS: usize = 4;

type LfmModule =
    crate::lumen::decode::Lfm25Decode<crate::r::lfm25_hybrid_cpu_backend::IntelIgcAotDecodeBackend>;

struct LumControl {
    target: Option<MatrixTarget>,
    prompts: VecDeque<String>,
    cancel_requested: bool,
    busy: bool,
}

impl LumControl {
    fn new() -> Self {
        Self {
            target: None,
            prompts: VecDeque::new(),
            cancel_requested: false,
            busy: false,
        }
    }
}

struct ConversationState {
    turns: u64,
    pending_reply_tail: Vec<u32>,
}

enum PromptPoll {
    Prompt(String),
    Wait,
    Cancel,
}

static LUM_CONTROL: Once<Mutex<LumControl>> = Once::new();
static LUM_WORK_AVAILABLE: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();

fn lum_control() -> &'static Mutex<LumControl> {
    LUM_CONTROL.call_once(|| Mutex::new(LumControl::new()))
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "lum: usage `lum \"hello how are you\"`");
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let prompt = match parse_quoted_prompt(rest.trim()) {
        Ok(prompt) if !prompt.trim().is_empty() => prompt,
        Ok(_) => {
            print_shell_line(io, "lum: prompt is empty");
            return ParseOutcome::Handled;
        }
        Err(error) => {
            print_shell_line(io, alloc::format!("lum: {error}").as_str());
            usage(io);
            return ParseOutcome::Handled;
        }
    };
    if prompt.len() > MAX_PROMPT_BYTES {
        print_shell_line(io, "lum: prompt exceeds 512 UTF-8 bytes");
        return ParseOutcome::Handled;
    }
    if !crate::intel::gpgpu::lfm25_q8_project_supported() {
        print_shell_line(
            io,
            "lum: Intel LFM kernel unavailable; requires ADL-S 8086:4680 revision 0x0c and ready GuC/RCS",
        );
        return ParseOutcome::Handled;
    }

    let source_target = matrix_target_for_backend(io);
    let existing = lum_control().lock().target.clone();
    if let Some(target) = existing {
        if matrix_target_interrupted(&target) || lum_control().lock().cancel_requested {
            print_shell_line(
                io,
                alloc::format!(
                    "lum: resident session in §{} is resetting",
                    matrix_target_slot_name(&target),
                )
                .as_str(),
            );
            return ParseOutcome::Handled;
        }
        if !matrix_targets_same_live_slot(&source_target, &target) {
            print_shell_line(
                io,
                alloc::format!(
                    "lum: resident session is bound to §{}; switch to that Matrix slot or close it",
                    matrix_target_slot_name(&target),
                )
                .as_str(),
            );
            return ParseOutcome::Handled;
        }
        let (queued, busy) = {
            let mut control = lum_control().lock();
            if control.prompts.len() >= MAX_QUEUED_PROMPTS {
                print_matrix_target_line(&target, "lum: prompt queue is full");
                return ParseOutcome::Handled;
            }
            control.prompts.push_back(prompt);
            (control.prompts.len(), control.busy)
        };
        print_matrix_target_line(
            &target,
            alloc::format!(
                "lum: prompt queued slot=§{} queue_depth={} session={}",
                matrix_target_slot_name(&target),
                queued,
                if busy { "running" } else { "warm" },
            )
            .as_str(),
        );
        LUM_WORK_AVAILABLE.signal(());
        return ParseOutcome::Handled;
    }

    let Some((worker_slot, core_kind, worker_spawner)) =
        crate::workers::pick_perf_background_spawner_with_slot()
    else {
        print_shell_line(io, "lum: no background P-core executor at AP2+ is available");
        return ParseOutcome::Handled;
    };
    let target =
        claim_matrix_target_for_app_slot_selected(output_target_for_backend(io), "lu1", "lum");
    {
        let mut control = lum_control().lock();
        if control.target.is_some() {
            print_shell_line(io, "lum: another resident session won the start race");
            return ParseOutcome::Handled;
        }
        control.target = Some(target.clone());
        control.prompts.push_back(prompt);
        control.cancel_requested = false;
        control.busy = false;
    }
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: queued slot=§{} executor=background-ap{} core_kind={} core=perf lifetime=matrix-slot",
            matrix_target_slot_name(&target),
            worker_slot,
            core_kind,
        )
        .as_str(),
    );
    match lum_task(target.clone(), worker_slot) {
        Ok(task) => worker_spawner.spawn(task),
        Err(_) => {
            release_lum_session(&target);
            print_shell_line(io, "lum: async conversation task unavailable");
        }
    }
    ParseOutcome::Handled
}

fn release_lum_session(target: &MatrixTarget) {
    set_matrix_target_active(target, false);
    let mut control = lum_control().lock();
    if control
        .target
        .as_ref()
        .is_some_and(|owner| matrix_targets_same_slot_lifetime(owner, target))
    {
        control.target = None;
        control.prompts.clear();
        control.cancel_requested = false;
        control.busy = false;
    }
}

fn session_should_stop(target: &MatrixTarget) -> bool {
    if matrix_target_interrupted(target) {
        return true;
    }
    let control = lum_control().lock();
    control.cancel_requested
        || !control
            .target
            .as_ref()
            .is_some_and(|owner| matrix_targets_same_slot_lifetime(owner, target))
}

fn poll_prompt(target: &MatrixTarget) -> PromptPoll {
    let mut control = lum_control().lock();
    if control.cancel_requested
        || !control
            .target
            .as_ref()
            .is_some_and(|owner| matrix_targets_same_slot_lifetime(owner, target))
    {
        return PromptPoll::Cancel;
    }
    match control.prompts.pop_front() {
        Some(prompt) => {
            control.busy = true;
            PromptPoll::Prompt(prompt)
        }
        None => PromptPoll::Wait,
    }
}

fn mark_prompt_complete(target: &MatrixTarget) {
    let mut control = lum_control().lock();
    if control
        .target
        .as_ref()
        .is_some_and(|owner| matrix_targets_same_slot_lifetime(owner, target))
    {
        control.busy = false;
    }
}

fn request_slot_cancel(slot_name: &str) {
    let should_wake = {
        let mut control = lum_control().lock();
        let matches = control
            .target
            .as_ref()
            .is_some_and(|target| matrix_target_slot_name(target) == slot_name);
        if matches {
            control.cancel_requested = true;
            control.prompts.clear();
        }
        matches
    };
    if should_wake {
        LUM_WORK_AVAILABLE.signal(());
    }
}

pub(crate) fn notify_matrix_slot_closed(slot_name: &str) {
    request_slot_cancel(slot_name);
}

pub(crate) fn notify_matrix_slot_interrupted(slot_name: &str) {
    request_slot_cancel(slot_name);
}

#[embassy_executor::task]
async fn lum_task(target: MatrixTarget, expected_worker_slot: u32) {
    let execution_slot = crate::percpu::current_slot() as u32;
    if execution_slot != expected_worker_slot
        || !crate::workers::is_background_worker_slot(execution_slot)
        || crate::workers::core_kind_for_slot(execution_slot) != crate::workers::CORE_KIND_PERF
    {
        print_matrix_target_line(
            &target,
            alloc::format!(
                "lum: refused executor residency expected_background_ap={} actual_cpu_slot={} actual_core_kind={}",
                expected_worker_slot,
                execution_slot,
                crate::workers::core_kind_for_slot(execution_slot),
            )
            .as_str(),
        );
        release_lum_session(&target);
        return;
    }
    set_matrix_target_active(&target, true);
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: loading sealed tokenizer and LFM2.5 CPU+Intel-IGC resident session executor_slot={} core_kind={} core=perf",
            execution_slot,
            crate::workers::core_kind_for_slot(execution_slot),
        )
        .as_str(),
    );
    let tokenizer = match crate::r::lfm25_tokenizer::load().await {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed tokenizer={error:?}").as_str(),
            );
            release_lum_session(&target);
            return;
        }
    };
    if session_should_stop(&target) {
        release_lum_session(&target);
        return;
    }
    let module = match crate::lumen::decode::open_intel_igc().await {
        Ok(module) => module,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed model-open={error:?}").as_str(),
            );
            release_lum_session(&target);
            return;
        }
    };
    set_matrix_target_active(&target, false);
    if session_should_stop(&target) {
        release_lum_session(&target);
        return;
    }
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: resident ready slot=§{} executor_slot={} context_tokens=0 backend=cpu+intel-igc-q8 completion=guc-rcs",
            matrix_target_slot_name(&target),
            execution_slot,
        )
        .as_str(),
    );

    let mut conversation = ConversationState {
        turns: 0,
        pending_reply_tail: Vec::new(),
    };
    let mut cancelled = false;
    'session: loop {
        let prompt = loop {
            if session_should_stop(&target) {
                cancelled = true;
                break 'session;
            }
            match poll_prompt(&target) {
                PromptPoll::Prompt(prompt) => break prompt,
                PromptPoll::Cancel => {
                    cancelled = true;
                    break 'session;
                }
                PromptPoll::Wait => LUM_WORK_AVAILABLE.wait().await,
            }
        };

        set_matrix_target_active(&target, true);
        let keep_session =
            run_lum_turn(&target, &tokenizer, &module, prompt, &mut conversation).await;
        set_matrix_target_active(&target, false);
        mark_prompt_complete(&target);

        if session_should_stop(&target) {
            cancelled = true;
            break;
        }
        if !keep_session {
            break;
        }
        let state = module.try_state();
        print_matrix_target_line(
            &target,
            alloc::format!(
                "lum: resident ready slot=§{} turns={} context_tokens={} pending_reply_tokens={} session=warm",
                matrix_target_slot_name(&target),
                conversation.turns,
                state.map(|state| state.position).unwrap_or(0),
                conversation.pending_reply_tail.len(),
            )
            .as_str(),
        );
    }

    if cancelled && matrix_target_lifetime_is_live(&target) {
        print_matrix_target_line(&target, "lum: interrupted; resident session reset");
    }
    release_lum_session(&target);
}

async fn run_lum_turn(
    target: &MatrixTarget,
    tokenizer: &trueos_lfm25_cpu::Lfm25Tokenizer,
    module: &LfmModule,
    prompt: String,
    conversation: &mut ConversationState,
) -> bool {
    let started = embassy_time_driver::now();
    let mut prompt_tokens = if conversation.turns == 0 {
        let encoded = if crate::spirit::LUMEN_AI_EMOTION_ENABLED {
            tokenizer.encode_system_user_turn(crate::spirit::LUMEN_SYSTEM_PROMPT, &prompt)
        } else {
            tokenizer.encode_user_turn(&prompt)
        };
        match encoded {
            Ok(tokens) => tokens,
            Err(error) => {
                print_matrix_target_line(
                    target,
                    alloc::format!("lum: failed tokenization={error:?}").as_str(),
                );
                return false;
            }
        }
    } else {
        let suffix = match tokenizer.encode_followup_user_turn(&prompt) {
            Ok(tokens) => tokens,
            Err(error) => {
                print_matrix_target_line(
                    target,
                    alloc::format!("lum: failed followup-tokenization={error:?}").as_str(),
                );
                return false;
            }
        };
        let mut tokens = conversation.pending_reply_tail.clone();
        if tokens.try_reserve_exact(suffix.len()).is_err() {
            print_matrix_target_line(target, "lum: failed followup-token allocation");
            return false;
        }
        tokens.extend(suffix);
        tokens
    };
    let Some(module_state) = module.try_state() else {
        print_matrix_target_line(target, "lum: failed decode lane is already in flight");
        return false;
    };
    let context_before = module_state.position as usize;
    if context_before
        .saturating_add(prompt_tokens.len())
        .saturating_add(MAX_REPLY_TOKENS)
        >= trueos_lfm25_model::lfm25::MODEL_INITIAL_CONTEXT as usize
    {
        print_matrix_target_line(
            target,
            alloc::format!(
                "lum: failed context full used={} prompt_tokens={} reserve_reply={}",
                context_before,
                prompt_tokens.len(),
                MAX_REPLY_TOKENS,
            )
            .as_str(),
        );
        return false;
    }
    conversation.pending_reply_tail.clear();
    let turn = conversation.turns.saturating_add(1);
    let reasoning = crate::r::ai_activity::begin_reasoning(
        crate::r::ai_activity::AiActivitySource::Lumen,
        turn,
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "lum: running turn={} prompt_tokens={} context_before={} backend=cpu+intel-igc-q8 completion=guc-rcs",
            turn,
            prompt_tokens.len(),
            context_before,
        )
        .as_str(),
    );

    let before = crate::intel::gpgpu::lfm25_q8_project_stats();
    let mut next_token = None;
    let callback_start = module_state.callback_sequence;
    let mut last_callback = callback_start;
    for (index, &token) in prompt_tokens.iter().enumerate() {
        if session_should_stop(target) {
            return false;
        }
        if index + 1 == prompt_tokens.len() {
            match module
                .decode_token(crate::lumen::decode::Lfm25DecodeInput::new(token))
                .await
            {
                Ok(output) => {
                    next_token = Some(output.token);
                    last_callback = output.callback_sequence;
                }
                Err(error) => {
                    print_matrix_target_line(
                        target,
                        alloc::format!(
                            "lum: failed stage=prefill token={}/{} error={error:?}",
                            index + 1,
                            prompt_tokens.len(),
                        )
                        .as_str(),
                    );
                    return false;
                }
            }
        } else {
            match module
                .prefill_token(crate::lumen::decode::Lfm25DecodeInput::new(token))
                .await
            {
                Ok(output) => last_callback = output.callback_sequence,
                Err(error) => {
                    print_matrix_target_line(
                        target,
                        alloc::format!(
                            "lum: failed stage=prefill token={}/{} error={error:?}",
                            index + 1,
                            prompt_tokens.len(),
                        )
                        .as_str(),
                    );
                    return false;
                }
            }
        }
        if (index + 1) % 4 == 0 || index + 1 == prompt_tokens.len() {
            print_matrix_target_line(
                target,
                alloc::format!("lum: prefill={}/{}", index + 1, prompt_tokens.len()).as_str(),
            );
        }
    }

    let first_token = match next_token {
        Some(token) => token,
        None => {
            print_matrix_target_line(target, "lum: failed stage=prefill no next token");
            return false;
        }
    };
    let first_piece_bytes = match tokenizer.decode(&[first_token], true) {
        Ok(piece) => piece,
        Err(error) => {
            print_matrix_target_line(
                target,
                alloc::format!("lum: failed stage=prefill-detokenize error={error:?}").as_str(),
            );
            return false;
        }
    };
    let first_piece = String::from_utf8_lossy(&first_piece_bytes);
    let prefill_after = crate::intel::gpgpu::lfm25_q8_project_stats();
    let callback_count = last_callback.saturating_sub(callback_start);
    let igpu_projections = prefill_after.launches.saturating_sub(before.launches);
    let igpu_submissions = prefill_after.submissions.saturating_sub(before.submissions);
    let igpu_failures = prefill_after.failures.saturating_sub(before.failures);
    let igpu_submit_ms = prefill_after
        .total_submit_ms
        .saturating_sub(before.total_submit_ms);
    let igpu_encode_us = prefill_after
        .total_encode_us
        .saturating_sub(before.total_encode_us);
    let igpu_admission_us = prefill_after
        .total_admission_us
        .saturating_sub(before.total_admission_us);
    let igpu_completion_us = prefill_after
        .total_completion_us
        .saturating_sub(before.total_completion_us);
    let igpu_gpu_us = prefill_after
        .total_gpu_us
        .saturating_sub(before.total_gpu_us);
    let igpu_gpu_samples = prefill_after
        .gpu_timestamp_samples
        .saturating_sub(before.gpu_timestamp_samples);
    let state_only_tokens = prompt_tokens.len().saturating_sub(1) as u64;
    let full_tokens = u64::from(!prompt_tokens.is_empty());
    let expected_callbacks = state_only_tokens
        * trueos_lfm25_model::lfm25_decode::OPS_PER_PREFILL_TOKEN as u64
        + full_tokens * trueos_lfm25_model::lfm25_decode::OPS_PER_TOKEN as u64;
    let expected_igpu_projections = state_only_tokens
        * crate::intel::gpgpu::LFM25_Q8_PROJECTIONS_PER_PREFILL_TOKEN
        + full_tokens * crate::intel::gpgpu::LFM25_Q8_PROJECTIONS_PER_TOKEN;
    let expected_igpu_submissions = state_only_tokens
        * crate::intel::gpgpu::LFM25_Q8_SUBMISSIONS_PER_PREFILL_TOKEN
        + full_tokens * crate::intel::gpgpu::LFM25_Q8_SUBMISSIONS_PER_TOKEN;
    print_matrix_target_line(
        target,
        alloc::format!(
            "lum: prefill_diag turn={} first_token={} first_piece={:?} callbacks={} igpu_projections={} igpu_submissions={} igpu_failures={} igpu_submit_ms={}",
            conversation.turns + 1,
            first_token,
            first_piece,
            callback_count,
            igpu_projections,
            igpu_submissions,
            igpu_failures,
            igpu_submit_ms,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "lum: prefill_phase_us turn={} encode={} admission={} completion={} gpu={} gpu_samples={} gpu_hz={}",
            conversation.turns + 1,
            igpu_encode_us,
            igpu_admission_us,
            igpu_completion_us,
            igpu_gpu_us,
            igpu_gpu_samples,
            prefill_after.gpu_timestamp_hz,
        )
        .as_str(),
    );
    if callback_count != expected_callbacks
        || igpu_projections != expected_igpu_projections
        || igpu_submissions != expected_igpu_submissions
        || igpu_failures != 0
        || (conversation.turns == 0
            && prompt == "hi"
            && (first_token != 36_309 || !first_piece.starts_with("Hello")))
    {
        print_matrix_target_line(
            target,
            alloc::format!(
                "lum: failed stage=prefill-parity expected_callbacks={} expected_igpu_projections={} expected_igpu_submissions={} hi_expected_token=36309",
                expected_callbacks,
                expected_igpu_projections,
                expected_igpu_submissions,
            )
            .as_str(),
        );
        return false;
    }

    let mut generated = alloc::vec::Vec::new();
    let mut stopped = false;
    for index in 0..MAX_REPLY_TOKENS {
        if session_should_stop(target) {
            return false;
        }
        let Some(token) = next_token else {
            break;
        };
        if tokenizer.is_stop(token) {
            stopped = true;
            break;
        }
        generated.push(token);
        if index + 1 == MAX_REPLY_TOKENS {
            break;
        }
        match module
            .decode_token(crate::lumen::decode::Lfm25DecodeInput::new(token))
            .await
        {
            Ok(output) => {
                next_token = Some(output.token);
                last_callback = output.callback_sequence;
            }
            Err(error) => {
                print_matrix_target_line(
                    target,
                    alloc::format!("lum: failed stage=reply token={} error={error:?}", index + 1,)
                        .as_str(),
                );
                return false;
            }
        }
    }

    let reply_bytes = match tokenizer.decode(&generated, !crate::spirit::LUMEN_AI_EMOTION_ENABLED) {
        Ok(bytes) => bytes,
        Err(error) => {
            print_matrix_target_line(
                target,
                alloc::format!("lum: failed detokenization={error:?}").as_str(),
            );
            return false;
        }
    };
    let raw_reply = String::from_utf8_lossy(&reply_bytes);
    let adapted_reply = if crate::spirit::LUMEN_AI_EMOTION_ENABLED {
        crate::spirit::adapt_lumen_reply(raw_reply.as_ref())
    } else {
        String::from(raw_reply.trim())
    };
    let reply = adapted_reply.as_str();
    let after = crate::intel::gpgpu::lfm25_q8_project_stats();
    reasoning.finish();
    let response_turn = conversation.turns.saturating_add(1);
    crate::spirit::enqueue_reasoning_response(response_turn, reply);
    print_matrix_target_line(target, alloc::format!("lum: {reply}").as_str());
    print_matrix_target_line(
        target,
        alloc::format!(
            "lum: done turn={} prompt_tokens={} reply_tokens={} stop={} callbacks={} igpu_projections={} igpu_submissions={} igpu_failures={} igpu_submit_ms={} elapsed_ms={}",
            conversation.turns + 1,
            prompt_tokens.len(),
            generated.len(),
            if stopped { "eot" } else { "limit" },
            last_callback.saturating_sub(callback_start),
            after.launches.saturating_sub(before.launches),
            after.submissions.saturating_sub(before.submissions),
            after.failures.saturating_sub(before.failures),
            after
                .total_submit_ms
                .saturating_sub(before.total_submit_ms),
            elapsed_ms_since(started),
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "lum: done_phase_us turn={} encode={} admission={} completion={} gpu={} gpu_samples={} gpu_hz={}",
            conversation.turns + 1,
            after
                .total_encode_us
                .saturating_sub(before.total_encode_us),
            after
                .total_admission_us
                .saturating_sub(before.total_admission_us),
            after
                .total_completion_us
                .saturating_sub(before.total_completion_us),
            after.total_gpu_us.saturating_sub(before.total_gpu_us),
            after
                .gpu_timestamp_samples
                .saturating_sub(before.gpu_timestamp_samples),
            after.gpu_timestamp_hz,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        "lum: path=quoted-prompt->sealed-bpe->resident-lumen-session->cpu-state+cpp-q8-project->igc-zebin->batched-guc-rcs",
    );
    conversation.pending_reply_tail.clear();
    if stopped {
        if let Some(token) = next_token {
            conversation.pending_reply_tail.push(token);
        }
    } else {
        if let Some(&token) = generated.last() {
            conversation.pending_reply_tail.push(token);
        }
        conversation.pending_reply_tail.push(tokenizer.im_end_id());
    }
    conversation.turns = conversation.turns.saturating_add(1);
    prompt_tokens.clear();
    true
}

fn parse_quoted_prompt(input: &str) -> Result<String, &'static str> {
    let quoted = input
        .strip_prefix('"')
        .ok_or("prompt must begin with a quote")?;
    let mut prompt = String::new();
    let mut escaped = false;
    for (offset, ch) in quoted.char_indices() {
        if escaped {
            match ch {
                '"' | '\\' => prompt.push(ch),
                'n' => prompt.push('\n'),
                'r' => prompt.push('\r'),
                't' => prompt.push('\t'),
                _ => return Err("unsupported quoted escape"),
            }
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => {
                if !quoted[offset + ch.len_utf8()..].trim().is_empty() {
                    return Err("unexpected text after closing quote");
                }
                return Ok(prompt);
            }
            _ => prompt.push(ch),
        }
    }
    Err("missing closing quote")
}

fn elapsed_ms_since(start_tick: u64) -> u64 {
    embassy_time_driver::now()
        .saturating_sub(start_tick)
        .saturating_mul(1_000)
        / embassy_time_driver::TICK_HZ
}

#[cfg(test)]
mod tests {
    use super::parse_quoted_prompt;

    #[test]
    fn accepts_one_quoted_sentence() {
        assert_eq!(parse_quoted_prompt("\"hello how are you\"").unwrap(), "hello how are you");
    }

    #[test]
    fn rejects_unquoted_or_trailing_arguments() {
        assert!(parse_quoted_prompt("hello").is_err());
        assert!(parse_quoted_prompt("\"hello\" again").is_err());
    }
}
