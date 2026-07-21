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
    Xor(u32, u32),
}

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "tga: usage `tga [status|test|add <u32> <u32>|xor <u32> <u32>]`");
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
        Some(operation @ ("add" | "xor")) => {
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
            if operation == "add" {
                TgaCall::Add(a, b)
            } else {
                TgaCall::Xor(a, b)
            }
        }
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    };

    if !crate::tga::is_online() || !crate::tga::protocol_alive() {
        print_shell_line(io, "tga: unavailable; endpoint or admitted firmware bundle is offline");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match tga_call_task(target.clone(), call) {
        Ok(token) => spawner.spawn(token),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "tga: runtime probe task unavailable");
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
        TgaCall::Xor(a, b) => match crate::r::fpga_offload::xor_u32(a, b).await {
            Ok(value) => {
                print_matrix_target_line(
                    &target,
                    alloc::format!("tga: xor {a:#010x} {b:#010x} -> {value:#010x}").as_str(),
                );
                Ok(())
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
    let xor = crate::r::fpga_offload::xor_u32(0xAA55_AA55, 0xFFFF_0000).await?;
    let pass = heartbeat && sum == 0x2345_6789 && xor == 0x55AA_AA55;
    let after = crate::r::fpga_offload::stats();

    print_matrix_target_line(
        target,
        alloc::format!(
            "tga: test={} heartbeat={} add={sum:#010x} xor={xor:#010x} calls_completed_delta={} transport=bar0-work-package",
            if pass { "pass" } else { "fail" },
            heartbeat,
            after.completed.saturating_sub(before.completed),
        )
        .as_str(),
    );
    Ok(())
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
