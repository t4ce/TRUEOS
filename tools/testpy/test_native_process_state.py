#!/usr/bin/env python3
"""Compile real native process/TLS/thread state at a controlled scheduler boundary."""
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = Path(__file__).with_name("native_process_state.rs")


def function(source, name):
    match = re.search(r'^(?:pub(?:\(crate\))? )?(?:unsafe )?(?:extern "C" )?fn ' + re.escape(name) + r'\(', source, re.M)
    start = match.start()
    opening = source.index("{", match.end())
    depth, cursor = 1, opening + 1
    while depth:
        depth += (source[cursor] == "{") - (source[cursor] == "}")
        cursor += 1
    return source[start:cursor]


def main():
    wait = (ROOT / "src/wait.rs").read_text()
    completion = wait[wait.index("pub struct CompletionCell<T>"):wait.index("type JobFuture =")]
    source = FIXTURE.read_text().replace("@ROOT@", str(ROOT)).replace("@COMPLETION@", completion)
    shim = (ROOT / "src/native_abi.rs").read_text()
    source = source.replace("@CABI@", "\n".join(function(shim, name) for name in [
        "trueos_cabi_thread_spawn", "trueos_cabi_thread_join", "trueos_cabi_thread_detach",
        "trueos_cabi_thread_current_id",
    ]))
    memory = (ROOT / "src/hv/memory.rs").read_text()
    source = source.replace("@MAPPING@", "\n".join(function(memory, name) for name in [
        "patch_guest_hull_rw_dynamic_state", "patch_guest_hull_rw_u8", "patch_guest_hull_rw_bytes",
        "guest_hull_rw_page_uses_kernel_backing",
    ]))
    source = source.replace("@VM_LIMIT@", re.search(r"pub const VM_ID_LIMIT: usize = ([0-9]+);",
        (ROOT / "src/allcaps.rs").read_text()).group(1))
    toolchain = re.search(r'channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()).group(1)
    installed = Path(subprocess.check_output(["rustup", "run", toolchain, "rustc", "--print", "sysroot"], text=True).strip())
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = str(installed / "lib") + ":" + env.get("LD_LIBRARY_PATH", "")
    with tempfile.TemporaryDirectory(prefix="trueos-native-process-") as temporary:
        directory = Path(temporary)
        harness = directory / "harness.rs"
        harness.write_text(source)
        for optimization in (0, 3):
            binary = directory / f"native-process-o{optimization}"
            subprocess.run([installed / "bin/rustc", "--edition=2024", "--test", "-C", f"opt-level={optimization}",
                            harness, "-o", binary], env=env, check=True)
            subprocess.run([binary, "--test-threads=1", "--nocapture"], env=env, check=True)


if __name__ == "__main__":
    main()
