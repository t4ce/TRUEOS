#!/usr/bin/env python3
"""Exercise production native Shell3 sessions and captured command IO.

Only VM ownership, AP admission, service data and the explicit human TTY are
fixtures. The real session worker, queues, byte parsing and command dispatch
are compiled together, without kernel hardware or a network connection.
"""

from pathlib import Path
import json
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name("shell3_sessions") / "harness.rs"


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    source = FIXTURE.read_text().replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
    with tempfile.TemporaryDirectory(prefix="trueos-shell3-sessions-") as directory:
        work = Path(directory)
        code, binary = work / "harness.rs", work / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
