#!/usr/bin/env python3
"""Host-check shared shader allocations and embedded Zebin/SPIR-V views."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-embedded-shaders-") as directory:
        work = Path(directory)
        helper = ROOT / "src/intel/gpgpu/artifacts/embedded.rs"
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(helper), "-o", str(work / "compute")
        ], cwd=ROOT, check=True)
        subprocess.run([str(work / "compute")], cwd=ROOT, check=True)

        # Compile the production metadata types and relevant pipelines without
        # pulling in unrelated hardware and external Voxy test fixtures.
        types = (ROOT / "src/intel/shader.rs").read_text().split(
            '#[path = "../../crates/trueos-shader/generated_patch_cube.rs"]'
        )[0]
        source = "#![allow(dead_code, unfulfilled_lint_expectations)]\n" + types
        for module in (
            "generated_triangle",
            "generated_clip_position3_uv_texture",
        ):
            path = ROOT / f"crates/trueos-shader/{module}.rs"
            source += f'#[path = "{path}"] mod {module};\n'
        source += r'''
#[test]
fn shared_sampled_fragment_matches_picasso_binary() {
    let words = &generated_clip_position3_uv_texture::SAMPLED_TEXTURE_PS_SIMD16_CODE;
    assert_eq!(bytes(words), include_bytes!("@ROOT@/crates/trueos-shader/clip_position3_uv_texture/sampled_texture.ps.simd16.bin"));
}
fn bytes(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}
'''.replace("@ROOT@", str(ROOT))
        rust = work / "graphics.rs"
        rust.write_text(source)
        subprocess.run([
            "rustc", "--edition=2024", "--test", str(rust), "-o", str(work / "graphics")
        ], cwd=ROOT, check=True)
        subprocess.run([str(work / "graphics")], cwd=ROOT, check=True)


if __name__ == "__main__":
    main()
