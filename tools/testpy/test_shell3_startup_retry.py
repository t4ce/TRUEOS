#!/usr/bin/env python3
"""Exercise production Shell3 presentation and startup dispatch at UI boundaries."""

import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def method(source: str, signature: str) -> str:
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 1
    cursor = opening + 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


def main() -> None:
    shell = (ROOT / "src/shell3/shell3.rs").read_text()
    worker = (ROOT / "src/shell3/service.rs").read_text()
    worker = worker[worker.index("async fn shell_worker_task("):]
    # The admitted request must reach the owner before its first presentation.
    assert worker.index("startup.configure_window(shell);") < worker.index("shell.pending_startup = startup;")
    assert worker.index("shell.pending_startup = startup;") < worker.index("shell.present().await")
    assert "startup.launch(shell)" not in worker
    assert "shell.presentation_pending()" in worker
    assert "Timer::after(Duration::from_millis(10)).await" in worker
    methods = "\n".join(method(shell, signature) for signature in (
        "pub async fn present(&mut self)",
        "pub(super) fn presentation_pending(&self)",
        "pub(super) fn show_is_closed(&self)",
    ))
    source = (Path(__file__).with_name("shell3_sessions") / "startup_retry.rs").read_text()
    source = source.replace("@PRESENTATION_METHODS@", methods)
    source = source.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
    for name in ("DEFAULT_COLUMNS", "DEFAULT_ROWS"):
        value = re.search(rf'pub const {name}: usize = (\d+);', shell)
        assert value, name
        source = source.replace("@" + name + "@", value.group(1))
    desktop = (ROOT / "src/ui4/link_service.rs").read_text()
    attribute = "#[trueos_executor::task(pool_size = 1)]"
    assert desktop.count(attribute) == 1
    desktop = desktop.replace(attribute, "")
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-startup-retry-") as directory:
        code, binary = Path(directory) / "harness.rs", Path(directory) / "tests"
        adapter = Path(directory) / "link_service.rs"
        adapter.write_text(desktop)
        source = source.replace("@LINK_SERVICE_PATH@", json.dumps(str(adapter))[1:-1])
        code.write_text(source)
        subprocess.run(["rustc", "+" + channel.group(1), "--edition=2024", "--test",
                        "--target", "x86_64-unknown-linux-gnu", str(code), "-o", str(binary)],
                       cwd=ROOT, check=True)
        subprocess.run([str(binary), "--test-threads=1"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
