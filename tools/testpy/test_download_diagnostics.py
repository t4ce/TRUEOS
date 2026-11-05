#!/usr/bin/env python3
"""Compile the pure production download diagnostics and run its host tests.

This checks evidence bounds and URL/header sanitization without fetching any
URLs, building the kernel, publishing an app, or changing the rig.
"""

import json
import os
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-download-diagnostics-") as directory:
        project = Path(directory)
        source = project / "src"
        source.mkdir()
        module = ROOT / "src/r/net/download_diagnostics.rs"
        (source / "lib.rs").write_text(
            '#![allow(dead_code)]\nextern crate alloc;\n'
            f'#[path = {json.dumps(str(module))}] mod download_diagnostics;\n')
        (project / "Cargo.toml").write_text('''[package]
name = "trueos-download-diagnostics-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
sha2 = { version = "=0.10.9", default-features = false }
serde_json = { version = "1.0", default-features = false, features = ["alloc"] }
''')
        environment = dict(os.environ)
        environment["CARGO_TARGET_DIR"] = str(ROOT / "tgt/download-diagnostics-host-tests")
        environment["RUSTUP_TOOLCHAIN"] = "nightly-2026-07-10"
        subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml")],
                       cwd=project, env=environment, check=True)


if __name__ == "__main__":
    main()
