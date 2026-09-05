#!/usr/bin/env python3
"""Test production VUE capture packets and summaries without accessing a GPU."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    primary = "src/intel/render/primary.rs"
    pipeline = "src/intel/render/pipeline.rs"
    constants = "src/intel/render/constants.rs"
    declarations = [constant(constants, name) for name in (
        "PIPE_CONTROL_CMD", "PIPE_CONTROL_CS_STALL", "PIPE_CONTROL_STALL_AT_SCOREBOARD",
        "CMD_3DSTATE_STREAMOUT", "CMD_3DSTATE_SO_BUFFER_INDEX_0",
        "RESULT_OA_BEGIN_DWORD", "RESULT_OA_END_DWORD", "RESULT_OA_REPORT_DWORDS",
    )]
    declarations += [constant(pipeline, name) for name in (
        "PICASSO_VUE_COUNTER_BEGIN_DWORD", "PICASSO_VUE_COUNTER_END_DWORD",
        "PICASSO_VUE_OFFSET_DWORD", "PICASSO_VUE_CS_CHICKEN1_DWORD",
        "PICASSO_VUE_FF_SLICE_CS_CHICKEN1_DWORD",
        "PICASSO_VUE_SELECTOR_SRM_SENTINEL",
        "PICASSO_VUE_L3ALLOC_DWORD", "PICASSO_VUE_L3ALLOC_SRM_SENTINEL",
        "PICASSO_VUE_RESULT_LIMIT_DWORD", "PICASSO_VUE_PREEMPTION_DELAY_DWORDS",
        "PICASSO_VUE_POSITION_RECORD_DWORDS", "PICASSO_VUE_PBR_RECORD_DWORDS",
    )]
    declarations += [item(pipeline, name) for name in (
        "picasso_vue_record_dwords", "picasso_vue_streamout_packets", "picasso_vue_counter_packets",
    )]
    declarations += [item(primary, name) for name in (
        "picasso_vue_capture_capacity", "picasso_vue_capture_complete", "picasso_vue_target_disjoint", "PicassoVueSummary",
        "summarize_picasso_vue_records", "picasso_vue_capture_tests",
        "PicassoVueSelectorReadback", "picasso_vue_selector_readback",
    )]
    with tempfile.TemporaryDirectory(prefix="trueos-vue-capture-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text("#![allow(dead_code)]\n" + "\n".join(declarations))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
