#!/usr/bin/env python3
"""Exercise the unchanged production object authority over the real TRUEOSFS engine.

Only block storage, mount admission and index/cache maintenance boundaries are
host fixtures. File records, publication, overwrite, append, rename and deletion
use the actual trueos-fs crate. No rig or user database is touched.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,re,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
from test_sdk_metadata_v2 import item
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--receipt',type=Path);args=parser.parse_args()
    source=ROOT/'src/r/fs/trueosfs_file_objects.rs';fixture=Path(__file__).with_name('fixtures')/'native_file_objects.rs'
    text=fixture.read_text().replace('@FILE_OBJECT_SOURCE@',str(source)).replace('@CABI_CODES_SOURCE@',str(ROOT/'src/r/cabi_codes.rs')).replace('@AUTHORIZED_SOURCE@',str(ROOT/'src/r/fs/trueosfs_authorized.rs')).replace('@CONTENT_TYPE_SOURCE@',str(ROOT/'src/r/fs/content_type_boundary.rs')).replace('@FS_HTML_SOURCE@',str(ROOT/'src/r/fs/fs_html.rs'))
    kernel=(ROOT/'src/r/fs/trueosfs.rs').read_text()
    helpers='\n'.join(item(kernel,marker) for marker in [
        'pub(crate) async fn node_record_under_lease(', 'pub(crate) async fn inherited_record_key_under_lease(',
        'async fn record_key_for_write_under_lease(', 'pub(crate) async fn stamp_directory_key_async(',
        'pub enum CreateDirectoryError', 'impl From<block::Error> for CreateDirectoryError',
        'pub async fn create_directory_async(', 'pub async fn create_directory_new_async(', 'async fn create_directory_with_policy_async(',
        'async fn file_in_with_metadata_async_for_actor(', 'pub enum RemoveNodeError', 'impl From<block::Error> for RemoveNodeError',
        'async fn remove_node_with_policy_async(', 'fn indexed_directory_is_empty(',
        'async fn file_rename_with_policy_async(', 'async fn dir_rename_for_actor_async(',
        'fn indexed_move_entry(', 'fn indexed_move_entry_with_policy(', 'fn collect_index_tree_moves(',
        'fn normalized_dir_prefix(', 'fn normalize_dir_name(',
        'pub enum CheckedWriteError', 'impl From<block::Error> for CheckedWriteError', 'enum CheckedWriteTypePolicy',
        'async fn file_in_checked_with_type_policy_async(', 'pub(crate) async fn file_write_begin_with_metadata_async_for_actor(',
        'struct FileWriteStream', 'pub struct FileReadHandle', 'pub async fn file_write_chunk_async(',
        'pub async fn file_write_finish_async(', 'pub async fn file_write_abort_async(',
        'pub async fn file_read_handle_range_async(', 'pub(crate) fn file_read_handle_is_current(',
        'pub(super) enum ReadRangePhase', 'fn push_json_string_escaped(', 'fn push_hex_bytes(', 'fn push_record_key_json(',
    ])
    helpers=helpers.replace('pub(super) enum ReadRangePhase','#[derive(Clone,Copy,Debug,Eq,PartialEq)]\npub(super) enum ReadRangePhase')
    # These declarations carry derives in production; the extractor deliberately starts at the item.
    helpers=helpers.replace('pub enum CreateDirectoryError','#[derive(Debug)]\npub enum CreateDirectoryError').replace('pub enum RemoveNodeError','#[derive(Debug,PartialEq)]\npub enum RemoveNodeError')
    helpers=helpers.replace('pub enum CheckedWriteError','#[derive(Debug,PartialEq)]\npub enum CheckedWriteError').replace('enum CheckedWriteTypePolicy','#[derive(Clone,Copy,PartialEq,Eq)]\nenum CheckedWriteTypePolicy').replace('pub struct FileReadHandle','#[derive(Clone,Copy)]\npub struct FileReadHandle')
    text=text.replace('// @KEY_HELPERS@',helpers)

    toolchain=re.search(r'channel\s*=\s*"([^"]+)"',(ROOT/'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='trueos-native-file-object-') as folder:
        path=Path(folder);(path/'src').mkdir();(path/'src/lib.rs').write_text(text)
        (path/'Cargo.toml').write_text(f'''[package]
name="trueos-native-file-object-tests"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
trueos-fs={{path={json.dumps(str(ROOT/'crates/trueos-fs'))}}}
trueos-math={{path={json.dumps(str(ROOT/'crates/trueos-math'))},features=["alloc"]}}
spin="0.10"
sha2={{version="=0.10.9",default-features=false}}
infer={{path={json.dumps(str(ROOT/'vendor/infer'))},default-features=false}}
trueos-crypto={{path={json.dumps(str(ROOT/'crates/trueos-crypto'))}}}
''')
        command=['cargo','+'+toolchain,'test','--offline','--manifest-path',str(path/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu','--target-dir','/tmp/trueos-native-file-object-tests','--','--test-threads=1']
        completed=subprocess.run(command,cwd=path,text=True,capture_output=True,timeout=180)
        print(completed.stdout,end='');print(completed.stderr,end='')
        if args.receipt:
            sources=[source,fixture,Path(__file__),ROOT/'crates/trueos-fs/src/lib.rs',ROOT/'src/r/fs/trueosfs.rs',ROOT/'src/r/fs/trueosfs_authorized.rs',ROOT/'src/r/fs/fs_html.rs']
            args.receipt.write_text(json.dumps({'status':'passed' if completed.returncode==0 else 'failed','utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command,'source_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'stdout':completed.stdout,'stderr':completed.stderr,'scope':'Exact production native object authority + real TRUEOSFS engine, memory-backed BlockIo; no rig or platform block driver validation.'},indent=2)+'\n')
        if completed.returncode:raise SystemExit(completed.returncode)
if __name__=='__main__':main()
