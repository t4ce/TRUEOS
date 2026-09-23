#!/usr/bin/env python3
"""Test actual local backup orchestration with host I/O and fault injection.
This checks ordering and cleanup, not TRUEOSFS layout or hardware driver behavior.
"""
from pathlib import Path
import subprocess
import tempfile

root = Path(__file__).resolve().parents[1]
with tempfile.TemporaryDirectory(prefix="trueos-backup-local-") as directory:
    project = Path(directory)
    (project / "Cargo.toml").write_text('''[package]
name = "trueos-backup-local-tests"
version = "0.0.0"
edition = "2024"
[dependencies]
sha2 = "=0.10.9"
[workspace]
''')
    (project / "src").mkdir()
    source = (root / "tools/backup-local-host-tests.rs").read_text().replace(
        "@ENGINE@", str(root / "src/disc/backup_local.rs"))
    (project / "src/lib.rs").write_text(source)
    subprocess.run(["cargo", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"),
                    "--target", "x86_64-unknown-linux-gnu"], cwd=project, check=True)
