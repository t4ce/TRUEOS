extern crate alloc;

use alloc::string::String;

use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

const MAX_PROMPT_BYTES: usize = 512;
const MAX_REPLY_TOKENS: usize = 32;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "lum: usage `lum \"hello how are you\"`");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
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
    if !crate::r::fpga_offload::lfm25_row_stream_available() {
        print_shell_line(
            io,
            "lum: FPGA FFN function unavailable; flash the admitted BAR2/MSI firmware",
        );
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match lum_task(target.clone(), prompt) {
        Ok(task) => spawner.spawn(task),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "lum: async conversation task unavailable");
        }
    }
    ParseOutcome::Handled
}

#[embassy_executor::task]
async fn lum_task(target: MatrixTarget, prompt: String) {
    print_matrix_target_line(&target, "lum: loading sealed tokenizer and LFM2.5 CPU+FPGA session");
    let started = embassy_time_driver::now();
    let tokenizer = match crate::r::lfm25_tokenizer::load().await {
        Ok(tokenizer) => tokenizer,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed tokenizer={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let prompt_tokens = match tokenizer.encode_user_turn(&prompt) {
        Ok(tokens) => tokens,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed tokenization={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
            return;
        }
    };
    if prompt_tokens.len().saturating_add(MAX_REPLY_TOKENS)
        >= trueos_fpga_abi::lfm25::MODEL_INITIAL_CONTEXT as usize
    {
        print_matrix_target_line(&target, "lum: failed prompt exceeds model context");
        set_matrix_target_active(&target, false);
        return;
    }

    let module = match crate::lumen::decode::open_hybrid_cpu().await {
        Ok(module) => module,
        Err(error) => {
            print_matrix_target_line(
                &target,
                alloc::format!("lum: failed model-open={error:?}").as_str(),
            );
            set_matrix_target_active(&target, false);
            return;
        }
    };
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: running prompt_tokens={} backend=cpu+truega-ffn completion=msi",
            prompt_tokens.len(),
        )
        .as_str(),
    );

    let before = crate::r::fpga_offload::stats();
    let mut next_token = None;
    let mut last_callback = 0u64;
    for (index, &token) in prompt_tokens.iter().enumerate() {
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
                set_matrix_target_active(&target, false);
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

    let mut generated = alloc::vec::Vec::new();
    let mut stopped = false;
    for index in 0..MAX_REPLY_TOKENS {
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
                set_matrix_target_active(&target, false);
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
            set_matrix_target_active(&target, false);
            return;
        }
    };
    let reply = String::from_utf8_lossy(&reply_bytes);
    let after = crate::r::fpga_offload::stats();
    print_matrix_target_line(&target, alloc::format!("lum: {reply}").as_str());
    print_matrix_target_line(
        &target,
        alloc::format!(
            "lum: done prompt_tokens={} reply_tokens={} stop={} callbacks={} irq_delta={} timeout_recovery_delta={} elapsed_ms={}",
            prompt_tokens.len(),
            generated.len(),
            if stopped { "eot" } else { "limit" },
            last_callback,
            after.interrupts.saturating_sub(before.interrupts),
            after
                .timeout_recoveries
                .saturating_sub(before.timeout_recoveries),
            elapsed_ms_since(started),
        )
        .as_str(),
    );
    print_matrix_target_line(
        &target,
        "lum: path=quoted-prompt->sealed-bpe->lumen-async-session->cpu-stages+truega-ffn->msi",
    );
    set_matrix_target_active(&target, false);
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
