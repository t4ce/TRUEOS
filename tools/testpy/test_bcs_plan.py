#!/usr/bin/env python3
"""Run the production BCS planner and sparse PPGTT tests without GPU access.

The kernel binary disables Cargo's test harness. Compile the exact planner and
mapping modules alongside their actual validation functions, with no hardware
or scheduling implementation replaced. Run from any directory with
``python3 tools/testpy/test_bcs_plan.py``.
"""

from pathlib import Path
import json
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def harness_source() -> str:
    driver = "src/intel/copy/blt.rs"
    declarations = [
        constant(driver, name)
        for name in (
            "DIRECT_BLT_PPGTT_PD_COUNT",
            "DIRECT_BLT_PPGTT_PT_COUNT",
            "DIRECT_BLT_PPGTT_LIMIT_BYTES",
            "BCS0_GGTT_BASE",
            "BCS0_GGTT_LIMIT",
        )
    ]
    declarations.extend(
        item(driver, name)
        for name in (
            "GucBcs0RgbaSurface",
            "GucBcs0RgbaCopy",
            "GucBcs0RgbaFill",
            "GucBcs0CopySubmitError",
            "guc_blt_valid_surface",
            "guc_blt_valid_copy",
            "guc_blt_valid_legacy_copy",
            "guc_blt_gpu_ranges_overlap",
            "guc_blt_physical_ranges_overlap",
            "guc_blt_source_mappings_compatible",
            "guc_blt_valid_fill",
            "guc_blt_valid_fill_rect",
        )
    )
    return (
        "#![allow(dead_code)]\nextern crate alloc;\nmod r {pub mod ui_surface {\n"
        + constant("src/r/ui_surface.rs", "UI_SURFACE_GPU_SPILL_LIMIT")
        + "\n}}\nmod intel {\n"
        + constant("src/intel/mod.rs", "WARM_ALIGN")
        + "\nmod blt {\n"
        + "\n".join(declarations)
        + "\n}\n#[path = "
        + json.dumps(str(ROOT / "src/intel/copy/bcs_plan.rs"))
        + "] mod bcs_plan;\n#[path = "
        + json.dumps(str(ROOT / "src/intel/copy/bcs_mappings.rs"))
        + "] mod bcs_mappings;\n}\n"
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-bcs-plan-") as temporary:
        directory = Path(temporary)
        source, executable = directory / "tests.rs", directory / "tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            check=True,
            cwd=ROOT,
        )
        subprocess.run([str(executable)], check=True)


if __name__ == "__main__":
    main()
