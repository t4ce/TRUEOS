use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;
use trueos_executor::Spawner;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum WinAction {
    Start,
    Status,
    Stop,
}

fn parse_action(input: &str) -> Option<WinAction> {
    match input.trim().to_ascii_lowercase().as_str() {
        "" | "start" => Some(WinAction::Start),
        "status" => Some(WinAction::Status),
        "stop" => Some(WinAction::Stop),
        _ => None,
    }
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    input: &str,
) -> ParseOutcome {
    match parse_action(input) {
        Some(WinAction::Start) => match crate::ui4::request_win_demo_start() {
            Ok(serial) => print_shell_line(io, alloc::format!(
                "win: queued=1 request={} windows=30 retained=1 controls=Escape stop=\"win stop\"", serial).as_str()),
            Err(reason) => print_shell_line(io, alloc::format!("win: queued=0 reason={}", reason).as_str()),
        },
        Some(WinAction::Status) => {
            let status = crate::ui4::gpgpu_preview_status();
            let active = status.desired_running && status.config.preset == crate::ui4::GpgpuPreviewPreset::Static30;
            print_shell_line(io, alloc::format!(
                "win: active={} phase={} request={} published={} failed={} error={}",
                active as u8, status.phase.label(), status.request_serial,
                status.metrics.published, status.metrics.failed, status.last_error).as_str());
        }
        Some(WinAction::Stop) => {
            let status = crate::ui4::gpgpu_preview_status();
            if status.desired_running && status.config.preset == crate::ui4::GpgpuPreviewPreset::Static30 {
                let serial = crate::ui4::request_gpgpu_preview_stop();
                print_shell_line(io, alloc::format!("win stop: queued=1 request={}", serial).as_str());
            } else { print_shell_line(io, "win stop: queued=0 reason=no-win-demo-active"); }
        }
        None => print_shell_line(io, "win [start|status|stop] — 30 retained UI4 windows; Escape closes"),
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn win_has_only_window_demo_actions() {
        assert_eq!(parse_action(""), Some(WinAction::Start));
        assert_eq!(parse_action(" STATUS "), Some(WinAction::Status));
        assert_eq!(parse_action("stop"), Some(WinAction::Stop));
        for input in ["gallery", "font", "audio", "spirit", "svg", "start extra"] {
            assert_eq!(parse_action(input), None);
        }
    }
}
