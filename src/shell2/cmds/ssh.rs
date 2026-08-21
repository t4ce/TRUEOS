//! Real SSH lives in the `ssh.bp` terminal Blueprint.
//!
//! Shell2 only owns command parsing and launch placement.  The Blueprint owns
//! SSH-2 transport, host-key verification, authentication, PTY channels, and
//! the interactive byte stream.  Launching it through the ordinary terminal
//! Blueprint path also reuses the existing VM/TUI handoff and resize logic.

use alloc::string::String;
use alloc::vec::Vec;

use trueos_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line, submit_online_to_target,
};

const SSH_APP: &str = "ssh";
const SSH_ARCHIVE: &str = "ssh.bp";

#[task(pool_size = 2)]
async fn launch_ssh(spawner: Spawner, target: MatrixTarget, app_args: Vec<String>) {
    match super::run::submit_archive_name_to_target_from_app_db_async(
        target.clone(),
        SSH_ARCHIVE,
        app_args.clone(),
    )
    .await
    {
        Ok(_) => {}
        Err(error) if error == "archive not found" => {
            let mut online_args = Vec::with_capacity(app_args.len().saturating_add(1));
            online_args.push(String::from(SSH_APP));
            online_args.extend(app_args);
            if submit_online_to_target(&spawner, target.clone(), online_args).is_err() {
                print_matrix_target_system_line(&target, "ssh: online launch task unavailable");
            }
        }
        Err(error) => print_matrix_target_system_line(
            &target,
            alloc::format!("ssh: could not launch {SSH_ARCHIVE}: {error}").as_str(),
        ),
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let args: Vec<String> = rest.split_whitespace().map(String::from).collect();

    if args.is_empty() {
        print_shell_line(io, "ssh: usage `ssh [user@]host[:port]`");
        return ParseOutcome::Handled;
    }

    match launch_ssh(*spawner, matrix_target_for_backend(io), args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "ssh: launch task unavailable"),
    }
    ParseOutcome::Handled
}
