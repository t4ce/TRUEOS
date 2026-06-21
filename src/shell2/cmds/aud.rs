use embassy_executor::Spawner;

use super::super::{
    ShellBackend2, matrix_target_for_backend, print_matrix_target_line, print_shell_line,
    set_matrix_target_active,
};
use crate::shell2::shell2_cmd::ParseOutcome;

const AUD_PATH: &str = "aud.m4a";

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    if !rest.trim().is_empty() {
        print_shell_line(io, "aud: usage `aud`");
        return ParseOutcome::Handled;
    }

    let target = matrix_target_for_backend(io);
    let _ = spawner;
    print_matrix_target_line(&target, format!("aud: queued {}", AUD_PATH).as_str());
    set_matrix_target_active(&target, true);

    if let Err(err) = crate::aud::file_service::submit_default(target.clone()) {
        set_matrix_target_active(&target, false);
        print_matrix_target_line(&target, format!("aud: service {err}").as_str());
    }

    ParseOutcome::Handled
}
