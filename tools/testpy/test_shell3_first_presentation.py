#!/usr/bin/env python3
"""Run the production first-paint wait with the real captured-session worker.

Reuse the existing hardware boundary fixture without modifying it. Only UI
readiness and timer yields are supplied here; both production state machines
are compiled directly.
"""
from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("shell3_sessions")


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    source = (FIXTURES / "harness.rs").read_text()
    ending = "    }\n}\n"
    assert source.endswith(ending), "fixture test-module ending changed"
    source = source[:-len(ending)] + (FIXTURES / "first_presentation.rs").read_text() + ending
    source = source.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
    with tempfile.TemporaryDirectory(prefix="trueos-first-presentation-") as directory:
        code, binary = Path(directory) / "harness.rs", Path(directory) / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), "first_presentation_wait", "--test-threads=1"],
                       cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
