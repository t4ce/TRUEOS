#!/usr/bin/env python3
"""Test the production Lilly pack decoder + cold loader with mocked DMA only."""
from pathlib import Path
import json
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def main():
    with tempfile.TemporaryDirectory(prefix='trueos-lily-pack-') as directory:
        work = Path(directory)
        (work / 'src').mkdir()
        (work / 'Cargo.toml').write_text('''[package]
name = "trueos-lily-loader-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
spin = "0.10"
trueos-lily-pack = { path = ''' + json.dumps(str(ROOT / 'crates/trueos-lily-pack')) + ''' }
''')
        template = (Path(__file__).with_name('spirit_assets') / 'harness.rs').read_text()
        (work / 'src/lib.rs').write_text(template.replace('@TRUEOS_ROOT@', str(ROOT)))
        environment = os.environ.copy()
        environment['CARGO_TARGET_DIR'] = str(ROOT / 'tgt/host-lily-loader-tests')
        subprocess.run(['cargo', 'test', '--release', '--offline', '--target', 'x86_64-unknown-linux-gnu', '--', '--nocapture'],
                       cwd=work, env=environment, check=True)

if __name__ == '__main__':
    main()
