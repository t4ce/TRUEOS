use alloc::format;

use embassy_executor::Spawner;
use embassy_time::{Duration, with_timeout};

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;
use crate::usb2::lab::{LabCommand, PendingLabRequest};

const COMMAND_TIMEOUT_SECS: u64 = 180;
const USAGE: &str = "xhci: usage `xhci status|journal|stage <1..5> [port] [arm] [live] [fused] [depth=1..3]|read|read64 <offset>|write|write64 <offset> <value> arm [live] [fused]|rmw <offset> <clear-mask> <set-mask> arm [live] [fused]`";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let command = match parse_command(rest) {
        Ok(command) => command,
        Err(reason) => {
            print_shell_line(io, format!("xhci: rejected reason={reason}").as_str());
            print_shell_line(io, USAGE);
            return ParseOutcome::Handled;
        }
    };
    let pending = match crate::usb2::lab::submit(command) {
        Ok(pending) => pending,
        Err(reason) => {
            print_shell_line(io, format!("xhci: submit failed reason={reason}").as_str());
            return ParseOutcome::Handled;
        }
    };
    print_shell_line(io, format!("xhci: queued run={}", pending.run_id).as_str());
    match xhci_command_task(io, pending) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "xhci: task spawn failed"),
    }
    ParseOutcome::Handled
}

fn parse_command(rest: &str) -> Result<LabCommand, &'static str> {
    let mut args = rest.split_whitespace();
    let Some(command) = args.next() else {
        return Err("missing-command");
    };
    match command {
        "status" if args.next().is_none() => Ok(LabCommand::Status),
        "journal" if args.next().is_none() => Ok(LabCommand::Journal),
        "stage" => {
            let stage = parse_u8(args.next().ok_or("missing-stage")?)?;
            parse_stage(stage, args)
        }
        "read" => {
            let offset = parse_usize(args.next().ok_or("missing-offset")?)?;
            if args.next().is_some() {
                return Err("read-takes-one-offset");
            }
            Ok(LabCommand::Read32 { offset })
        }
        "read64" => {
            let offset = parse_usize(args.next().ok_or("missing-offset")?)?;
            if args.next().is_some() {
                return Err("read64-takes-one-offset");
            }
            Ok(LabCommand::Read64 { offset })
        }
        "write" => {
            let offset = parse_usize(args.next().ok_or("missing-offset")?)?;
            let value = parse_u32(args.next().ok_or("missing-value")?)?;
            let mut armed = false;
            let mut include_fused = false;
            let mut allow_live_device = false;
            for token in args {
                match token {
                    "arm" => armed = true,
                    "fused" => include_fused = true,
                    "live" => allow_live_device = true,
                    _ => return Err("invalid-write-option"),
                }
            }
            Ok(LabCommand::Write32 {
                offset,
                value,
                armed,
                include_fused,
                allow_live_device,
            })
        }
        "write64" => {
            let offset = parse_usize(args.next().ok_or("missing-offset")?)?;
            let value = parse_u64(args.next().ok_or("missing-value")?)?;
            let mut armed = false;
            let mut include_fused = false;
            let mut allow_live_device = false;
            for token in args {
                match token {
                    "arm" => armed = true,
                    "fused" => include_fused = true,
                    "live" => allow_live_device = true,
                    _ => return Err("invalid-write64-option"),
                }
            }
            Ok(LabCommand::Write64 {
                offset,
                value,
                armed,
                include_fused,
                allow_live_device,
            })
        }
        "rmw" => {
            let offset = parse_usize(args.next().ok_or("missing-offset")?)?;
            let clear_mask = parse_u32(args.next().ok_or("missing-clear-mask")?)?;
            let set_mask = parse_u32(args.next().ok_or("missing-set-mask")?)?;
            let mut armed = false;
            let mut include_fused = false;
            let mut allow_live_device = false;
            for token in args {
                match token {
                    "arm" => armed = true,
                    "fused" => include_fused = true,
                    "live" => allow_live_device = true,
                    _ => return Err("invalid-rmw-option"),
                }
            }
            Ok(LabCommand::ReadModifyWrite32 {
                offset,
                clear_mask,
                set_mask,
                armed,
                include_fused,
                allow_live_device,
            })
        }
        number if number.as_bytes().iter().all(u8::is_ascii_digit) => {
            parse_stage(parse_u8(number)?, args)
        }
        _ => Err("unknown-command"),
    }
}

fn parse_stage<'a>(
    stage: u8,
    mut args: impl Iterator<Item = &'a str>,
) -> Result<LabCommand, &'static str> {
    if !(1..=5).contains(&stage) {
        return Err("stage-out-of-range");
    }
    let mut port = None;
    let mut armed = false;
    let mut include_fused = false;
    let mut allow_live_device = false;
    let mut depth = 2u8;
    for token in args.by_ref() {
        match token {
            "arm" => armed = true,
            "fused" => include_fused = true,
            "live" => allow_live_device = true,
            _ if token.starts_with("depth=") => {
                depth = parse_u8(token.trim_start_matches("depth="))?;
                if !(1..=3).contains(&depth) {
                    return Err("depth-out-of-range");
                }
            }
            _ if port.is_none() => port = Some(parse_u8(token)?),
            _ => return Err("invalid-stage-option"),
        }
    }
    if stage <= 2 && port.is_some() {
        return Err("stage-does-not-take-port");
    }
    Ok(LabCommand::Stage {
        stage,
        port,
        armed,
        include_fused,
        allow_live_device,
        depth,
    })
}

fn parse_u8(raw: &str) -> Result<u8, &'static str> {
    parse_u64(raw).and_then(|value| u8::try_from(value).map_err(|_| "number-out-of-range"))
}

fn parse_u32(raw: &str) -> Result<u32, &'static str> {
    parse_u64(raw).and_then(|value| u32::try_from(value).map_err(|_| "number-out-of-range"))
}

fn parse_usize(raw: &str) -> Result<usize, &'static str> {
    parse_u64(raw).and_then(|value| usize::try_from(value).map_err(|_| "number-out-of-range"))
}

fn parse_u64(raw: &str) -> Result<u64, &'static str> {
    if let Some(hex) = raw.strip_prefix("0x").or_else(|| raw.strip_prefix("0X")) {
        u64::from_str_radix(hex, 16).map_err(|_| "invalid-number")
    } else {
        raw.parse::<u64>().map_err(|_| "invalid-number")
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn xhci_command_task(io: &'static dyn ShellBackend2, pending: PendingLabRequest) {
    let run_id = pending.run_id;
    match with_timeout(Duration::from_secs(COMMAND_TIMEOUT_SECS), pending.wait()).await {
        Ok(Ok(report)) => {
            for line in report.lines {
                print_shell_line(io, line.as_str());
            }
            print_shell_line(
                io,
                format!("xhci: complete run={} status=ok", report.run_id).as_str(),
            );
        }
        Ok(Err(reason)) => print_shell_line(
            io,
            format!("xhci: complete run={run_id} status=failed reason={reason}").as_str(),
        ),
        Err(_) => print_shell_line(
            io,
            format!("xhci: complete run={run_id} status=timeout timeout_s={COMMAND_TIMEOUT_SECS}")
                .as_str(),
        ),
    }
}
