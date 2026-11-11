#!/usr/bin/env python3
"""Exercise production raw keyboard CABI, vmcall helpers and HUT redaction."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/r/io/input_cabi.rs").read_text()
    wire = (ROOT / "src/r/io/input_wire.rs").read_text()
    fixture = (ROOT / "tools/testpy/fixtures/raw_keyboard_isolation.rs").read_text()
    fixture = fixture.replace("@PRODUCTION@", "\n".join(item(source, name) for name in [
        "fn input_combo_guest(", "fn guest_combo_device_owned(",
        "fn raw_keyboard_device_visible(", "fn read_keyboard_output_for_caller(",
        "pub(crate) fn read_keyboard_snapshot_for_caller(",
        "pub fn host_input_hut_snapshot_v1(", "pub fn host_input_pop_keyboard_output(",
        "pub fn host_input_keyboard_output_since(", "unsafe fn input_pop_keyboard_output(",
        "unsafe fn input_read_keyboard_output_since(",
        "fn guest_input_hut_snapshot_v1(", "pub(crate) unsafe fn guest_hid_hut_read_keyboards(",
        "fn guest_input_pop_keyboard_output(", "fn guest_input_read_keyboard_output_since(",
        "pub unsafe extern \"C\" fn trueos_cabi_input_pop_keyboard_output(",
        "pub unsafe extern \"C\" fn trueos_cabi_input_read_keyboard_output_since(",
    ]))
    fixture = fixture.replace("@HUT_GETTER@", item(
        (ROOT / "src/usb3/hid/mod.rs").read_text(),
        'pub unsafe extern "C" fn trueos_cabi_hid_hut_read_keyboards(',
    ))
    fixture = fixture.replace("@WIRE@", "\n".join(item(wire, name) for name in [
        "fn begin(", "fn put<", "pub(crate) fn hut_snapshot_record_len(",
        "pub(crate) fn hut_snapshot_response_valid(", "pub(crate) fn hut_keyboard(",
        "pub(crate) fn decode_hut_keyboard(", "pub(crate) fn keyboard_output(",
    ]))
    for placeholder, path, signature in [
        ("@EVENT@", "src/r/keyboard.rs", "pub struct TrueosKeyboardOutputEvent"),
        ("@STATE@", "src/usb3/hid/hut.rs", "pub struct TrueosHidHutKeyboardState"),
    ]:
        fixture = fixture.replace(placeholder, "#[repr(C)]\n#[derive(Clone,Copy,Debug,Default)]\n" + item((ROOT / path).read_text(), signature))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-raw-keyboard-") as directory:
        directory = Path(directory)
        source_path = directory / "tests.rs"
        source_path.write_text(fixture)
        executable = directory / "tests"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(source_path), "-o", str(executable)], check=True, cwd=directory)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
