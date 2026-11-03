#!/usr/bin/env python3
"""Compile and run the production button-region wire codec on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def extract_item(source: str, start: str) -> str:
    begin = source.index(start)
    brace = source.index("{", begin)
    depth = 0
    for end in range(brace, len(source)):
        if source[end] == "{":
            depth += 1
        elif source[end] == "}":
            depth -= 1
            if depth == 0:
                return source[begin : end + 1]
    raise ValueError(f"unterminated Rust item starting with {start!r}")


def main() -> None:
    abi = (ROOT / "crates/trueos-v/src/bp_abi.rs").read_text()
    record = re.search(
        r"#\[repr\(C\)\]\s*#\[derive\([^]]+\)\]\s*"
        r"pub struct TrueosUi4ButtonRegionV1 \{[^}]+\}",
        abi,
    ).group()
    api = (ROOT / "src/ui4/blueprint_ui4/button_outline_api.rs").read_text()
    wire_size = re.search(r"const REGION_WIRE_BYTES: usize = (\d+);", api).group(1)
    codec = extract_item(api, "fn encode_regions(")
    assertions = "\n".join(
        f"const _: () = assert!(core::mem::offset_of!(Region, {name}) == {offset});"
        for name, offset in [
            ("x", 0), ("y", 4), ("width", 8), ("height", 12),
            ("clip_x", 16), ("clip_y", 20), ("clip_width", 24),
            ("clip_height", 28), ("border_rgba", 32),
        ]
    )
    source = f"""\
#![allow(dead_code)]
extern crate alloc;
mod v {{ pub mod bp_abi {{ {record} }} }}
use v::bp_abi::TrueosUi4ButtonRegionV1 as Region;
const REGION_WIRE_BYTES: usize = {wire_size};
const _: () = assert!(core::mem::size_of::<Region>() == REGION_WIRE_BYTES);
{assertions}
{codec}
#[test]
fn all_fields_encode_in_little_endian_order() {{
    let bytes = encode_regions(&[Region {{
        x: -2, y: 3, width: 4, height: 5, clip_x: -6,
        clip_y: 7, clip_width: 8, clip_height: 9, border_rgba: 0x1122_3344,
    }}]);
    let expected: [u8; 36] = [
        0xfe, 0xff, 0xff, 0xff, 3, 0, 0, 0, 4, 0, 0, 0,
        5, 0, 0, 0, 0xfa, 0xff, 0xff, 0xff, 7, 0, 0, 0,
        8, 0, 0, 0, 9, 0, 0, 0, 0x44, 0x33, 0x22, 0x11,
    ];
    assert_eq!(bytes, expected);
    assert!(encode_regions(&[]).is_empty());
}}
"""
    with tempfile.TemporaryDirectory(prefix="trueos-button-region-wire-") as temp:
        binary = Path(temp) / "button-region-wire-tests"
        source_path = Path(temp) / "test.rs"
        source_path.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source_path), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
