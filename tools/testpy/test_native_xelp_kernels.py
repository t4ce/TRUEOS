#!/usr/bin/env python3
"""Compile the real admission code against the five exported kernel fixtures."""
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
ASSETS = ROOT.parent / 'odin-son/trueos/gpu-demo/assets'


class NativeKernelAdmission(unittest.TestCase):
    @unittest.skipUnless(ASSETS.is_dir(), "requires sibling Odin GPU fixtures")
    def test_profile(self):
        source = f'#[path="{ROOT}/src/gpu/native_xelp.rs"] mod admission;\n'
        for name in ('gpu_simplest', 'gpu_float_less', 'gpu_if_else', 'gpu_loop_continue', 'gpu_nested_loops'):
            source += f'''#[test] fn {name}() {{
                assert!(admission::validate_simd8(include_bytes!("{ASSETS}/{name}.eu")));
            }}\n'''
        source += f'''
#[test] fn accepts_new_arithmetic_without_a_hash_allowlist() {{
    let mut code=include_bytes!("{ASSETS}/gpu_simplest.eu").to_vec();
    code[48]=0x41; // Change ADD to MUL, retaining the supported register ABI.
    assert!(admission::validate_simd8(&code));
}}
#[test] fn rejects_other_memory_messages_and_reserved_register_writes() {{
    let original=include_bytes!("{ASSETS}/gpu_simplest.eu");
    for (offset, byte) in [(0, 1), (48, 0x31), (55, 119), (original.len()-1, 1)] {{
        let mut code=original.to_vec(); code[offset]=byte;
        assert!(!admission::validate_simd8(&code), "offset {{offset}}");
    }}
    assert!(!admission::validate_simd8(&original[..original.len()-1]));
}}
#[test] fn rejects_branches_outside_body_and_into_exit_messages() {{
    let original=include_bytes!("{ASSETS}/gpu_if_else.eu");
    for offset in [i32::MIN, -64, 1, 128, i32::MAX] {{
        let mut code=original.to_vec();
        code[76..80].copy_from_slice(&offset.to_le_bytes());
        assert!(!admission::validate_simd8(&code), "offset {{offset}}");
    }}
}}
'''
        with tempfile.TemporaryDirectory() as directory:
            work = Path(directory)
            (work / 'tests.rs').write_text(source)
            subprocess.run(['rustc', '--edition=2024', '--test', str(work / 'tests.rs'), '-o', str(work / 'tests')],
                           check=True, timeout=20)
            subprocess.run([str(work / 'tests')], check=True, timeout=20)


if __name__ == '__main__':
    unittest.main()
