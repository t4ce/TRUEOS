use core::str::SplitWhitespace;

use super::super::{ShellBackend2, minimum_line_width_for_backend, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const MIN_LINE_WIDTH: usize = 50;
const MAX_LINE_WIDTH: usize = 500;

pub(crate) fn try_parse(
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    let Some(width_text) = args.next() else {
        print_shell_line(io, "set: usage `set <width>`");
        return ParseOutcome::Handled;
    };
    if args.next().is_some() {
        print_shell_line(io, "set: usage `set <width>`");
        return ParseOutcome::Handled;
    }

    let Ok(width) = width_text.parse::<usize>() else {
        print_shell_line(io, "set: width must be a number");
        return ParseOutcome::Handled;
    };
    let min_width = minimum_line_width_for_backend(io).max(MIN_LINE_WIDTH);
    if !(min_width..=MAX_LINE_WIDTH).contains(&width) {
        let msg = alloc::format!("set: width must be {}..{}", min_width, MAX_LINE_WIDTH);
        print_shell_line(io, msg.as_str());
        return ParseOutcome::Handled;
    }

    let msg = alloc::format!("set: width={}", width);
    print_shell_line(io, msg.as_str());
    ParseOutcome::SetLineWidth(width)
}
