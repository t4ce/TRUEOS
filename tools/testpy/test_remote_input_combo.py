#!/usr/bin/env python3
"""Exercise production combo admission, ownership and cleanup for remote control."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/r/io/input_cabi.rs").read_text()
    fixture = (ROOT / "tools/testpy/fixtures/remote_input_combo.rs").read_text()
    signatures = [
        "fn input_combo_guest(", "fn guest_combo_device_owned(",
        "pub(crate) fn remote_input_combo_owned(", "fn authorize_owned_input_combo(",
        "fn authorize_input_combo_mutation(", "fn input_combo_source_kind(",
        "fn guest_control_call(", "unsafe fn guest_input_combo_request(",
        "fn guest_input_combo_bind(", "pub(crate) fn host_input_combo_control_v1(",
        'pub unsafe extern "C" fn trueos_cabi_input_combo_request(',
        'pub extern "C" fn trueos_cabi_input_combo_bind_keyboard(',
        'pub extern "C" fn trueos_cabi_input_combo_bind_mouse(',
        'pub extern "C" fn trueos_cabi_input_combo_bind_tablet(',
        'pub extern "C" fn trueos_cabi_input_combo_bind_gamepad(',
        'pub extern "C" fn trueos_cabi_input_combo_set_color(',
        'pub extern "C" fn trueos_cabi_input_combo_remove(',
    ]
    fixture = fixture.replace("@PRODUCTION@", "\n".join(item(source, signature) for signature in signatures))
    fixture = fixture.replace("@POLICY@", str(ROOT / "src/r/io/input_combo_policy.rs"))
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-remote-combo-") as directory:
        directory = Path(directory)
        (directory / "tests.rs").write_text(fixture)
        executable = directory / "tests"
        subprocess.run(["rustc", "+" + channel, "--edition=2024", "--test", str(directory / "tests.rs"), "-o", str(executable)], check=True)
        subprocess.run([str(executable), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
