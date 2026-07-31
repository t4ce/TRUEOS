use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::r::helio_game::{self, LaunchRequest};
use crate::shell2::shell2_cmd::ParseOutcome;

fn print_status(io: &'static dyn ShellBackend2) {
    let status = helio_game::status();
    print_shell_line(
        io,
        format!(
            "helio: state={} artifact=embedded:simple-cube.trueos.intel.helio bytes={} path=helioa-v1+render-ir-v1->render/guc->ui4",
            status.state.label(),
            status.artifact_bytes,
        )
        .as_str(),
    );
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (None, None) | (Some("start"), None) | (Some("run"), None) => {
            let message = match helio_game::request_launch() {
                LaunchRequest::Queued => "helio: simple-cube launch queued",
                LaunchRequest::AlreadyRequested => "helio: simple-cube launch already queued",
                LaunchRequest::AlreadyStarting => "helio: simple-cube is starting",
                LaunchRequest::AlreadyOnline => "helio: simple-cube is already online",
            };
            print_shell_line(io, message);
        }
        (Some("status"), None) => print_status(io),
        (Some("help" | "-h" | "--help"), None) => {
            print_shell_line(io, "helio: usage `helio [start|status]`");
        }
        _ => print_shell_line(io, "helio: expected start or status"),
    }
    ParseOutcome::Handled
}
