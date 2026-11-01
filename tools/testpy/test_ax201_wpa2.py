#!/usr/bin/env python3
"""Host-vector tests for the production AX201 WPA2/CCMP implementation.

A temporary Cargo crate avoids the kernel target and its build-std settings.
Only cached RustCrypto dependencies are used; no firmware or hardware runs.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='trueos-ax201-wpa2-') as temporary:
    directory = Path(temporary)
    (directory / 'src').mkdir()
    (directory / 'Cargo.toml').write_text('''[package]
name = "trueos-ax201-wpa2-host-tests"
version = "0.1.0"
edition = "2024"
[dependencies]
hmac = { version = "0.12.1", default-features = false }
sha1 = { version = "=0.10.6", default-features = false }
aes = { version = "0.8.4", default-features = false }
cipher = { version = "0.4.4", default-features = false }
''')
    (directory / 'src/lib.rs').write_text('''#![allow(dead_code)]
extern crate alloc;
#[path = "''' + str(ROOT / 'src/net/iwl_ax201_wpa2.rs') + '''"]
mod wpa2;
''')
    subprocess.run(['cargo', 'test', '--target', 'x86_64-unknown-linux-gnu', '--offline'],
                   cwd=directory, check=True)
