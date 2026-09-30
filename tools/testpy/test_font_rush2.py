#!/usr/bin/env python3
"""Run the retained Rush2 engine face-cycle tests (the CPP command was removed)."""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def existing_test(path: str, name: str) -> str:
    source = (ROOT / path).read_text()
    matches = list(re.finditer(rf"^    #\[test\]\n    fn {re.escape(name)}\(", source, re.MULTILINE))
    if len(matches) != 1:
        raise ValueError(f"{path}: expected one test {name}")
    match = matches[0]
    end = re.search(r"^    }\n", source[match.end():], re.MULTILINE)
    if end is None:
        raise ValueError(f"{path}: missing end of {name}")
    return source[match.start():match.end() + end.end()]


def main() -> None:
    rush2 = "src/ui4/gpgpu_preview_consumer/font_rush2.rs"
    declarations = [constant(rush2, name) for name in ("CPP_FONT_RUSH2_FACE_MS", "CPP_FONT_RUSH2_FACES")]
    declarations += [item(rush2, "cpp_font_rush2_next_face_index")]
    declarations += [existing_test(rush2, "cpp_font_rush2_rotates_all_registered_faces_every_thirty_seconds")]
    source = "#![allow(dead_code)]\nmod intel { pub mod gpu_font {\n"
    source += item("src/intel/gpu_font.rs", "GpuFontFace") + "\n}}\n"
    source += "\n".join(declarations)
    with tempfile.TemporaryDirectory(prefix="trueos-font-rush2-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source),
                        "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)
    print("Retained Rush2 engine face-cycle tests passed")


if __name__ == "__main__":
    main()
