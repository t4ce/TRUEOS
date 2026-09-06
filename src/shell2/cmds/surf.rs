use trueos_executor::Spawner;

use super::super::shell2_cmd::ParseOutcome;
use super::super::shell2_surf;
use super::super::{ShellBackend2, print_shell_line};

fn print_usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "usage: surf [url]");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let trimmed = rest.trim();
    if matches!(trimmed, "help" | "-h" | "--help") || trimmed.eq_ignore_ascii_case("help") {
        print_usage(io);
        return ParseOutcome::Handled;
    }

    shell2_surf::prepare_call_with_url(spawner, io, trimmed);

    ParseOutcome::Handled
}
