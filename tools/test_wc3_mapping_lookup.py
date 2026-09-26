#!/usr/bin/env python3
"""Differential-test the kernel's mapping lookup without booting the kernel."""
from pathlib import Path
import subprocess
import tempfile

source = (Path(__file__).resolve().parents[1] / 'src/hv/wc3/x86_runtime.rs').read_text()
function = source[source.index('fn mapping_index('):source.index('pub(super) fn address_space_create')]
harness = '''
const ERR_INVALID: i32 = -22;
const ERR_NOT_FOUND: i32 = -2;
struct Mapping { start: u32, len: u32, permissions: u32 }
struct AddressSpace { mappings: Vec<Mapping> }
''' + function + '''
fn linear(space: &AddressSpace, address: u32, len: usize, required: u32) -> Result<usize, i32> {
    let end = address.checked_add(u32::try_from(len).map_err(|_| ERR_INVALID)?).ok_or(ERR_INVALID)?;
    space.mappings.iter().position(|m| m.permissions & required == required
        && address >= m.start && end <= m.start + m.len).ok_or(ERR_NOT_FOUND)
}
fn main() {
    let mut space = AddressSpace { mappings: Vec::new() };
    for i in (0..64).rev() {
        let start = i * 8192;
        let at = space.mappings.partition_point(|m| m.start < start);
        space.mappings.insert(at, Mapping { start, len: if i % 2 == 0 { 8192 } else { 4096 }, permissions: i % 7 + 1 });
    }
    for pass in 0..2 {
        if pass == 1 { space.mappings.remove(17); }
        for address in (0..64 * 8192).step_by(127).chain([0, 4095, 4096, 8191, 8192, u32::MAX]) {
            for len in [0, 1, 2, 4096, 8192, usize::MAX] {
                for permission in 1..8 {
                    assert_eq!(mapping_index(&space, address, len, permission), linear(&space, address, len, permission),
                        "address={address:x} len={len} permission={permission}");
                }
            }
        }
    }
    space.mappings.clear();
    assert_eq!(mapping_index(&space, 0, 1, 1), Err(ERR_NOT_FOUND));
    println!("PASS: binary mapping lookup matches linear reference across gaps, permissions, boundaries, removal and overflow");
}
'''
with tempfile.TemporaryDirectory(prefix='wc3-mapping-test-') as directory:
    work = Path(directory)
    (work / 'test.rs').write_text(harness)
    subprocess.run(['rustc', '--edition=2024', '-O', str(work / 'test.rs'), '-o', str(work / 'test')], check=True)
    subprocess.run([str(work / 'test')], check=True)
