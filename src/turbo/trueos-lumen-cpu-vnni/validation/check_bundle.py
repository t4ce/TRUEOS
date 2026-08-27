#!/usr/bin/env python3
"""Static consistency checks for the integration bundle itself."""

from __future__ import annotations

import ast
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def require(text: str, needle: str, label: str) -> None:
    if needle not in text:
        raise AssertionError(f"missing {label}: {needle!r}")


def forbid(text: str, needle: str, label: str) -> None:
    if needle in text:
        raise AssertionError(f"forbidden {label}: {needle!r}")


def main() -> None:
    apply_path = ROOT / "apply.py"
    apply_text = apply_path.read_text(encoding="utf-8")
    ast.parse(apply_text, filename=str(apply_path))
    transformer_check = ROOT / "validation/check_transformer.py"
    ast.parse(
        transformer_check.read_text(encoding="utf-8"),
        filename=str(transformer_check),
    )
    require(
        apply_text,
        "02e0e7a8add0fd793d8fd8d5084c0c7e7dc9a3b8",
        "current TRUEOS source anchor",
    )

    kernel_path = ROOT / "files/crates/trueos-lfm25-cpu/src/cpu_vnni.rs"
    kernel = kernel_path.read_text(encoding="utf-8")
    for needle, label in (
        ("pub const Q8_VNNI_ROWS_PER_TILE: usize = 4;", "four-row tile"),
        ('#[target_feature(enable = "avx2,avxvnni,fma")]', "target-feature gate"),
        ("_mm256_sign_epi8", "VPSIGNB intrinsic"),
        ("_mm256_dpbusd_avx_epi32", "VPDPBUSD intrinsic"),
        ("_mm256_setzero_si256(),", "zeroed VPDPBUSD destination"),
        ("_mm256_cvtepi32_ps", "i32-to-f32 conversion"),
        ("_mm256_fmadd_ps", "FMA intrinsic"),
        ("let a0 = lanes[0] + lanes[4];", "fixed reduction level one"),
        ("let b0 = a0 + a2;", "fixed reduction level two"),
        ("b0 + b1", "fixed reduction root"),
        ("block[2..].contains(&0x80)", "weight -128 admission"),
        ("if quant == i8::MIN", "activation -128 admission"),
        ("pub fn project_rows(", "contiguous row-range surface"),
    ):
        require(kernel, needle, label)


    if kernel.count("_mm256_dpbusd_avx_epi32(") != 1:
        raise AssertionError("kernel must contain exactly one VPDPBUSD operation site")

    for needle, label in (
        ("_mm256_dpbusds", "saturating byte VNNI"),
        ("_mm256_dpwssd", "word VNNI"),
        ("_mm256_dpwssds", "saturating word VNNI"),
        ("_mm_dpbusd", "128-bit byte VNNI"),
        ("_mm_dpbusds", "128-bit saturating byte VNNI"),
        ("_mm_dpwssd", "128-bit word VNNI"),
        ("_mm_dpwssds", "128-bit saturating word VNNI"),
        ("zero_point", "zero-point correction"),
        ("weight_sum", "weight-sum correction"),
    ):
        forbid(kernel, needle, label)

    readme = (ROOT / "README.md").read_text(encoding="utf-8")
    for needle in ("93", "354,418,688", "11,075,584", "376,569,856"):
        require(readme, needle, "fixed model arithmetic")

    visualizer = (ROOT / "reference/vnni_factorio_demo.html").read_text(encoding="utf-8")
    require(visualizer, "AVX‑VNNI Dataflow Factory", "uploaded visualizer")
    require(visualizer, "dpbusd_256", "visualizer VPDPBUSD 256 card")

    boot_reference = (ROOT / "reference/lfm25_boot_warm.cpu_vnni.rs").read_text(
        encoding="utf-8"
    )
    require(boot_reference, "Q8VnniProjector::detect()", "CPU boot admission")
    forbid(boot_reference, "crate::intel", "GPU boot admission")

    print("bundle static consistency: PASS")


if __name__ == "__main__":
    main()
