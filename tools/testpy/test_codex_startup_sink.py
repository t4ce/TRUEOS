#!/usr/bin/env python3
"""Exercise the production startup timing crate and arg0 reexport with host std.

Each sink configuration runs in a fresh process. A capturing TRUEOS logger
avoids tracing, Tokio, network access, and changes to the production sources.
"""

import json
import math
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
CODEX = Path(os.environ.get("TRUEOS_CODEX_ROOT", ROOT.parent / "codex"))
STARTUP_TIMING = CODEX / "codex-rs/arg0/src/startup_timing.rs"
SHARED_TIMING = CODEX / "codex-rs/startup-timing/src/lib.rs"

HARNESS = r'''#[path = @STARTUP_TIMING@] mod startup_timing;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Context, Poll, Wake, Waker};
use std::time::Duration;

static CAPTURE: Mutex<Vec<(u32, String, String)>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
extern "C" fn trueos_cabi_log(
    level: u32,
    target_ptr: *const u8,
    target_len: usize,
    message_ptr: *const u8,
    message_len: usize,
) -> i32 {
    let target = unsafe { std::slice::from_raw_parts(target_ptr, target_len) };
    let message = unsafe { std::slice::from_raw_parts(message_ptr, message_len) };
    CAPTURE.lock().unwrap().push((
        level,
        String::from_utf8_lossy(target).into_owned(),
        String::from_utf8_lossy(message).into_owned(),
    ));
    std::env::var("HARNESS_LOG_ERRNO")
        .ok().and_then(|value| value.parse().ok()).unwrap_or(0)
}

#[derive(Default)]
struct WakeCounter(AtomicUsize);
impl Wake for WakeCounter {
    fn wake(self: Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
    fn wake_by_ref(self: &Arc<Self>) { self.0.fetch_add(1, Ordering::SeqCst); }
}

struct PendingOnce(bool);
impl Future for PendingOnce {
    type Output = usize;
    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<usize> {
        if self.0 {
            Poll::Ready(17)
        } else {
            self.0 = true;
            context.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn exercise_spans() {
    let wakes = Arc::new(WakeCounter::default());
    let waker = Waker::from(Arc::clone(&wakes));
    let mut context = Context::from_waker(&waker);
    for value in [3, 9] {
        let mut future = Box::pin(startup_timing::measure("repeat_phase", std::future::ready(value)));
        assert_eq!(future.as_mut().poll(&mut context), Poll::Ready(value));
    }
    let mut pending = Box::pin(startup_timing::measure("pending_phase", PendingOnce(false)));
    assert!(pending.as_mut().poll(&mut context).is_pending());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    // This delay is outside poll: it must contribute to wall time, not poll time.
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(pending.as_mut().poll(&mut context), Poll::Ready(17));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let mut blocking = Box::pin(startup_timing::measure("blocking_phase", async {
        std::thread::sleep(Duration::from_millis(20));
        23
    }));
    assert_eq!(blocking.as_mut().poll(&mut context), Poll::Ready(23));
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let mut cancelled = Box::pin(startup_timing::measure("cancelled_phase", std::future::pending::<()>()));
    assert!(cancelled.as_mut().poll(&mut context).is_pending());
    drop(cancelled);
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    let phase: startup_timing::Span = startup_timing::span("direct_phase");
    phase.finish("ok");
    drop(startup_timing::span("dropped_phase"));
}

fn main() {
    // No tracing dependency or subscriber is linked into this harness.
    startup_timing::mark("arg0_entry");
    startup_timing::mark("arg0_entry");
    match std::env::args().nth(1).as_deref() {
        Some("concurrent") => {
            let workers: Vec<_> = (0..8).map(|index| {
                std::thread::spawn(move || {
                    for _ in 0..32 {
                        startup_timing::mark("worker_shared");
                        startup_timing::mark(&format!("worker_{index}"));
                    }
                })
            }).collect();
            for worker in workers { worker.join().unwrap(); }
        }
        Some("span_concurrent") => {
            let workers: Vec<_> = (0..8).map(|_| {
                std::thread::spawn(|| {
                    for _ in 0..4 { startup_timing::span("worker_phase").finish("ok"); }
                })
            }).collect();
            for worker in workers { worker.join().unwrap(); }
        }
        Some("spans") => exercise_spans(),
        Some("budget") => {
            // Invalid names must not consume the bounded admission budget.
            for _ in 0..300 { startup_timing::span("invalid budget label").finish("ok"); }
            for _ in 0..300 { startup_timing::span("budget_phase").finish("ok"); }
            startup_timing::mark("after_budget");
        }
        Some("invalid") => {
            for name in ["", "private label secret", "private/path", "bad\nline", "this_label_exceeds_the_ninety_six_byte_bound_and_must_not_be_logged_even_though_it_only_contains_identifier_characters"] {
                startup_timing::span(name).finish("ok");
            }
            startup_timing::span("invalid_outcome_phase").finish("private outcome secret");
        }
        _ => {}
    }
    startup_timing::mark("runtime_done");
    startup_timing::mark("runtime_done");
    for (level, target, message) in CAPTURE.lock().unwrap().iter() {
        println!("{level}\t{target}\t{message}");
    }
}
'''


def validate_timestamps(rows: list[str]) -> list[str]:
    stages = []
    previous_elapsed = 0.0
    process_id = None
    for row in rows:
        pid, elapsed, delta, stage = row.split("\t")
        assert int(pid) > 0, row
        if process_id is None:
            process_id = pid
        assert process_id == pid, row
        elapsed_ms, delta_ms = float(elapsed), float(delta)
        assert math.isfinite(elapsed_ms) and math.isfinite(delta_ms), row
        assert elapsed_ms >= previous_elapsed and delta_ms >= 0, row
        assert math.isclose(
            elapsed_ms - previous_elapsed, delta_ms, abs_tol=0.002
        ), row
        previous_elapsed = elapsed_ms
        stages.append(stage)
    return stages


def validate_rows(rows: list[str], expected: set[str]) -> None:
    stages = validate_timestamps(rows)
    assert len(stages) == len(set(stages)), f"duplicate milestone: {stages}"
    assert set(stages) == expected, (stages, expected)


def validate_spans(
    rows: list[str], extra_milestones: set[str] | None = None
) -> list[dict[str, str]]:
    active = {}
    finished = []
    seen_ids = set()
    milestones = []
    for stage in validate_timestamps(rows):
        kind, _, fields_text = stage.partition(" ")
        if kind not in {"span_begin", "span_end"}:
            milestones.append(stage)
            continue
        fields = dict(field.split("=", 1) for field in fields_text.split())
        identifier = int(fields["id"])
        assert identifier > 0, fields
        if kind == "span_begin":
            assert set(fields) == {"id", "name"}, fields
            assert identifier not in seen_ids, fields
            seen_ids.add(identifier)
            active[identifier] = fields["name"]
            continue
        assert active.pop(identifier) == fields["name"], fields
        assert set(fields) == {
            "id", "name", "outcome", "duration_ms", "poll_n", "poll_total_ms", "poll_max_ms"
        }, fields
        duration, total, maximum = [float(fields[key]) for key in
                                    ["duration_ms", "poll_total_ms", "poll_max_ms"]]
        assert all(math.isfinite(value) and value >= 0 for value in [duration, total, maximum])
        assert maximum <= total + 0.002 <= duration + 0.004, fields
        assert int(fields["poll_n"]) >= 0, fields
        finished.append(fields)
    assert active == {}, active
    expected_milestones = {"arg0_entry", "runtime_done"} | (extra_milestones or set())
    assert set(milestones) == expected_milestones, milestones
    assert len(milestones) == len(expected_milestones), milestones
    assert [stage for stage in milestones if stage in {"arg0_entry", "runtime_done"}] == [
        "arg0_entry", "runtime_done"
    ], milestones
    assert milestones[-1] == "runtime_done", milestones
    return finished


def validate_future_cases(
    rows: list[str], extra_milestones: set[str] | None = None
) -> None:
    records = validate_spans(rows, extra_milestones)
    by_name = {}
    for record in records:
        by_name.setdefault(record["name"], []).append(record)
    assert set(by_name) == {
        "repeat_phase", "pending_phase", "blocking_phase", "cancelled_phase",
        "direct_phase", "dropped_phase"
    }, by_name
    assert len(by_name["repeat_phase"]) == 2, by_name
    assert all(record["outcome"] == "complete" and record["poll_n"] == "1"
               for record in by_name["repeat_phase"])
    pending = by_name["pending_phase"][0]
    assert pending["poll_n"] == "2" and pending["outcome"] == "complete", pending
    # The known sleep outside polling is present even if the host deschedules a poll.
    assert float(pending["duration_ms"]) - float(pending["poll_total_ms"]) >= 20, pending
    blocking = by_name["blocking_phase"][0]
    assert blocking["poll_n"] == "1" and blocking["outcome"] == "complete", blocking
    assert float(blocking["poll_max_ms"]) >= 15, blocking
    cancelled = by_name["cancelled_phase"][0]
    assert cancelled["outcome"] == "cancelled" and cancelled["poll_n"] == "1", cancelled
    assert by_name["direct_phase"][0]["outcome"] == "ok", by_name
    assert by_name["dropped_phase"][0]["outcome"] == "cancelled", by_name
    assert by_name["dropped_phase"][0]["poll_n"] == "0", by_name


def validate_request_tags(rows: list[str], expected_request: int | None) -> list[str]:
    """Require launch correlation on every row, then remove only its final tag."""
    normalized = []
    for row in rows:
        fields = row.split("\t")
        assert len(fields) == 4, row
        stage = fields[3]
        if expected_request is None:
            assert not any(field.startswith("request=") for field in stage.split()), row
        else:
            suffix = f" request={expected_request}"
            assert stage.endswith(suffix), row
            fields[3] = stage.removesuffix(suffix)
        normalized.append("\t".join(fields))
    return normalized


def run_case(executable: Path, env: dict[str, str], scenario: str | None = None) -> list[str]:
    arguments = [str(executable)] + ([scenario] if scenario else [])
    result = subprocess.run(
        arguments, env=env, capture_output=True, text=True, check=True, timeout=10
    )
    assert result.stderr == "", result.stderr
    rows = []
    for line in result.stdout.splitlines():
        level, target, message = line.split("\t", 2)
        assert level == "3", line
        assert target == "codex_startup", line
        prefix = "[codex-startup] "
        assert message.startswith(prefix), line
        rows.append(message.removeprefix(prefix))
    return rows


def main() -> None:
    for path in [STARTUP_TIMING, SHARED_TIMING]:
        if not path.is_file():
            raise SystemExit(f"missing production source: {path}")
    with tempfile.TemporaryDirectory(prefix="trueos-codex-startup-sink-") as temporary:
        directory = Path(temporary)
        source = directory / "harness.rs"
        source.write_text(
            HARNESS.replace("@STARTUP_TIMING@", json.dumps(str(STARTUP_TIMING)))
        )
        executable = directory / "harness"
        library = directory / "libcodex_startup_timing.rlib"
        compile_flags = ["rustc", "--edition=2024", "--cfg", 'target_os="trueos"',
                         "-Aexplicit_builtin_cfgs_in_flags"]
        subprocess.run(
            compile_flags + ["--crate-type=rlib", "--crate-name=codex_startup_timing",
                             str(SHARED_TIMING), "-o", str(library)],
            check=True, timeout=60,
        )
        subprocess.run(
            compile_flags + [str(source), "--extern", f"codex_startup_timing={library}",
                             "-o", str(executable)],
            check=True, timeout=60,
        )
        env = dict(os.environ)
        for name in [
            "CODEX_STARTUP_TIMING_FILE", "CODEX_STARTUP_TIMING_STDERR", "HARNESS_LOG_ERRNO",
            "TRUEOS_STARTUP_REQUEST", "TRUEOS_APP_GENERATION", "TRUEOS_APP_INSTANCE",
            "TRUEOS_APP_SHA256",
        ]:
            env.pop(name, None)
        stages = {"arg0_entry", "runtime_done"}

        assert run_case(executable, env) == []
        assert run_case(executable, env | {"CODEX_STARTUP_TIMING_STDERR": "0"}) == []
        assert run_case(executable, env, "spans") == []
        assert run_case(executable, env, "budget") == []

        logger_env = env | {"CODEX_STARTUP_TIMING_STDERR": "1"}
        validate_rows(run_case(executable, logger_env), stages)
        validate_rows(
            run_case(executable, logger_env | {"HARNESS_LOG_ERRNO": "5"}), stages
        )
        concurrent_stages = stages | {"worker_shared"} | {
            f"worker_{index}" for index in range(8)
        }
        validate_rows(run_case(executable, logger_env, "concurrent"), concurrent_stages)
        validate_future_cases(run_case(executable, logger_env, "spans"))
        validate_future_cases(run_case(executable, logger_env | {"HARNESS_LOG_ERRNO": "5"}, "spans"))
        concurrent_spans = validate_spans(run_case(executable, logger_env, "span_concurrent"))
        assert len(concurrent_spans) == 32, concurrent_spans
        assert all(record["name"] == "worker_phase" and record["outcome"] == "ok"
                   for record in concurrent_spans), concurrent_spans
        invalid_spans = validate_spans(run_case(executable, logger_env, "invalid"))
        assert len(invalid_spans) == 1, invalid_spans
        assert invalid_spans[0]["name"] == "invalid_outcome_phase", invalid_spans
        assert invalid_spans[0]["outcome"] == "invalid_outcome", invalid_spans
        budget_spans = validate_spans(run_case(executable, logger_env, "budget"), {
            "timing_span_budget_exhausted", "after_budget"
        })
        assert len(budget_spans) == 256, len(budget_spans)
        assert {int(record["id"]) for record in budget_spans} == set(range(1, 257))
        assert all(record["name"] == "budget_phase" and record["outcome"] == "ok"
                   for record in budget_spans), budget_spans

        valid_context = logger_env | {
            "TRUEOS_STARTUP_REQUEST": "42", "TRUEOS_APP_GENERATION": "7",
            "TRUEOS_APP_INSTANCE": "01234567-89ab-cdef-0123-456789abcdef",
            "TRUEOS_APP_SHA256": "ABCDEF01" * 8,
            "UNRELATED_PRIVATE_CONTEXT": "must-not-appear",
        }
        context_stage = ("process_context request=42 app_gen=7 "
                         "instance=01234567-89ab-cdef-0123-456789abcdef "
                         f"archive_sha256={'abcdef01' * 8}")
        context_rows = validate_request_tags(run_case(executable, valid_context), 42)
        validate_rows(context_rows, stages | {context_stage})
        assert context_rows[0].endswith(context_stage), context_rows
        padded_context_rows = run_case(executable, logger_env | {
            "TRUEOS_STARTUP_REQUEST": "0" * 4096 + "42",
            "TRUEOS_APP_GENERATION": "0" * 4096 + "7",
        })
        assert max(map(len, padded_context_rows)) < 256, "numeric context was not bounded"
        validate_rows(validate_request_tags(padded_context_rows, 42), stages | {
            "process_context request=42 app_gen=7"
        })
        # Span begin/end records must remain attributable when native processes share pid=1.
        validate_future_cases(
            validate_request_tags(run_case(
                executable, logger_env | {"TRUEOS_STARTUP_REQUEST": "42"}, "spans"
            ), 42),
            {"process_context request=42"},
        )
        invalid_context = logger_env | {
            "TRUEOS_STARTUP_REQUEST": "secret\nrequest", "TRUEOS_APP_GENERATION": "18446744073709551616",
            "TRUEOS_APP_INSTANCE": "private-instance", "TRUEOS_APP_SHA256": "X" * 64,
            "UNRELATED_PRIVATE_CONTEXT": "must-not-appear", "OPENAI_API_KEY": "private-api-key",
        }
        validate_rows(validate_request_tags(run_case(executable, invalid_context), None), stages)
        boundary_context = logger_env | {
            "TRUEOS_STARTUP_REQUEST": "18446744073709551615", "TRUEOS_APP_GENERATION": "0",
        }
        validate_rows(validate_request_tags(
            run_case(executable, boundary_context), 18446744073709551615
        ), stages | {
            "process_context request=18446744073709551615 app_gen=0"
        })
        validate_rows(validate_request_tags(run_case(
            executable, logger_env | {"TRUEOS_STARTUP_REQUEST": "0"}
        ), 0), stages | {"process_context request=0"})
        # Other validated context survives even when the launch request is missing or malformed.
        for request in [None, "", "private request", "-1", "1e2", " 42", "42 ",
                        "18446744073709551616"]:
            sha_context = logger_env | {"TRUEOS_APP_SHA256": "ABCDEF01" * 8}
            if request is not None:
                sha_context["TRUEOS_STARTUP_REQUEST"] = request
            validate_rows(validate_request_tags(
                run_case(executable, sha_context), None
            ), stages | {f"process_context archive_sha256={'abcdef01' * 8}"})

        timing_file = directory / "startup.tsv"
        timing_file.write_text("existing rows\n")
        assert run_case(
            executable, env | {"CODEX_STARTUP_TIMING_FILE": str(timing_file)}
        ) == []
        file_lines = timing_file.read_text().splitlines()
        assert file_lines[0] == "existing rows", "timing file was truncated"
        validate_rows(file_lines[1:], stages)

        dual_file = directory / "dual.tsv"
        logger_rows = run_case(
            executable, logger_env | {"CODEX_STARTUP_TIMING_FILE": str(dual_file)}, "spans"
        )
        validate_future_cases(logger_rows)
        assert dual_file.read_text().splitlines() == logger_rows

        unavailable_file = directory / "missing-parent" / "startup.tsv"
        validate_rows(
            run_case(
                executable,
                logger_env | {"CODEX_STARTUP_TIMING_FILE": str(unavailable_file)},
            ),
            stages,
        )
        assert run_case(
            executable, env | {"CODEX_STARTUP_TIMING_FILE": str(unavailable_file)}
        ) == []

        # Opening succeeds on Linux, while writes fail: the logger still runs.
        if Path("/dev/full").exists():
            validate_rows(
                run_case(
                    executable, logger_env | {"CODEX_STARTUP_TIMING_FILE": "/dev/full"}
                ),
                stages,
            )
        print("TRUEOS Codex startup sink: milestones, spans/polls/cancellation, concurrency, context, and sink failures passed")


if __name__ == "__main__":
    main()
