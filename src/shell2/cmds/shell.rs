//! app.db-backed UI4 shell launcher.

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};

const SHELL_ARCHIVE: &str = "shell.bp";

#[task(pool_size = 2)]
async fn launch_shell(target: MatrixTarget) {
    if let Err(error) = super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        SHELL_ARCHIVE,
        alloc::vec::Vec::new(),
    )
    .await
    {
        print_matrix_target_system_line(
            &target,
            alloc::format!("shell: could not launch {SHELL_ARCHIVE} from app.db: {error}").as_str(),
        );
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") {
        print_shell_line(
            io,
            "shell: open a UI4 Shell2 and enter it in this Matrix terminal; Ctrl+\\ returns",
        );
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "shell: no arguments expected; use `shell --help`");
        return ParseOutcome::Handled;
    }

    match launch_shell(matrix_target_for_backend(io)) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "shell: launch task unavailable"),
    }
    ParseOutcome::Handled
}
