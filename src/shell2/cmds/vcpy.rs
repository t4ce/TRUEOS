use alloc::format;

use trueos_executor::Spawner;

use super::super::{print_shell_line, ShellBackend2};
use super::super::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vcpy start|stop|status");
}

pub(crate) fn try_parse(
    spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    rest: &str,
) -> ParseOutcome {
    let mut args = rest.split_whitespace();
    match (args.next(), args.next()) {
        (Some(action), None) if action.eq_ignore_ascii_case("start") => {
            match crate::r::services::vcpy_service::start(spawner) {
                Ok(()) => print_shell_line(io, "vcpy: service started (250 ms cadence)"),
                Err(reason) => print_shell_line(io, format!("vcpy: start failed: {reason}").as_str()),
            }
        }
        (Some(action), None) if action.eq_ignore_ascii_case("stop") => {
            crate::r::services::vcpy_service::stop();
            print_shell_line(io, "vcpy: stop requested");
        }
        (Some(action), None) if action.eq_ignore_ascii_case("status") => {
            let status = crate::r::services::vcpy_service::status();
            let demo = crate::ui4::vcpy_demo::status();
            print_shell_line(
                io,
                format!(
                    "vcpy: running={} polls={} failures={} rows={}/20 pending={} pinned={} copy_cadence_ms=250",
                    status.running as u8,
                    status.ticks,
                    status.failures,
                    demo.published_rows,
                    demo.pending as u8,
                    demo.pinned as u8,
                )
                .as_str(),
            );
        }
        _ => usage(io),
    }
    ParseOutcome::Handled
}
