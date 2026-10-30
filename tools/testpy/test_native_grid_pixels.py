"""Exercise POC-to-pixel admission and the emitted grid-stride envelope."""
from pathlib import Path
import subprocess
import tempfile

from test_blueprint_archive_scope import function

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT.parent / 'odin-son/trueos/gpu-demo/assets'


def main():
    driver = (ROOT / 'src/intel/gpgpu/operations/generated_pixel.rs').read_text()
    source = f'pub mod gpu {{ #[path="{ROOT}/src/gpu/native_xelp.rs"] pub mod native_xelp; }}\n'
    for name in ('generated_grid_pixel_envelope', 'generated_pixel_binary', 'generated_pixel_broadcast'):
        source += function(driver, 'fn ' + name + '(') + '\n'
    source += f'''
fn fixture() -> Vec<u8> {{ include_bytes!("{ASSETS}/gpu_simplest.eu").to_vec() }}
fn read(code: &[u8], offset: usize) -> [u32; 4] {{
    core::array::from_fn(|i| u32::from_le_bytes(code[offset+i*4..offset+i*4+4].try_into().unwrap()))
}}
#[test] fn preserves_compiler_control_flow_between_driver_stages() {{
    for code in [include_bytes!("{ASSETS}/gpu_loop_continue.eu").as_slice(),
                 include_bytes!("{ASSETS}/gpu_nested_loops.eu").as_slice()] {{
        let body = gpu::native_xelp::pixel_body_simd8(code).unwrap();
        let wrapped = generated_grid_pixel_envelope(body).unwrap();
        assert!(wrapped.windows(body.len()).any(|window| window == body));
        let position = wrapped.len()-8*16;
        let branch = read(&wrapped, position);
        assert_eq!(branch[0], 0x01030127);
        let target = position as i32 + branch[3] as i32;
        assert!(target >= 16 && target < position as i32);
        assert_eq!(target as usize % 16, 0);
        // Every repeated pixel recomputes its pitched row address.
        assert_eq!(read(&wrapped, target as usize), [0x00030141,0x72050220,0x01467105,0x000079a4]);
        assert_eq!(read(&wrapped, wrapped.len()-16), [0x80000131,4,0x70207f0c,0]);
        assert_eq!(wrapped.chunks_exact(16).filter(|w| read(w,0)[0] == 0x01034031).count(), 1);
    }}
}}
#[test] fn rejects_driver_registers_even_when_scalar_dispatch_allows_them() {{
    for register in [112u8, 113, 114, 115, 116, 117] {{
        for byte in [55, 57] {{
            let mut code = fixture();
            code[byte] = register;
            assert!(gpu::native_xelp::validate_simd8(&code));
            assert!(gpu::native_xelp::pixel_body_simd8(&code).is_none());
        }}
    }}
}}
#[test] fn reciprocal_requires_exact_sbid_wait_and_protects_driver_registers() {{
    let mut code = fixture();
    let insert = code.len()-48;
    let math = [0x00039238u32,0x04050aa0,0x10460105,0,
                0x00002201,0x00010000,0,0];
    code.splice(insert..insert, math.into_iter().flat_map(u32::to_le_bytes));
    assert!(gpu::native_xelp::pixel_body_simd8(&code).is_some());
    for (offset, value) in [(insert+2, 0x04), (insert+8, 0x06),
                             (insert+16+1, 0x23), (insert+7, 112),
                             (insert+9, 112), (insert+12, 1)] {{
        let mut bad = code.clone(); bad[offset] = value;
        assert!(gpu::native_xelp::pixel_body_simd8(&bad).is_none());
    }}
    let mut bad = code.clone(); bad.drain(insert..insert+16);
    assert!(!gpu::native_xelp::validate_simd8(&bad));
}}
#[test] fn rejects_missing_or_changed_memory_envelopes() {{
    let original = fixture();
    assert!(gpu::native_xelp::pixel_body_simd8(&original).is_some());
    assert!(gpu::native_xelp::pixel_body_simd8(&original[48..original.len()-48]).is_none());
    for offset in [0, original.len()-48, original.len()-1] {{
        let mut code = original.clone(); code[offset] ^= 1;
        assert!(gpu::native_xelp::pixel_body_simd8(&code).is_none());
    }}
}}
'''
    with tempfile.TemporaryDirectory(prefix='trueos-native-grid-') as temporary:
        directory = Path(temporary)
        (directory / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'),
                        '-o', str(directory / 'tests')], check=True, timeout=20)
        subprocess.run([str(directory / 'tests')], check=True, timeout=20)


if __name__ == '__main__':
    main()
