use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, _rest: &str) -> ParseOutcome {
    print_shell_line(io, "txt: noop");
    ParseOutcome::Handled
}
