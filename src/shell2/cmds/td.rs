use alloc::string::String;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};

const TD_ARCHIVE: &str = "termdir.bp";
const TD_LAUNCH_SCRIPT: &str = "fs-scope trueosfs\nbrowse /\ndepth 2\n";

#[task(pool_size = 2)]
async fn launch_td(target: MatrixTarget) {
    if let Err(error) =
        super::run::submit_archive_name_to_target_from_app_db_with_launch_script_async(
            target.clone(),
            TD_ARCHIVE,
            String::from(TD_LAUNCH_SCRIPT),
        )
        .await
    {
        print_matrix_target_system_line(
            &target,
            alloc::format!("td: could not launch {TD_ARCHIVE} from app.db: {error}").as_str(),
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
        print_shell_line(io, "td: launch termdir at the TRUEOSFS root with depth 2");
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "td: no arguments expected; use `td --help`");
        return ParseOutcome::Handled;
    }

    match launch_td(matrix_target_for_backend(io)) {
        Ok(token) => {
            crate::intel::begin_transient_global_gt_boost(spawner, "shell2-td");
            spawner.spawn(token);
        }
        Err(_) => print_shell_line(io, "td: termdir app.db launch task unavailable"),
    }
    ParseOutcome::Handled
}
