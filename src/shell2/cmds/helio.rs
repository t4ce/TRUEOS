use alloc::format;

use super::super::{ShellBackend2, print_shell_line};
use crate::r::helio_game::{self, LaunchRequest, StopRequest};
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StopTarget {
    Instance(u32),
    All,
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
    print_shell_line(io, "  probe 2|4             one launch-scoped ADL-S retained-GPU proof");
    print_shell_line(io, "  stop INSTANCE_ID        close one generated instance");
    print_shell_line(io, "  stop all                close every Helio instance");
    print_shell_line(
        io,
        "  monitor [SECONDS]    temporary Spirit 256x256 direct GPU logger (aliases: perf, logger)",
    );
    print_shell_line(
        io,
        "  pool soft cap: 10 independent instances; an 11th live launch evicts the oldest with a warning",
    );
    print_shell_line(io, "  focused Escape closes that Helio instance naturally");
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
    let pool = helio_game::status();
    let logger = crate::spirit::gpu_logger::status();
    let active = pool.instances.len().saturating_sub(pool.queued);
    print_shell_line(
        io,
        format!(
            "helio: instances={} active={} queued={} capacity={} cpu_carriers={} cpu_policy=instance-id-mod-carrier-count gpu_principal=render0 gpu_context=shared-single-render-runtime gpu_affinity=none policy=soft-cap-evict-oldest path=helioa-v1->render/guc->ui4 spirit_logger_active={} spirit_logger_source={} spirit_logger_remaining_ms={}",
            pool.instances.len(),
            active,
            pool.queued,
            pool.capacity,
            pool.cpu_carriers.len(),
            logger.active as u8,
            logger_source_name(logger.source),
            logger.remaining_ms,
        )
        .as_str(),
    );
    for carrier in &pool.cpu_carriers {
        print_shell_line(
            io,
            format!(
                "helio: cpu_carrier={} worker_slot={} core_kind={} placement=background-ap2+ gpu_principal=render0 gpu_context=shared-single-render-runtime gpu_affinity=none",
                carrier.id, carrier.worker_slot, carrier.core_kind,
            )
            .as_str(),
        );
    }
    for instance in &pool.instances {
        let slot = instance
            .slot
            .map(|slot| format!("{}", slot))
            .unwrap_or_else(|| "pending".into());
        let cpu_carrier = instance
            .cpu_carrier_id
            .map(|carrier| format!("{}", carrier))
            .unwrap_or_else(|| "pending".into());
        let worker_slot = instance
            .worker_slot
            .map(|worker_slot| format!("{}", worker_slot))
            .unwrap_or_else(|| "pending".into());
        let core_kind = instance
            .core_kind
            .map(|core_kind| format!("{}", core_kind))
            .unwrap_or_else(|| "pending".into());
        print_shell_line(
            io,
            format!(
                "helio: instance={} slot={} state={} example={}:{} retained_probe={} cpu_carrier={} worker_slot={} core_kind={} gpu_principal=render0 gpu_context=shared-single-render-runtime last_error={} artifact=embedded:{} bytes={}",
                instance.instance_id,
                slot,
                instance.state.label(),
                instance.example_id,
                example_name(instance.example_id),
                instance.retained_probe as u8,
                cpu_carrier,
                worker_slot,
                core_kind,
                instance.last_error.unwrap_or("none"),
                instance.artifact_name,
                instance.artifact_bytes,
            )
            .as_str(),
        );
    }
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

fn queued_launch_message(instance_id: u32, id: u8) -> alloc::string::String {
    format!("helio: instance={} example={}:{} launch queued", instance_id, id, example_name(id),)
}

fn eviction_message(instance_id: u32, stopping_instance_id: u32) -> alloc::string::String {
    format!(
        "helio: warning soft-cap={} reached; evicting oldest instance={} for new instance={}",
        helio_game::INSTANCE_CAPACITY,
        stopping_instance_id,
        instance_id,
    )
}

fn launch(io: &'static dyn ShellBackend2, id: u8) {
    match helio_game::request_launch(id) {
        LaunchRequest::Queued { instance_id } => {
            print_shell_line(io, queued_launch_message(instance_id, id).as_str())
        }
        LaunchRequest::Replacing {
            instance_id,
            stopping_instance_id,
        } => {
            print_shell_line(io, eviction_message(instance_id, stopping_instance_id).as_str());
            print_shell_line(io, queued_launch_message(instance_id, id).as_str());
        }
        LaunchRequest::Reserved => {
            print_shell_line(io, format!("helio: example {} is reserved", id).as_str())
        }
        LaunchRequest::ProbeUnsupported | LaunchRequest::ProbeBusy { .. } => {
            print_shell_line(io, "helio: internal launch-mode mismatch")
        }
    }
}

fn launch_probe(io: &'static dyn ShellBackend2, id: u8) {
    match helio_game::request_retained_probe_launch(id) {
        LaunchRequest::Queued { instance_id } => print_shell_line(
            io,
            format!(
                "helio: instance={} example={}:{} retained ADL-S GPU probe queued; launch-scoped; no production admission change",
                instance_id,
                id,
                example_name(id),
            )
            .as_str(),
        ),
        LaunchRequest::Replacing {
            instance_id,
            stopping_instance_id,
        } => {
            print_shell_line(io, eviction_message(instance_id, stopping_instance_id).as_str());
            print_shell_line(
                io,
                format!(
                    "helio: instance={} example={}:{} retained ADL-S GPU probe queued; launch-scoped",
                    instance_id,
                    id,
                    example_name(id),
                )
                .as_str(),
            );
        }
        LaunchRequest::ProbeBusy { instance_id } => print_shell_line(
            io,
            format!(
                "helio: retained GPU probe already active or stopping instance={}",
                instance_id,
            )
            .as_str(),
        ),
        LaunchRequest::ProbeUnsupported | LaunchRequest::Reserved => {
            print_shell_line(io, "helio: retained GPU probe supports only examples 2 and 4")
        }
    }
}

fn stop_message(instance_id: u32, outcome: StopRequest) -> alloc::string::String {
    match outcome {
        StopRequest::Stopping => format!("helio: instance={} stop queued", instance_id),
        StopRequest::AlreadyStopping => {
            format!("helio: instance={} is already stopping", instance_id)
        }
        StopRequest::CancelledQueued => {
            format!("helio: instance={} queued launch cancelled", instance_id)
        }
        StopRequest::NotFound => format!("helio: instance={} not found", instance_id),
    }
}

fn stop(io: &'static dyn ShellBackend2, instance_id: u32) {
    let message = stop_message(instance_id, helio_game::request_stop(instance_id));
    print_shell_line(io, message.as_str());
}

fn stop_all(io: &'static dyn ShellBackend2) {
    let count = helio_game::request_stop_all();
    print_shell_line(io, format!("helio: stop all queued count={}", count).as_str());
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

fn parse_instance_id(value: &str) -> Option<u32> {
    value.parse::<u32>().ok().filter(|id| *id != 0)
}

fn parse_stop_target(value: &str) -> Option<StopTarget> {
    if value == "all" {
        Some(StopTarget::All)
    } else {
        parse_instance_id(value).map(StopTarget::Instance)
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
                "helio: usage `helio [1|2|3|4|probe 2|probe 4|list|status|stop INSTANCE_ID|stop all|monitor [SECONDS|status|off]]`",
            );
            print_shell_line(
                io,
                "helio: examples 1=simple-cube, 2=churn-benchmark, 3=shape-battle-royale, 4=pendulum-bigcloth",
            );
            print_shell_line(
                io,
                "helio: monitor defaults to 30s, accepts 1..300s, aliases are perf and logger; Spirit 256x256 direct GPU logger bypasses UI4/composition and auto-restores",
            );
            print_shell_line(
                io,
                "helio: pool soft cap is 10 independent instances; an 11th live launch evicts the oldest with a warning; focused Escape closes one naturally",
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
        (Some("stop"), Some(target), None) if parse_stop_target(target).is_some() => {
            match parse_stop_target(target).unwrap() {
                StopTarget::Instance(instance_id) => stop(io, instance_id),
                StopTarget::All => stop_all(io),
            }
        }
        (Some("stop"), _, _) => {
            print_shell_line(io, "helio: usage `helio stop INSTANCE_ID` or `helio stop all`")
        }
        (Some("probe"), Some(id), None) if matches!(parse_id(id), Some(2 | 4)) => {
            launch_probe(io, parse_id(id).unwrap());
        }
        (Some("probe"), _, _) => {
            print_shell_line(io, "helio: usage `helio probe 2` or `helio probe 4`")
        }
        (Some("start" | "run"), None, None) => launch(io, 1),
        (Some("start" | "run"), Some(id), None) if parse_id(id).is_some() => {
            launch(io, parse_id(id).unwrap());
        }
        (Some(id), None, None) if parse_id(id).is_some() => launch(io, parse_id(id).unwrap()),
        _ => print_shell_line(
            io,
            "helio: expected 1, 2, 3, 4, probe, list, status, stop, or monitor/perf/logger",
        ),
    }
    ParseOutcome::Handled
}

#[cfg(test)]
mod tests {
    use super::{
        MONITOR_DEFAULT_SECONDS, MonitorCommand, StopTarget, eviction_message, example_name,
        parse_instance_id, parse_monitor_command, parse_stop_target, queued_launch_message,
        stop_message,
    };
    use crate::r::helio_game::StopRequest;

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

    #[test]
    fn generated_instance_ids_are_positive_u32_values() {
        assert_eq!(parse_instance_id("1"), Some(1));
        assert_eq!(parse_instance_id("4294967295"), Some(u32::MAX));
        for invalid in ["0", "-1", "4294967296", "three", "1.5"] {
            assert_eq!(parse_instance_id(invalid), None);
        }
    }

    #[test]
    fn stop_targets_accept_one_generated_id_or_all() {
        assert_eq!(parse_stop_target("23"), Some(StopTarget::Instance(23)));
        assert_eq!(parse_stop_target("all"), Some(StopTarget::All));
        for invalid in ["0", "-1", "everything", ""] {
            assert_eq!(parse_stop_target(invalid), None);
        }
    }

    #[test]
    fn launch_messages_expose_generated_and_evicted_instance_ids() {
        assert_eq!(
            queued_launch_message(23, 3),
            "helio: instance=23 example=3:shape-battle-royale launch queued"
        );
        assert_eq!(
            eviction_message(23, 7),
            "helio: warning soft-cap=10 reached; evicting oldest instance=7 for new instance=23"
        );
    }

    #[test]
    fn stop_messages_distinguish_every_instance_lifecycle_outcome() {
        assert_eq!(stop_message(23, StopRequest::Stopping), "helio: instance=23 stop queued");
        assert_eq!(
            stop_message(23, StopRequest::AlreadyStopping),
            "helio: instance=23 is already stopping"
        );
        assert_eq!(
            stop_message(23, StopRequest::CancelledQueued),
            "helio: instance=23 queued launch cancelled"
        );
        assert_eq!(stop_message(23, StopRequest::NotFound), "helio: instance=23 not found");
    }
}
