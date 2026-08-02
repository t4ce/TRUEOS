use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

const USAGE: &str = "dobby: usage `dobby [status|start|stop|reset|PROMPT]`; a prompt atomically stops the autonomous loop before it is queued";

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    let input = rest.trim();
    if input.is_empty() || input.eq_ignore_ascii_case("status") {
        let status = crate::r::remote_ai_service::status_text();
        print_shell_line(io, status.as_str());
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("help") || matches!(input, "-h" | "--help") {
        print_shell_line(io, USAGE);
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("start") {
        let _ = crate::r::remote_ai_service::start();
        let status = crate::r::remote_ai_service::status_text();
        print_shell_line(io, status.as_str());
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("stop") {
        let _ = crate::r::remote_ai_service::stop();
        let status = crate::r::remote_ai_service::status_text();
        print_shell_line(io, status.as_str());
        return ParseOutcome::Handled;
    }
    if input.eq_ignore_ascii_case("reset") {
        let _ = crate::r::remote_ai_service::reset();
        let status = crate::r::remote_ai_service::status_text();
        print_shell_line(io, status.as_str());
        return ParseOutcome::Handled;
    }

    match crate::r::remote_ai_service::submit_user_prompt(input) {
        Ok(_) => print_shell_line(
            io,
            "dobby: autonomous loop stopped; user prompt queued for a fresh remote turn",
        ),
        Err(reason) => {
            print_shell_line(io, alloc::format!("dobby: prompt rejected reason={reason}").as_str())
        }
    }
    ParseOutcome::Handled
}
