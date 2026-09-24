#!/usr/bin/env python3
"""Run the real termdir archive UI state test on the host.

Unused kernel imports abort if reached. This ensures polling a pending future
or rejecting duplicate starts does not issue accidental filesystem operations.
"""
from pathlib import Path
import os
import subprocess
import tempfile
ROOT=Path(__file__).resolve().parents[1]
SYMBOLS=[
    'async_fs_status','async_fs_result_len','async_fs_result_read','async_fs_discard',
    'async_fs_stat_start','poll_once','archive_discard','archive_unpack_start',
    'archive_pack_start','archive_pack_many_start','archive_status','archive_report',
    'async_fs_typed_list_dir_start','write','blueprint_shutdown',
]
def main():
    with tempfile.TemporaryDirectory(prefix='trueos-termdir-test-') as td:
        folder=Path(td); source=folder/'imports.c'; obj=folder/'imports.o'
        source.write_text('#include <stdlib.h>\n'+''.join(
            f'__attribute__((noreturn)) void trueos_cabi_{name}(void) {{ abort(); }}\n' for name in SYMBOLS))
        subprocess.run(['cc','-c',str(source),'-o',str(obj)],check=True)
        env=os.environ.copy();env['RUSTFLAGS']=f'-C link-arg={obj}'
        subprocess.run(['cargo','+nightly-2026-07-10','test','--manifest-path',
            str(ROOT.parent/'TRUEOS-Blueprints/buildins/termdir/Cargo.toml'),
            '--target','x86_64-unknown-linux-gnu','--target-dir',str(ROOT/'bld/termdir-archive-tests'),
            'archive_operation_polls_once_and_rejects_duplicate_start'],cwd=folder,env=env,check=True)
if __name__=='__main__':main()
