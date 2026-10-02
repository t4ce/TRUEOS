#!/usr/bin/env python3
"""Verify the real infer registry and TRUEOSFS ingress boundary for Matroska.

Optionally set MKV_CONTENT_FIXTURE to check an external video header too.
"""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='trueos-mkv-identity-') as tmp:
    folder = Path(tmp)
    lib = folder / 'libinfer.rlib'
    subprocess.run(['rustc', '--edition=2021', '--crate-name', 'infer', '--crate-type',
                    'rlib', str(ROOT / 'vendor/infer/src/lib.rs'), '-o', str(lib)], check=True)
    source = folder / 'test.rs'
    source.write_text('''
#[path = "''' + str(ROOT / 'src/r/fs/content_type_boundary.rs') + '''"]
mod boundary;
#[test] fn matroska_identity_survives_foreign_ingress() {
    let mut paths = vec!["''' + str(ROOT / 'vendor/infer/testdata/sample.mkv') + '''".to_owned()];
    if let Ok(path) = std::env::var("MKV_CONTENT_FIXTURE") { paths.push(path); }
    for path in paths {
        use std::io::Read;
        let mut prefix = vec![0; 4096];
        let n = std::fs::File::open(path).unwrap().read(&mut prefix).unwrap();
        prefix.truncate(n);
        assert_eq!(infer::get(&prefix).unwrap().content_type_id(), infer::ContentTypeId::MATROSKA);
        assert_eq!(boundary::verify_named_bytes("video.MKV", &prefix), Ok(infer::ContentTypeId::MATROSKA));
        assert!(boundary::verify_named_bytes("video.mp4", &prefix).is_err());
        assert_eq!(boundary::mime_or_octet_stream(infer::ContentTypeId::MATROSKA), "video/x-matroska");
    }
    assert_eq!(infer::content_type_from_extension("mkv"), Some(infer::ContentTypeId::MATROSKA));
    assert_ne!(infer::ContentTypeId::MATROSKA, infer::ContentTypeId::WEBM);
}
''')
    binary = folder / 'test'
    subprocess.run(['rustc', '--edition=2024', '--test', str(source), '--extern',
                    f'infer={lib}', '-o', str(binary)], check=True)
    subprocess.run([str(binary)], check=True, env=os.environ.copy())
