#!/usr/bin/env python3
"""Exercise AVC header-to-command propagation and ordered decode status capture."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def constant(source, name):
    return re.search(
        rf"^(?:pub\([^)]*\) )?const {name}:.*?;", source,
        re.MULTILINE | re.DOTALL,
    )[0]


def main():
    engine = (ROOT / "src/intel/media/engine.rs").read_text()
    backend = (ROOT / "src/intel/media/pic_backend.rs").read_text()
    media_names = (
        "MI_STORE_DWORD_IMM_GEN4", "MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT",
        "MI_FLUSH_DW", "MI_FLUSH_DW_VIDEO_PIPELINE_CACHE_INVALIDATE",
        "MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE", "MI_ARB_CHECK",
        "MI_BATCH_BUFFER_END", "MI_NOOP", "MEDIA_RESULT_SLOT_BYTES",
        "MEDIA_RESULT_KICKOFF_SLOT", "MEDIA_RESULT_PRESUBMIT_SLOT",
        "MEDIA_RESULT_POSTSUBMIT_SLOT", "MEDIA_RESULT_COMPLETE_SLOT",
        "MEDIA_RESULT_BITSTREAM_ADDR_LO_SLOT", "MEDIA_RESULT_BITSTREAM_ADDR_HI_SLOT",
        "MEDIA_RESULT_BITSTREAM_BYTES_SLOT", "MEDIA_RESULT_SAMPLE_NALS_SLOT",
        "MEDIA_RESULT_STAGE_FLAGS_SLOT", "MEDIA_RESULT_OUTPUT_SURFACE_ADDR_LO_SLOT",
        "MEDIA_RESULT_OUTPUT_SURFACE_ADDR_HI_SLOT", "MEDIA_RESULT_OUTPUT_SURFACE_BYTES_SLOT",
        "MEDIA_RESULT_FRAME_DIMS_SLOT",
    )
    media = "\n".join(constant(engine, name) for name in media_names)
    media += "\n" + "\n".join(item(engine, signature) for signature in (
        "pub(super) fn emit_store_dword_ppgtt", "pub(super) fn begin_batch_packet",
    ))
    definitions = "\n".join(constant(backend, name) for name in (
        "AVC_RESULT_ERROR_STATUS_SLOT", "AVC_RESULT_MB_COUNT_SLOT",
        "AVC_STATUS_NOT_CAPTURED", "AVC_DECODE_ERROR_MASK", "AVC_MI_STORE_REGISTER_MEM_PPGTT",
        "MEDIA_STAGE_FLAG_AVC_SINGLE_IDR", "MEDIA_STAGE_FLAG_AVC_REAL_SCRATCH",
        "MEDIA_STAGE_FLAG_AVC_COMMAND_STREAM",
    ))
    definitions += "\n#[derive(Copy, Clone, Debug)]\n"
    definitions += item(backend, "pub(super) struct AvcDecodeHealth")
    definitions += "\n" + "\n".join(item(backend, signature) for signature in (
        "impl AvcDecodeHealth", "fn avc_decode_status_registers",
        "fn emit_avc_decode_status", "fn build_avc_single_idr_batch_skeleton",
    ))
    harness = (ROOT / "tools/testpy/fixtures/avc_decode_correctness.rs").read_text()
    harness = harness.replace("@AVC_PATH@", str(ROOT / "src/intel/media/h264_cmd.rs"))
    harness = harness.replace("@SFC_PATH@", str(ROOT / "src/intel/media/sfc_cmd.rs"))
    harness = harness.replace("@MEDIA@", media).replace("@BACKEND@", definitions)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-avc-decode-correctness-") as directory:
        directory = Path(directory)
        source = directory / "tests.rs"
        source.write_text(harness)
        for optimization in ([], ["-O"]):
            binary = directory / ("tests-release" if optimization else "tests-debug")
            subprocess.run([
                "rustc", "+" + channel, "--edition=2024", "--test", str(source),
                "-o", str(binary), *optimization,
            ], check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
