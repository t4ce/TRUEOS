#!/usr/bin/env python3
"""Exercise Codex's production startup sink before tracing is initialized.

Link the actual module with host std and a capturing TRUEOS logger. Each sink
configuration runs in a fresh process so OnceLock and environment stay isolated.
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

HARNESS = r'''#[path = @STARTUP_TIMING@] mod startup_timing;
use std::sync::Mutex;

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

fn main() {
    // No tracing dependency or subscriber is linked into this harness.
    startup_timing::mark("arg0_entry");
    startup_timing::mark("arg0_entry");
    if std::env::args().nth(1).as_deref() == Some("concurrent") {
        let workers: Vec<_> = (0..8).map(|index| {
            std::thread::spawn(move || {
                for _ in 0..32 {
                    startup_timing::mark("worker_shared");
                    startup_timing::mark(&format!("worker_{index}"));
                }
            })
        }).collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
    startup_timing::mark("runtime_done");
    startup_timing::mark("runtime_done");
    for (level, target, message) in CAPTURE.lock().unwrap().iter() {
        println!("{level}\t{target}\t{message}");
    }
}
'''


def validate_rows(rows: list[str], stages: set[str]) -> None:
    seen = set()
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
        assert stage not in seen, f"duplicate milestone: {stage}"
        seen.add(stage)
    assert seen == stages, (seen, stages)


def run_case(executable: Path, env: dict[str, str], concurrent=False) -> list[str]:
    arguments = [str(executable)]
    if concurrent:
        arguments.append("concurrent")
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
    if not STARTUP_TIMING.is_file():
        raise SystemExit(f"missing production source: {STARTUP_TIMING}")
    with tempfile.TemporaryDirectory(prefix="trueos-codex-startup-sink-") as temporary:
        directory = Path(temporary)
        source = directory / "harness.rs"
        source.write_text(
            HARNESS.replace("@STARTUP_TIMING@", json.dumps(str(STARTUP_TIMING)))
        )
        executable = directory / "harness"
        subprocess.run(
            [
                "rustc", "--edition=2024", "--cfg", 'target_os="trueos"',
                "-Aexplicit_builtin_cfgs_in_flags", str(source), "-o", str(executable),
            ],
            check=True,
            timeout=60,
        )
        env = dict(os.environ)
        for name in [
            "CODEX_STARTUP_TIMING_FILE", "CODEX_STARTUP_TIMING_STDERR", "HARNESS_LOG_ERRNO"
        ]:
            env.pop(name, None)
        stages = {"arg0_entry", "runtime_done"}

        assert run_case(executable, env) == []
        assert run_case(executable, env | {"CODEX_STARTUP_TIMING_STDERR": "0"}) == []

        logger_env = env | {"CODEX_STARTUP_TIMING_STDERR": "1"}
        validate_rows(run_case(executable, logger_env), stages)
        validate_rows(
            run_case(executable, logger_env | {"HARNESS_LOG_ERRNO": "5"}), stages
        )
        concurrent_stages = stages | {"worker_shared"} | {
            f"worker_{index}" for index in range(8)
        }
        validate_rows(run_case(executable, logger_env, concurrent=True), concurrent_stages)

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
            executable, logger_env | {"CODEX_STARTUP_TIMING_FILE": str(dual_file)}
        )
        validate_rows(logger_rows, stages)
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
        print("TRUEOS Codex startup sink: disabled, once/concurrent, failures, and file sinks passed")


if __name__ == "__main__":
    main()
