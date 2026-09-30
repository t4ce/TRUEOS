#!/usr/bin/env python3
"""Check production render-batch register addressing without a GPU."""
from pathlib import Path
import json
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    pipeline = "src/intel/render/pipeline.rs"
    constants = "src/intel/render/constants.rs"
    declarations = [constant(constants, name) for name in (
        "MI_LOAD_REGISTER_IMM", "MI_LRI_FORCE_POSTED",
        "GEN12_L3ALLOC", "GEN12_L3ALLOC_ADL_DEFAULT",
        "MI_LOAD_REGISTER_MEM", "MI_LRI_CS_MMIO", "RCS_RING_BASE",
        "RCS_3DPRIM_BASE_VERTEX", "RCS_3DPRIM_INSTANCE_COUNT",
        "RCS_3DPRIM_START_INSTANCE", "RCS_3DPRIM_START_VERTEX", "RCS_3DPRIM_VERTEX_COUNT",
        "RCS_3DPRIM_XP_BASE_VERTEX", "RCS_3DPRIM_XP_DRAW_ID",
        "TRIANGLE_VS_URB_START", "TRIANGLE_VS_URB_ENTRIES",
    )]
    declarations += [item(pipeline, name) for name in (
        "render_batch_lri_packet", "render_batch_lri_tests",
        "encode_draw_indexed_indirect_register_loads", "draw_indexed_indirect_encoder_tests",
    )]
    metadata = json.loads((ROOT / "picasso/picasso-retained-pbr-forward/metadata.json").read_text())
    units = metadata["vs_state"]["urb_entry_64b"]
    slots = metadata["vs_state"]["vue_slots"]
    declarations += [f"const PBR_ENTRY_BYTES: u32 = {units * 64};",
                     f"const PBR_OUTPUT_BYTES: u32 = {slots * 16};", r"""
#[test]
fn pbr_artifact_fits_requested_partition_but_not_reset_partition() {
    let packet = render_batch_lri_packet(GEN12_L3ALLOC, GEN12_L3ALLOC_ADL_DEFAULT).unwrap();
    let requested_bytes = ((packet[2] >> 1) & 0x7F) * 4 * 4096;
    let reset_bytes = ((0xD0000020u32 >> 1) & 0x7F) * 4 * 4096;
    let draw_bytes = TRIANGLE_VS_URB_START * 8192 + TRIANGLE_VS_URB_ENTRIES * PBR_ENTRY_BYTES;
    assert!(PBR_OUTPUT_BYTES <= PBR_ENTRY_BYTES);
    assert_eq!((draw_bytes, requested_bytes, reset_bytes), (490496, 524288, 262144));
    assert!(draw_bytes <= requested_bytes);
    assert!(draw_bytes > reset_bytes);
}
"""]
    with tempfile.TemporaryDirectory(prefix="trueos-render-lri-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text("#![allow(dead_code)]\n" + "\n".join(declarations))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
