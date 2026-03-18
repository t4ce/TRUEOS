pub(crate) fn demo_ecma48(io: &dyn super::ShellBackend2, rest: &str, cols: usize) {



fn cmd_go(io: &'static dyn ShellBackend) {
    const GO_CHARS: [char; 9] = ['⣿', '⣾', '⣽', '⣻', '⢿', '⡿', '⣟', '⣯', '⣷'];
}

fn cmd_go_two(io: &'static dyn ShellBackend) {
    const GO_TWO_CHARS: [char; 9] = ['⢈', '⡈', '⡐', '⡠', '⣀', '⢄', '⢂', '⢁', '⡁'];
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
