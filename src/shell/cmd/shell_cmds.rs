
use core::fmt::Write;
use crate::shell::CommandAction;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};

pub(crate) fn cmd_cmd(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let mut cmds: heapless::Vec<&'static str, 64> = heapless::Vec::new();
    crate::shell::cmd::registry::list_command_names(&mut cmds);
    cmds.as_mut_slice().sort_unstable();

    ctx.io.write_str("\r\n");
    
    let light_green = (100, 255, 100);
    let mut col_count = 0;
    
    for name in cmds {
        // Skip subcommands (containing dot)
        if name.contains('.') {
            continue;
        }

        // [name] + space
        let len = name.len() + 3; 
        if col_count + len > *ctx.term_cols {
            ctx.io.write_str("\r\n");
            col_count = 0;
        }
        
        ctx.io.write_str("[");
        let color = if name.eq_ignore_ascii_case("install") || name.eq_ignore_ascii_case("update") {
             (255, 55, 255)
        } else {
             light_green
        };
        ctx.io.write_fmt(format_args!("{}", crate::ecma48::color(name, color)));
        ctx.io.write_str("] ");
        
        col_count += len;
    }
    ctx.io.write_str("\r\n");

    CommandAction::None
}

pub(crate) fn cmd_section(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    // No args: list slots.
    let Some(args) = args else {
        let mut buf: heapless::String<512> = heapless::String::new();
        crate::matrix::list_slots(&mut buf);
        ctx.io.write_str(buf.as_str());
        if let Some(active) = crate::shell::statusbar::active_slot() {
            ctx.io.write_fmt(format_args!("status: active §{}\r\n", active + 1));
        } else {
            ctx.io.write_str("status: active (none)\r\n");
        }
        return CommandAction::None;
    };

    // With id: set active status slot.
    let id = args.get_u8(0).unwrap_or(0);
    if id == 0 {
        ctx.io.write_str("§: ids are 1..\r\n");
        return CommandAction::None;
    }
    let slot_id = id - 1;

    if crate::shell::statusbar::set_active_slot(slot_id) {
        ctx.io.write_fmt(format_args!("status: active §{}\r\n", id));
    } else {
        ctx.io.write_str("§: not found\r\n");
    }

    CommandAction::None
}

pub(crate) fn cmd_ecma48(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    // Escaped IO not needed, forward not possible with new lifetimes easily for spawned tasks
    ctx.io.write_str("ecma48: local echo only in prepend mode\r\n");
    let arg = args
        .and_then(|a| a.get_str(0))
        .unwrap_or("");
    crate::shell::ecma48::handle_ecma48(ctx.io, arg, *ctx.term_cols);
    CommandAction::None
}

pub(crate) fn cmd_go(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterGo
}

pub(crate) fn cmd_mandel(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    crate::vga::draw_mandelbrot();
    ctx.io.write_str("mandel ok\r\n");
    CommandAction::None
}

pub(crate) fn cmd_set(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let Some(args) = args else {
        ctx.io.write_str("set: usage set <cols> <rows>\r\n");
        return CommandAction::None;
    };

    let cols = args.get_usize(0).unwrap_or(0);
    let rows = args.get_usize(1).unwrap_or(0);

    if cols == 0 || rows == 0 {
        ctx.io.write_str("set: cols/rows must be >= 1\r\n");
        ctx.io.write_str("usage: set <cols> <rows>\r\n");
        return CommandAction::None;
    }

    *ctx.term_cols = cols;
    *ctx.term_rows = rows;

    crate::shell::apply_shell_scroll_region(ctx.io, rows);
    // Restore cursor to safe area (Row 3) because DECSTBM resets to (1,1)
    ctx.io.write_fmt(format_args!("{}", crate::ecma48::pos(3, 1)));

    let mut buf: heapless::String<64> = heapless::String::new();
    let _ = write!(&mut buf, "term set: {}x{}\r\n", cols, rows);
    ctx.io.write_str(buf.as_str());
    crate::shell::draw_corners(ctx.io, cols, rows);
    CommandAction::None
}

pub(crate) fn cmd_cube(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterCube
}

pub(crate) fn cmd_ico(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterIco
}

pub(crate) fn cmd_txt(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let arg = args.and_then(|a| a.get_str(0)).unwrap_or("").trim();

    if !arg.is_empty() {
        ctx.io.write_str("txt: argument no longer supported\r\n");
    }

    let Some(slot_id) = crate::matrix::alloc_slot("txt") else {
        ctx.io.write_str("txt: matrix full\r\n");
        return CommandAction::None;
    };

    let mut filename: heapless::String<48> = heapless::String::new();
    let _ = write!(filename, "§{}", slot_id + 1);
    CommandAction::EnterTxtEdt { filename, slot_id }
}

pub(crate) fn cmd_insane(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    let cols = (*ctx.term_cols).max(1);
    ctx.io.write_str("insane: iterating U+0000..=U+10FFFF (Ctrl-C to abort)\r\n");

    let mut col: usize = 0;
    for cp in 0u32..=0x10FFFF {
        if (cp & 0x3FF) == 0 {
            if let Some(b) = ctx.io.read_byte() {
                if b == 0x03 {
                    ctx.io.write_str("\r\ninsane: aborted\r\n");
                    return CommandAction::None;
                }
            }
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

pub(crate) fn cmd_qjs(ctx: &mut ShellCommandCtx<'_>, args: Option<&ParsedArgs<'_>>) -> CommandAction {
    let src = args.and_then(|a| a.get_str(0)).unwrap_or("");
    let src = src.trim();
    if src.is_empty() {
        crate::shell::shellqjs::help(ctx.io);
        CommandAction::None
    } else {
        let mut buf: heapless::String<192> = heapless::String::new();
        for ch in src.chars() {
            if buf.push(ch).is_err() {
                break;
            }
        }
        CommandAction::Qjs { src: buf }
    }
}
