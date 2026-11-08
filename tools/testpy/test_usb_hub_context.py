#!/usr/bin/env python3
"""Execute production hub setup with real xHCI contexts and command TRBs.

Only USB control transfers, command completion, and DMA storage are replaced.
The real hub configuration method decides ordering, and the real xHCI method
selects the command and populates its input context.
"""

import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor/CrabUSB/usb-host/src/backend/kmod"
FIXTURE = Path(__file__).with_name("usb_hub_context") / "harness.rs"


def item(source, signature):
    start = source.index(signature)
    end = source.index("{", start) + 1
    depth = 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def run():
    device = (VENDOR / "xhci/device.rs").read_text()
    hub = (VENDOR / "hub/device.rs").read_text()
    context = (VENDOR / "xhci/context.rs").read_text()
    source = FIXTURE.read_text()
    for token, text, signature in (
        ("@UPDATE_HUB@", device, "async fn update_hub_inner("),
        ("@EVALUATE@", device, "async fn evaluate("),
        ("@TT_ENCODE@", device, "fn xhci_tt_think_time("),
        ("@CONFIGURE_HUB@", hub, "pub async fn configure("),
        ("@TT_DECODE@", hub, "fn hub_tt_think_time_ns("),
        ("@CONTEXT_CHANGE@", context, "pub fn perper_change("),
        ("@HUB_PARAMS@", (ROOT / "vendor/CrabUSB/usb-host/src/backend/ty/mod.rs").read_text(), "pub struct HubParams"),
    ):
        source = source.replace(token, item(text, signature))
    with tempfile.TemporaryDirectory(prefix="trueos-usb-hub-context-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(source)
        (project / "Cargo.toml").write_text(
            '[package]\nname="usb-hub-context-regression"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nfutures="0.3"\nxhci="0.9"\nanyhow="1"\n'
            f'usb-if={{path="{ROOT / "vendor/CrabUSB/usb-if"}"}}\n'
            '[profile.dev]\ndebug=0\nincremental=false\n'
        )
        channel = re.search(
            r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()
        ).group(1)
        env = os.environ.copy()
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        subprocess.run(
            ["cargo", f"+{channel}", "test", "--offline", "--manifest-path", str(project / "Cargo.toml")],
            cwd=project, env=env, check=True,
        )


if __name__ == "__main__":
    run()
