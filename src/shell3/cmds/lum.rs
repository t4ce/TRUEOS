use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const LUMEN_APP: &str = "lumen";
const LUMEN_ARCHIVE: &str = "lumen.bp";

#[task(pool_size = 2)]
async fn launch_lumen(spawner: Spawner, target: MatrixTarget) {
    match super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        LUMEN_ARCHIVE,
        Vec::new(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            if submit_online_to_target(
                &spawner,
                target.clone(),
                alloc::vec![String::from(LUMEN_APP)],
            )
            .is_err()
            {
                print_matrix_target_system_line(&target, "lum: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("lum: could not launch {LUMEN_ARCHIVE}: {error}").as_str(),
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
            "lum: usage `lum`; enter prompts after the Lumen Blueprint reports template ready",
        );
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(
            io,
            "lum: this is now an alias for the Lumen Blueprint; launch with `lum`, then enter prompts",
        );
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    match launch_lumen(*spawner, target) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "lum: launch task unavailable"),
    }
    ParseOutcome::Handled
}
