#!/usr/bin/env python3
"""Host-test retained material validation/packing and the versioned wire layout.

Extract the production ABI records and pure broker helpers because the kernel
itself has test=false. No GPU submission or generated replacement logic is used.
"""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def harness_source() -> str:
    abi = "crates/trueos-v/src/vgpu.rs"
    broker = "src/gpu/vgpu.rs"
    declarations = [
        constant(abi, name)
        for name in (
            "RETAINED_MATERIAL_FLAG_DOUBLE_SIDED",
            "RETAINED_MATERIAL_TEXTURE_COUNT",
            "MAX_RETAINED_TRANSFORM_SEEDS",
            "MAX_RETAINED_STATIC_DRAWS",
            "MAX_RETAINED_SCENE_INSTANCES",
            "MAX_RETAINED_SCENE_DRAWS",
        )
    ]
    declarations.extend(
        item(abi, name)
        for name in (
            "RetainedMaterialParameters",
            "RetainedMaterial",
            "RetainedCamera",
            "RetainedTransformSeed",
            "IndexedBatchDrawV2",
            "RetainedFrameSubmit",
            "RetainedFrameSubmitV2",
            "RetainedDrawRange",
            "RetainedFrameSubmitV3",
        )
    )
    defaults = re.findall(
        r"^impl Default for RetainedMaterialParameters \{.*?^}\n",
        (ROOT / abi).read_text(),
        re.MULTILINE | re.DOTALL,
    )
    if len(defaults) != 1:
        raise ValueError("Expected one production material default implementation")
    declarations.extend(defaults)
    kernel = "\n".join(
        item(broker, name)
        for name in (
            "pack_retained_material_parameters",
            "retained_material_contract_accepts",
            "retained_material_parameters_tests",
        )
    )
    # Alias the harness crate as v so the exact production paths resolve.
    return (
        "#![allow(dead_code)]\nextern crate self as v;\npub mod vgpu {\n"
        + "\n".join(declarations)
        + "\n}\n"
        + kernel
        + r"""
#[test]
fn v2_keeps_the_v1_frame_prefix_and_exact_extension_size() {
    use vgpu::*;
    use core::mem::{align_of, offset_of, size_of};
    assert_eq!(size_of::<RetainedMaterial>(), 56);
    assert_eq!(size_of::<RetainedFrameSubmit>(), 816);
    assert_eq!(offset_of!(RetainedFrameSubmit, camera), 120);
    assert_eq!(offset_of!(RetainedFrameSubmit, seeds), 488);
    assert_eq!(size_of::<RetainedMaterialParameters>(), 64);
    assert_eq!(offset_of!(RetainedMaterialParameters, normal_scale), 28);
    assert_eq!(offset_of!(RetainedMaterialParameters, flags), 48);
    assert_eq!(size_of::<RetainedFrameSubmitV2>(), 880);
    assert_eq!(align_of::<RetainedFrameSubmitV2>(), 8);
    assert_eq!(offset_of!(RetainedFrameSubmitV2, frame), 0);
    assert_eq!(offset_of!(RetainedFrameSubmitV2, material_parameters), 816);
}

#[test]
fn v3_keeps_both_older_prefixes_and_fits_the_vm_payload() {
    use vgpu::*;
    use core::mem::{align_of, offset_of, size_of};
    assert_eq!(MAX_RETAINED_TRANSFORM_SEEDS, 4);
    assert_eq!(size_of::<RetainedTransformSeed>(), 64);
    assert_eq!(offset_of!(RetainedTransformSeed, draw_group), 56);
    assert_eq!(offset_of!(RetainedTransformSeed, flags), 60);
    assert_eq!(size_of::<RetainedDrawRange>(), 8);
    assert_eq!(size_of::<RetainedFrameSubmitV3>(), 944);
    assert_eq!(align_of::<RetainedFrameSubmitV3>(), 8);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, frame), 0);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, seed_buffer), 880);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, seed_offset), 888);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, seed_count), 896);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, draw_count), 900);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, draws), 904);
    assert_eq!(offset_of!(RetainedFrameSubmitV3, reserved), 936);
}
"""
    )


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-retained-material-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text(harness_source())
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
