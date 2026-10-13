#!/usr/bin/env python3
"""Build a disposable four-crate text utility fold and retain original tests.

All source edits occur under this experiment's ignored generated directory.
No Codex crate, pack stage, build cache, or Blueprint package is changed.
"""

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import time


UTILITIES = ["elapsed", "fuzzy-match", "stream-parser", "template"]
PROFILE = """[profile.release]
opt-level = 1
codegen-units = 8
incremental = false
"""


def manifest(name):
    return f'''[package]
name = "{name}"
version = "0.0.0"
edition = "2024"
license = "Apache-2.0"

[lib]
doctest = false

[dev-dependencies]
pretty_assertions = "=1.4.1"
'''


def prepare(source, output):
    generated = output / "generated"
    if generated.exists():
        shutil.rmtree(generated)
    baseline = generated / "baseline"
    baseline.mkdir(parents=True)
    members = ", ".join(json.dumps(name) for name in UTILITIES)
    (baseline / "Cargo.toml").write_text(f'[workspace]\nmembers = [{members}]\nresolver = "3"\n\n{PROFILE}')
    merged = generated / "merged"
    merged.mkdir()
    (merged / "Cargo.toml").write_text(manifest("codex-pure-text-utilities-prototype") + '\n[workspace]\n\n' + PROFILE)
    (merged / "src").mkdir()
    roots = []
    rewrites = []
    for name in UTILITIES:
        baseline_crate = baseline / name
        baseline_crate.mkdir()
        (baseline_crate / "Cargo.toml").write_text(manifest(f"codex-utils-{name}"))
        shutil.copytree(source / name / "src", baseline_crate / "src")
        module = name.replace("-", "_")
        target = merged / "src" / module
        shutil.copytree(source / name / "src", target)
        for path in target.rglob("*.rs"):
            original = path.read_text()
            updated = original.replace("crate::", f"crate::{module}::")
            if updated != original:
                rewrites.append({"file": str(path.relative_to(merged)), "crate_path_references_rebased": original.count("crate::")})
                path.write_text(updated)
        roots.append(f'#[path = "{module}/lib.rs"]\npub mod {module};')
    (merged / "src" / "lib.rs").write_text("\n\n".join(roots) + "\n")
    return baseline, merged, rewrites


def run(workspace, label, output, fresh):
    target = output / "build" / label
    if fresh and target.exists():
        shutil.rmtree(target)
    environment = dict(os.environ)
    environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
    environment.pop("RUSTFLAGS", None)
    environment["CARGO_TARGET_DIR"] = str(target)
    command = [
        "cargo", "+1.95.0", "test", "--offline", "--release", "--workspace",
        "--target", "x86_64-unknown-linux-gnu", "--message-format=json",
        "--manifest-path", str(workspace / "Cargo.toml"),
    ]
    build_command = command.copy()
    build_command[2] = "build"
    started = time.monotonic()
    # Run outside all repositories so parent Cargo config cannot inject TRUEOS
    # target flags or build-std into this host-side source folding experiment.
    with tempfile.TemporaryDirectory(prefix="codex-text-fold-cwd-") as cwd:
        build = subprocess.run(build_command, cwd=cwd, env=environment, text=True, capture_output=True)
        result = subprocess.run(command, cwd=cwd, env=environment, text=True, capture_output=True)
    elapsed = time.monotonic() - started
    raw = output / "raw"
    raw.mkdir(exist_ok=True)
    stdout = build.stdout + result.stdout
    stderr = build.stderr + result.stderr
    (raw / f"{label}.stdout.log").write_text(stdout)
    (raw / f"{label}.stderr.log").write_text(stderr)
    libraries = []
    test_results = []
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            match = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", line)
            if match:
                status, passed, failed, ignored = match.groups()
                test_results.append({"status": status, "passed": int(passed), "failed": int(failed), "ignored": int(ignored)})
            continue
        if event.get("reason") != "compiler-artifact":
            continue
        if not event.get("package_id", "").startswith("path+file://"):
            continue
        for filename in event.get("filenames", []):
            path = Path(filename)
            if path.suffix == ".rlib":
                libraries.append({"crate": event["target"]["name"], "bytes": path.stat().st_size, "path": str(path)})
    return {
        "build_command": build_command,
        "command": command,
        "build_exit_code": build.returncode,
        "exit_code": result.returncode,
        "elapsed_seconds": round(elapsed, 3),
        "source_crate_rlibs": libraries,
        "source_crate_rlib_count": len(libraries),
        "aggregate_source_crate_rlib_bytes": sum(row["bytes"] for row in libraries),
        "original_test_suites": test_results,
        "tests_passed": sum(row["passed"] for row in test_results),
        "tests_failed": sum(row["failed"] for row in test_results),
        "tests_ignored": sum(row["ignored"] for row in test_results),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--codex-utils", type=Path, default=Path("/home/t4ce/Repos/codex/codex-rs/utils"))
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--fresh", action="store_true", help="Remove only this experiment's ignored build directories")
    args = parser.parse_args()
    output = args.output.resolve()
    baseline, merged, rewrites = prepare(args.codex_utils.resolve(), output)
    results = {
        "schema_version": 1,
        "scope": "Host x86_64 Linux source fold, release opt-level=1/codegen-units=8, original tests retained. Not a TRUEOS package-size benchmark.",
        "source_crates": [f"codex-utils-{name}" for name in UTILITIES],
        "changes": rewrites,
        "baseline": run(baseline, "baseline", output, args.fresh),
        "merged": run(merged, "merged", output, args.fresh),
        "bp_byte_savings": None,
    }
    results["validation"] = {
        "both_builds_passed": results["baseline"]["build_exit_code"] == results["merged"]["build_exit_code"] == results["baseline"]["exit_code"] == results["merged"]["exit_code"] == 0,
        "original_test_count_equal": results["baseline"]["tests_passed"] == results["merged"]["tests_passed"],
        "source_crate_units_before": results["baseline"]["source_crate_rlib_count"],
        "source_crate_units_after": results["merged"]["source_crate_rlib_count"],
        "expected_crate_units": results["baseline"]["source_crate_rlib_count"] == 4 and results["merged"]["source_crate_rlib_count"] == 1,
    }
    (output / "prototype-results.json").write_text(json.dumps(results, indent=2) + "\n")
    print(json.dumps(results, indent=2))
    if not all(results["validation"][key] for key in ("both_builds_passed", "original_test_count_equal", "expected_crate_units")):
        raise SystemExit(1)


if __name__ == "__main__":
    main()
