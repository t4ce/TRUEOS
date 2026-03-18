pub(crate) fn cmd_hv(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    #[inline]
    fn print_usage(io: &dyn ShellIo) {
        io.write_str("hv: usage hv [status|start|stop|log]\r\n");
        io.write_str("hv: single-VM milestone target is vm1\r\n");
    }

    let op = args.and_then(|a| a.get_str(0)).unwrap_or("status").trim();

    if op.is_empty() || op.eq_ignore_ascii_case("status") {
        let s = crate::hv::status();
        ctx.io.write_fmt(format_args!(
            "hv: vmx intel={} msr={} vmx={} fc_lock={} fc_vmx_outside_smx={}\r\n",
            s.vendor_intel as u8,
            s.has_msr as u8,
            s.has_vmx as u8,
            s.feature_control_locked as u8,
            s.feature_control_vmx_outside_smx as u8
        ));
        ctx.io.write_fmt(format_args!(
            "hv: vm1 running={} starting={} marker_seen={} guest_module={}\r\n",
            s.vm1_running as u8,
            s.vm1_starting as u8,
            s.vm1_marker_seen as u8,
            s.guest_module_present as u8
        ));
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("start") {
        // SAFETY: Upgrade IO lifetime for background task. Backend is static (Serial/VGA).
        let io_static: &'static dyn ShellBackend = unsafe { core::mem::transmute(ctx.io) };
        match crate::hv::start(ctx.spawner, io_static) {
            Ok(()) => ctx.io.write_str("hv: vm1 started\r\n"),
            Err(e) => ctx
                .io
                .write_fmt(format_args!("hv: start failed: {:?}\r\n", e)),
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("stop") {
        if crate::hv::stop() {
            ctx.io.write_str("hv: vm1 stop requested\r\n");
        } else {
            ctx.io.write_str("hv: vm1 not running\r\n");
        }
        return CommandAction::None;
    }

    if op.eq_ignore_ascii_case("log") {
        crate::hv::write_logs(ctx.io);
        return CommandAction::None;
    }

    print_usage(ctx.io);
    CommandAction::None
}
