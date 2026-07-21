use alloc::string::String;
use alloc::sync::Arc;
use core::fmt::Write;
use embassy_executor::Spawner;
use embassy_sync::signal::Signal;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

#[derive(Clone, Copy)]
enum TgaCall {
    Test,
    Add(u32, u32),
    Q8,
    ModelVerify,
    ModelRow0,
    ModelFfn0,
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(
        io,
        "tga: usage `tga [status|test|add <u32> <u32>|q8|model verify|model row0|model ffn0]`",
    );
}

fn parse_u32(value: &str) -> Option<u32> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse().ok(), |hex| u32::from_str_radix(hex, 16).ok())
}

fn print_status(io: &'static dyn ShellBackend2) {
    let stats = crate::r::fpga_offload::stats();
    let hardware = crate::tga::completion_irq_hardware_stats().unwrap_or_default();
    print_shell_line(
        io,
        alloc::format!(
            "tga: online={} protocol={} irq_ready={} generation={} submitted={} completed={} failed={} queued={} active={}",
            crate::tga::is_online(),
            crate::tga::protocol_alive(),
            crate::tga::completion_interrupt_configured(),
            crate::tga::connection_generation(),
            stats.submitted,
            stats.completed,
            stats.failed,
            stats.queued,
            stats.active,
        )
        .as_str(),
    );
    print_shell_line(
        io,
        alloc::format!(
            "tga: irq={} wakes={} timeout_recoveries={} hw_retire={} hw_req={} hw_ack={} hw_state={:#04x}",
            stats.interrupts,
            stats.interrupt_wakes,
            stats.timeout_recoveries,
            hardware.retirements,
            hardware.requests,
            hardware.controller_acks,
            hardware.state,
        )
        .as_str(),
    );
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let call = match args.next() {
        None | Some("status") => {
            if args.next().is_some() {
                usage(io);
            } else {
                print_status(io);
            }
            return ParseOutcome::Handled;
        }
        Some("help" | "-h" | "--help") => {
            usage(io);
            return ParseOutcome::Handled;
        }
        Some("test") if args.next().is_none() => TgaCall::Test,
        Some("q8") if args.next().is_none() => TgaCall::Q8,
        Some("model") => match (args.next(), args.next()) {
            (Some("verify"), None) => TgaCall::ModelVerify,
            (Some("row0"), None) => TgaCall::ModelRow0,
            (Some("ffn0"), None) => TgaCall::ModelFfn0,
            _ => {
                usage(io);
                return ParseOutcome::Handled;
            }
        },
        Some("add") => {
            let Some(a) = args.next().and_then(parse_u32) else {
                usage(io);
                return ParseOutcome::Handled;
            };
            let Some(b) = args.next().and_then(parse_u32) else {
                usage(io);
                return ParseOutcome::Handled;
            };
            if args.next().is_some() {
                usage(io);
                return ParseOutcome::Handled;
            }
            TgaCall::Add(a, b)
        }
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    };

    if !matches!(call, TgaCall::ModelVerify) && !crate::tga::is_online() {
        print_shell_line(io, "tga: unavailable; admitted firmware bundle is offline");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match tga_call_task(target.clone(), call) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "tga: function call task unavailable");
        }
    }
    ParseOutcome::Handled
}

#[embassy_executor::task(pool_size = 2)]
async fn tga_call_task(target: MatrixTarget, call: TgaCall) {
    let result = match call {
        TgaCall::Test => run_test(&target).await,
        TgaCall::Add(a, b) => match crate::r::fpga_offload::add_u32(a, b).await {
            Ok(value) => {
                print_matrix_target_line(
                    &target,
                    alloc::format!("tga: add {a:#010x} {b:#010x} -> {value:#010x}").as_str(),
                );
                Ok(())
            }
            Err(error) => Err(error),
        },
        TgaCall::Q8 => match run_q8_golden().await {
            Ok(result) => {
                let pass = q8_matches_golden(&result);
                print_matrix_target_line(
                    &target,
                    alloc::format!(
                        "tga: q8={} dot={} term_q30={}",
                        if pass { "pass" } else { "fail" },
                        result.dot,
                        result.term_q30,
                    )
                    .as_str(),
                );
                if pass {
                    Ok(())
                } else {
                    Err(crate::r::fpga_offload::Error::Protocol)
                }
            }
            Err(error) => Err(error),
        },
        TgaCall::ModelVerify => {
            run_model_verify(&target).await;
            Ok(())
        }
        TgaCall::ModelRow0 => run_model_gate_row0(&target).await,
        TgaCall::ModelFfn0 => run_model_ffn0(&target).await,
    };

    if let Err(error) = result
        && !matches!(call, TgaCall::ModelFfn0)
    {
        print_matrix_target_line(
            &target,
            alloc::format!("tga: package failed: {error:?}").as_str(),
        );
    }
    set_matrix_target_active(&target, false);
}

async fn run_model_ffn0(target: &MatrixTarget) -> Result<(), crate::r::fpga_offload::Error> {
    use crate::r::lfm25_ffn;

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model ffn0=start path=trueosfs:/{} seal=checking expected_calls={}",
            crate::r::lfm25_model::NATIVE_IMAGE_PATH,
            lfm25_ffn::FPGA_CALLS_PER_FFN,
        )
        .as_str(),
    );
    let start_tick = embassy_time_driver::now();
    let mut milestones = [0u8; 4];
    let report = match lfm25_ffn::run(|progress| {
        let stage_index = match progress.stage {
            lfm25_ffn::Stage::Gate => 0,
            lfm25_ffn::Stage::Up => 1,
            lfm25_ffn::Stage::Silu => 2,
            lfm25_ffn::Stage::Down => 3,
        };
        let quarter =
            core::cmp::min(4, progress.completed.saturating_mul(4) / progress.total.max(1)) as u8;
        if quarter > milestones[stage_index] {
            milestones[stage_index] = quarter;
            print_matrix_target_line(
                target,
                alloc::format!(
                    "tga: model ffn0 stage={} progress={}%",
                    progress.stage.name(),
                    quarter * 25,
                )
                .as_str(),
            );
        }
    })
    .await
    {
        Ok(report) => report,
        Err(lfm25_ffn::Error::Model(error)) => {
            print_model_verify_error(target, error);
            return Err(crate::r::fpga_offload::Error::Protocol);
        }
        Err(error) => {
            print_matrix_target_line(
                target,
                alloc::format!("tga: model ffn0=fail reason={error:?}").as_str(),
            );
            return Err(crate::r::fpga_offload::Error::Protocol);
        }
    };

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model ffn0=pass sealed=true gate=4608 up=4608 silu=4608 down=1024 calls={} irq_global_delta={} timeout_recovery_delta={} elapsed_ms={}",
            report.fpga_calls,
            report.interrupt_delta,
            report.timeout_recovery_delta,
            elapsed_ms_since(start_tick),
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model ffn0 error_max gate={:.9} up={:.9} silu={:.9} down={:.9}",
            report.gate_max_abs,
            report.up_max_abs,
            report.silu_max_abs,
            report.down_max_abs,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model ffn0 down_q30_sha256={} completion=msi-worker-callback",
            digest_hex(&report.down_sha256),
        )
        .as_str(),
    );
    Ok(())
}

const MODEL_VERIFY_PROGRESS_BYTES: u64 = 64 * 1024 * 1024;

async fn run_model_verify(target: &MatrixTarget) {
    use crate::r::lfm25_model;

    let image = match lfm25_model::open().await {
        Ok(image) => image,
        Err(error) => {
            print_model_verify_error(target, error);
            return;
        }
    };

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model=verifying path=trueosfs:/{} bytes={}",
            lfm25_model::NATIVE_IMAGE_PATH,
            image.len(),
        )
        .as_str(),
    );

    let start_tick = embassy_time_driver::now();
    let mut next_progress = MODEL_VERIFY_PROGRESS_BYTES;
    let actual_sha = match lfm25_model::verify_with_progress(&image, |offset, expected_len| {
        if offset >= next_progress && offset < expected_len {
            print_matrix_target_line(
                target,
                alloc::format!(
                    "tga: model=verifying progress={}% bytes={offset}/{expected_len}",
                    offset.saturating_mul(100) / expected_len,
                )
                .as_str(),
            );
            next_progress = next_progress.saturating_add(MODEL_VERIFY_PROGRESS_BYTES);
        }
    })
    .await
    {
        Ok(digest) => digest,
        Err(error) => {
            print_model_verify_error(target, error);
            return;
        }
    };

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model=ready path=trueosfs:/{} bytes={} sha256={} verify_ms={}",
            lfm25_model::NATIVE_IMAGE_PATH,
            image.len(),
            digest_hex(&actual_sha),
            elapsed_ms_since(start_tick),
        )
        .as_str(),
    );
}

fn print_model_verify_error(target: &MatrixTarget, error: crate::r::lfm25_model::Error) {
    use crate::r::lfm25_model::{self, Error};

    let message = match error {
        Error::RootUnavailable => "tga: model=not-ready reason=trueosfs-root-unavailable".into(),
        Error::Missing => alloc::format!(
            "tga: model=not-ready reason=missing path=trueosfs:/{}",
            lfm25_model::NATIVE_IMAGE_PATH,
        ),
        Error::Open { source } => alloc::format!(
            "tga: model=not-ready reason=open-failed path=trueosfs:/{} error={source:?}",
            lfm25_model::NATIVE_IMAGE_PATH,
        ),
        Error::SizeMismatch { observed, expected } => alloc::format!(
            "tga: model=not-ready reason=size-mismatch path=trueosfs:/{} observed={observed} expected={expected}",
            lfm25_model::NATIVE_IMAGE_PATH,
        ),
        Error::BufferUnavailable => "tga: model=not-ready reason=verify-buffer-unavailable".into(),
        Error::Read { offset, source } => alloc::format!(
            "tga: model=not-ready reason=read-failed offset={offset} error={source:?}"
        ),
        Error::ShortRead {
            offset,
            observed,
            expected,
        } => alloc::format!(
            "tga: model=not-ready reason=short-read offset={offset} expected={expected} observed={observed}"
        ),
        Error::HashMismatch { observed, expected } => alloc::format!(
            "tga: model=not-ready reason=sha256-mismatch actual={} expected={}",
            digest_hex(&observed),
            digest_hex(&expected),
        ),
    };
    print_matrix_target_line(target, message.as_str());
}

fn digest_hex(digest: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in digest {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn elapsed_ms_since(start_tick: u64) -> u64 {
    let ticks = embassy_time_driver::now().saturating_sub(start_tick);
    let hz = embassy_time_driver::TICK_HZ.max(1);
    ticks.saturating_mul(1000) / hz
}

async fn run_model_gate_row0(target: &MatrixTarget) -> Result<(), crate::r::fpga_offload::Error> {
    use crate::r::lfm25_model;
    use trueos_fpga_abi::builtins::lfm25_q8_row_block as function;

    let _lane = crate::r::fpga_offload::acquire_lfm25_ffn_step_lane().await;
    let image = match lfm25_model::open().await {
        Ok(image) => image,
        Err(error) => {
            print_model_verify_error(target, error);
            return Err(crate::r::fpga_offload::Error::Protocol);
        }
    };
    let mut weights = [0u8; function::Q8_0_BLOCK_BYTES * function::GATE_ROW0_BLOCKS];
    if let Err(error) = image
        .read_exact_at(function::GATE_ROW0_NATIVE_OFFSET, &mut weights)
        .await
    {
        print_model_verify_error(target, error);
        return Err(crate::r::fpga_offload::Error::Protocol);
    }

    let before = crate::r::fpga_offload::stats();
    let mut expected_row = 0i64;
    let mut final_row = 0i64;
    for block in 0..function::GATE_ROW0_BLOCKS {
        let weight_start = block * function::Q8_0_BLOCK_BYTES;
        let weight: &[u8; function::Q8_0_BLOCK_BYTES] = weights
            [weight_start..weight_start + function::Q8_0_BLOCK_BYTES]
            .try_into()
            .map_err(|_| crate::r::fpga_offload::Error::Protocol)?;
        let result = q8_row_block_via_callback(
            block == 0,
            block + 1 == function::GATE_ROW0_BLOCKS,
            block as u8,
            &function::GOLDEN_GATE_ROW0_ACTIVATIONS[block],
            weight,
        )
        .await?;
        expected_row = expected_row
            .checked_add(function::GOLDEN_GATE_ROW0_TERMS_Q30[block])
            .ok_or(crate::r::fpga_offload::Error::Protocol)?;
        if result.dot != function::GOLDEN_GATE_ROW0_DOTS[block]
            || result.term_q30 != function::GOLDEN_GATE_ROW0_TERMS_Q30[block]
            || result.row_q30 != expected_row
        {
            print_matrix_target_line(
                target,
                alloc::format!(
                    "tga: model row0=fail block={block} dot={}/{} term_q30={}/{} row_q30={}/{}",
                    result.dot,
                    function::GOLDEN_GATE_ROW0_DOTS[block],
                    result.term_q30,
                    function::GOLDEN_GATE_ROW0_TERMS_Q30[block],
                    result.row_q30,
                    expected_row,
                )
                .as_str(),
            );
            return Err(crate::r::fpga_offload::Error::Protocol);
        }
        final_row = result.row_q30;
    }

    let after = crate::r::fpga_offload::stats();
    let interrupt_delta = after.interrupts.saturating_sub(before.interrupts);
    let timeout_recovery_delta = after
        .timeout_recoveries
        .saturating_sub(before.timeout_recoveries);
    let fp_difference = final_row
        .saturating_sub(function::GOLDEN_GATE_ROW0_FP_Q30)
        .unsigned_abs();
    let pass = final_row == function::GOLDEN_GATE_ROW0_Q30
        && fp_difference <= function::GOLDEN_GATE_ROW0_FP_BOUND_Q30 as u64
        && interrupt_delta >= function::GATE_ROW0_BLOCKS as u64
        && timeout_recovery_delta == 0;
    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model row0={} blocks={} row_q30={} fp_q30={} error_q30={} bound_q30={}",
            if pass { "pass" } else { "fail" },
            function::GATE_ROW0_BLOCKS,
            final_row,
            function::GOLDEN_GATE_ROW0_FP_Q30,
            fp_difference,
            function::GOLDEN_GATE_ROW0_FP_BOUND_Q30,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: model row0 source=trueosfs:/{} offset={:#010x} irq_delta={} timeout_recovery_delta={} completion=msi-worker-callback",
            lfm25_model::NATIVE_IMAGE_PATH,
            function::GATE_ROW0_NATIVE_OFFSET,
            interrupt_delta,
            timeout_recovery_delta,
        )
        .as_str(),
    );
    if pass {
        Ok(())
    } else {
        Err(crate::r::fpga_offload::Error::Protocol)
    }
}

async fn q8_row_block_via_callback(
    first: bool,
    last: bool,
    block_index: u8,
    activation: &[u8; 34],
    weight: &[u8; 34],
) -> Result<
    trueos_fpga_abi::builtins::lfm25_q8_row_block::Q8RowBlockResult,
    crate::r::fpga_offload::Error,
> {
    use trueos_fpga_abi::builtins::lfm25_q8_row_block as function;

    let reply = Arc::new(Signal::<
        crate::wait::EmbassySpinRawMutex,
        crate::r::fpga_offload::CallResult,
    >::new());
    let callback_reply = Arc::clone(&reply);
    let input = function::encode(first, last, block_index, activation, weight);
    crate::r::fpga_offload::submit_with_callback(
        function::ID,
        &input,
        function::OUTPUT_BYTES,
        move |result| callback_reply.signal(result),
    )?;
    let completion = reply.wait().await?;
    function::decode(completion.output()).ok_or(crate::r::fpga_offload::Error::Protocol)
}

async fn run_test(target: &MatrixTarget) -> Result<(), crate::r::fpga_offload::Error> {
    let before = crate::r::fpga_offload::stats();
    let heartbeat = crate::r::fpga_offload::led_step_heartbeat().await?;
    let sum = add_u32_via_callback(0x1234_5678, 0x1111_1111).await?;
    let q8 = run_q8_golden().await?;
    let after = crate::r::fpga_offload::stats();
    let interrupt_delta = after.interrupts.saturating_sub(before.interrupts);
    let timeout_recovery_delta = after
        .timeout_recoveries
        .saturating_sub(before.timeout_recoveries);
    let pass = heartbeat
        && sum == 0x2345_6789
        && q8_matches_golden(&q8)
        && interrupt_delta >= 3
        && timeout_recovery_delta == 0;

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: test={} heartbeat={} add={sum:#010x} q8_dot={} q8_term_q30={}",
            if pass { "pass" } else { "fail" },
            heartbeat,
            q8.dot,
            q8.term_q30,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: test counters completed_delta={} irq_delta={} timeout_recovery_delta={}",
            after.completed.saturating_sub(before.completed),
            interrupt_delta,
            timeout_recovery_delta,
        )
        .as_str(),
    );
    print_matrix_target_line(
        target,
        "tga: test path transport=bar0-inline completion=msi-worker-callback",
    );
    if pass {
        Ok(())
    } else {
        Err(crate::r::fpga_offload::Error::Protocol)
    }
}

async fn add_u32_via_callback(a: u32, b: u32) -> Result<u32, crate::r::fpga_offload::Error> {
    use trueos_fpga_abi::builtins::add_u32 as function;

    let reply = Arc::new(Signal::<
        crate::wait::EmbassySpinRawMutex,
        crate::r::fpga_offload::CallResult,
    >::new());
    let callback_reply = Arc::clone(&reply);
    let input = function::encode(a, b);
    crate::r::fpga_offload::submit_with_callback(
        function::ID,
        &input,
        function::OUTPUT_BYTES,
        move |result| callback_reply.signal(result),
    )?;

    let completion = reply.wait().await?;
    function::decode(completion.output()).ok_or(crate::r::fpga_offload::Error::Protocol)
}

async fn run_q8_golden() -> Result<
    trueos_fpga_abi::builtins::lfm25_q8_row_block::Q8RowBlockResult,
    crate::r::fpga_offload::Error,
> {
    use trueos_fpga_abi::builtins::lfm25_q8_row_block as function;

    crate::r::fpga_offload::lfm25_q8_block(&function::GOLDEN_ACTIVATION, &function::GOLDEN_WEIGHT)
        .await
}

fn q8_matches_golden(
    result: &trueos_fpga_abi::builtins::lfm25_q8_row_block::Q8RowBlockResult,
) -> bool {
    let expected = trueos_fpga_abi::builtins::lfm25_q8_row_block::GOLDEN_RESULT;
    result.dot == expected.dot
        && result.term_q30 == expected.term_q30
        && result.row_q30 == expected.row_q30
}

#[cfg(test)]
mod tests {
    use super::parse_u32;

    #[test]
    fn parses_decimal_and_hex_u32() {
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32("0xAA55"), Some(0xAA55));
        assert_eq!(parse_u32("0Xffff0000"), Some(0xFFFF_0000));
        assert_eq!(parse_u32("0x100000000"), None);
    }
}
