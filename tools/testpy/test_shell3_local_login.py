#!/usr/bin/env python3
"""Exercise production local sign-in lifecycle with deterministic host boundaries.

The actual admin admission, login request arm, cancellation guard, completion,
return, and menu loop compile unchanged. Terminal storage, delayed Cry responses,
input decoding, and timers are fixtures; cryptographic verification is covered by
test_cry_frontend_identity.py. No rig or account files are modified.
"""

import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name("fixtures") / "shell3_local_login.rs"


def production_harness() -> str:
    admin = (ROOT / "src/shell3/admin.rs").read_text()
    fields = {
        "START_LOGIN": item(admin, "pub(super) fn start_login("),
        "LOGIN_ATTEMPT": item(admin, "struct LoginAttempt"),
        "LOGIN_ATTEMPT_IMPL": item(admin, "impl LoginAttempt"),
        "OUTCOME": item(admin, "struct Outcome"),
        "OUTCOME_IMPL": item(admin, "impl Outcome"),
        "JOB": item(admin, "struct Job"),
        "DROP": item(admin, "impl Drop for View"),
        "RUN_MENU": item(admin, "async fn run_menu("),
    }
    for name, signature in (
        ("SUBMIT_LOGIN", "fn submit_login("),
        ("CANCEL_LOGIN", "fn cancel_login("),
        ("COMPLETE", "fn complete("),
        ("LEAVE", "fn leave("),
    ):
        fields[name] = item(admin, signature)
    start = admin.index("            Self::Login {")
    fields["LOGIN_ARM"] = admin[start:admin.index("            Self::Unlock {", start)]
    start = admin.index("    Login {")
    fields["LOGIN_VARIANT"] = admin[start:admin.index("    Unlock {", start)]
    harness = FIXTURE.read_text()
    for name, code in fields.items():
        harness = harness.replace("@" + name + "@", code)
    assert not re.search(r"@[A-Z_]+@", harness), "unexpanded fixture marker"
    return harness


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-local-login-") as directory:
        work = Path(directory)
        output = args.output.resolve() if args.output else work / "evidence"
        output.mkdir(parents=True, exist_ok=True)
        code = work / "harness.rs"
        code.write_text(production_harness())
        checks = []
        for profile, flags in (("debug", []), ("release", ["-O"])):
            binary = work / ("tests-" + profile)
            commands = [
                ["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                 "--target", "x86_64-unknown-linux-gnu", *flags, str(code), "-o", str(binary)],
                [str(binary), "--test-threads=1"],
            ]
            for stage, command in zip(("compile", "tests"), commands):
                result = subprocess.run(command, cwd=ROOT, text=True,
                                        capture_output=True, timeout=120)
                log = output / f"{profile}-{stage}.log"
                log.write_text(result.stdout + result.stderr)
                checks.append({"profile": profile, "stage": stage,
                               "exit": result.returncode, "log": str(log)})
                if result.returncode:
                    raise RuntimeError(log.read_text()[-14000:])
                if stage == "tests":
                    passed = re.search(r"test result: ok\. (\d+) passed", result.stdout)
                    assert passed and int(passed[1]) == 10, result.stdout
                    checks[-1]["passed"] = int(passed[1])
        paths = [ROOT / "src/shell3/admin.rs", Path(__file__), FIXTURE]
        (output / "receipt.json").write_text(json.dumps({
            "status": "passed", "boundary": __doc__, "checks": checks,
            "sources": [{"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}
                        for path in paths],
        }, indent=2) + "\n")
        print(output / "receipt.json")


if __name__ == "__main__":
    main()
