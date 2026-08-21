use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const GRIDP_APP: &str = "gridp";
const GRIDP_ARCHIVE: &str = "gridp.bp";

#[task(pool_size = 8)]
async fn launch_gridp(spawner: Spawner, target: MatrixTarget) {
    match super::run::submit_archive_name_to_target_prefer_trueosfs_with_instance_async(
        target.clone(),
        GRIDP_ARCHIVE,
        Vec::new(),
        crate::hv::BlueprintInstanceRequest::default(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            if submit_online_to_target(
                &spawner,
                target.clone(),
                alloc::vec![alloc::string::String::from(GRIDP_APP)],
            )
            .is_err()
            {
                print_matrix_target_system_line(&target, "gridp: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("gridp: could not launch {GRIDP_ARCHIVE}: {error}").as_str(),
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
        print_shell_line(io, "gridp: plain `gridp` opens another movable UI4 frame");
        return ParseOutcome::Handled;
    }
    if !trimmed.is_empty() {
        print_shell_line(
            io,
            "gridp: launch arguments are not supported; configure it after launch",
        );
        return ParseOutcome::Handled;
    }
    let target = matrix_target_for_backend(io);
    match launch_gridp(*spawner, target) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "gridp: launch task unavailable"),
    }
    ParseOutcome::Handled
}
