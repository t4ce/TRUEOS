#!/usr/bin/env python3
"""Host-test the production Picasso primary counter packets without GPU access."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    primary = "src/intel/render/primary.rs"
    constants = "src/intel/render/constants.rs"
    names = [
        "GPU_VA_BATCH_BASE", "RESIDENT_SCENE_PRIMARY_BATCH_BYTES",
        "RESIDENT_SCENE_SECONDARY_BATCH_BYTES", "MI_BATCH_BUFFER_START_GEN8",
        "MI_BATCH_2ND_LEVEL", "MI_BATCH_PPGTT", "MI_STORE_DATA_IMM_GGTT_DW1",
        "MI_BATCH_BUFFER_END", "MI_NOOP", "RESULT_SLOT_SCENE_FRAME_DWORD",
        "RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_LO", "RCS_EXEC_RESULT_SCENE_RCS_RELEASE_DONE_HI",
        "PIPE_CONTROL_CMD", "PIPE_CONTROL_CS_STALL", "PIPE_CONTROL_STALL_AT_SCOREBOARD",
        "PIPE_CONTROL_SCENE_COLOR_RELEASE_HEADER_BITS", "PIPE_CONTROL_SCENE_COLOR_RELEASE_BITS",
        "PIPE_CONTROL_SCENE_RELEASE_MARKER_BITS", "PIPE_CONTROL_HDC_PIPELINE_FLUSH_HEADER",
        "PIPE_CONTROL_DEPTH_CACHE_FLUSH", "PIPE_CONTROL_DC_FLUSH_ENABLE",
        "PIPE_CONTROL_RENDER_TARGET_CACHE_FLUSH", "PIPE_CONTROL_DEPTH_STALL",
        "PIPE_CONTROL_TILE_CACHE_FLUSH", "PIPE_CONTROL_FLUSH_ENABLE",
        "PIPE_CONTROL_L3_FABRIC_FLUSH", "PIPE_CONTROL_POST_SYNC_WRITE_IMMEDIATE",
        "RESULT_OA_BEGIN_DWORD",
    ]
    declarations = [constant(constants, name) for name in names]
    declarations += [constant(primary, name) for name in (
        "RESULT_SLOT_SECONDARY_RETURN_DWORD", "RCS_EXEC_RESULT_SECONDARY_RETURN_BASE",
        "PICASSO_PIPELINE_STATS_BEGIN_DWORD", "PICASSO_PIPELINE_STATS_END_DWORD",
        "PICASSO_PIPELINE_STATS_LIMIT_DWORD", "PICASSO_PIPELINE_STAT_REGISTERS",
    )]
    declarations.append(constant("src/intel/render/pipeline.rs", "RESULT_SLOT_DEPTH_STATE_WA_DWORD"))
    declarations += [item(primary, name) for name in (
        "emit_picasso_pipeline_stat_snapshot", "picasso_pipeline_stats_delta",
        "encode_resident_scene_primary_commands", "picasso_pipeline_stats_tests",
    )]
    stats = ROOT / "src/intel/stats.rs"
    source = '#![allow(dead_code)]\nmod intel { pub mod stats { include!("' + str(stats) + '"); } }\n'
    source += "\n".join(declarations)
    with tempfile.TemporaryDirectory(prefix="trueos-picasso-stats-tests-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        rust_source.write_text(source)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(rust_source), "-o", str(executable)],
            cwd=ROOT, check=True,
        )
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
