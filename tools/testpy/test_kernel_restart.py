#!/usr/bin/env python3
"""Run the real restart function with host kernel/VM test doubles."""
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix="trueos-kernel-restart-") as directory:
        project = pathlib.Path(directory)
        (project / "src").mkdir()
        (project / "Cargo.toml").write_text(
            '[package]\nname="trueos-restart-native"\nversion="0.1.0"\nedition="2024"\n'
            '[dependencies]\nserde={version="1",features=["derive"]}\nserde_json="1"\n'
        )
        (project / "startup.json").write_text(json.dumps({
            "autostart": [
                {"action": "launch", "archive": "voxy.bp", "slot": "voxy", "settle_ms": 0},
                {"action": "skip", "archive": "closed.bp", "slot": "closed", "settle_ms": 0},
            ]
        }))
        source = (ROOT / "src/r/restart.rs").read_text()
        source = source[source.index("use alloc::"):].replace("#[trueos_executor::task]\n", "")
        (project / "src/restart.rs").write_text(source)
        (project / "src/lib.rs").write_text(
            (ROOT / "tools/testpy/kernel_restart_harness.rs").read_text()
        )
        subprocess.run([
            "cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
            "--target", "x86_64-unknown-linux-gnu", "--target-dir",
            str(pathlib.Path(tempfile.gettempdir()) / "trueos-kernel-restart-test-target"),
            "--", "--test-threads=1",
        ], cwd=project, check=True)


if __name__ == "__main__":
    main()
