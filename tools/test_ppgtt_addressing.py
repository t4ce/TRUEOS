#!/usr/bin/env python3
"""Run production Gen12 physical-address checks without booting the kernel."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item, constant

path = 'src/intel/ppgtt.rs'
constants = ('PAGE_BYTES', 'GEN12_PPGTT_PHYS_ADDR_BITS', 'GEN12_PPGTT_PHYS_ADDR_LIMIT',
             'ENTRY_ADDR_MASK', 'PAGE_PRESENT', 'PAGE_RW', 'GEN12_PPGTT_PTE_PAT0',
             'GEN12_PPGTT_PTE_PAT1', 'GEN12_PPGTT_PTE_PAT2',
             'GEN12_SYSTEM_MEMORY_WB_PAT_INDEX', 'PTE_PRESENT_RW_PAT0_WB')
source = '\n'.join(constant(path, name) for name in constants)
source += '\n'.join(item(path, name) for name in
                    ('gen12_ppgtt_leaf_flags', 'gen12_ppgtt_phys_range_encodable', 'tests'))
with tempfile.TemporaryDirectory(prefix='trueos-ppgtt-') as tmp:
    src, exe = Path(tmp) / 'tests.rs', Path(tmp) / 'tests'
    src.write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(src), '-o', str(exe)], check=True)
    subprocess.run([str(exe)], check=True)
