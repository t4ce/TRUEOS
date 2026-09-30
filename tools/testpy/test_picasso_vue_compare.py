#!/usr/bin/env python3
"""Run production same-frame VUE reference tests without a GPU or kernel build."""
from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    source_path = "src/intel/render/picasso_vue_compare.rs"
    pipeline = "src/intel/render/pipeline.rs"
    declarations = [constant(pipeline, name) for name in (
        "PICASSO_VUE_POSITION_RECORD_DWORDS", "PICASSO_VUE_PBR_RECORD_DWORDS",
    )]
    declarations += [item(pipeline, "picasso_vue_record_dwords")]
    declarations += [item(source_path, name) for name in (
        "PicassoVueCompareInputs", "PicassoVueCompareError", "PicassoVueMismatch",
        "PicassoVueVaryingComparison", "PicassoVueVaryingMismatch",
        "PicassoVueComparison", "picasso_vue_compare_error", "picasso_vue_byte_range",
        "picasso_vue_read_u32", "picasso_vue_read_matrix", "picasso_vue_gpu_range",
        "compare_picasso_vue_records", "picasso_vue_compare_tests",
    )]
    with tempfile.TemporaryDirectory(prefix="trueos-vue-compare-tests-") as temporary:
        directory = Path(temporary)
        source = directory / "host_tests.rs"
        executable = directory / "host_tests"
        source.write_text("#![allow(dead_code)]\n" + "\n".join(declarations))
        subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(executable)],
                       cwd=ROOT, check=True)
        subprocess.run([str(executable)], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
