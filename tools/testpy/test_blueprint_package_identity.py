#!/usr/bin/env python3
"""Verify the launch environment hashes complete packed bytes, independent of names.

The real SHA-256 implementation and production environment builder are compiled;
Python hashlib supplies independent expected values. No guest/package self hash.
"""
import hashlib
from pathlib import Path
import subprocess
import tempfile

from test_posix_renameat import function
from test_timezone_export import harness as timezone_harness

ROOT = Path(__file__).resolve().parents[2]
BLUEPRINT = ROOT / 'src/hv/blueprint/blueprint.rs'


def main():
    packed = b'TRUEOSBP\x03\x00\x18\x00packed-compressed-payload\x00\xff'
    other = b'OTHER-BP' + packed[8:]
    expected = hashlib.sha256(packed).hexdigest()
    changed = hashlib.sha256(other).hexdigest()
    source = '#![allow(dead_code)]\n' + timezone_harness().split('const CASES:')[0]
    source += '''
use sha2::{Digest, Sha256};
mod hv {
 pub struct BlueprintInstanceIdentity {
  pub instance:[u8;16], pub lineage:[u8;16], pub generation:u64,
  pub clone:bool, pub name:Option<String>, pub peer:Option<String>,
 }
 pub fn format_blueprint_uuid(_: &[u8;16])->String{"test-uuid".into()}
}
'''
    source += '\n'.join(function(BLUEPRINT, name) for name in (
        'build_process_env', 'sha256_hex', 'safe_archive_stem',
        'uses_volatile_codex_files', 'app_fs_common_root'))
    source += f'''
const PACKED:&[u8]=&{list(packed)};
const CHANGED:&[u8]=&{list(other)};
#[test]fn packed_bytes_not_name_root_instance_or_script_define_identity(){{
 let identity=hv::BlueprintInstanceIdentity{{instance:[1;16],lineage:[2;16],generation:7,clone:true,name:Some("instance".into()),peer:None}};
 for (archive,root,id,script,scope) in [
  ("codex.bp",Some("apps/codex"),None,None,false),
  ("renamed.bp",Some("apps/renamed"),Some(&identity),Some("script"),true),
 ]{{
  let env=build_process_env(archive,PACKED,root,id,script,scope);
  assert_eq!(env["TRUEOS_APP_SHA256"],"{expected}");
  assert_eq!(env["TRUEOS_APP_ARCHIVE"],archive);
  assert_eq!(env["TRUEOS_APP_SHA256"].len(),64);
 }}
}}
#[test]fn changing_packed_header_changes_identity_with_same_archive_name(){{
 let first=build_process_env("codex.bp",PACKED,None,None,None,false);
 let second=build_process_env("codex.bp",CHANGED,None,None,None,false);
 assert_eq!(first["TRUEOS_APP_SHA256"],"{expected}");
 assert_eq!(second["TRUEOS_APP_SHA256"],"{changed}");
 assert_ne!(first["TRUEOS_APP_SHA256"],second["TRUEOS_APP_SHA256"]);
}}
#[test]fn digest_is_sha256_not_an_embedded_payload_or_empty_identity(){{
 assert_eq!(sha256_hex(b""),"{hashlib.sha256(b'').hexdigest()}");
 assert_eq!(sha256_hex(b"abc"),"{hashlib.sha256(b'abc').hexdigest()}");
 assert_ne!(sha256_hex(PACKED),sha256_hex(&PACKED[24..]));
}}
'''
    with tempfile.TemporaryDirectory(prefix='blueprint-package-identity-') as name:
        path = Path(name)
        (path / 'src').mkdir()
        (path / 'src/lib.rs').write_text(source)
        (path / 'Cargo.toml').write_text(f'''[package]
name="blueprint-package-identity-test"
version="0.0.0"
edition="2024"
[dependencies]
sha2="=0.10.9"
trueos-locale={{path="{ROOT / 'crates/trueos-locale'}"}}
''')
        subprocess.run(['cargo', 'test', '--lib', '--offline', '--quiet', '--manifest-path', str(path / 'Cargo.toml')], cwd=path, check=True)


if __name__ == '__main__':
    main()
