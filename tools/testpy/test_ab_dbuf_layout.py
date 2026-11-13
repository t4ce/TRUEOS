#!/usr/bin/env python3
"""Exercise production A/B DBUF allocation helpers against real reservations."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import constant, item

ROOT = Path(__file__).resolve().parents[2]


def main():
    declarations = []
    for prefix in ["PLANE_DBUF_SLOT", "PLANE_DBUF_S2_SLOT"]:
        for slot in range(5):
            for edge in ["START", "END"]:
                code = constant("src/intel/display/regs.rs", f"{prefix}_{slot}_{edge}")
                declarations.append(re.sub(r"^pub(?:\([^)]*\))?\s+", "", code))
    declarations.append(re.sub(r"^pub(?:\([^)]*\))?\s+", "",
                              constant("src/intel/display/regs.rs", "PLANE_DBUF_S2_BASE")))
    for name in ["native_ui4_for_pipe", "uses_ab_dbuf_layout", "plane_dbuf_range"]:
        declarations.append(item("src/intel/display.rs", name))
    # Keep Spirit's production cursor regression tests runnable with the same
    # real allocation helpers, without pulling in cursor MMIO or surface code.
    spirit = "src/spirit/intel_cursor.rs"
    cursor_items = [constant(spirit, name) for name in [
        "SPIRIT_CHANNEL_COUNT", "SPIRIT_CURSOR_DDB_BLOCKS",
        "SPIRIT_CURSOR_DDB", "SPIRIT_CURSOR_DDB_AB",
    ]]
    cursor_items.extend(item(spirit, name) for name in [
        "cursor_ddb_allocation", "dbuf_layout_tests",
    ])
    declarations.append("mod spirit_cursor_layout {\n" + "\n".join(cursor_items) + "\n}")
    fixture = (ROOT / "tools/testpy/fixtures/ab_dbuf_layout.rs").read_text()
    fixture = fixture.replace("@NATIVE_LAYOUT_PATH@", str(ROOT / "src/intel/display/tgl_ui4_layout.rs"))
    fixture = fixture.replace("// @PRODUCTION_ITEMS@", "\n".join(declarations))
    cursor_reservation = constant("src/intel/mod.rs", "SPIRIT_CURSOR_DBUF_S1_START")
    fixture = fixture.replace("// @CURSOR_RESERVATION@", cursor_reservation)
    channel = re.search(r'channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-ab-dbuf-") as directory:
        directory = Path(directory)
        source = directory / "tests.rs"
        source.write_text(fixture)
        for mode, flags in [
            ("debug", []),
            ("optimized", ["-O"]),
            ("kernel-release", ["-Copt-level=2", "-Clto=fat", "-Ccodegen-units=1"]),
        ]:
            binary = directory / mode
            subprocess.run(["rustup", "run", channel, "rustc", "--edition=2024",
                            "--test", *flags, str(source), "-o", str(binary)],
                           check=True, timeout=60)
            subprocess.run([str(binary)], check=True, timeout=30)


if __name__ == "__main__":
    main()
