#!/usr/bin/env python3
"""Exercise production USB transfer synchronization with the real DMA API.

Only the physical DMA backend and completion delivery are simulated. The real
transfer constructor/sync helpers and xHCI completion method decide copy ranges.
"""
import os
from pathlib import Path
import re
import subprocess
import shutil
import tempfile

ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor/CrabUSB/usb-host/src/backend"


def item(source, signature):
    start = source.index(signature)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def run():
    endpoint = (VENDOR / "kmod/xhci/endpoint.rs").read_text()
    transfer = (VENDOR / "ty/transfer.rs").read_text()
    transfer = transfer.replace('#[cfg_attr(umod, derive(Clone))]', '')
    transfer = transfer.replace('#[cfg(kmod)]', '')
    transfer = re.sub(r'    #\[cfg\(umod\)\]\n    pub buffer:[^\n]+\n', '', transfer)
    source = Path(__file__).with_name("usb_dma_transfers").joinpath("harness.rs").read_text()
    source = source.replace('@TRANSFER_TYPE@', transfer)
    source = source.replace('@TRANSFER_IMPL@', (VENDOR / "kmod/transfer.rs").read_text())
    source = source.replace('@COMPLETION@', item(endpoint, 'fn transfer_from_completion('))
    source = source.replace('@NORMAL_ACTUAL@', item(endpoint, 'fn event_actual_length('))
    source = source.replace('@NORMAL_REQUESTED@', item(endpoint, 'fn requested('))
    source = source.replace('@ISO_ACTUAL@', item(endpoint, 'fn iso_packet_actual_length('))
    with tempfile.TemporaryDirectory(prefix="trueos-usb-dma-transfers-") as temporary:
        project = Path(temporary)
        # Detach the interface manifest from CrabUSB's nested workspace while
        # keeping its actual production source and dependency requirements.
        interface = project / "usb-if"
        shutil.copytree(ROOT / "vendor/CrabUSB/usb-if/src", interface / "src")
        manifest = (ROOT / "vendor/CrabUSB/usb-if/Cargo.toml").read_text()
        manifest = manifest.replace('edition.workspace = true', 'edition = "2024"')
        manifest = manifest.replace('license.workspace = true', 'license = "MIT"')
        manifest = manifest.replace('repository.workspace = true', 'repository = "https://github.com/drivercraft/CrabUSB"')
        manifest = manifest.replace('futures = {workspace = true, features = ["alloc"]}', 'futures = {version = "0.3", default-features = false, features = ["alloc"]}')
        manifest = manifest.replace('thiserror = {workspace = true}', 'thiserror = {version = "2", default-features = false}')
        manifest = manifest.replace('../../log-os', str(ROOT / 'vendor/log-os'))
        (interface / 'Cargo.toml').write_text(manifest)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(source)
        (project / "Cargo.toml").write_text(
            '[package]\nname="usb-dma-transfer-regression"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nxhci="0.9"\nanyhow="1"\nmbarrier="0.1"\n'
            f'dma-api={{path="{ROOT / "vendor/dma-api-0.10.2"}"}}\n'
            'usb-if={path="usb-if"}\n'
            '[profile.dev]\ndebug=0\nincremental=false\n'
        )
        channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text()).group(1)
        env = os.environ.copy()
        env.pop('RUSTFLAGS', None)
        env.pop('CARGO_ENCODED_RUSTFLAGS', None)
        subprocess.run(['cargo', f'+{channel}', 'test', '--offline', '--manifest-path', str(project / 'Cargo.toml')], cwd=project, env=env, check=True)


if __name__ == '__main__':
    run()
