#!/usr/bin/env python3
"""Host tests for production inference, archive restore, and checked-write wire parsing."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def item(source, signature):
    start = source.index(signature)
    opening = source.index('{', start)
    depth, end = 1, opening + 1
    while depth:
        depth += (source[end] == '{') - (source[end] == '}')
        end += 1
    return source[start:end]

def main():
    codec = (ROOT / 'src/r/codec.rs').read_text()
    abi = (ROOT / 'src/r/io/async_fs_cabi.rs').read_text()
    helper = (ROOT / 'crates/trueos-v/src/content_identity.rs').read_text()
    harness = '''#![allow(dead_code)]
extern crate alloc;
use alloc::vec::Vec;
use infer::ContentTypeId;
const BLUEPRINT_ASYNC_FS_MAX_PATH: usize = 4096;
const FS_ERR_BAD_PARAM: i32 = -4;
const FS_ERR_BAD_UTF8: i32 = -1;
mod r { pub mod tar { pub struct Entry { pub name: String, pub bytes: Vec<u8>, pub content_type_raw: Option<u32> } }
 pub mod fs {
  #[path = "BOUNDARY"] pub mod content_type_boundary;
  pub mod trueosfs { pub use infer::ContentTypeId; #[derive(Debug)] pub enum ContentIdentityRejectReason { TypeRequired, UnregisteredType } }
 }
}
#[derive(Debug)] enum CodecError { LimitExceeded, UnsupportedContentType(r::fs::trueosfs::ContentIdentityRejectReason) }
'''.replace('BOUNDARY', str(ROOT / 'src/r/fs/content_type_boundary.rs'))
    harness += '#[derive(Debug)]\n' + item(abi, 'pub(crate) struct CheckedWriteOptions')
    harness += item(abi, 'pub(crate) fn decode_checked_write_payload')
    harness += '#[derive(Clone, Copy, Debug, Eq, PartialEq)]\n' + item(codec, 'enum RestoreDisposition')
    harness += item(codec, 'fn restored_content_types').replace('super::tar::Entry', 'crate::r::tar::Entry')
    harness += item(helper, 'pub fn infer_named_bytes')
    harness += r'''
#[test] fn archive_inference_preserves_explicit_metadata_and_falls_back() {
 let make = |name: &str, raw| r::tar::Entry { name: name.into(), bytes: b"\x89PNG\r\n\x1a\nrest".to_vec(), content_type_raw: raw };
 let result = restored_content_types(&[make("image.PNG", None), make("wrong.jpg", None), make("image.png", Some(ContentTypeId::BLOB.raw())), make("unknown.zzz", None)]).unwrap();
 assert_eq!(result, [RestoreDisposition::Typed(ContentTypeId::PNG), RestoreDisposition::LegacyBlob, RestoreDisposition::Typed(ContentTypeId::BLOB), RestoreDisposition::LegacyBlob]);
 assert!(restored_content_types(&[make("image.png", Some(0))]).is_err());
 assert!(restored_content_types(&[make("image.png", Some(0xffff_fffe))]).is_err());
}
#[test] fn preview_and_kernel_gate_agree() {
 for (name, bytes) in [("text.txt", &b"hello"[..]), ("text.jpg", &b"hello"[..]), ("file.bin", &b"hello"[..]), ("none", &b"hello"[..]), ("empty.txt", &b""[..]), ("bad.txt", &b"\xff"[..])] {
  assert_eq!(infer_named_bytes(name, bytes), r::fs::content_type_boundary::verify_named_bytes(name, bytes).ok());
 }
}
#[test] fn wire_requires_digest_for_inference_and_rejects_unknown_flags() {
 for flags in 0u32..16 {
  let mut bytes = flags.to_le_bytes().to_vec(); bytes.extend_from_slice(&[0;32]); bytes.extend_from_slice(b"file.png");
  let parsed = decode_checked_write_payload(&bytes);
  assert_eq!(parsed.is_ok(), matches!(flags, 0 | 1 | 2 | 6));
  if let Ok((options, path)) = parsed {
   assert_eq!(options.infer_blob, flags == 6); assert_eq!(options.expected_sha256.is_some(), flags & 2 != 0); assert_eq!(options.create_new, flags == 1); assert_eq!(path, "file.png");
  }
 }
}
'''
    actions = (ROOT.parent / 'TRUEOS-Buildins/termdir/src/actions.rs').read_text()
    for signature in ['pub enum MenuCommand', 'pub enum MenuContext', 'pub enum MenuSection', 'pub struct MenuEntry']:
        harness += '#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n' + item(actions, signature)
    start = actions.index('pub const MENU_ENTRIES:')
    end = actions.index('\n];', start) + 3
    harness += actions[start:end]
    harness += 'struct MenuState; impl MenuState {'
    harness += item(actions, 'pub fn is_visible(')
    harness += item(actions, 'pub fn index_for_command(') + '}'
    harness += '#[test]\n' + item(actions, 'fn infer_is_only_visible_for_one_blob_file(')
    with tempfile.TemporaryDirectory(prefix='trueos-inference-') as tmp:
        tmp = Path(tmp)
        lib = tmp / 'libinfer.rlib'
        subprocess.run(['rustc', '--edition=2021', '--crate-name', 'infer', '--crate-type=rlib', str(ROOT / 'vendor/infer/src/lib.rs'), '-o', str(lib)], check=True)
        source = tmp / 'tests.rs'
        source.write_text(harness)
        subprocess.run(['rustc', '--edition=2024', '--test', str(source), '--extern', f'infer={lib}', '-o', str(tmp / 'tests')], check=True)
        subprocess.run([str(tmp / 'tests')], check=True)

if __name__ == '__main__':
    main()
