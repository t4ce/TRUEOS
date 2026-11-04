#!/usr/bin/env python3
"""Compare strict/relaxed Edit native ISA with the pinned local ocloc.

This reports static code and resource counts. It does not measure GPU speed.
Both inputs must have identical SPIR-V and matching bakery manifests.
"""

from __future__ import annotations

import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess


KERNEL = "edit_life_rgba8"
INSTRUCTION = re.compile(r"^\s*(?:\([^)]*\)\s*)?([a-z][a-z0-9_.]*)\b")


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def instructions(assembly: str) -> list[tuple[int, int, str, str]]:
    result = []
    offset = 0
    for line in assembly.splitlines():
        label = re.fullmatch(r"L(\d+):", line)
        if label:
            if int(label[1]) != offset:
                raise ValueError(f"IGA label {label[1]} disagrees with decoded offset {offset}")
            continue
        code = line.split("//", 1)[0]
        match = INSTRUCTION.match(code)
        if not match:
            continue
        opcode = match[1]
        if opcode in ("call", "calla"):
            raise ValueError("Outlined ISA helpers require a different instruction-count boundary")
        size = 8 if "Compacted" in code else 16
        result.append((offset, size, opcode, code))
        offset += size
        if "EOT" in code:
            return result
    raise ValueError("IGA assembly has no end-of-thread instruction")


def mode_blocks(decoded: list[tuple[int, int, str, str]]) -> dict:
    """Recognize this kernel's mode<2/mode<3 dispatch before naming blocks."""
    branches = []
    for index, instruction in enumerate(decoded):
        if instruction[2] != "if":
            continue
        expected = len(branches) + 2
        predicate = re.search(r"\(~(f\d\.\d)\)\s+if", instruction[3])
        labels = [int(value) for value in re.findall(r"\bL(\d+)\b", instruction[3])]
        preceding = "\n".join(item[3] for item in decoded[max(0, index - 4):index])
        condition = (rf"\bcmp\s+\([^)]*\)\s+\(lt\){re.escape(predicate[1])}"
                     rf"\s+.*\s+(?:{expected}:w|0x{expected}:uw)\b") if predicate else ""
        if not predicate or len(labels) != 2 or not re.search(condition, preceding):
            return {"validated": False, "reason": "Compiler mode-dispatch layout was not recognized"}
        branches.append(labels)
        if len(branches) == 2:
            break
    if len(branches) != 2:
        return {"validated": False, "reason": "Compiler mode-dispatch branches were not found"}
    boundaries = {item[0] for item in decoded}
    result = {"validated": True}
    for name, (start, end) in (("composition_mode2", branches[1]), ("seed_step_modes0_1", branches[0])):
        if start not in boundaries or end not in boundaries or start >= end:
            return {"validated": False, "reason": "Mode branch bounds do not match decoded instructions"}
        subset = [item for item in decoded if start <= item[0] < end]
        result[name] = {
            "start": start, "end": end, "bytes": end - start,
            "static_instructions": len(subset),
            "opcodes": dict(sorted(Counter(item[2] for item in subset).items())),
        }
    return result


def analyze(binary: Path, dump: Path, ocloc: Path, environment: dict[str, str]) -> dict:
    manifest = json.loads(binary.with_suffix(".manifest.json").read_text())
    binary_hash = digest(binary)
    spirv_hash = digest(binary.with_suffix(".spv"))
    if manifest["artifact"]["elf"]["sha256"] != binary_hash:
        raise ValueError(f"Bakery manifest does not match {binary}")
    if manifest["artifact"]["spirv"]["sha256"] != spirv_hash:
        raise ValueError(f"Bakery manifest does not match {binary.with_suffix('.spv')}")
    kernels = manifest["artifact"]["kernels"]
    if len(kernels) != 1 or kernels[0]["kernel_name"] != KERNEL:
        raise ValueError(f"Expected exactly one {KERNEL} kernel in {binary}")
    kernel = kernels[0]
    dump.mkdir(parents=True, exist_ok=True)
    subprocess.run([str(ocloc), "disasm", "-file", str(binary), "-device", "adl-s", "-dump", str(dump)],
                   env=environment, check=True)
    assembly = dump / f".text.{KERNEL}.asm"
    decoded = instructions(assembly.read_text())
    histogram = Counter(item[2] for item in decoded)
    return {
        "binary": str(binary), "assembly": str(assembly),
        "binary_bytes": binary.stat().st_size, "binary_sha256": binary_hash,
        "spirv_sha256": spirv_hash, "text_entry_bytes": kernel["text"]["entry_size"],
        "code_through_eot_bytes": decoded[-1][0] + decoded[-1][1],
        "static_instructions_through_eot": len(decoded),
        **{key: kernel[key] for key in ("grf_count", "scratch_bytes", "slm_bytes", "simd_width")},
        "send_instructions": sum(value for opcode, value in histogram.items() if opcode.startswith("send.")),
        "math_integer_quotient_instructions": histogram["math.iqot"],
        "math_reciprocal_instructions": histogram["math.inv"],
        "fp_typed_instructions": dict(sorted(Counter(item[2] for item in decoded if ":f" in item[3]).items())),
        "opcode_counts": dict(sorted(histogram.items())), "blocks": mode_blocks(decoded),
        "profile": manifest["provenance"]["profile"],
        "abi": {key: kernel[key] for key in ("bindings", "cross_thread_data_bytes", "per_thread_data_bytes",
                 "payload_args", "implicit_payload_args", "per_thread_payload_args")},
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--toolchain", type=Path, default=Path("/tmp/trueos-edit-life-toolchain"))
    parser.add_argument("--root", type=Path, default=Path("/tmp/trueos-edit-math"))
    parser.add_argument("--strict", type=Path, default=Path("strict"), help="Artifact directory, relative to --root or absolute")
    parser.add_argument("--relaxed", type=Path, default=Path("relaxed"), help="Artifact directory, relative to --root or absolute")
    parser.add_argument("--output", type=Path, help="JSON destination; defaults to --root/code-comparison.json")
    args = parser.parse_args()
    root = args.root.expanduser().resolve()
    toolchain = args.toolchain.expanduser().resolve() / "root"
    ocloc = toolchain / "usr/bin/ocloc-26.05.1"
    library_path = toolchain / "usr/lib/x86_64-linux-gnu"
    if not ocloc.is_file() or not library_path.is_dir():
        parser.error(f"Pinned ocloc toolchain is missing under {toolchain}")
    environment = os.environ.copy()
    environment["LD_LIBRARY_PATH"] = str(library_path) + (
        os.pathsep + environment["LD_LIBRARY_PATH"] if environment.get("LD_LIBRARY_PATH") else "")
    variants = {}
    for name, directory in (("strict", args.strict), ("relaxed", args.relaxed)):
        binary = (root / directory.expanduser()).resolve() / f"{KERNEL}.bin"
        variants[name] = analyze(binary, root / "isa" / name, ocloc, environment)
    strict, relaxed = variants["strict"], variants["relaxed"]
    if strict["spirv_sha256"] != relaxed["spirv_sha256"]:
        raise ValueError("Backend math comparison requires identical SPIR-V inputs")
    report = {
        "method": "Pinned ocloc/IGA disassembly; static counts through first EOT exclude compiler footer/padding. No timings are measured.",
        "variants": variants, "identical_spirv": True, "identical_abi": strict["abi"] == relaxed["abi"],
        "changes": {key: {"strict": strict[key], "relaxed": relaxed[key],
                    "delta": relaxed[key] - strict[key],
                    "percent": (relaxed[key] - strict[key]) * 100 / strict[key]}
                    for key in ("binary_bytes", "text_entry_bytes", "code_through_eot_bytes", "static_instructions_through_eot")},
    }
    output = args.output.expanduser().resolve() if args.output else root / "code-comparison.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps({"changes": report["changes"], "identical_spirv": True,
                     "identical_abi": report["identical_abi"], "report": str(output)}, indent=2))


if __name__ == "__main__":
    main()
