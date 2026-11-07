#!/usr/bin/env python3
"""Exercise the actual single-directory publication against controlled native storage races."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import item
ROOT=Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/r/fs/trueosfs.rs').read_text()
    production='\n'.join([
        '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n'+item(source,'pub enum CreateDirectoryError'),
        item(source,'impl From<block::Error> for CreateDirectoryError'),
        item(source,'pub async fn create_directory_new_async('),
        item(source,'fn validate_indexed_directory_target('),
    ])
    fixture=(Path(__file__).with_name('fixtures')/'native_create_dir.rs').read_text().replace('// @PRODUCTION@',production)
    toolchain=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-native-create-dir-') as tmp:
        path=Path(tmp);rust=path/'test.rs';binary=path/'test';rust.write_text(fixture)
        subprocess.run(['rustc','+'+toolchain,'--edition=2024','--test','-Dwarnings',str(rust),'-o',str(binary)],check=True,timeout=60)
        subprocess.run([str(binary),'--test-threads=1'],check=True,timeout=60)
if __name__=='__main__':main()
