pub(crate) fn cmd_turbo(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let op = args.and_then(|a| a.get_str(0)).unwrap_or("").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let armed = crate::turbo::armed();
        match crate::turbo::local_state() {
            Ok(st) => {
                ctx.io
                    .write_fmt(format_args!("turbo: armed={} state={:?}\r\n", armed, st));
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io
                    .write_fmt(format_args!("turbo: unsupported (intel-only)\r\n"));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // Reads should never require arming; keep for forward-compat.
                ctx.io.write_fmt(format_args!("turbo: disarmed\r\n"));
            }
        }
        if !armed {
            ctx.io
                .write_str("turbo: writes are disarmed (run 'turbo arm')\r\n");
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("arm") {
        crate::turbo::set_armed(true);
        ctx.io.write_str("turbo: armed\r\n");
        return CommandAction::None;
    }
    if op.eq_ignore_ascii_case("disarm") {
        crate::turbo::set_armed(false);
        ctx.io.write_str("turbo: disarmed\r\n");
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("verify") {
        let spins = args.and_then(|a| a.get_usize(1)).unwrap_or(200_000);

        match crate::turbo::verify_all(spins) {
            Ok(r) => {
                ctx.io.write_fmt(format_args!(
                    "turbo: verify spins={} turbo={} noturbo={} unknown={} completed_aps={}/{} online_aps={} busy={} total_cpus={} seq={}{}\r\n",
                    spins,
                    r.turbo_cpus,
                    r.noturbo_cpus,
                    r.unknown_cpus,
                    r.completed_aps,
                    r.submitted_aps,
                    r.online_aps,
                    r.busy_aps,
                    r.total_cpus,
                    r.seq,
                    if r.timed_out { " TIMEOUT" } else { "" }
                ));
            }
            Err(crate::turbo::TurboSetError::Disarmed) => {
                // verify is read-only; keep for forward-compat and clarity.
                ctx.io
                    .write_str("turbo: msr disarmed (verify should not require arm)\r\n");
            }
            Err(crate::turbo::TurboSetError::Unsupported) => {
                ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
            }
        }

        return CommandAction::None;
    }

    let enable = if op.eq_ignore_ascii_case("on") {
        Some(true)
    } else if op.eq_ignore_ascii_case("off") {
        Some(false)
    } else {
        None
    };

    let Some(enable) = enable else {
        ctx.io
            .write_str("turbo: usage turbo [status|arm|disarm|on|off|verify [spins]]\r\n");
        return CommandAction::None;
    };

    match crate::turbo::set_enabled_all(enable) {
        Ok(r) => {
            ctx.io.write_fmt(format_args!(
                "turbo: requested={} ap_submitted={}/{} busy={} total_cpus={} seq={}\r\n",
                if r.requested_enable { "on" } else { "off" },
                r.submitted_aps,
                r.targeted_aps,
                r.busy_aps,
                r.total_cpus,
                r.seq
            ));
        }
        Err(crate::turbo::TurboSetError::Disarmed) => {
            ctx.io
                .write_str("turbo: msr disarmed (run 'turbo arm')\r\n");
        }
        Err(crate::turbo::TurboSetError::Unsupported) => {
            ctx.io.write_str("turbo: unsupported (intel-only)\r\n");
        }
    }

    CommandAction::None
}