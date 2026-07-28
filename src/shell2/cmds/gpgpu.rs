use core::str::SplitWhitespace;

use embassy_executor::Spawner;

use super::super::{ShellBackend2, print_shell_line};
use crate::shell2::shell2_cmd::ParseOutcome;

fn usage(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "gpgpu svg start [basic|curves|holes]");
    print_shell_line(io, "gpgpu svg status");
    print_shell_line(io, "gpgpu svg stop");
    print_shell_line(
        io,
        "gpgpu: legacy preview, lab256, copy-rect, and font-tessel shell routes were removed; use cpp",
    );
}

fn expect_no_more(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) -> bool {
    if args.next().is_none() {
        true
    } else {
        usage(io);
        false
    }
}

fn run_svg(io: &'static dyn ShellBackend2, args: &mut SplitWhitespace<'_>) {
    let Some(action) = args.next() else {
        usage(io);
        return;
    };
    if action.eq_ignore_ascii_case("start") {
        let demo = match args.next() {
            Some(raw) => match parse_svg_demo(raw) {
                Some(demo) => demo,
                None => {
                    usage(io);
                    return;
                }
            },
            None => crate::intel::gpgpu::SvgOutlineProbeDemo::Basic,
        };
        if !expect_no_more(io, args) {
            return;
        }
        let config = crate::ui4::GpgpuSvgProbeConfig { demo };
        match crate::ui4::request_gpgpu_svg_probe_start(config) {
            Ok(serial) => {
                let status = crate::ui4::gpgpu_svg_probe_status();
                print_shell_line(
                    io,
                    alloc::format!(
                        "gpgpu svg start: queued=1 request={} demo={} service_online={} ui4_consumer=retained-svg-outline frames=1 windows=1 buffering=double plane=universal-1 interaction=movable-fixed-size",
                        serial,
                        demo.label(),
                        status.online as u8,
                    )
                    .as_str(),
                );
            }
            Err(reason) => print_shell_line(
                io,
                alloc::format!("gpgpu svg start: queued=0 reason={reason}").as_str(),
            ),
        }
    } else if action.eq_ignore_ascii_case("status") {
        if expect_no_more(io, args) {
            print_svg_status(io);
        }
    } else if action.eq_ignore_ascii_case("stop") {
        if expect_no_more(io, args) {
            let serial = crate::ui4::request_gpgpu_svg_probe_stop();
            print_shell_line(
                io,
                alloc::format!("gpgpu svg stop: queued=1 request={serial}").as_str(),
            );
        }
    } else {
        usage(io);
    }
}

fn parse_svg_demo(raw: &str) -> Option<crate::intel::gpgpu::SvgOutlineProbeDemo> {
    if raw.eq_ignore_ascii_case("basic") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Basic)
    } else if raw.eq_ignore_ascii_case("curves") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Curves)
    } else if raw.eq_ignore_ascii_case("holes") {
        Some(crate::intel::gpgpu::SvgOutlineProbeDemo::Holes)
    } else {
        None
    }
}

fn print_svg_status(io: &'static dyn ShellBackend2) {
    let status = crate::ui4::gpgpu_svg_probe_status();
    print_shell_line(
        io,
        alloc::format!(
            "gpgpu svg status: online={} phase={} desired_running={} request={} applied={} demo={} frame={} window={} extent={}x{} attempted={} submitted={} published={} layers={} ops={} nonzero_pixels={} submit_ms={} buffering=double plane=universal-1 engine_ready_boundary=surflive error={}",
            status.online as u8,
            status.phase.label(),
            status.desired_running as u8,
            status.request_serial,
            status.applied_serial,
            status.config.demo.label(),
            status.frame.map(|frame| frame.raw()).unwrap_or(0),
            status.window.map(|window| window.raw()).unwrap_or(0),
            status.width,
            status.height,
            status.metrics.attempted,
            status.metrics.submitted,
            status.metrics.published,
            status.metrics.layers,
            status.metrics.ops,
            status.metrics.nonzero_pixels,
            status.metrics.submit_ms,
            status.last_error,
        )
        .as_str(),
    );
}

pub(crate) fn try_parse(
    _spawner: &Spawner,
    io: &'static dyn ShellBackend2,
    args: &mut SplitWhitespace<'_>,
) -> ParseOutcome {
    match args.next() {
        Some(cmd) if cmd.eq_ignore_ascii_case("svg") => run_svg(io, args),
        _ => usage(io),
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::parse_svg_demo;

    #[test]
    fn retained_svg_demos_remain_available() {
        for demo in ["basic", "curves", "holes"] {
            assert!(parse_svg_demo(demo).is_some());
        }
        assert!(parse_svg_demo("preview").is_none());
    }
}
