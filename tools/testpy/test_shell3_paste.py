#!/usr/bin/env python3
"""Exercise the production SSH terminal decoder, paste append, and ANSI modes.

Keyboard/TUI and display drawing are fixtures. Production input, paste
collection, history, ANSI control flow, and storage methods compile unchanged.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name("shell3_paste") / "harness.rs"


def method(source: str, signature: str) -> str:
    start = source.index(signature)
    cursor = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    shell = (ROOT / "src/shell3/shell3.rs").read_text()
    source = FIXTURE.read_text()
    source = source.replace("@MATRIX_PASTE_METHOD@",
                            method(shell, "fn echo_pasted_context("))
    source = source.replace("@SHELL_PASTE_METHOD@",
                            method(shell, "fn record_pasted_context("))
    remote = (ROOT / "src/shell3/tui/remote.rs").read_text()
    reset = re.search(r'^const RESET:.*;$', remote, re.MULTILINE)
    assert reset, "remote terminal reset missing"
    source = source.replace("@REMOTE_RESET@", reset.group(0))
    source = source.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
    with tempfile.TemporaryDirectory(prefix="trueos-shell3-paste-") as directory:
        work = Path(directory)
        code, binary = work / "harness.rs", work / "tests"
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
