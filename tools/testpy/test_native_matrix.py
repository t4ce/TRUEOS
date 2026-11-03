#!/usr/bin/env python3
"""Check matrix admission and execute emitted integer instructions on a host model.

This validates arithmetic/addressing, not GPU execution, cache behavior, or occupancy.
Build the sibling odin-son matrix demo first.
"""
from pathlib import Path
import random
import struct
import subprocess
import tempfile
import unittest

from test_blueprint_archive_scope import function

ROOT = Path(__file__).resolve().parents[2]
KERNEL = ROOT.parent / 'odin-son/trueos/build/matrix-demo/mac.bin'
MASK = 0xffffffff


def reference(matrix, n, saturate):
    output = []
    for row in range(n):
        for col in range(n):
            acc = 0
            for k in range(n):
                value = matrix[row*n+k]*matrix[k*n+col]+acc
                acc = min(value, MASK) if saturate else value & MASK
            output.append(acc)
    return output


def execute(code, matrix, n, saturate):
    instructions = list(struct.iter_unpack('<4I', code))
    output = [None]*(n*n)
    for row in range(n):
        for group in range((n+7)//8):
            for lane in range(8):
                registers = [0]*128
                registers[2] = lane
                flags = [False]*4
                pc = 1
                steps = 0

                def source(word):
                    reg = (word >> 8) & 255
                    region = word & 0x00ff00ff
                    if region == 0x00460005:
                        return registers[reg]
                    if word & 0x00ff001f == 4:
                        element = (word >> 5) & 7
                        if reg == 0:
                            return {1: group, 6: row}[element]
                        if reg == 121:
                            return {0: n, 1: saturate}[element]
                    raise AssertionError(f'unsupported source {word:08x}')

                while True:
                    steps += 1
                    assert steps < 40000, 'unbounded kernel'
                    w0, w1, w2, w3 = instructions[pc]
                    op = w0 & 127
                    flag_index = (w0 >> 22) & 3
                    pred = flags[flag_index]
                    if w0 & 0x10000000:
                        pred = not pred
                    predicated = bool(w0 & 0x1000000)
                    if op == 0x22:
                        pc += 1 if pred else w3//16
                        continue
                    if op == 0x24:
                        pc += w3//16
                        continue
                    if op == 0x25:
                        pc += 1
                        continue
                    if op == 0x27:
                        pc += struct.unpack('<i', struct.pack('<I', w3))[0]//16 if pred else 1
                        continue
                    if w0 == 0x00039331:
                        address = registers[127]
                        assert address % 4 == 0 and 0 <= address < len(matrix)*4
                        registers[w1 >> 24] = matrix[address//4]
                        pc += 1
                        continue
                    if w0 == 0x01034031:
                        assert w2 == 0xcc02030c and w3 == 0x009a740c
                        if pred:
                            address = registers[3]
                            assert address % 4 == 0 and 0 <= address < len(matrix)*4
                            assert output[address//4] is None, 'duplicate output write'
                            output[address//4] = registers[116]
                        break
                    if op == 1:
                        pc += 1
                        continue
                    dst = w1 >> 24
                    if op == 0x61:
                        value = w3 if w1 & 0x4000 else source(w2)
                    else:
                        a = source(w2)
                        if op == 0x41 and w2 >> 24 == 1:
                            if w3 & 0x00ff00ef == 0x00560006:
                                b = registers[(w3 >> 8) & 255]
                                b = (b >> (16 if w3 & 16 else 0)) & 0xffff
                            else:
                                b = source(w3) & 0xffff
                        else:
                            b = w3 if w1 & 0x8000 else source(w3)
                        if op == 0x40:
                            value = a+b
                        elif op == 0x41:
                            value = a*b
                        elif op == 0x65:
                            value = a & b
                        elif op == 0x66:
                            value = a | b
                        elif op == 0x67:
                            value = a ^ b
                        elif op == 0x68:
                            value = a >> (b & 31)
                        elif op == 0x69:
                            value = a << (b & 31)
                        elif op == 0x62:
                            value = a if pred else b
                        elif op == 0x70:
                            if (w1 >> 4) & 15 == 6:
                                a = a-(1 << 32) if a & (1 << 31) else a
                                b = b-(1 << 32) if b & (1 << 31) else b
                            condition = w2 >> 28
                            truth = {1: a == b, 2: a != b, 3: a > b,
                                     4: a >= b, 5: a < b, 6: a <= b}[condition]
                            flags[flag_index] = truth
                            value = MASK if truth else 0
                        else:
                            raise AssertionError(f'unsupported instruction {op:x}')
                    if dst:
                        registers[dst] = value & MASK
                    pc += 1
    assert all(value is not None for value in output)
    return output


class NativeMatrix(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        if not KERNEL.exists():
            raise unittest.SkipTest('build sibling odin-son/trueos/build-matrix-demo.py first')
        source = '#![allow(dead_code)]\n'
        source += f'pub mod gpu {{ #[path="{ROOT}/src/gpu/native_xelp.rs"] pub mod native_xelp; }}\n'
        driver = (ROOT / 'src/intel/gpgpu/operations/native_matrix.rs').read_text()
        pixels = (ROOT / 'src/intel/gpgpu/operations/generated_pixel.rs').read_text()
        source += function(driver, 'fn native_matrix_envelope(')+'\n'
        source += function(pixels, 'fn generated_pixel_binary(')+'\n'
        source += r'''
fn main() {
    let code = std::fs::read(std::env::args().nth(1).unwrap()).unwrap();
    let body = gpu::native_xelp::matrix_body_simd8(&code).unwrap();
    for (offset, value) in [(48, 0x31), (55, 112), (57, 112), (0, 0)] {
        let mut bad = code.clone(); bad[offset] = value;
        assert!(gpu::native_xelp::matrix_body_simd8(&bad).is_none());
    }
    for (i, words) in body.chunks_exact(16).enumerate() {
        if words[0] & 127 == 0x22 {
            let mut bad = code.clone();
            bad[48+i*16+12..48+i*16+16].copy_from_slice(&(-16i32).to_le_bytes());
            assert!(gpu::native_xelp::matrix_body_simd8(&bad).is_none());
        }
    }
    let wrapped = native_matrix_envelope(body).unwrap();
    assert_eq!(&wrapped[wrapped.len()-16..], [0x80000131u32,4,0x70207f0c,0].map(u32::to_le_bytes).as_flattened());
    std::fs::write(std::env::args().nth(2).unwrap(), wrapped).unwrap();
}
'''
        with tempfile.TemporaryDirectory(prefix='native-matrix-') as directory:
            work = Path(directory)
            (work / 'main.rs').write_text(source)
            subprocess.run(['rustc', '--edition=2024', str(work / 'main.rs'), '-o', str(work / 'check')], check=True, timeout=20)
            subprocess.run([str(work / 'check'), str(KERNEL), str(work / 'kernel.bin')], check=True, timeout=20)
            cls.code = (work / 'kernel.bin').read_bytes()

    def test_two_by_two(self):
        for mode in (0, 1):
            self.assertEqual(execute(self.code, [1, 2, 3, 4], 2, mode), [7, 10, 15, 22])

    def test_overflow_and_dependent_squares(self):
        fixtures = [[MASK]*4, [0, MASK, MASK, 0], [0x10000, 0xffff, 1, 0x80000000]]
        for matrix in fixtures:
            for mode in (0, 1):
                actual, expected = matrix, matrix
                for _ in range(4):
                    actual = execute(self.code, actual, 2, mode)
                    expected = reference(expected, 2, mode)
                    self.assertEqual(actual, expected)

    def test_random_matrices_and_masked_tails(self):
        rng = random.Random(0x4d415458)
        for n in (1, 3, 7, 8, 9, 16):
            for maximum in (255, 0xffff, MASK):
                matrix = [rng.randrange(maximum+1) for _ in range(n*n)]
                for mode in (0, 1):
                    self.assertEqual(execute(self.code, matrix, n, mode), reference(matrix, n, mode))


if __name__ == '__main__':
    unittest.main()
