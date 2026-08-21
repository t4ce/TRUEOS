//! app.db-backed Microsoft Edit launcher.

use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};

const EDIT_ARCHIVE: &str = "edit.bp";

#[task(pool_size = 2)]
async fn launch_edit(target: MatrixTarget, app_args: Vec<String>) {
    if let Err(error) = super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        EDIT_ARCHIVE,
        app_args,
    )
    .await
    {
        print_matrix_target_system_line(
            &target,
            alloc::format!("edit: could not launch {EDIT_ARCHIVE} from app.db: {error}").as_str(),
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
        print_shell_line(io, "edit: open the app.db-backed terminal editor: `edit [path]`");
        return ParseOutcome::Handled;
    }

    let app_args = trimmed.split_whitespace().map(String::from).collect();
    match launch_edit(matrix_target_for_backend(io), app_args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "edit: launch task unavailable"),
    }
    ParseOutcome::Handled
}
