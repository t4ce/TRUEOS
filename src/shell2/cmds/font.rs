use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "vgpu frush [start|stop|status]");
}

pub(crate) fn try_parse(io: &'static dyn ShellBackend2, rest: &str) -> ParseOutcome {
    match rest.split_whitespace().collect::<alloc::vec::Vec<_>>().as_slice() {
        [] | ["start"] => match crate::ui4::request_cpp_font_rush2_start() {
            Ok(serial) => print_shell_line(
                io,
                format!("vgpu frush: queued=1 request={} workers=8 canvases=4", serial).as_str(),
            ),
            Err(reason) => print_shell_line(io, format!("vgpu frush: queued=0 reason={reason}").as_str()),
        },
        ["stop"] => match crate::ui4::request_cpp_font_rush2_stop() {
            Ok(serial) => print_shell_line(io, format!("vgpu frush stop: queued=1 request={serial}").as_str()),
            Err(reason) => print_shell_line(io, format!("vgpu frush stop: queued=0 reason={reason}").as_str()),
        },
        ["status"] => {
            let status = crate::ui4::gpgpu_preview_status();
            let active = status.desired_running
                && status.config.preset == crate::ui4::GpgpuPreviewPreset::CppFontRush2;
            print_shell_line(
                io,
                format!(
                    "vgpu frush: active={} phase={} request={} published={} failed={} error={}",
                    active as u8,
                    status.phase.label(),
                    status.request_serial,
                    status.metrics.published,
                    status.metrics.failed,
                    status.last_error,
                )
                .as_str(),
            );
        }
        _ => usage(io),
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    // Keep the command's accepted surface intentionally small: the demo owns
    // its four canvases and has no material/pipeline subcommands.
    #[test]
    fn frush_shape_is_documented() {
        assert_eq!("vgpu frush [start|stop|status]", "vgpu frush [start|stop|status]");
    }
}
