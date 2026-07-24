extern crate alloc;

use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};

use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, claim_matrix_target_for_app_slot_selected,
    matrix_target_interrupted, matrix_target_slot_name, output_target_for_backend,
    print_matrix_target_line, print_shell_line, set_matrix_target_active,
    set_matrix_target_app_label,
};

const MAX_PROMPT_BYTES: usize = 512;
const MAX_REPLY_TOKENS: usize = 32;
static LUM_SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

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

    let Some((worker_slot, core_kind, worker_spawner)) =
        crate::workers::pick_perf_background_spawner_with_slot()
            .or_else(crate::workers::pick_background_spawner_with_slot)
    else {
        print_shell_line(io, "lum: no background AP executor is available");
        return ParseOutcome::Handled;
    };
    if LUM_SESSION_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        print_shell_line(io, "lum: one LFM session is already running");
        return ParseOutcome::Handled;
    }

    let target =
        claim_matrix_target_for_app_slot_selected(output_target_for_backend(io), "lu1", "lum");
    set_matrix_target_active(&target, true);
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: queued slot=§{} executor=background-ap{} core_kind={}",
            matrix_target_slot_name(&target),
            worker_slot,
            core_kind,
        )
        .as_str(),
    );
    match lum_task(target.clone(), prompt, worker_slot) {
        Ok(task) => worker_spawner.spawn(task),
        Err(_) => {
            finish_lum_session(&target);
            print_shell_line(io, "lum: async conversation task unavailable");
        }
    }
    ParseOutcome::Handled
}

fn finish_lum_session(target: &MatrixTarget) {
    set_matrix_target_active(target, false);
    set_matrix_target_app_label(target, "");
    LUM_SESSION_ACTIVE.store(false, Ordering::Release);
}

fn stop_if_interrupted(target: &MatrixTarget) -> bool {
    if !matrix_target_interrupted(target) {
        return false;
    }
    print_matrix_target_line(target, "lum: interrupted");
    finish_lum_session(target);
    true
}

#[embassy_executor::task]
async fn lum_task(target: MatrixTarget, prompt: String, expected_worker_slot: u32) {
    let execution_slot = crate::percpu::current_slot() as u32;
    if execution_slot != expected_worker_slot
        || !crate::workers::is_background_worker_slot(execution_slot)
    {
        print_matrix_target_line(
            &target,
            alloc::format!(
                "lum: refused executor residency expected_background_ap={} actual_cpu_slot={}",
                expected_worker_slot,
                execution_slot,
            )
            .as_str(),
        );
        finish_lum_session(&target);
        return;
    }
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: loading sealed tokenizer and LFM2.5 CPU+Intel-IGC session executor_slot={}",
            execution_slot,
        )
        .as_str(),
    );
    let started = embassy_time_driver::now();
    let tokenizer = match crate::r::lfm25_tokenizer::load().await {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed tokenizer={error:?}").as_str(),
            );
            finish_lum_session(&target);
            return;
        }
    };
    if stop_if_interrupted(&target) {
        return;
    }
    let prompt_tokens = match tokenizer.encode_user_turn(&prompt) {
        Ok(tokens) => tokens,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed tokenization={error:?}").as_str(),
            );
            finish_lum_session(&target);
            return;
        }
    };
    if prompt_tokens.len().saturating_add(MAX_REPLY_TOKENS)
        >= trueos_fpga_abi::lfm25::MODEL_INITIAL_CONTEXT as usize
    {
        print_matrix_target_line(&target, "lum: failed prompt exceeds model context");
        finish_lum_session(&target);
        return;
    }

    let module = match crate::lumen::decode::open_intel_igc().await {
        Ok(module) => module,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed model-open={error:?}").as_str(),
            );
            finish_lum_session(&target);
            return;
        }
    };
    if stop_if_interrupted(&target) {
        return;
    }
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: running prompt_tokens={} backend=cpu+intel-igc-q8 completion=guc-rcs",
            prompt_tokens.len(),
        )
        .as_str(),
    );

    let before = crate::intel::gpgpu::lfm25_q8_project_stats();
    let mut next_token = None;
    let mut last_callback = 0u64;
    for (index, &token) in prompt_tokens.iter().enumerate() {
        if stop_if_interrupted(&target) {
            return;
        }
        match ::lumen::async_module::forward(
            &module,
            crate::lumen::decode::Lfm25DecodeInput::new(token),
        )
        .await
        {
            Ok(output) => {
                next_token = Some(output.token);
                last_callback = output.callback_sequence;
            }
            Err(error) => {
                print_matrix_target_line(
                    &target,
                    alloc::format!(
                        "lum: failed stage=prefill token={}/{} error={error:?}",
                        index + 1,
                        prompt_tokens.len(),
                    )
                    .as_str(),
                );
                finish_lum_session(&target);
                return;
            }
        }
        if (index + 1) % 4 == 0 || index + 1 == prompt_tokens.len() {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: prefill={}/{}", index + 1, prompt_tokens.len()).as_str(),
            );
        }
    }

    let first_token = match next_token {
        Some(token) => token,
        None => {
            print_matrix_target_line(&target, "lum: failed stage=prefill no next token");
            finish_lum_session(&target);
            return;
        }
    };
    let first_piece_bytes = match tokenizer.decode(&[first_token], true) {
        Ok(piece) => piece,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed stage=prefill-detokenize error={error:?}").as_str(),
            );
            finish_lum_session(&target);
            return;
        }
    };
    let first_piece = String::from_utf8_lossy(&first_piece_bytes);
    let prefill_after = crate::intel::gpgpu::lfm25_q8_project_stats();
    let callback_count = last_callback;
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
    let expected_callbacks =
        prompt_tokens.len() as u64 * trueos_fpga_abi::lfm25_decode::OPS_PER_TOKEN as u64;
    let expected_igpu_projections =
        prompt_tokens.len() as u64 * crate::intel::gpgpu::LFM25_Q8_PROJECTIONS_PER_TOKEN;
    let expected_igpu_submissions =
        prompt_tokens.len() as u64 * crate::intel::gpgpu::LFM25_Q8_SUBMISSIONS_PER_TOKEN;
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: prefill_diag first_token={} first_piece={:?} callbacks={} igpu_projections={} igpu_submissions={} igpu_failures={} igpu_submit_ms={} phase_us=encode:{},admission:{},completion:{},gpu:{} gpu_samples={} gpu_hz={}",
            first_token,
            first_piece,
            callback_count,
            igpu_projections,
            igpu_submissions,
            igpu_failures,
            igpu_submit_ms,
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
        || (prompt == "hi" && (first_token != 36_309 || !first_piece.starts_with("Hello")))
    {
        print_matrix_target_line(
            &target,
            alloc::format!(
                "lum: failed stage=prefill-parity expected_callbacks={} expected_igpu_projections={} expected_igpu_submissions={} hi_expected_token=36309",
                expected_callbacks,
                expected_igpu_projections,
                expected_igpu_submissions,
            )
            .as_str(),
        );
        finish_lum_session(&target);
        return;
    }

    let mut generated = alloc::vec::Vec::new();
    let mut stopped = false;
    for index in 0..MAX_REPLY_TOKENS {
        if stop_if_interrupted(&target) {
            return;
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
        match ::lumen::async_module::forward(
            &module,
            crate::lumen::decode::Lfm25DecodeInput::new(token),
        )
        .await
        {
            Ok(output) => {
                next_token = Some(output.token);
                last_callback = output.callback_sequence;
            }
            Err(error) => {
                print_matrix_target_line(
                    &target,
                    alloc::format!("lum: failed stage=reply token={} error={error:?}", index + 1,)
                        .as_str(),
                );
                finish_lum_session(&target);
                return;
            }
        }
    }

    let reply_bytes = match tokenizer.decode(&generated, true) {
        Ok(bytes) => bytes,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed detokenization={error:?}").as_str(),
            );
            finish_lum_session(&target);
            return;
        }
    };
    let reply = String::from_utf8_lossy(&reply_bytes);
    let after = crate::intel::gpgpu::lfm25_q8_project_stats();
    print_matrix_target_line(&target, alloc::format!("lum: {reply}").as_str());
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: done prompt_tokens={} reply_tokens={} stop={} callbacks={} igpu_projections={} igpu_submissions={} igpu_failures={} igpu_submit_ms={} phase_us=encode:{},admission:{},completion:{},gpu:{} gpu_samples={} gpu_hz={} elapsed_ms={}",
            prompt_tokens.len(),
            generated.len(),
            if stopped { "eot" } else { "limit" },
            last_callback,
            after.launches.saturating_sub(before.launches),
            after.submissions.saturating_sub(before.submissions),
            after.failures.saturating_sub(before.failures),
            after
                .total_submit_ms
                .saturating_sub(before.total_submit_ms),
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
            elapsed_ms_since(started),
        )
        .as_str(),
    );
    print_matrix_target_line(
        &target,
        "lum: path=quoted-prompt->sealed-bpe->lumen-async-session->cpu-state+cpp-q8-project->igc-zebin->batched-guc-rcs",
    );
    finish_lum_session(&target);
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
