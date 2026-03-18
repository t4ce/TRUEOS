use core::str::SplitWhitespace;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn smp_state_name(st: u8) -> &'static str {
    match st {
        crate::smp::STATE_IDLE => "idle",
        crate::smp::STATE_PENDING => "pending",
        crate::smp::STATE_RUNNING => "running",
        crate::smp::STATE_DONE => "done",
        _ => "unknown",
    }
}

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "smp: usage `smp [slot]`");
}

fn dump_slot(io: &'static dyn ShellBackend2, slot: usize) {
    let Some(r) = crate::smp::read(slot) else {
        let msg = alloc::format!("smp: slot={} <unavailable>", slot);
        print_shell_line(io, msg.as_str());
        return;
    };

    let msg = alloc::format!(
        "smp: slot={} online={} state={} seq={} ret=0x{:016X}",
        slot,
        if r.online { 1 } else { 0 },
        smp_state_name(r.state),
        r.seq,
        r.ret
    );
    print_shell_line(io, msg.as_str());
}

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    if !crate::smp::is_init() {
        print_shell_line(io, "smp: not initialized");
        return ParseOutcome::Handled;
    }

    let total = crate::smp::cpu_count();
    let count_msg = alloc::format!("smp: cpu_count={}", total);
    print_shell_line(io, count_msg.as_str());

    if let Some(raw_slot) = args.next() {
        let Ok(slot) = raw_slot.parse::<usize>() else {
            print_usage(io);
            return ParseOutcome::Handled;
        };
        if args.next().is_some() {
            print_usage(io);
            return ParseOutcome::Handled;
        }
        if slot >= total {
            print_usage(io);
            return ParseOutcome::Handled;
        }

        dump_slot(io, slot);
        return ParseOutcome::Handled;
    }

    for slot in 0..total {
        dump_slot(io, slot);
    }

    ParseOutcome::Handled
}
