#!/usr/bin/env python3
"""Compile and test the production typed input wire codecs against ABI records."""

from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(path: str, name: str) -> str:
    source = (ROOT / path).read_text()
    matches = list(re.finditer(rf"^(?:pub(?:\([^)]*\))?\s+)?struct\s+{name}\b", source, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one {name}, found {len(matches)}")
    start = matches[0].start()
    attributes = re.search(r"(?:^#[^\n]*\n)+\Z", source[:start], re.MULTILINE)
    if attributes:
        start = attributes.start()
    ending = re.search(r"^}\s*\n", source[matches[0].end():], re.MULTILINE)
    if not ending:
        raise ValueError(f"{path}: no closing brace for {name}")
    return source[start:matches[0].end() + ending.end()]


def main() -> None:
    records = {
        "TrueosHidCursorEvent": item("crates/trueos-v/src/bp_abi.rs", "TrueosHidCursorEvent"),
        "MouseMotionCommand": item("crates/trueos-v/src/bp_abi.rs", "MouseMotionCommand"),
        "KeyboardControlCommand": item("crates/trueos-v/src/bp_abi.rs", "KeyboardControlCommand"),
        "TrueosKeyboardOutputEvent": item("src/r/keyboard.rs", "TrueosKeyboardOutputEvent"),
        "MidiInputEventV1": item("src/usb3/hid/midi.rs", "MidiInputEventV1"),
    }
    harness = f'''#![allow(dead_code)]
extern crate self as v;
pub mod vinput {{ {records["TrueosHidCursorEvent"]}
{records["MouseMotionCommand"]}
{records["KeyboardControlCommand"]} }}
mod usb2 {{ pub mod hid {{ pub use crate::vinput::TrueosHidCursorEvent; }} }}
mod r {{ pub mod keyboard {{ {records["TrueosKeyboardOutputEvent"]} }} }}
mod usb3 {{ pub mod hid {{ pub mod midi {{ {records["MidiInputEventV1"]} }} }} }}
#[path = "{ROOT / 'src/r/io/input_wire.rs'}"] mod input_wire;
'''
    with tempfile.TemporaryDirectory(prefix="trueos-input-wire-") as temp:
        source = Path(temp) / "harness.rs"
        binary = Path(temp) / "tests"
        source.write_text(harness)
        build = subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                               text=True, capture_output=True)
        if build.returncode:
            print(build.stdout + build.stderr)
            raise SystemExit(build.returncode)
        raise SystemExit(subprocess.run([str(binary)], check=False).returncode)


if __name__ == "__main__":
    main()
