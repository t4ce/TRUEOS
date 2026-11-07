#!/usr/bin/env python3
"""Fault-inject the production USB probe loop and xHCI admission ownership.

Only hardware-facing operations are replaced. Production code decides whether
other ports continue, which successful devices are returned, and whether an
unsuccessful or cancelled admission releases its persistent slot context and
EP0 ring owner. Temporary control-transfer data buffers are outside this model.
"""

import os
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
VENDOR = ROOT / "vendor/CrabUSB/usb-host/src/backend/kmod"
FIXTURE = Path(__file__).with_name("usb_enumeration") / "harness.rs"


def item(source, signature):
    start = source.index(signature)
    opening = source.index("{", start)
    end, depth = opening + 1, 1
    while depth:
        depth += (source[end] == "{") - (source[end] == "}")
        end += 1
    return source[start:end]


def production_core():
    source = (VENDOR / "kcore.rs").read_text()
    signatures = (
        "fn hub_infos(",
        "async fn _probe_devices(",
        "async fn hub_changed_ports(",
        "async fn probe_devices(",
    )
    return "\n".join(item(source, signature) for signature in signatures)


def production_owner():
    source = (VENDOR / "kcore.rs").read_text()
    return "\n".join(item(source, signature) for signature in (
        "pub(crate) struct EnumerationOwner<T>",
        "impl<T> EnumerationOwner<T>",
        "impl<T> Drop for EnumerationOwner<T>",
    ))


def run():
    source = FIXTURE.read_text().replace("@CORE_METHODS@", production_core())
    source = source.replace("@ENUMERATION_OWNER@", production_owner())
    source = source.replace("@NEW_DEVICE@", item(
        (VENDOR / "xhci/host.rs").read_text(), "async fn new_device("
    ))
    source = source.replace("@DEVICE_INIT@", item(
        (VENDOR / "xhci/device.rs").read_text(), "pub(crate) async fn init("
    ))
    root_hub = (VENDOR / "xhci/hub.rs").read_text()
    source = source.replace("@ROOT_PORT_METHODS@", "\n".join(item(root_hub, signature) for signature in (
        "fn consume_port_change_edges(", "async fn handle_uninit(", "async fn handle_reseted(",
        "fn init_emulated_full_reset_ports("
    )))
    registers = (VENDOR / "xhci/reg.rs").read_text()
    masks = "\n".join(re.search(rf"const {name}: u32 = [^;]+;", registers).group() for name in (
        "PORTSC_RO_MASK", "PORTSC_RWS_MASK", "PORTSC_CHANGE_MASK"
    ))
    source = source.replace("@NEUTRAL_PORTSC_WRITE@", masks + "\n" + item(registers, "pub fn write_portsc_neutral("))
    source = source.replace("@HUB_TYPES@", (
        ROOT / "vendor/CrabUSB/usb-if/src/host/hub.rs"
    ).read_text().split("#[cfg(test)]", 1)[0])
    source = source.replace("@ENUMERATION_DIAGNOSTICS@", (
        ROOT / "vendor/CrabUSB/usb-host/src/diag.rs"
    ).read_text().split("/// One Supported Protocol", 1)[0].replace("//!", "//"))
    source = source.replace("@EP0_PACKET_SIZE@", item(
        (VENDOR / "xhci/device.rs").read_text(), "fn ep0_max_packet_size("
    ))
    source = source.replace("@TRANSACTION_TRANSLATOR@", item(
        (VENDOR / "xhci/device.rs").read_text(), "fn transaction_translator("
    ))
    hub = (VENDOR / "hub/device.rs").read_text()
    source = source.replace("@HUB_HELPERS@", "\n".join(item(hub, signature) for signature in (
        "fn parse_hub_descriptor(", "fn decode_hub_port_status("
    )))
    source = source.replace("@EXTERNAL_PORT_METHODS@", "\n".join(item(hub, signature) for signature in (
        "pub async fn changed_ports(", "async fn changed_port("
    )))
    with tempfile.TemporaryDirectory(prefix="trueos-usb-enumeration-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(source)
        (project / "Cargo.toml").write_text(
            '[package]\nname="usb-enumeration-regression"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\nfutures="0.3"\nid-arena="2"\nanyhow="1"\n'
        )
        channel = re.search(
            r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()
        ).group(1)
        env = os.environ.copy()
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        subprocess.run(
            ["cargo", f"+{channel}", "test", "--offline", "--manifest-path", str(project / "Cargo.toml")],
            cwd=project,
            env=env,
            check=True,
        )


if __name__ == "__main__":
    run()
