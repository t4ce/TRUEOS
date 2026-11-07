#!/usr/bin/env python3
"""Run production native namespace ABI wrappers and VM admission guards at controlled boundaries."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import item
ROOT=Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/r/io/async_fs_cabi.rs').read_text()
    signatures=['fn parse_path(','fn direct_owner(','fn guest_start(','pub unsafe extern "C" fn trueos_cabi_async_fs_create_dir_start_v1(','pub unsafe extern "C" fn trueos_cabi_async_fs_remove_leaf_start_v1(']
    production='\n'.join(item(source,s) for s in signatures)
    vm=(ROOT/'src/hv/vmcall.rs').read_text()
    fixture=(Path(__file__).with_name('fixtures')/'native_namespace_abi.rs').read_text().replace('// @ADAPTER@',production)
    for label,name in [('CREATE','CREATE_DIR'),('REMOVE','REMOVE_LEAF')]:
        fixture=fixture.replace('// @'+label+'_GUARD@',item(vm,'if op == trueos_vm::vmcall::OP_BP_ASYNC_FS_'+name+'_START_V1'))
    toolchain=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-native-namespace-abi-') as tmp:
        path=Path(tmp);rust=path/'test.rs';binary=path/'test';rust.write_text(fixture)
        subprocess.run(['rustc','+'+toolchain,'--edition=2024','--test','-Dwarnings',str(rust),'-o',str(binary)],check=True,timeout=60)
        subprocess.run([str(binary)],check=True,timeout=60)
if __name__=='__main__':main()
