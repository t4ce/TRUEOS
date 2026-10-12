#!/usr/bin/env python3
"""Exercise production runtime admission independently of provenance text."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
ARTIFACTS = ROOT / 'crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp'
PROGRAMS = {
    1: 'shadertoy_mandelbrot', 2: 'shadertoy_cube_field', 3: 'shadertoy_nguyen',
    4: 'shadertoy_palette_grid', 5: 'shadertoy_cosmic_strands',
    6: 'shadertoy_protean_clouds', 7: 'cpp_audio_visualizer_rgba8',
    8: 'cpp_demo_rgba8', 15: 'particle_craft', 16: 'shadertoy_mandelbox',
    17: 'skybox_sample_rgb565',
}


def main():
    code = '''extern crate alloc;
#[macro_export] macro_rules! log_error {
    (target: $target:literal; $($arg:tt)*) => { eprintln!($($arg)*) };
}
struct Abi { zebin_sha256:[u8;32], spv_sha256:[u8;32] }
struct Artifact { abi_contract:Option<&'static Abi> }
'''
    for shader, name in PROGRAMS.items():
        source = (ARTIFACTS / f'{name}.contract.rs').read_text()
        hashes = [re.search(rf'{field}:\s*(\[[^]]+\])', source).group(1)
                  for field in ('zebin_sha256', 'spv_sha256')]
        code += f'static ABI_{shader}:Abi=Abi{{zebin_sha256:{hashes[0]},spv_sha256:{hashes[1]}}};\n'
    code += '''use ABI_17 as SKYBOX_SAMPLE_RGB565_ADLS_CPP_ABI_CONTRACT;
use ABI_15 as PARTICLE_CRAFT_STEP_ADLS_CPP_ABI_CONTRACT;
fn shadertoy_artifact_slot(id:u32)->Option<(Artifact,(),())> {
 let abi=match id {
'''
    code += ''.join(f'{shader}=>&ABI_{shader},\n' for shader in PROGRAMS)
    code += '_=>return None}; Some((Artifact{abi_contract:Some(abi)},(),())) }\n'
    code += f'#[path="{ROOT}/src/intel/gpgpu/artifacts/shadertoy_package.rs"] mod packages;\n'
    package = ROOT.parent / 'TRUEOS-Blueprints/apps/shadertoy/assets/palette_grid.stpkg'
    code += f'const PACKAGE:&[u8]=include_bytes!("{package}");\n'
    code += '#[test] fn all_current_packages_pass() {\n'
    for shader, name in PROGRAMS.items():
        if shader == 17:
            fixture = ROOT.parent / 'TRUEOS-Blueprints/apps/skybox/assets/skybox.stpkg'
        elif shader == 16:
            fixture = ROOT.parent / 'Cubes/Cube/mandelbox/mandelbox.stpkg'
        else:
            names = {7: 'audio_visualizer', 8: 'cpp_gallery', 15: 'particle_craft'}
            fixture = ROOT.parent / f'TRUEOS-Blueprints/apps/shadertoy/assets/{names.get(shader, name.removeprefix("shadertoy_"))}.stpkg'
        code += f'assert!(packages::contract({shader}).unwrap().payloads(include_bytes!("{fixture}")).is_some(), "shader {shader}");\n'
    code += '}\n'
    code += r'''
#[test] fn provenance_changes_do_not_change_execution_compatibility() {
 let c=packages::contract(4).unwrap();
 assert!(c.payloads(PACKAGE).is_some());
 let mut changed=PACKAGE.to_vec();
 // Extend the final provenance section, as a source-path comment change does.
 let length=u32::from_le_bytes(changed[28..32].try_into().unwrap());
 changed[28..32].copy_from_slice(&(length+8).to_le_bytes());
 changed.extend_from_slice(b"// note\n");
 assert!(c.payloads(&changed).is_some());
 let mut upload=packages::ShaderToyPackageUpload::new(4,changed.len()).unwrap();
 for (i, chunk) in changed.chunks(2048).enumerate() {
   assert!(upload.append(4,i*2048,chunk));
 }
 assert!(upload.complete());
 assert!(upload.contract.payloads(&upload.bytes).is_some());
}
#[test] fn executable_and_spirv_corruption_are_rejected() {
 let c=packages::contract(4).unwrap();
 for offset in [32,32+c.bin_bytes] {
   let mut corrupt=PACKAGE.to_vec(); corrupt[offset]^=1;
   assert!(c.payloads(&corrupt).is_none());
 }
}
#[test] fn framing_and_upload_bounds_are_enforced() {
 let c=packages::contract(4).unwrap();
 assert!(c.payloads(&PACKAGE[..PACKAGE.len()-1]).is_none());
 let mut bad=PACKAGE.to_vec(); bad[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
 assert!(c.payloads(&bad).is_none());
 let mut bad=PACKAGE.to_vec(); bad[0]^=1;
 assert!(c.payloads(&bad).is_none());
 assert!(packages::ShaderToyPackageUpload::new(4,31).is_none());
 assert!(packages::ShaderToyPackageUpload::new(4,packages::MAX_PACKAGE_BYTES+1).is_none());
 let mut upload=packages::ShaderToyPackageUpload::new(4,PACKAGE.len()).unwrap();
 assert!(!upload.append(4,1,&PACKAGE[..1]));
 assert!(!upload.append(3,0,&PACKAGE[..1]));
 assert!(upload.append(4,0,&PACKAGE[..1]));
 assert!(!upload.complete());
}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-shader-admission-') as directory:
        path = Path(directory)
        (path / 'src').mkdir()
        (path / 'Cargo.toml').write_text('[package]\nname="shader-admission-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\nsha2="0.10"\n')
        (path / 'src/lib.rs').write_text(code)
        subprocess.run(['cargo', 'test', '--offline', '--manifest-path', str(path / 'Cargo.toml'),
                        '--target', 'x86_64-unknown-linux-gnu'], cwd=directory, check=True)


if __name__ == '__main__':
    main()
