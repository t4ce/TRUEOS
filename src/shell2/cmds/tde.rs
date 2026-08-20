use alloc::string::String;

use embassy_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};

const TDE_ARCHIVE: &str = "texplo.bp";
const TDE_LAUNCH_SCRIPT: &str = "fs-scope trueosfs\nbrowse /\ndepth 2\n";

#[task(pool_size = 2)]
async fn launch_tde(target: MatrixTarget) {
    match super::run::submit_archive_name_to_target_prefer_trueosfs_with_launch_script_async(
        target.clone(),
        TDE_ARCHIVE,
        String::from(TDE_LAUNCH_SCRIPT),
    )
    .await
    {
        Ok(_) => {}
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("tde: could not launch {TDE_ARCHIVE}: {error}").as_str(),
        ),
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
            "tde: launch the Terminal Directory Explorer at the TRUEOSFS root with depth 2",
        );
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "tde: no arguments expected; use `tde --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    match launch_tde(target) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "tde: launch task unavailable"),
    }
    ParseOutcome::Handled
}
