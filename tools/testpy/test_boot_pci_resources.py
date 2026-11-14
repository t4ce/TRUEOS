#!/usr/bin/env python3
"""Exercise the actual UEFI BAR producer and shared reservation wire format.

Firmware services use the producer's inline mocks. No PCI hardware, boot files,
or running collectors are accessed by this host test.
"""
import json
from pathlib import Path
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]
HOST = "x86_64-unknown-linux-gnu"


def main():
    channel = tomllib.loads((ROOT / "rust-toolchain.toml").read_text())["toolchain"]["channel"]
    producer = ROOT / "tools/trueos-boot/src/pci_resources.rs"
    wire = ROOT / "src/pci/firmware_bar_wire.rs"
    source = '''#![allow(dead_code, unsafe_op_in_unsafe_fn)]
extern crate alloc;
mod handoff {
    pub struct Blob<'a> {
        pub phys: u64,
        pub len: usize,
        pub path: &'a str,
        pub cmdline: &'a str,
    }
}
'''
    source += f'#[path={json.dumps(str(wire))}] mod firmware_bar_wire;\n'
    source += f'#[path={json.dumps(str(producer))}] mod pci_resources;\n'
    with tempfile.TemporaryDirectory(prefix="trueos-boot-pci-resources-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "src/lib.rs").write_text(source)
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-boot-pci-resource-tests"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
r-efi = "=5.3.0"
''')
        # Run from the isolated directory so the kernel's custom build-std
        # configuration cannot mix its alloc/core with the host standard library.
        subprocess.run([
            "cargo", "+" + channel, "test", "--offline", "--quiet",
            "--target", HOST,
        ], cwd=work, check=True)


if __name__ == "__main__":
    main()
