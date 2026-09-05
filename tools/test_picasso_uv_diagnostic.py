#!/usr/bin/env python3
"""Host-test the retained UV replay over an unchanged 48-byte PBR mesh."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main() -> None:
    state = "src/intel/render/state.rs"
    pipeline = "src/intel/render/pipeline.rs"
    churn = (ROOT / "crates/trueos-helio-runtime/src/churn.rs").read_text()
    # The host needs only these associated sizes, extracted from the real ABI.
    size_types = []
    for name in ("GpuCameraUniforms", "GpuInstanceData"):
        match = re.search(rf"impl {name} \{{\s*(pub const BYTE_LEN: usize = [^;]+;)", churn)
        if match is None:
            raise ValueError(f"Missing production {name}::BYTE_LEN")
        size_types.append(f"pub struct {name}; impl {name} {{ {match[1]} }}")
    source = (
        "#![allow(dead_code, unfulfilled_lint_expectations)]\n"
        "extern crate self as trueos_helio_runtime;\n"
        "extern crate self as trueos_helio_artifact;\n"
        "pub mod churn {\n" + "\n".join(size_types) + "\n}\n"
        "pub mod churn_forward {\n"
        + constant("crates/trueos-helio-artifact/src/churn_forward.rs", "VERTEX_STRIDE")
        + "\n}\n"
    )
    runtime = (ROOT / "crates/trueos-helio-runtime/src/lib.rs").read_text()
    indirect_size = re.search(r"impl DrawIndexedIndirectArgs \{\s*(pub const BYTE_LEN: usize = [^;]+;)", runtime)
    if indirect_size is None:
        raise ValueError("Missing production DrawIndexedIndirectArgs::BYTE_LEN")
    source += "pub struct DrawIndexedIndirectArgs; impl DrawIndexedIndirectArgs { " + indirect_size[1] + " }\n"
    source += "\n" + item("src/intel/render/constants.rs", "ChurnHardwareAdmission")
    source += "\n" + "\n".join(item(state, name) for name in (
        "TriangleIndexBufferPrep", "TriangleVertexFormat", "TriangleStorageBufferBinding",
        "TriangleSampledTextureBinding", "TriangleVfInstancingState", "TriangleNativeDrawContract",
        "TrianglePbrMaterial", "TriangleDrawPrep", "TriangleFrontEndContract",
    ))
    source += "\n" + "\n".join(constant(pipeline, name) for name in (
        "RETAINED_UV_POSITION_BYTE_OFFSET", "RETAINED_UV_TEXCOORD_BYTE_OFFSET",
    ))
    source += "\n" + item("src/intel/render/resources.rs", "configure_picasso_retained_uv_diagnostic")
    source += "\n" + "\n".join(item(pipeline, name) for name in (
        "validate_triangle_native_draw_contract", "native_raster_dw1",
        "retained_native_matrix_draw_contract_tests",
    ))
    resources = "src/intel/render/resources.rs"
    source += (
        "\nextern crate alloc;\nmod intel {\n"
        f'#[path = "{ROOT / "src/intel/shader.rs"}"] pub(crate) mod shader;\n'
        + item("src/intel/mod.rs", "align_up") + "}\n"
        # Substitute only synchronization; production shader loading and
        # metadata selection still execute unchanged in the host process.
        "mod spin { pub struct Mutex<T>(std::sync::Mutex<T>); impl<T> Mutex<T> {\n"
        "pub const fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }\n"
        "pub fn lock(&self) -> std::sync::MutexGuard<'_, T> { self.0.lock().unwrap() }\n"
        "} }\n"
        "static PICASSO_RETAINED_TEXTURED_PIPELINE: spin::Mutex<Option<intel::shader::TrianglePipeline>> = spin::Mutex::new(None);\n"
    )
    source += "\n" + "\n".join(constant(resources, name).replace(
        '"../../../picasso/', f'"{ROOT}/picasso/') for name in (
        "PICASSO_RETAINED_TEXTURED_VS", "PICASSO_RETAINED_TEXTURED_PS",
        "PICASSO_RETAINED_TEXTURED_PS8", "PICASSO_RETAINED_TEXTURED_PS8_WORDS",
    ))
    source += "\n" + "\n".join(item(resources, name) for name in (
        "churn_forward_stage_words", "picasso_retained_textured_pipeline",
        "picasso_retained_uv_simd8_words", "picasso_retained_uv_with_simd8_ps",
        "picasso_uv_simd8_tests",
    ))
    with tempfile.TemporaryDirectory(prefix="trueos-picasso-uv-diagnostic-") as temporary:
        directory = Path(temporary)
        rust_source = directory / "tests.rs"
        executable = directory / "tests"
        rust_source.write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(rust_source),
                        "-o", str(executable)], cwd=ROOT, check=True)
        subprocess.run([str(executable), "uv_diagnostic"], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
