use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const GRID_APP: &str = "grid";
const GRID_ARCHIVE: &str = "grid.bp";

#[task(pool_size = 4)]
async fn launch_grid(spawner: Spawner, target: MatrixTarget, instance_name: Option<String>) {
    let instance = match instance_name.clone() {
        Some(name) => crate::hv::BlueprintInstanceRequest::named(name),
        None => crate::hv::BlueprintInstanceRequest::default(),
    };
    match super::run::submit_archive_name_to_target_prefer_trueosfs_with_instance_async(
        target.clone(),
        GRID_ARCHIVE,
        Vec::new(),
        instance,
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            // The online catalog spells a named instance `online new <app> <name>`.
            let online_args = match instance_name {
                Some(name) => alloc::vec![String::from("new"), String::from(GRID_APP), name,],
                None => alloc::vec![String::from(GRID_APP)],
            };
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "grid: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("grid: could not launch {GRID_ARCHIVE}: {error}").as_str(),
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
            "grid: usage `grid [INSTANCE-NAME]`; opens one buffered UI4 scene frame",
        );
        print_shell_line(
            io,
            "grid: plain `grid` claims the single shared default slot; name it to run more at once",
        );
        return ParseOutcome::Handled;
    }
    if trimmed.split_whitespace().count() > 1 {
        print_shell_line(io, "grid: expected at most one instance name");
        return ParseOutcome::Handled;
    }
    let instance_name = (!trimmed.is_empty()).then(|| String::from(trimmed));

    let target = matrix_target_for_backend(io);
    match launch_grid(*spawner, target, instance_name) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "grid: launch task unavailable"),
    }
    ParseOutcome::Handled
}
