#!/usr/bin/env python3
"""Decode every prepared built-in using the production kernel LZ4/tar code.

Run after preparing builtins. This test compiles a host-only decoder; it never
publishes an image, starts PXE or resets hardware.
"""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--builtins', type=Path, default=ROOT / 'tgt/buildins/dev')
    args = parser.parse_args()
    output = args.builtins.resolve()
    names = json.loads((output / 'buildins.json').read_text())['buildins']
    with tempfile.TemporaryDirectory(prefix='trueos-buildins-decode-') as directory:
        path = Path(directory)
        (path / 'src').mkdir()
        lz4 = (ROOT / 'src/r/lz4.rs').read_text().split('\npub async fn compress_frame(')[0]
        (path / 'src/lz4.rs').write_text(lz4)
        source = r'''
#![allow(dead_code)]
#[macro_export] macro_rules! log_warn { ($($tt:tt)*) => {} }
#[macro_export] macro_rules! log_info { ($($tt:tt)*) => {} }
#[path="lz4.rs"] mod lz4;
#[path=TAR_SOURCE] mod tar;
use redb::{Database, ReadableDatabase, ReadableTableMetadata, StorageBackend, TableDefinition};
use sha2::{Digest, Sha256};
const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const HASHES: TableDefinition<&str, &str> = TableDefinition::new("hashes");
const UPDATED: TableDefinition<&str, &str> = TableDefinition::new("updated");
fn main() {
    let root = std::path::PathBuf::from(std::env::args().nth(1).unwrap());
    let names: Vec<_> = std::env::args().skip(2).collect();
    for name in std::env::args().skip(2) {
        let package = std::fs::read(root.join("dist").join(format!("{name}.bp"))).unwrap();
        assert_eq!(&package[..4], b"TRBP");
        let raw_len = u32::from_le_bytes(package[20..24].try_into().unwrap()) as usize;
        let decoded = tar::decode_named_single_lz4(&package[24..], raw_len, "module.elf").unwrap();
        let expected = std::fs::read(root.join("objects").join(&name).join("module.stripped.o")).unwrap();
        assert_eq!(decoded.len(), raw_len);
        assert_eq!(decoded, expected, "{name}: kernel decoder output differs");
        assert_eq!(&decoded[..4], b"\x7fELF");
        assert_eq!(u16::from_le_bytes(decoded[16..18].try_into().unwrap()), 1);
        assert_eq!(u16::from_le_bytes(decoded[18..20].try_into().unwrap()), 62);
        println!("decoded {name}: {raw_len} bytes, x86-64 REL");
    }
    let compressed = std::fs::read(root.join("app.db.lz4")).unwrap();
    let expected = std::fs::read(root.join("app.db")).unwrap();
    let decoded = lz4::decompress_frame_cpu(&compressed, expected.len()).unwrap();
    assert_eq!(decoded, expected);
    let backend = redb::backends::InMemoryBackend::new();
    backend.set_len(decoded.len() as u64).unwrap();
    backend.write(0, &decoded).unwrap();
    drop(decoded);
    let database = Database::builder().set_cache_size(64 * 1024 * 1024).create_with_backend(backend).unwrap();
    {
        let transaction = database.begin_read().unwrap();
        let apps = transaction.open_table(APPS).unwrap();
        let hashes = transaction.open_table(HASHES).unwrap();
        let updated = transaction.open_table(UPDATED).unwrap();
        assert_eq!(apps.len().unwrap(), names.len() as u64);
        assert_eq!(hashes.len().unwrap(), apps.len().unwrap());
        assert_eq!(updated.len().unwrap(), apps.len().unwrap());
        for name in &names {
            let archive = format!("{name}.bp");
            let bytes = apps.get(archive.as_str()).unwrap().unwrap();
            assert_eq!(bytes.value(), std::fs::read(root.join("dist").join(&archive)).unwrap());
            assert_eq!(hashes.get(archive.as_str()).unwrap().unwrap().value(), format!("{:x}", Sha256::digest(bytes.value())));
            assert!(!updated.get(archive.as_str()).unwrap().unwrap().value().is_empty());
        }
    }
    let write = database.begin_write().unwrap();
    write.open_table(APPS).unwrap().insert("online-test.bp", b"new app".as_slice()).unwrap();
    write.commit().unwrap();
    assert_eq!(database.begin_read().unwrap().open_table(APPS).unwrap().get("online-test.bp").unwrap().unwrap().value(), b"new app");
    println!("app.db image: all apps/hashes/dates match; writable in memory");
}
'''.replace('TAR_SOURCE', json.dumps(str(ROOT / 'src/r/tar.rs')))
        (path / 'src/main.rs').write_text(source)
        (path / 'Cargo.toml').write_text('''[package]
name="trueos-buildins-decode-test"
version="0.0.0"
edition="2024"
[dependencies]
lz4_flex={version="0.11.6",default-features=false,features=["safe-encode","safe-decode","checked-decode"]}
twox-hash={version="2.1.2",default-features=false,features=["xxhash32"]}
redb={version="=4.4.0",default-features=false}
sha2="=0.10.9"
''')
        subprocess.run(['cargo', 'run', '--offline', '--quiet', '--manifest-path', str(path / 'Cargo.toml'), '--', str(output), *names],
                       cwd=path, check=True)
    for name in names:
        package = (output / 'dist' / f'{name}.bp').read_bytes()
        magic, version, flags, entry, size, raw_size = struct.unpack('<4sHHQII', package[:24])
        assert version == 1 and flags & 0xff == 3 and size == len(package) - 24
        relocations = subprocess.check_output(['readelf', '-rW', str(output / 'objects' / name / 'module.stripped.o')], text=True)
        supported = {'R_X86_64_NONE', 'R_X86_64_64', 'R_X86_64_32S', 'R_X86_64_PC32', 'R_X86_64_PLT32',
                     'R_X86_64_GOTPCREL', 'R_X86_64_GOTPCRELX', 'R_X86_64_REX_GOTPCRELX'}
        for line in relocations.splitlines():
            if 'R_X86_64_' in line:
                assert line.split()[2] in supported, f'{name}: unsupported relocation: {line}'
    print(f'All {len(names)} packages use the production decode path and supported relocations.')


if __name__ == '__main__':
    main()
