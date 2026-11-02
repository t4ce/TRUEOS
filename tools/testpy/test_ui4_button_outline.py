#!/usr/bin/env python3
"""Run production button-outline geometry and cursor/occlusion state on the host."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def main():
    abi = (ROOT / 'crates/trueos-v/src/bp_abi.rs').read_text()
    record = re.search(r'pub struct TrueosUi4ButtonRegionV1 \{[^}]+\}', abi).group()
    harness = (Path(__file__).parent / 'ui4_button_outline/harness.rs').read_text()
    harness = harness.replace('REGION_RECORD', record).replace('GEOMETRY_PATH', str(ROOT / 'src/ui4/button_outline_geometry.rs')).replace('STATE_PATH', str(ROOT / 'src/ui4/button_outline.rs'))
    with tempfile.TemporaryDirectory(prefix='trueos-button-outline-') as tmp:
        directory = Path(tmp)
        (directory / 'Cargo.toml').write_text('[package]\nname="ui4-button-outline-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nspin="0.10.1"\n[workspace]\n')
        (directory / 'src').mkdir()
        (directory / 'src/lib.rs').write_text(harness)
        env = os.environ.copy()
        env['CARGO_TARGET_DIR'] = str(ROOT / 'tgt/ui4-button-outline-tests')
        subprocess.run(['cargo', 'test', '--quiet', '--manifest-path', str(directory / 'Cargo.toml')], cwd='/tmp', env=env, check=True)

if __name__ == '__main__':
    main()
