#!/usr/bin/env python3
"""Exercise production conditional writes at controlled lease and storage boundaries."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/r/fs/trueosfs.rs').read_text()
    production = '\n'.join([
        '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n' + item(source, 'pub enum CheckedWriteError'),
        item(source, 'impl From<block::Error> for CheckedWriteError'),
        '#[derive(Clone, Copy, Eq, PartialEq)]\n' + item(source, 'enum CheckedWriteTypePolicy'),
        item(source, 'pub async fn file_in_typed_checked_async('),
        item(source, 'pub async fn reset_blob_checked_async('),
        item(source, 'async fn file_in_checked_with_type_policy_async('),
    ])
    fixture = (Path(__file__).with_name('fixtures') / 'native_checked_write.rs').read_text()
    toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-native-checked-write-') as temporary:
        path = Path(temporary)
        (path / 'src').mkdir()
        (path / 'src/lib.rs').write_text(fixture.replace('// @PRODUCTION@', production))
        (path / 'Cargo.toml').write_text('''[package]
name = "trueos-native-checked-write-fixture"
version = "0.0.0"
edition = "2024"
[dependencies]
sha2 = { version = "=0.10.9", default-features = false }
''')
        subprocess.run(['cargo', '+' + toolchain, 'test', '--offline', '--quiet',
                        '--manifest-path', str(path / 'Cargo.toml'), '--', '--test-threads=1'],
                       cwd=path, check=True, timeout=60)


if __name__ == '__main__':
    main()
