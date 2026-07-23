use alloc::string::String;
use alloc::vec::Vec;

use embassy_executor::{Spawner, task};

use super::super::shell2_cmd::ParseOutcome;
use super::super::{
    MatrixTarget, ShellBackend2, matrix_target_for_backend, print_matrix_target_system_line,
    print_shell_line,
};

const SSHD_ARCHIVE: &str = "sshd.bp";

#[task]
async fn launch_sshd(target: MatrixTarget, args: Vec<String>) {
    if let Err(error) = super::run::submit_archive_name_to_target_prefer_trueosfs_async(
        target.clone(),
        SSHD_ARCHIVE,
        args,
    )
    .await
    {
        print_matrix_target_system_line(
            &target,
            alloc::format!("sshd: could not launch {SSHD_ARCHIVE}: {error}").as_str(),
        );
    }
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let args: Vec<String> = rest.split_whitespace().map(String::from).collect();
    if args
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "help" | "-h" | "--help"))
    {
        print_shell_line(io, "sshd: `sshd authorize <OpenSSH-public-key>`; then `sshd` (port 22)");
        return ParseOutcome::Handled;
    }
    match launch_sshd(matrix_target_for_backend(io), args) {
        Ok(token) => spawner.spawn(token),
        Err(_) => print_shell_line(io, "sshd: launch task unavailable"),
    }
    ParseOutcome::Handled
}
