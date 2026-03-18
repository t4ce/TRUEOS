
fn smp_state_name(st: u8) -> &'static str {
    match st {
        crate::smp::STATE_IDLE => "idle",
        crate::smp::STATE_PENDING => "pending",
        crate::smp::STATE_RUNNING => "running",
        crate::smp::STATE_DONE => "done",
        _ => "unknown",
    }
}

pub(crate) fn cmd_smp(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    if !crate::smp::is_init() {
        ctx.io.write_str("smp: not initialized\r\n");
        return CommandAction::None;
    }

    let total = crate::smp::cpu_count();
    ctx.io
        .write_fmt(format_args!("smp: cpu_count={}\r\n", total));

    let slot_opt = args.and_then(|a| a.get_usize(0));

    let dump_slot = |slot: usize| {
        let Some(r) = crate::smp::read(slot) else {
            ctx.io
                .write_fmt(format_args!("smp: slot={} <unavailable>\r\n", slot));
            return;
        };
        ctx.io.write_fmt(format_args!(
            "smp: slot={} online={} state={} seq={} ret=0x{:016X}\r\n",
            slot,
            if r.online { 1 } else { 0 },
            smp_state_name(r.state),
            r.seq,
            r.ret
        ));
    };

    if let Some(slot) = slot_opt {
        if slot >= total {
            ctx.io.write_str("smp: usage smp [slot]\r\n");
            return CommandAction::None;
        }
        dump_slot(slot);
        return CommandAction::None;
    }

    for slot in 0..total {
        dump_slot(slot);
    }

    CommandAction::None
}
