
pub(crate) fn cmd_ecma48(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    // Escaped IO not needed, forward not possible with new lifetimes easily for spawned tasks
    ctx.io
        .write_str("ecma48: local echo only in prepend mode\r\n");
    let arg = args.and_then(|a| a.get_str(0)).unwrap_or("");
    crate::shell::ecma48::handle_ecma48(ctx.io, arg, *ctx.term_cols);
    CommandAction::None
}

pub(crate) fn cmd_go(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterGo
}

pub(crate) fn cmd_go_two(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterGoTwo
}

// rework so its from 0 to 3949(whatever) better readable
pub(crate) fn cmd_insane(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let cols = (*ctx.term_cols).max(1);
    ctx.io
        .write_str("insane: iterating U+0000..=U+10FFFF (Ctrl-C to abort)\r\n");

    let mut col: usize = 0;
    for cp in 0u32..=0x10FFFF {
        if (cp & 0x3FF) == 0
            && let Some(b) = ctx.io.read_byte()
            && b == 0x03
        {
            ctx.io.write_str("\r\ninsane: aborted\r\n");
            return CommandAction::None;
        }

        let ch = match core::char::from_u32(cp) {
            Some(ch) if !ch.is_control() => ch,
            Some(_) => '.',
            None => '\u{FFFD}',
        };

        ctx.io.write_char(ch);

        col += 1;
        if col >= cols {
            ctx.io.write_str("\r\n");
            col = 0;
        }
    }

    if col != 0 {
        ctx.io.write_str("\r\n");
    }
    ctx.io.write_str("insane: done\r\n");
    CommandAction::None
}
