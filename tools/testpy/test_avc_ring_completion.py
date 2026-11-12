#!/usr/bin/env python3
"""Execute the encoder's real ring epilogue and IRQ-driven completion waiter."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    engine = (ROOT / "src/intel/media/engine.rs").read_text()
    encoder = (ROOT / "src/intel/media/avc_encode_probe.rs").read_text()
    interrupts = (ROOT / "src/intel/interrupts.rs").read_text()
    constants = []
    for name in (
        "MI_STORE_DWORD_IMM_GEN4", "MI_STORE_DWORD_IMM_GEN4_LEN_DW4_PPGTT",
        "MI_FLUSH_DW_LEN_DW4", "MI_FLUSH_DW_USE_GGTT",
        "MI_FLUSH_DW_POST_SYNC_WRITE_IMMEDIATE", "MI_USER_INTERRUPT",
        "MI_BATCH_BUFFER_START_GEN8", "MI_BATCH_PPGTT", "MI_BATCH_SECOND_LEVEL",
        "MI_ARB_CHECK", "MI_BATCH_BUFFER_END", "MI_NOOP",
        "MEDIA_RESULT_SLOT_BYTES", "MEDIA_RESULT_KICKOFF_SLOT", "MEDIA_RESULT_PRESUBMIT_SLOT",
        "MEDIA_RESULT_POSTSUBMIT_SLOT", "MEDIA_RESULT_COMPLETE_SLOT",
    ):
        constants.append(re.search(
            rf"^(?:pub\([^)]*\) )?const {name}:.*?;", engine, re.MULTILINE | re.DOTALL
        )[0])
    definitions = "\n".join(constants)
    definitions += "\n" + "\n".join(
        "#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n" + item(engine, signature)
        for signature in (
            "pub(super) enum MediaCodecMode", "pub(super) enum MediaSubmissionOwner",
            "pub(super) enum MediaBatchLevel", "pub(super) struct MediaJobMode",
        )
    )
    definitions += "\n" + "\n".join(item(engine, signature) for signature in (
        "impl MediaJobMode", "pub(super) fn build_ring_batch_start_words",
        "pub(super) fn append_ring_batch_start_completion_words",
        "pub(super) fn build_primary_second_level_return_words",
    ))
    definitions += "\n" + item(encoder, "fn next_completion_sequence")
    definitions += "\n#[derive(Copy, Clone, Debug, Eq, PartialEq)]\n"
    definitions += item(encoder, "struct RingCompletionWait")
    definitions += "\n" + item(encoder, "fn read_ring_completion")
    definitions += "\n" + item(encoder, "async fn wait_for_ring_completion")
    definitions = re.sub(r"pub\(super\) ", "", definitions)
    harness = (ROOT / "tools/testpy/fixtures/avc_ring_completion.rs").read_text()
    harness = harness.replace("@PRODUCTION@", definitions)
    harness = harness.replace("@IRQ_WAITER@", item(
        interrupts, "pub(super) async fn wait_vcs0_after"
    ).replace("pub(super)", "pub(crate)"))
    harness = harness.replace("@GENERATION_PATH@", str(ROOT / "src/intel/interrupts/generation.rs"))
    harness = harness.replace("@TIMEOUT_NS@", re.search(
        r"^const TIMEOUT_NS:.*?;", encoder, re.MULTILINE
    )[0])
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-avc-ring-completion-") as directory:
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
