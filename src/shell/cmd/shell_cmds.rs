use alloc::string::String;
use embassy_executor::task;
use heapless::String as HString;

use crate::shell::CommandAction;
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use core::fmt::Write;

pub(crate) fn cmd_cmd(ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    // Keep this comfortably above the total command count because `list_command_names()` includes
    // dotted subcommands, and we filter them out below.
    let mut cmds: heapless::Vec<&'static str, 256> = heapless::Vec::new();
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
        ctx.io
            .write_fmt(format_args!("{}", crate::ecma48::color(name, color)));
        ctx.io.write_str("] ");

        col_count += len;
    }
    ctx.io.write_str("\r\n");

    CommandAction::None
}

pub(crate) fn cmd_section(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    // No args: list slots.
    let Some(args) = args else {
        let mut buf: heapless::String<512> = heapless::String::new();
        crate::matrix::list_slots(&mut buf);
        ctx.io.write_str(buf.as_str());
        if let Some(active) = crate::shell::statusbar::active_slot() {
            ctx.io
                .write_fmt(format_args!("status: active §{}\r\n", active + 1));
        } else {
            ctx.io.write_str("status: active (none)\r\n");
        }
        return CommandAction::None;
    };

    // With id: set active status slot and dump slot contents (without clearing).
    let id = args.get_u8(0).unwrap_or(0);
    if id == 0 {
        ctx.io.write_str("§: ids are 1..\r\n");
        return CommandAction::None;
    }
    let slot_id = id - 1;

    if !crate::shell::statusbar::set_active_slot(slot_id) {
        ctx.io.write_str("§: not found\r\n");
        return CommandAction::None;
    }

    ctx.io.write_fmt(format_args!("status: active §{}\r\n", id));

    if let Some(blob) = crate::matrix::clone_blob(slot_id)
        && !blob.is_empty()
    {
        if let Ok(s) = core::str::from_utf8(blob.as_slice()) {
            let upgraded = crate::shell::ecma48::json_upgrade(s);
            write_crlf_lines(ctx.io, upgraded.as_str());
        } else {
            let lossy = alloc::string::String::from_utf8_lossy(blob.as_slice());
            let upgraded = crate::shell::ecma48::json_upgrade(lossy.as_ref());
            write_crlf_lines(ctx.io, upgraded.as_str());
        }
        return CommandAction::None;
    }

    let mut buf: heapless::String<1024> = heapless::String::new();
    if crate::matrix::dump_slot(&mut buf, slot_id) {
        ctx.io.write_str(buf.as_str());
    } else {
        ctx.io.write_str("§: not found\r\n");
    }

    CommandAction::None
}

fn write_crlf_lines(io: &dyn crate::shell::ShellIo, s: &str) {
    for line in s.split('\n') {
        io.write_str(line.trim_end_matches('\r'));
        io.write_str("\r\n");
    }
}

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

pub(crate) fn cmd_go_two(
    _ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    CommandAction::EnterGoTwo
}

pub(crate) fn cmd_mandel(
    ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    crate::vga::draw_mandelbrot();
    ctx.io.write_str("mandel ok\r\n");
    CommandAction::None
}

pub(crate) fn cmd_set(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
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

    CommandAction::SetTermSize { cols, rows }
}

pub(crate) fn cmd_cube(
    _ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    CommandAction::EnterCube
}

pub(crate) fn cmd_ico(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterIco
}

pub(crate) fn cmd_txt(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
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

pub(crate) fn cmd_tetris(
    _ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    CommandAction::EnterTetris
}

pub(crate) fn cmd_ai(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let text = args.and_then(|a| a.get_str(0)).unwrap_or("").trim();
    if text.is_empty() {
        ctx.io.write_str("ai: usage ai <text>\r\n");
        return CommandAction::None;
    }

    let entry = trueos_qjs::ai_task::AiInputEntry {
        text: String::from(text),
        web_search: false,
        file_search: false,
        new_conversation: false,
        computer_use: true,
    };

    match trueos_qjs::ai_task::ensure_started(ctx.spawner) {
        trueos_qjs::ai_task::EnsureStartedResult::Ready => {}
        trueos_qjs::ai_task::EnsureStartedResult::BrowserNotReady => {
            ctx.io.write_str("ai: browser not ready yet\r\n");
            return CommandAction::None;
        }
        trueos_qjs::ai_task::EnsureStartedResult::SpawnFailed => {
            ctx.io.write_str("ai: ai-task start failed\r\n");
            return CommandAction::None;
        }
    }

    if !trueos_qjs::ai_task::queue_ai_input(entry) {
        ctx.io.write_str("ai: ai-task not running\r\n");
        return CommandAction::None;
    }

    ctx.io.write_str("ai: queued\r\n");
    CommandAction::None
}

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

pub(crate) fn cmd_surf(
    ctx: &mut ShellCommandCtx<'_>,
    args: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    let Some(raw) = args.and_then(|a| a.get_str(0)) else {
        ctx.io.write_str("surf: usage surf <url>\r\n");
        return CommandAction::None;
    };

    let mut trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let b = trimmed.as_bytes();
        let first = b[0];
        let last = b[b.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            trimmed = &trimmed[1..trimmed.len() - 1];
            trimmed = trimmed.trim();
        }
    }
    if trimmed.is_empty() {
        ctx.io.write_str("surf: usage surf <url>\r\n");
        return CommandAction::None;
    }

    let mut url: HString<256> = HString::new();
    for ch in trimmed.chars() {
        if url.push(ch).is_err() {
            ctx.io.write_str("surf: url too long (max 256 chars)\r\n");
            return CommandAction::None;
        }
    }

    if ctx.spawner.spawn(surf_job(url)).is_err() {
        ctx.io.write_str("surf: spawn failed\r\n");
        return CommandAction::None;
    }

    ctx.io.write_str("surf: started\r\n");
    CommandAction::None
}

#[task]
async fn surf_job(url: HString<256>) {
    let source_url = {
        let raw = url.as_str().trim();
        if raw.starts_with("https://") || raw.starts_with("http://") {
            String::from(raw)
        } else {
            alloc::format!("https://{}", raw)
        }
    };
    match crate::tst_html::fetch_html_best_effort(url).await {
        Ok(html) => {
            if !trueos_qjs::browser_task::queue_set_html_with_url(
                String::from(html.as_str()),
                Some(source_url),
            ) {
                crate::log!("surf: browser not running\n");
            }
        }
        Err(e) => {
            if e == "timed out" {
                crate::log!("surf: download timed out\n");
            } else {
                crate::log!("surf: fetch failed: {}\n", e);
            }
        }
    }
}
