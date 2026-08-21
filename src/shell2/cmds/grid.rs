use trueos_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    ShellBackend2, matrix_target_for_backend, print_shell_line, submit_online_to_target,
};

const GRIDPAPER_APP: &str = "gridpaper";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(io, "grid: launch the online Gridpaper app");
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "grid: no arguments expected; use `grid --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    if submit_online_to_target(
        spawner,
        target,
        alloc::vec![alloc::string::String::from(GRIDPAPER_APP)],
    )
    .is_err()
    {
        print_shell_line(io, "grid: online Gridpaper launch task unavailable");
    }
    ParseOutcome::Handled
}
