#!/usr/bin/env python3
"""Run Link's production mouse UI and HID bucket tests on the host.

The kernel has test=false and needs hardware services. This isolated no_std
crate imports the unchanged Link model and HID registry, including their inline
tests, with the real v input contract. It checks parsing, layout, drag/drop and
the single-cursor bucket rule without claiming a hardware-rendered TUI test.
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
HOST = "x86_64-unknown-linux-gnu"


def main() -> None:
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    hut = ROOT / "src/usb3/hid/hut.rs"
    model = ROOT / "src/shell3/system/link_model.rs"
    assert hut.is_file(), f"HID registry missing: {hut}"
    assert model.is_file(), f"Link UI model missing: {model}"

    with tempfile.TemporaryDirectory(prefix="trueos-link-tui-") as directory:
        work = Path(directory)
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-link-tui-tests"
version = "0.1.0"
edition = "2024"

[workspace]

[dependencies]
heapless = "=0.9.1"
spin = "=0.10.1"
v = { path = ''' + json.dumps(str(ROOT / "crates/trueos-v")) + ''' }

[lib]
path = "lib.rs"
''')
        (work / "lib.rs").write_text('''#![no_std]
#![allow(dead_code)]

extern crate alloc;

pub mod usb3 {
    pub mod hid {
        #[path = ''' + json.dumps(str(hut)) + ''']
        pub mod hut;
    }
}

#[path = ''' + json.dumps(str(model)) + ''']
mod link_model;
''')
        result = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--lib",
            "--target", HOST,
        ], cwd=work)
        raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
