use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::r::helio_game::{self, LaunchRequest};
use crate::shell2::shell2_cmd::ParseOutcome;

const fn example_name(id: u8) -> &'static str {
    match id {
        1 => "simple-cube",
        2 => "churn-benchmark",
        _ => "reserved",
    }
}

fn print_list(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "helio examples:");
    print_shell_line(io, "  1  simple-cube       static full-stack smoke scene");
    print_shell_line(io, "  2  churn-benchmark   live retained-batch stress scene");
    print_shell_line(io, "  3  reserved");
    print_shell_line(io, "  4  reserved");
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = helio_game::status();
    let example = status
        .state
        .example_id()
        .map(|id| format!("{}:{}", id, example_name(id)))
        .unwrap_or_else(|| "none".into());
    print_shell_line(
        io,
        format!(
            "helio: state={} example={} last_error={} artifact=embedded:simple-cube.trueos.intel.helio bytes={} path=helioa-v1->render/guc->ui4",
            status.state.label(),
            example,
            status.last_error.unwrap_or("none"),
            status.artifact_bytes,
        )
        .as_str(),
    );
}

fn launch(io: &'static dyn ShellBackend2, id: u8) {
    let selected = format!("{}:{}", id, example_name(id));
    let message = match helio_game::request_launch(id) {
        LaunchRequest::Queued => format!("helio: example {} launch queued", selected),
        LaunchRequest::AlreadyRequested(active) => format!(
            "helio: example {} already queued (requested {})",
            selected,
            example_name(active)
        ),
        LaunchRequest::AlreadyStarting(active) => format!(
            "helio: example {} cannot start; {} is starting",
            selected,
            example_name(active)
        ),
        LaunchRequest::AlreadyOnline(active) => format!(
            "helio: example {} cannot start; {} is already online",
            selected,
            example_name(active)
        ),
        LaunchRequest::Reserved => format!("helio: example {} is reserved", id),
    };
    print_shell_line(io, message.as_str());
}

fn parse_id(value: &str) -> Option<u8> {
    match value {
        "1" => Some(1),
        "2" => Some(2),
        "3" => Some(3),
        "4" => Some(4),
        _ => None,
    }
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    let first = args.next();
    let second = args.next();
    let third = args.next();
    match (first, second, third) {
        (None, None, None) => launch(io, 1),
        (Some("status"), None, None) => print_status(io),
        (Some("list"), None, None) => print_list(io),
        (Some("help" | "-h" | "--help"), None, None) => {
            print_shell_line(io, "helio: usage `helio [1|2|3|4|list|status]`");
        }
        (Some("start" | "run"), None, None) => launch(io, 1),
        (Some("start" | "run"), Some(id), None) if parse_id(id).is_some() => {
            launch(io, parse_id(id).unwrap());
        }
        (Some(id), None, None) if parse_id(id).is_some() => launch(io, parse_id(id).unwrap()),
        _ => print_shell_line(io, "helio: expected 1, 2, 3, 4, list, or status"),
    }
    ParseOutcome::Handled
}
