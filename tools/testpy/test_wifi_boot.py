#!/usr/bin/env python3
"""Validate production boot-credential parsing using synthetic credentials only."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = '#![allow(dead_code)]\nmod net {\n    #[path="' + str(ROOT / 'src/net/wifi_boot.rs') + '"] mod wifi_boot;\n}\n'
with tempfile.TemporaryDirectory(prefix='trueos-wifi-boot-') as temporary:
    directory = Path(temporary)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
