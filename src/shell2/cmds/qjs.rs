use embassy_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{ShellBackend2, matrix_target_for_backend, print_shell_line, shell2_qjs};

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(io, "qjs: usage `qjs`; exit the workbench with ESC or `:quit`");
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "qjs: no arguments expected; use `qjs --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    match shell2_qjs::begin_session(spawner, &target) {
        Ok(()) => ParseOutcome::StartSession(shell2_qjs::session_kind()),
        Err(error) => {
            print_shell_line(
                io,
                alloc::format!("qjs: could not open workbench: {error}").as_str(),
            );
            ParseOutcome::Handled
        }
    }
}
