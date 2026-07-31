#!/usr/bin/env python3
"""Verify that the Kokoro ConvInteger artifact retained SIMD16 DP4A."""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
ARTIFACT_ROOT = ROOT / "crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp"
KERNEL = "kokoro_conv1d_u8_u8"


def resolve_iga(requested: Path | None) -> Path:
    if requested is not None:
        candidate = requested.expanduser().resolve()
        if candidate.is_file():
            return candidate
        raise SystemExit(f"kokoro-conv1d-isa: missing IGA executable: {candidate}")
    from_path = shutil.which("iga64")
    if from_path is not None:
        return Path(from_path).resolve()
    sibling = (
        ROOT.parent
        / "blender-default-cube-toggle/lib/linux_x64/dpcpp/lib/igc/bin/iga64"
    )
    if sibling.is_file():
        return sibling.resolve()
    raise SystemExit("kokoro-conv1d-isa: iga64 was not found")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--iga", type=Path)
    args = parser.parse_args()

    binary_path = ARTIFACT_ROOT / f"{KERNEL}.bin"
    manifest_path = ARTIFACT_ROOT / f"{KERNEL}.manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    kernels = manifest["artifact"]["kernels"]
    if len(kernels) != 1 or kernels[0]["kernel_name"] != KERNEL:
        raise SystemExit("kokoro-conv1d-isa: artifact kernel set rejected")
    kernel = kernels[0]
    if (
        kernel["simd_width"] != 16
        or kernel["grf_count"] != 128
        or kernel["scratch_bytes"] != 0
        or kernel["slm_bytes"] != 0
    ):
        raise SystemExit("kokoro-conv1d-isa: execution environment rejected")

    entry_offset = int(kernel["text"]["entry_offset"])
    entry_size = int(kernel["text"]["entry_size"])
    binary = binary_path.read_bytes()
    if entry_size <= 0 or entry_size > 16_384 or entry_offset + entry_size > len(binary):
        raise SystemExit("kokoro-conv1d-isa: kernel entry bounds rejected")

    iga = resolve_iga(args.iga)
    with tempfile.TemporaryDirectory(prefix="trueos-kokoro-conv1d-isa.") as directory:
        temporary = Path(directory)
        kernel_path = temporary / "kernel.krn"
        assembly_path = temporary / "kernel.asm"
        kernel_path.write_bytes(binary[entry_offset : entry_offset + entry_size])
        completed = subprocess.run(
            [
                str(iga),
                "-d",
                "-p=12p1",
                "-Xprint-pc",
                str(kernel_path),
                "-o",
                str(assembly_path),
            ],
            check=False,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
        )
        if completed.returncode != 0:
            raise SystemExit(
                "kokoro-conv1d-isa: IGA disassembly failed:\n" + completed.stdout
            )
        assembly = assembly_path.read_text(encoding="utf-8")

    dp4a = len(re.findall(r"\bdp4a\s+\(16\|", assembly))
    byte_gathers = assembly.count("byte gathering read")
    instruction_lines = sum(
        1 for line in assembly.splitlines() if line.lstrip().startswith("/* [")
    )
    if dp4a < 2:
        raise SystemExit(
            f"kokoro-conv1d-isa: expected raw-dot and correction DP4A, found {dp4a}"
        )
    if byte_gathers != 0:
        raise SystemExit(
            f"kokoro-conv1d-isa: packed loop regressed to {byte_gathers} byte gathers"
        )
    if instruction_lines > 1_000:
        raise SystemExit("kokoro-conv1d-isa: instruction count exceeded guard")

    print(
        "kokoro-conv1d-isa: PASS "
        f"entry_bytes={entry_size} instructions={instruction_lines} "
        f"simd16_dp4a={dp4a} byte_gathers={byte_gathers} "
        "grf=128 scratch=0 slm=0"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
