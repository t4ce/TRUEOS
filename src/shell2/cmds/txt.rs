use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embassy_executor::{Spawner, task};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const TXT_ARCHIVE: &str = "txt.bp";

#[task(pool_size = 2)]
async fn launch_txt(target: MatrixTarget, app_args: Vec<String>) {
    if let Err(error) = super::run::submit_archive_name_to_target_prefer_trueosfs_async(
        target.clone(),
        TXT_ARCHIVE,
        app_args,
    )
    .await
    {
        print_matrix_target_system_line(
            &target,
            alloc::format!("txt: could not launch {TXT_ARCHIVE}: {error}").as_str(),
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
        print_shell_line(io, "txt: usage `txt [FILE]`");
        return ParseOutcome::Handled;
    }

    let app_args = if trimmed.is_empty() {
        Vec::new()
    } else {
        alloc::vec![trimmed.to_string()]
    };
    let target = matrix_target_for_backend(io);
    match launch_txt(target, app_args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "txt: launch task unavailable"),
    }
    ParseOutcome::Handled
}
