use embassy_executor::Spawner;

use crate::shell2::shell2_cmd::ParseOutcome;
use crate::shell2::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_line,
    print_shell_line, set_matrix_target_active,
};

#[derive(Clone, Copy)]
enum TgaCall {
    Ping,
    Add(u32, u32),
    Test,
}
fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "tga: usage `tga [status|ping|test|add <u32> <u32>|led <on|off|u32>]`");
}

fn parse_u32(value: &str) -> Option<u32> {
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse().ok(), |hex| u32::from_str_radix(hex, 16).ok())
}

fn print_status(io: &'static dyn ShellBackend2) {
    let device = crate::tga::status();
    let rpc = crate::r::tga_rpc::stats();
    let bdf = device
        .bdf
        .map(|(bus, slot, function)| alloc::format!("{bus:02X}:{slot:02X}.{function}"))
        .unwrap_or_else(|| alloc::string::String::from("-"));
    print_shell_line(
        io,
        alloc::format!(
            "tga: online={} protocol={} bdf={} bar0={:#x} size={:#x} msi={} generation={} interrupts={}",
            device.online,
            device.protocol_alive,
            bdf,
            device.bar_phys.unwrap_or(0),
            device.bar_size.unwrap_or(0),
            device.msi_ready,
            device.generation,
            device.interrupts,
        )
        .as_str(),
    );
    print_shell_line(
        io,
        alloc::format!(
            "tga: rpc submitted={} completed={} failed={} queued={} inflight={} wakes={} timeout_recoveries={}",
            rpc.submitted,
            rpc.completed,
            rpc.failed,
            rpc.queued,
            rpc.inflight,
            rpc.interrupt_wakes,
            rpc.timeout_recoveries,
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
        None | Some("status") if args.next().is_none() => {
            print_status(io);
            return ParseOutcome::Handled;
        }
        Some("help" | "-h" | "--help") if args.next().is_none() => {
            usage(io);
            return ParseOutcome::Handled;
        }
        Some("ping") if args.next().is_none() => TgaCall::Ping,
        Some("test") if args.next().is_none() => TgaCall::Test,
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
        Some("led") => {
            let value = match (args.next(), args.next()) {
                (Some("on"), None) => 1,
                (Some("off"), None) => 0,
                (Some(value), None) => match parse_u32(value) {
                    Some(value) => value,
                    None => {
                        usage(io);
                        return ParseOutcome::Handled;
                    }
                },
                _ => {
                    usage(io);
                    return ParseOutcome::Handled;
                }
            };
            crate::tga::tga_led_write(value);
            print_shell_line(io, alloc::format!("tga: raw LED fallback wrote {value:#x}").as_str());
            return ParseOutcome::Handled;
        }
        _ => {
            usage(io);
            return ParseOutcome::Handled;
        }
    };

    if !crate::tga::is_online() {
        print_shell_line(io, "tga: device is offline");
        return ParseOutcome::Handled;
    }
    let target = matrix_target_for_backend(io);
    set_matrix_target_active(&target, true);
    match tga_call_task(target.clone(), call) {
        Ok(task) => spawner.spawn(task),
        Err(_) => {
            set_matrix_target_active(&target, false);
            print_shell_line(io, "tga: RPC task unavailable");
        }
    }
    ParseOutcome::Handled
}

#[embassy_executor::task(pool_size = 2)]
async fn tga_call_task(target: MatrixTarget, call: TgaCall) {
    let result = match call {
        TgaCall::Ping => crate::r::tga_rpc::heartbeat()
            .await
            .map(|alive| alloc::format!("tga: ping {}", if alive { "ok" } else { "bad-reply" })),
        TgaCall::Add(a, b) => crate::r::tga_rpc::add_u32(a, b)
            .await
            .map(|sum| alloc::format!("tga: add {a} + {b} = {sum}")),
        TgaCall::Test => match crate::r::tga_rpc::heartbeat().await {
            Ok(true) => crate::r::tga_rpc::add_u32(0x1122_3344, 0x0102_0304)
                .await
                .and_then(|sum| {
                    (sum == 0x1224_3648)
                        .then(|| alloc::format!("tga: test pass heartbeat=ok add={sum:#010x}"))
                        .ok_or(crate::r::tga_rpc::Error::Protocol)
                }),
            Ok(false) => Err(crate::r::tga_rpc::Error::Protocol),
            Err(error) => Err(error),
        },
    };
    match result {
        Ok(line) => print_matrix_target_line(&target, line.as_str()),
        Err(error) => {
            print_matrix_target_line(&target, alloc::format!("tga: RPC failed: {error:?}").as_str())
        }
    }
    set_matrix_target_active(&target, false);
}
