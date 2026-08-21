use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const QJS_APP: &str = "qjs";
const QJS_ARCHIVE: &str = "qjs.bp";

#[task(pool_size = 2)]
async fn launch_qjs(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    match super::run::submit_archive_name_to_target_prefer_trueosfs_async(
        target.clone(),
        QJS_ARCHIVE,
        app_args,
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            if submit_online_to_target(&spawner, target.clone(), alloc::vec![QJS_APP.into()])
                .is_err()
            {
                print_matrix_target_system_line(&target, "qjs: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("qjs: could not launch {QJS_ARCHIVE}: {error}").as_str(),
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
            "qjs: usage `qjs`; evaluate with Ctrl-Enter/F5, exit with ESC, Ctrl-Q, or :quit",
        );
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "qjs: no arguments expected; use `qjs --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    match launch_qjs(*spawner, target, Vec::new()) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "qjs: launch task unavailable"),
    }
    ParseOutcome::Handled
}
