#!/usr/bin/env python3
"""Exercise production guarded removal with controlled storage and lease races."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/r/fs/trueosfs.rs').read_text()
    production = '\n'.join([
        '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n' + item(source, 'pub enum RemoveNodeError'),
        item(source, 'impl From<block::Error> for RemoveNodeError'),
        item(source, 'pub async fn remove_recursive_async('),
        item(source, 'pub async fn remove_leaf_async('),
        item(source, 'fn indexed_directory_is_empty('),
        item(source, 'async fn remove_node_with_policy_async('),
        item(source, 'fn indexed_removal_entry('),
        item(source, 'fn normalized_dir_prefix('),
        item(source, 'fn update_root_index_delete('),
        item(source, 'fn remove_index_subtree('),
        item(source, 'fn update_root_index_delete_tree('),
    ])
    fixture = (Path(__file__).with_name('fixtures') / 'native_remove_leaf.rs').read_text().replace('// @PRODUCTION@', production)
    toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-native-remove-leaf-') as temporary:
        path = Path(temporary)
        rust = path / 'test.rs'
        binary = path / 'test'
        rust.write_text(fixture)
        subprocess.run(['rustc', '+' + toolchain, '--edition=2024', '--test', '-Dwarnings', str(rust), '-o', str(binary)], check=True, timeout=60)
        subprocess.run([str(binary), '--test-threads=1'], check=True, timeout=60)


if __name__ == '__main__':
    main()
