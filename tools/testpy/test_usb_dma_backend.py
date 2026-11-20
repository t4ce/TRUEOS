#!/usr/bin/env python3
"""Exercise the production USB DMA backend with host memory and synthetic PMM mappings."""
from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    fixture = (Path(__file__).with_name('fixtures') / 'usb_dma_backend.rs').read_text()
    with tempfile.TemporaryDirectory(prefix='trueos-usb-dma-') as temporary:
        path = Path(temporary)
        manifest = '''[package]
name = "trueos-usb-dma-backend-tests"
version = "0.0.0"
edition = "2024"

[workspace]

[dependencies]
dma-api = { path = %s, default-features = false }

[lib]
path = "tests.rs"
''' % json.dumps(str(ROOT / 'vendor/dma-api-0.10.2'))
        (path / 'Cargo.toml').write_text(manifest)
        (path / 'tests.rs').write_text(fixture.replace('@BACKEND@', str(ROOT / 'src/usb3/dma_backend.rs')))
        subprocess.run([
            'cargo', '+' + toolchain, 'test', '--manifest-path', str(path / 'Cargo.toml'),
            '--target', 'x86_64-unknown-linux-gnu',
            '--target-dir', str(ROOT / 'tgt/usb-dma-backend-host-tests'),
        ], cwd=path, check=True, timeout=180)


if __name__ == '__main__':
    main()
