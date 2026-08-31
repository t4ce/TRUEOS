use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const AUD_APP: &str = "Player";
const AUD_ARCHIVE: &str = "Player.bp";

#[task(pool_size = 2)]
async fn launch_aud(spawner: Spawner, target: MatrixTarget) {
    match super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        AUD_ARCHIVE,
        alloc::vec::Vec::new(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let online_args = alloc::vec![alloc::string::String::from(AUD_APP)];
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "aud: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("aud: could not launch {AUD_ARCHIVE}: {error}").as_str(),
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
        print_shell_line(io, "aud: open the Player Blueprint terminal UI");
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(io, "aud: no arguments expected; use `aud --help`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    match launch_aud(*spawner, target) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "aud: launch task unavailable"),
    }
    ParseOutcome::Handled
}
