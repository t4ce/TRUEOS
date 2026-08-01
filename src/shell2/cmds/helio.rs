use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::r::helio_game::{self, LaunchRequest};
use crate::shell2::shell2_cmd::ParseOutcome;

const MONITOR_DEFAULT_SECONDS: u64 = 30;
const MONITOR_MIN_SECONDS: u64 = 1;
const MONITOR_MAX_SECONDS: u64 = 300;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum MonitorCommand {
    Start(u64),
    Status,
    Off,
}

const fn example_name(id: u8) -> &'static str {
    match id {
        1 => "simple-cube",
        2 => "churn-benchmark",
        3 => "shape-battle-royale",
        4 => "pendulum-bigcloth",
        _ => "reserved",
    }
}

fn print_list(io: &'static dyn ShellBackend2) {
    print_shell_line(io, "helio examples:");
    print_shell_line(io, "  1  simple-cube       static full-stack smoke scene");
    print_shell_line(io, "  2  churn-benchmark   live retained-batch stress scene");
    print_shell_line(io, "  3  shape-battle-royale   physics arena scene");
    print_shell_line(io, "  4  pendulum-bigcloth     linked-cloth physics scene");
    print_shell_line(
        io,
        "  monitor [SECONDS]    temporary Spirit 256x256 direct GPU logger (aliases: perf, logger)",
    );
}

const fn logger_source_name(
    source: Option<crate::spirit::gpu_logger::GpuLoggerSource>,
) -> &'static str {
    match source {
        Some(crate::spirit::gpu_logger::GpuLoggerSource::Helio) => "helio",
        #[cfg(test)]
        Some(crate::spirit::gpu_logger::GpuLoggerSource::Other) => "other",
        None => "none",
    }
}

fn print_status(io: &'static dyn ShellBackend2) {
    let status = helio_game::status();
    let logger = crate::spirit::gpu_logger::status();
    let example = status
        .state
        .example_id()
        .map(|id| format!("{}:{}", id, example_name(id)))
        .unwrap_or_else(|| "none".into());
    print_shell_line(
        io,
        format!(
            "helio: state={} example={} last_error={} artifact=embedded:{} bytes={} path=helioa-v1->render/guc->ui4 spirit_logger_active={} spirit_logger_source={} spirit_logger_remaining_ms={}",
            status.state.label(),
            example,
            status.last_error.unwrap_or("none"),
            status.artifact_name,
            status.artifact_bytes,
            logger.active as u8,
            logger_source_name(logger.source),
            logger.remaining_ms,
        )
        .as_str(),
    );
}

fn parse_monitor_command(value: Option<&str>) -> Option<MonitorCommand> {
    match value {
        None => Some(MonitorCommand::Start(MONITOR_DEFAULT_SECONDS)),
        Some("status") => Some(MonitorCommand::Status),
        Some("off") => Some(MonitorCommand::Off),
        Some(raw) => raw.parse::<u64>().ok().and_then(|seconds| {
            (MONITOR_MIN_SECONDS..=MONITOR_MAX_SECONDS)
                .contains(&seconds)
                .then_some(MonitorCommand::Start(seconds))
        }),
    }
}

fn print_monitor_status(io: &'static dyn ShellBackend2) {
    let status = crate::spirit::gpu_logger::status();
    let sample =
        crate::spirit::gpu_logger::latest_sample(crate::spirit::gpu_logger::GpuLoggerSource::Helio);
    print_shell_line(
        io,
        format!(
            "helio monitor: active={} source={} generation={} remaining_ms={} sample_source=helio frame={} cadence_us={} fps={} frame_us={} geometry_us={} prepare_us={} retire_wait_us={} poll_iters={} objects={} draws={} triangles={} busy_retries={} incomplete_retries={}; Spirit 256x256 direct GPU logger; temporary; bypasses UI4/composition; auto-restores",
            status.active as u8,
            logger_source_name(status.source),
            status.generation,
            status.remaining_ms,
            sample.frame_index,
            sample.cadence_us,
            crate::spirit::gpu_logger::fps_from_cadence_us(sample.cadence_us),
            sample.frame_us,
            sample.geometry_us,
            sample.prepare_us,
            sample.retire_wait_us,
            sample.poll_iters,
            sample.objects,
            sample.draws,
            sample.triangles,
            sample.busy_retries,
            sample.incomplete_retries,
        )
        .as_str(),
    );
}

fn monitor(io: &'static dyn ShellBackend2, command: MonitorCommand) {
    use crate::spirit::gpu_logger::{self, GpuLoggerLease, GpuLoggerSource};

    match command {
        MonitorCommand::Start(seconds) => {
            let duration_ms = seconds.saturating_mul(1_000);
            match gpu_logger::request(GpuLoggerSource::Helio, duration_ms) {
                Ok(lease) => print_shell_line(
                    io,
                    format!(
                        "helio monitor: started=1 source=helio generation={} duration={}s; Spirit 256x256 direct GPU logger; temporary; bypasses UI4/composition; auto-restores",
                        lease.generation, seconds,
                    )
                    .as_str(),
                ),
                Err(busy) => print_shell_line(
                    io,
                    format!(
                        "helio monitor: started=0 busy_source={} busy_generation={} remaining_ms={}; Spirit 256x256 direct GPU logger remains temporary, bypasses UI4/composition, and auto-restores",
                        logger_source_name(Some(busy.active_source)),
                        busy.active_generation,
                        busy.remaining_ms,
                    )
                    .as_str(),
                ),
            }
        }
        MonitorCommand::Status => print_monitor_status(io),
        MonitorCommand::Off => {
            let current = gpu_logger::status();
            let stopped = current.active
                && current.source == Some(GpuLoggerSource::Helio)
                && gpu_logger::release(GpuLoggerLease {
                    generation: current.generation,
                    source: GpuLoggerSource::Helio,
                });
            print_shell_line(
                io,
                format!(
                    "helio monitor: stopped={}; Spirit 256x256 direct GPU logger released for Helio; normal Spirit presentation restored (or unchanged); automatic expiry remains the fallback",
                    stopped as u8,
                )
                .as_str(),
            );
        }
    }
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
            print_shell_line(
                io,
                "helio: usage `helio [1|2|3|4|list|status|monitor [SECONDS|status|off]]`",
            );
            print_shell_line(
                io,
                "helio: examples 1=simple-cube, 2=churn-benchmark, 3=shape-battle-royale, 4=pendulum-bigcloth",
            );
            print_shell_line(
                io,
                "helio: monitor defaults to 30s, accepts 1..300s, aliases are perf and logger; Spirit 256x256 direct GPU logger bypasses UI4/composition and auto-restores",
            );
        }
        (Some("monitor" | "perf" | "logger"), value, None) => {
            if let Some(command) = parse_monitor_command(value) {
                monitor(io, command);
            } else {
                print_shell_line(
                    io,
                    "helio monitor: expected SECONDS (1..300), status, or off; Spirit 256x256 direct GPU logger is temporary and auto-restores",
                );
            }
        }
        (Some("start" | "run"), None, None) => launch(io, 1),
        (Some("start" | "run"), Some(id), None) if parse_id(id).is_some() => {
            launch(io, parse_id(id).unwrap());
        }
        (Some(id), None, None) if parse_id(id).is_some() => launch(io, parse_id(id).unwrap()),
        _ => {
            print_shell_line(io, "helio: expected 1, 2, 3, 4, list, status, or monitor/perf/logger")
        }
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{MONITOR_DEFAULT_SECONDS, MonitorCommand, example_name, parse_monitor_command};

    #[test]
    fn all_numbered_example_slots_have_names() {
        assert_eq!(example_name(1), "simple-cube");
        assert_eq!(example_name(2), "churn-benchmark");
        assert_eq!(example_name(3), "shape-battle-royale");
        assert_eq!(example_name(4), "pendulum-bigcloth");
        assert_eq!(example_name(5), "reserved");
    }

    #[test]
    fn monitor_defaults_to_a_bounded_thirty_second_lease() {
        assert_eq!(
            parse_monitor_command(None),
            Some(MonitorCommand::Start(MONITOR_DEFAULT_SECONDS))
        );
        assert_eq!(parse_monitor_command(Some("1")), Some(MonitorCommand::Start(1)));
        assert_eq!(parse_monitor_command(Some("300")), Some(MonitorCommand::Start(300)));
    }

    #[test]
    fn monitor_parses_lifecycle_actions_and_rejects_unbounded_durations() {
        assert_eq!(parse_monitor_command(Some("status")), Some(MonitorCommand::Status));
        assert_eq!(parse_monitor_command(Some("off")), Some(MonitorCommand::Off));
        for invalid in ["0", "301", "-1", "forever", "1.5"] {
            assert_eq!(parse_monitor_command(Some(invalid)), None);
        }
    }
}
