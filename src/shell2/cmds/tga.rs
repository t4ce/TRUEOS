use embassy_executor::Spawner;

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
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "tga: usage `tga [status|test|add <u32> <u32>|q8]`");
}

fn parse_u32(value: &str) -> Option<u32> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse().ok(), |hex| u32::from_str_radix(hex, 16).ok())
}

fn print_status(io: &'static dyn ShellBackend2) {
    let stats = crate::r::fpga_offload::stats();
    print_shell_line(
        io,
        alloc::format!(
            "tga: online={} protocol={} generation={} submitted={} completed={} failed={} queued={} active={}",
            crate::tga::is_online(),
            crate::tga::protocol_alive(),
            crate::tga::connection_generation(),
            stats.submitted,
            stats.completed,
            stats.failed,
            stats.queued,
            stats.active,
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

    if !crate::tga::is_online() {
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
    };

    if let Err(error) = result {
        print_matrix_target_line(
            &target,
            alloc::format!("tga: package failed: {error:?}").as_str(),
        );
    }
    set_matrix_target_active(&target, false);
}

async fn run_test(target: &MatrixTarget) -> Result<(), crate::r::fpga_offload::Error> {
    let before = crate::r::fpga_offload::stats();
    let heartbeat = crate::r::fpga_offload::led_step_heartbeat().await?;
    let sum = crate::r::fpga_offload::add_u32(0x1234_5678, 0x1111_1111).await?;
    let q8 = run_q8_golden().await?;
    let pass = heartbeat && sum == 0x2345_6789 && q8_matches_golden(&q8);
    let after = crate::r::fpga_offload::stats();

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: test={} heartbeat={} add={sum:#010x} q8_dot={} q8_term_q30={} calls_completed_delta={} transport=bar0-work-package",
            if pass { "pass" } else { "fail" },
            heartbeat,
            q8.dot,
            q8.term_q30,
            after.completed.saturating_sub(before.completed),
        )
        .as_str(),
    );
    if pass {
        Ok(())
    } else {
        Err(crate::r::fpga_offload::Error::Protocol)
    }
}

async fn run_q8_golden()
-> Result<trueos_fpga_abi::builtins::lfm25_q8_block::Q8BlockResult, crate::r::fpga_offload::Error> {
    use trueos_fpga_abi::builtins::lfm25_q8_block as function;

    crate::r::fpga_offload::lfm25_q8_block(&function::GOLDEN_ACTIVATION, &function::GOLDEN_WEIGHT)
        .await
}

fn q8_matches_golden(result: &trueos_fpga_abi::builtins::lfm25_q8_block::Q8BlockResult) -> bool {
    let expected = trueos_fpga_abi::builtins::lfm25_q8_block::GOLDEN_RESULT;
    result.dot == expected.dot && result.term_q30 == expected.term_q30
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
