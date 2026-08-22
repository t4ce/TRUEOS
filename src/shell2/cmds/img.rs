//! Resident UI4 image viewer Blueprint launcher.
//!
//! `img` without arguments opens its VMX-minishell.  Supplying a path makes
//! that the first `show` command; the Blueprint remains alive afterwards so
//! further media can be opened without another VM launch.

use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const IMG_APP: &str = "img";
const IMG_ARCHIVE: &str = "img.bp";

#[task(pool_size = 2)]
async fn launch_img(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    match super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        IMG_ARCHIVE,
        app_args.clone(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let mut online_args = Vec::with_capacity(app_args.len().saturating_add(1));
            online_args.push(String::from(IMG_APP));
            online_args.extend(app_args);
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "img: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("img: could not launch {IMG_ARCHIVE}: {error}").as_str(),
        ),
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let args = rest.split_whitespace().map(String::from).collect();
    match launch_img(*spawner, matrix_target_for_backend(io), args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "img: launch task unavailable"),
    }
    ParseOutcome::Handled
}
