#!/usr/bin/env python3
"""Test production native environment CABI and std env/args/stdio backends.

The actual owned Environment, byte ABI, decoders, and blocking stream code are
compiled unchanged. Only realm selection, pointer translation, launch/console
transport, and the kernel's surrounding process-context storage are modeled.
"""

import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

from test_thread_control_abi import function

ROOT = Path(__file__).resolve().parents[2]


def main():
    channel = re.search(r'channel = "([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()).group(1)
    installed = Path(subprocess.check_output(["rustup", "run", channel, "rustc", "--print", "sysroot"], text=True).strip())
    library = installed / "lib/rustlib/src/rust/library"
    source = Path(__file__).with_name("native_std_environment.rs").read_text()
    source = source.replace("@ROOT@", str(ROOT)).replace("@LIBRARY@", str(library))
    source = source.replace("@TERMINAL_QUERY@", function((ROOT / "src/hv/mod.rs").read_text(), "blueprint_stdio_is_terminal"))
    source = source.replace("@PROCESS_QUERY@", function((ROOT / "src/hv/mod.rs").read_text(), "blueprint_native_process_identity"))
    source = source.replace("@ARGS_FUNCTION@", function((ROOT / "src/r/io/fs_cabi.rs").read_text(), "trueos_cabi_env_arg"))
    source = source.replace("@COPY_TEXT@", function((ROOT / "src/r/io/fs_cabi.rs").read_text(), "copy_text"))
    env = dict(os.environ)
    env["LD_LIBRARY_PATH"] = str(installed / "lib") + ":" + env.get("LD_LIBRARY_PATH", "")
    spec = importlib.util.spec_from_file_location("native_std_installer", Path(__file__).with_name("apply_trueos_rust_std_environment_backend.py"))
    installer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(installer)
    with tempfile.TemporaryDirectory(prefix="trueos-native-std-environment-") as temporary:
        directory = Path(temporary)
        for relative in ["env/mod.rs", "args/mod.rs", "stdio/mod.rs", "io/mod.rs", "paths/mod.rs", "process/mod.rs", "process/unix/mod.rs", "process/unix/common.rs", "pal/unix/mod.rs", "exit.rs"]:
            target = directory / "library/std/src/sys" / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(library / "std/src/sys" / relative, target)
        target = directory / "library/std/src/io/stdio.rs"
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(library / "std/src/io/stdio.rs", target)
        assert installer.install(directory)
        assert not installer.install(directory)
        assert not installer.install(directory, check=True)
        for module, filename in installer.MODULES.items():
            assert (directory / "library/std/src/sys" / module / "trueos.rs").read_text() == (ROOT / "tools/rust-std" / filename).read_text()
        harness = directory / "contracts.rs"
        harness.write_text(source)
        for optimization in (0, 3):
            binary = directory / f"native-environment-o{optimization}"
            subprocess.run([installed / "bin/rustc", "--edition=2024", "--test", "-C", f"opt-level={optimization}", harness, "-o", binary], env=env, check=True)
            subprocess.run([binary, "--test-threads=1"], env=env, check=True)
    print("native std environment/args/stdio installer: idempotence and exact source checks passed")


if __name__ == "__main__":
    main()
