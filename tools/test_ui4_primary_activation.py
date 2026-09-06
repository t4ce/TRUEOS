#!/usr/bin/env python3
"""Verify the production opt-in selection-click policy without hardware."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

source = 'src/ui4/input_broker.rs'
harness = ''.join(constant(source, name) for name in ('PRIMARY_BUTTON_MASK', 'SECONDARY_BUTTON_MASK'))
harness += item(source, 'absorb_selection_gesture')
harness += item(source, 'primary_activation_tests')
with tempfile.TemporaryDirectory(prefix='trueos-primary-activation-') as directory:
    root = Path(directory)
    (root / 'test.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', '--test', str(root / 'test.rs'), '-o', str(root / 'test')], check=True)
    subprocess.run([str(root / 'test')], check=True)
