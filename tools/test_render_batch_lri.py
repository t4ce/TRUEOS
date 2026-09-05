#!/usr/bin/env python3
"""Check production render-batch register addressing without a GPU."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    pipeline = "src/intel/render/pipeline.rs"
    constants = "src/intel/render/constants.rs"
    declarations = [constant(constants, name) for name in (
        "MI_LOAD_REGISTER_IMM", "MI_LRI_FORCE_POSTED",
        "GEN12_L3ALLOC", "GEN12_L3ALLOC_ADL_DEFAULT",
    )]
    declarations += [item(pipeline, name) for name in (
        "render_batch_lri_packet", "render_batch_lri_tests",
    )]
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
