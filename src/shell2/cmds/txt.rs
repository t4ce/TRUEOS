use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embassy_executor::{Spawner, task};

use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const TXT_APP: &str = "txt";
const TXT_ARCHIVE: &str = "txt.bp";

#[task(pool_size = 2)]
async fn launch_txt(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    let online_args = core::iter::once(String::from(TXT_APP))
        .chain(app_args.iter().cloned())
        .collect();
    match super::run::submit_archive_name_to_target_prefer_trueosfs_async(
        target.clone(),
        TXT_ARCHIVE,
        app_args,
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "txt: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("txt: could not launch {TXT_ARCHIVE}: {error}").as_str(),
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
        print_shell_line(io, "txt: usage `txt [FILE]`");
        return ParseOutcome::Handled;
    }

    let app_args = if trimmed.is_empty() {
        Vec::new()
    } else {
        alloc::vec![trimmed.to_string()]
    };
    let target = matrix_target_for_backend(io);
    match launch_txt(*spawner, target, app_args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "txt: launch task unavailable"),
    }
    ParseOutcome::Handled
}
