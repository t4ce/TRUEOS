#!/usr/bin/env python3
"""Compile production AX201 station serializers and their defensive wire tests.

No MMIO or radio operation is simulated; the exact embedded API77 metadata is
used to validate station command versions, BSS parsing and management frames.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = r'''
#![allow(dead_code)]
extern crate alloc;
mod net {
    #[path="FIRMWARE"] pub(crate) mod iwl_ax201_fw;
    #[path="STATION"] pub(crate) mod iwl_ax201_station;
}
'''
source = source.replace('FIRMWARE', str(ROOT / 'src/net/iwl_ax201_fw.rs'))
source = source.replace('STATION', str(ROOT / 'src/net/iwl_ax201_station.rs'))
with tempfile.TemporaryDirectory(prefix='trueos-ax201-station-') as directory:
    directory = Path(directory)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
