#!/usr/bin/env python3
"""Host-test the actual kernel staging/admission code against Blueprint packages."""
import json
import os
import shutil
import subprocess
import tempfile
from pathlib import Path

from package_blueprint import ROOT, PROGRAMS


def main():
    bp = Path(os.environ.get("TRUEOS_BLUEPRINTS_ROOT", ROOT.parent / "TRUEOS-Blueprints"))
    subprocess.run(["python3", str(Path(__file__).with_name("package_blueprint.py")),
                    "--blueprints-root", str(bp), "--check"], check=True)
    gpu = ROOT / "src/intel/gpgpu"
    # Compile the production pure admission functions, without hardware upload code.
    contract = (gpu / "artifacts/contract.rs").read_text().split("#[cfg(test)]")[0]
    metadata = (gpu / "artifacts/metadata.rs").read_text().split(
        "pub(crate) const COPY_RECT_RGBA8_ADLS_ARTIFACT")[0]
    runtime = (gpu / "artifacts/runtime.rs").read_text()
    runtime = runtime[runtime.index("#[derive(Copy, Clone, Debug, Eq, PartialEq)]\npub(crate) enum GpgpuArtifactAdmissionError"):]
    runtime = runtime.split("#[cfg(test)]")[0]
    code = "#![allow(dead_code)]\nextern crate alloc;\nuse sha2::{Digest, Sha256};\nuse core::fmt::Write;\n"
    code += contract + metadata + runtime
    code += f'\n#[path = "{gpu}/artifacts/shadertoy_package.rs"] mod package;\n'
    for _, name in PROGRAMS:
        code += f'include!("{bp}/apps/shadertoy/assets/{name}/kernel.contract.rs");\n'
    code += 'fn fixtures() -> Vec<(u32, &\'static [u8], &\'static GpgpuKernelAbiContract)> { vec![\n'
    for index, name in PROGRAMS:
        manifest = json.loads((bp / f"apps/shadertoy/assets/{name}/kernel.manifest.json").read_text())
        for symbol in manifest["rust_symbols"].values():
            code += f'({index}, include_bytes!("{bp}/apps/shadertoy/assets/{name}.stpkg"), &{symbol}),\n'
    code += '] }\n' + TESTS
    with tempfile.TemporaryDirectory(prefix="trueos-shadertoy-packages-") as temporary:
        project = Path(temporary)
        # The Blueprint's build guard must reject edited source beside a stale bundle.
        guard = project / "check-assets"
        subprocess.run(["rustc", "--edition=2024", str(bp / "apps/shadertoy/build.rs"),
                        "-o", str(guard)], check=True)
        shutil.copytree(bp / "apps/shadertoy/assets", project / "assets")
        environment = dict(os.environ, CARGO_MANIFEST_DIR=str(project))
        subprocess.run([str(guard)], env=environment, check=True, stdout=subprocess.DEVNULL)
        source = project / "assets/mandelbrot/input.glsl"
        source.write_bytes(source.read_bytes() + b"\n// unbundled edit\n")
        rejected = subprocess.run([str(guard)], env=environment, capture_output=True, text=True)
        assert rejected.returncode != 0 and "stale package" in rejected.stderr
        source.write_bytes(source.read_bytes().removesuffix(b"\n// unbundled edit\n"))
        native_source = project / "assets/cpp_gallery/input.sources.json"
        native_source.write_bytes(native_source.read_bytes() + b"\n ")
        rejected = subprocess.run([str(guard)], env=environment, capture_output=True, text=True)
        assert rejected.returncode != 0 and "stale package" in rejected.stderr
        print("Blueprint stale-source build guards (GLSL and native archive): passed")
        (project / "Cargo.toml").write_text('[package]\nname="shadertoy-package-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nsha2="0.10"\n[workspace]\n')
        (project / "src").mkdir()
        (project / "src/lib.rs").write_text(code)
        subprocess.run(["cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu",
                        "--target-dir", str(ROOT / "bld/shadertoy-package-host-tests")],
                       cwd=project, check=True)


TESTS = r'''
#[test]
fn gallery_selectors_require_their_canonical_program() {
    for id in 1..=15 {
        let program = package::program_id(id).unwrap();
        assert_eq!(program, if (8..=14).contains(&id) { 8 } else { id });
        assert!(package::contract(program).is_some());
    }
    for id in [0, 16, 31, 32, u32::MAX] { assert!(package::program_id(id).is_none()); }
}

#[test]
fn real_packages_pass_all_existing_admission_checks() {
    for (id, bytes, abi) in fixtures() {
        let contract = package::contract(id).unwrap();
        let (bin, spv) = contract.payloads(bytes).unwrap();
        let artifact = GpgpuKernelArtifact::contracted(abi.kernel_name, &[], &[], abi);
        assert_eq!(admit_kernel_artifact_payloads(artifact, 0x4680, 0x0c, bin, spv), Ok(abi.zebin_sha256));
        assert_eq!(admit_kernel_artifact_payloads(artifact, 0xffff, 0x0c, bin, spv), Err(GpgpuArtifactAdmissionError::UnsupportedPciDevice));
        assert_eq!(admit_kernel_artifact_payloads(artifact, 0x4680, 0, bin, spv), Err(GpgpuArtifactAdmissionError::UnsupportedRevision));
        let mut bad_bin = bin.to_vec();
        bad_bin[64] ^= 1;
        assert_eq!(admit_kernel_artifact_payloads(artifact, 0x4680, 0x0c, &bad_bin, spv), Err(GpgpuArtifactAdmissionError::ZebinHashMismatch));
        let mut bad_spv = spv.to_vec();
        bad_spv[0] ^= 1;
        assert_eq!(admit_kernel_artifact_payloads(artifact, 0x4680, 0x0c, bin, &bad_spv), Err(GpgpuArtifactAdmissionError::ContractSpirvHashMismatch));
        assert!(admit_kernel_artifact_bytes(artifact, 0x4680, 0x0c, bin).is_err());
    }
}

#[test]
fn every_package_component_is_authenticated_including_raw_source() {
    for (id, bytes, _) in fixtures() {
        let contract = package::contract(id).unwrap();
        let mut offset = 0;
        for part in 0..7 {
            let mut changed = bytes.to_vec();
            changed[offset] ^= 1;
            assert!(contract.payloads(&changed).is_none(), "shader {id} part {part}");
            offset = if part == 0 { 32 } else {
                let at = 8 + (part - 1) * 4;
                offset + u32::from_le_bytes(bytes[at..at+4].try_into().unwrap()) as usize
            };
        }
        assert!(contract.payloads(&bytes[..bytes.len()-1]).is_none());
        let mut oversized = bytes.to_vec(); oversized.push(0);
        assert!(contract.payloads(&oversized).is_none());
        let wrong = package::contract(id % 6 + 1).unwrap();
        assert!(wrong.payloads(bytes).is_none());
    }
}

#[test]
fn staging_requires_complete_contiguous_correct_shader_bytes() {
    for (id, bytes, _) in fixtures() {
        let mut upload = package::ShaderToyPackageUpload::new(id).unwrap();
        assert!(!upload.append(id, usize::MAX, &[0]));
        assert!(!upload.append(id, 0, &[]));
        assert!(!upload.append(id % 6 + 1, 0, &[0]));
        for (index, chunk) in bytes.chunks(2048).enumerate() {
            assert!(!upload.complete());
            assert!(upload.append(id, index * 2048, chunk));
        }
        assert!(upload.complete());
        assert!(!upload.append(id, bytes.len(), &[0]));
        assert!(!upload.append(id, 0, &[0]));
        assert_eq!(upload.bytes, bytes);
        assert!(upload.contract.payloads(&upload.bytes).is_some());
    }
    for id in [0, 9, 14, 16, 31, 32, u32::MAX] {
        assert!(package::ShaderToyPackageUpload::new(id).is_none());
    }
}
'''

if __name__ == "__main__":
    main()
