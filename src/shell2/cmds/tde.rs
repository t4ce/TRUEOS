use embassy_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    ShellBackend2, matrix_target_for_backend, print_shell_line,
    submit_online_launch_script_to_target,
};

const TDE_APP: &str = "texplo";
const TDE_LAUNCH_SCRIPT: &str = "fs-scope trueosfs\nbrowse /\ndepth 2\n";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(
            io,
            "tde: launch the Terminal Directory Explorer at the TRUEOSFS root with depth 2",
        );
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "tde: no arguments expected; use `tde --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    if submit_online_launch_script_to_target(spawner, target, TDE_APP, TDE_LAUNCH_SCRIPT).is_err() {
        print_shell_line(io, "tde: Texplo online launch task unavailable");
    }
    ParseOutcome::Handled
}
