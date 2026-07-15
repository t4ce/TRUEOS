use super::super::{SCROLL_TOP_ROW, ShellBackend2, ecma48};
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let text = quoted_text(rest).unwrap_or("fnt: usage `fnt \"text\"`");
    io.raw_write_str(ecma48::SAVE_CURSOR);
    io.raw_write_fmt(format_args!("\x1b[{};1H\x1b[2K", SCROLL_TOP_ROW));
    io.raw_write_str(text);
    io.raw_write_str(ecma48::RESTORE_CURSOR);
    ParseOutcome::Handled
}

fn quoted_text(rest: &str) -> Result<&str, &'static str> {
    let input = rest.trim();
    let Some(quoted) = input.strip_prefix('"') else {
        return Err("fnt: usage `fnt \"text\"`");
    };
    quoted.strip_suffix('"').ok_or("fnt: usage `fnt \"text\"`")
}
