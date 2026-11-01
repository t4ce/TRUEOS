#!/usr/bin/env python3
"""Exercise optional Wi-Fi build provisioning with synthetic inputs only."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

with tempfile.TemporaryDirectory(prefix="trueos-wifi-credentials-build-") as directory:
    binary = Path(directory) / "test"
    subprocess.run([
        "rustc", "--edition=2024", "--test",
        str(ROOT / "tools/build/wifi_credentials.rs"), "-o", str(binary),
    ], check=True)
    subprocess.run([str(binary)], check=True)
