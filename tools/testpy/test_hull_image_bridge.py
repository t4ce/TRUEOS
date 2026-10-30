#!/usr/bin/env python3
"""Exercise the production web-image UI4 bridge with modeled DMA and GPU fences.

The complete bridge is compiled unchanged. Only allocation, VM transport,
service admission and GPU objects are modeled; production VA placement and
source destruction are also imported. This runs without booting the kernel.
"""
from pathlib import Path
import json
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def function(source: str, name: str) -> str:
    match = re.search(rf'^(?:pub(?:\([^)]*\))?\s+)?fn\s+{name}\b', source, re.MULTILINE)
    assert match, name
    end = re.search(r'^}\s*$', source[match.end():], re.MULTILINE)
    assert end, name
    return source[match.start():match.end() + end.end()] + '\n'


def main() -> None:
    bridge = (ROOT / 'src/ui4/blueprint_ui4/hull_image_api.rs').read_text()
    parent = (ROOT / 'src/ui4/blueprint_ui4.rs').read_text()
    abi = (ROOT / 'crates/trueos-v/src/bp_abi.rs').read_text()
    harness = (ROOT / 'tools/testpy/hull_image_bridge/harness.rs').read_text()
    tests = (ROOT / 'tools/testpy/hull_image_bridge/tests.rs').read_text()
    with tempfile.TemporaryDirectory(prefix='trueos-hull-image-bridge-') as temporary:
        directory = Path(temporary)
        source = directory / 'bridge.rs'
        source.write_text(bridge + '\n' + tests)
        harness = harness.replace('@BRIDGE_PATH@', json.dumps(str(source)))
        harness = harness.replace('@ALLOCATE@', function(parent, 'allocate_sprite_gpu_va'))
        harness = harness.replace('@DESTROY@', function(parent, 'destroy_rgba8_surface'))
        info = abi[:abi.index('pub const UI4_HULL_IMAGE_PENDING')]
        harness = harness.replace('@ABI@', info)
        fixture = directory / 'harness.rs'
        fixture.write_text(harness)
        executable = directory / 'tests'
        subprocess.run(['rustc', '--edition=2024', '--test', str(fixture), '-o', str(executable)], check=True)
        subprocess.run([str(executable), '--test-threads=1'], check=True)


if __name__ == '__main__':
    main()
