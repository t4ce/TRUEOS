#!/usr/bin/env python3
"""Select the native filesystem object backend in a private Rust source tree."""
from pathlib import Path
import argparse
import hashlib
import os
import tempfile

ROOT=Path(__file__).resolve().parents[2]

def replace_once(source,needle,replacement,path):
    if replacement in source:return source
    if source.count(needle)!=1:raise ValueError(f'{path}: missing or ambiguous FS adaptation anchor {needle!r}')
    return source.replace(needle,replacement,1)

def installation(root):
    library=root/'library/std/src'
    changes={}
    path=library/'sys/fs/trueos.rs';reference=(ROOT/'tools/rust-std/trueos_fs.rs').read_text()
    checksum=path.with_suffix('.sha256')
    if path.exists() and path.read_text()!=reference:
        if not checksum.exists() or checksum.read_text().strip()!=hashlib.sha256(path.read_bytes()).hexdigest():
            raise ValueError(f'{path}: unrecognized native filesystem source; refusing to overwrite edits')
    changes[path]=reference;changes[checksum]=hashlib.sha256(reference.encode()).hexdigest()+'\n'
    path=library/'sys/fs/mod.rs';source=path.read_text()
    before='cfg_select! {\n';after=before+'''    target_os = "trueos" => {
        mod trueos;
        use trueos as imp;
        pub use trueos::{chown, fchown, lchown, mkfifo, chroot};
        pub(crate) use trueos::debug_assert_fd_is_open;
    }
'''
    source=replace_once(source,before,after,path)
    source=replace_once(source,'#[cfg(not(any(target_family = "unix", target_os = "windows", target_os = "wasi")))]','#[cfg(any(target_os = "trueos", not(any(target_family = "unix", target_os = "windows", target_os = "wasi"))))]',path)
    changes[path]=source
    for relative,signatures in {
        'os/fd/owned.rs':['impl AsFd for fs::File {','impl From<fs::File> for OwnedFd {','impl From<OwnedFd> for fs::File {'],
        'os/fd/raw.rs':['impl AsRawFd for fs::File {','impl FromRawFd for fs::File {','impl IntoRawFd for fs::File {'],
        'os/unix/fs.rs':['impl MetadataExt for fs::Metadata {'],
        'os/linux/fs.rs':['impl MetadataExt for Metadata {'],
    }.items():
        path=library/relative;source=path.read_text()
        for signature in signatures:source=replace_once(source,signature,'#[cfg(not(target_os = "trueos"))]\n'+signature,path)
        if relative=='os/unix/fs.rs':
            source=replace_once(source,'use super::platform::fs::MetadataExt as _;','#[cfg(not(target_os = "trueos"))]\nuse super::platform::fs::MetadataExt as _;',path)
        changes[path]=source
    path=library/'os/unix/io/mod.rs';source=path.read_text()
    source=replace_once(source,'fn null_fd() -> io::Result<OwnedFd> {','''#[cfg(target_os = "trueos")]
fn null_fd() -> io::Result<OwnedFd> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "native file objects cannot become Unix descriptors"))
}
#[cfg(not(target_os = "trueos"))]
fn null_fd() -> io::Result<OwnedFd> {''',path)
    changes[path]=source
    # Native identity is explicit. No libc stat struct or invented timestamp.
    path=library/'fs.rs';source=path.read_text()
    addition='''
#[cfg(target_os = "trueos")]
impl Metadata {
    #[stable(feature = "trueos_fs_identity", since = "1.99.0")]
    pub fn root_id(&self) -> u32 { self.0.root_id() }
    #[stable(feature = "trueos_fs_identity", since = "1.99.0")]
    pub fn file_id(&self) -> Option<u64> { self.0.file_id() }
    #[stable(feature = "trueos_fs_identity", since = "1.99.0")]
    pub fn revision(&self) -> Option<u64> { self.0.revision() }
}
'''
    if addition not in source:source+=addition
    access_key_addition='''
#[cfg(target_os = "trueos")]
impl File {
    /// Compare the effective native access-key requirements of these open files.
    ///
    /// Both captured and current ancestor requirements participate. This proves
    /// equivalence without changing either file's keys. Files from different
    /// native roots cannot be compared. Access and identity failures propagate.
    #[stable(feature = "trueos_fs_access_key", since = "1.99.0")]
    pub fn has_same_access_key(&self, other: &File) -> io::Result<bool> {
        self.inner.has_same_access_key(&other.inner)
    }
}
'''
    if access_key_addition not in source:source+=access_key_addition
    changes[path]=source
    return changes

def install(root,check=False):
    changes=installation(root)
    needed=[p for p,s in changes.items() if not p.exists() or p.read_text()!=s]
    if check and needed:raise ValueError('native FS backend not installed: '+', '.join(map(str,needed)))
    staged=[]
    try:
        for path in needed:
            with tempfile.NamedTemporaryFile(mode='w',encoding='utf-8',dir=path.parent,delete=False) as f:
                f.write(changes[path]);staged.append((Path(f.name),path))
            staged[-1][0].chmod(0o644)
        for temporary,path in staged:os.replace(temporary,path)
    finally:
        for temporary,_ in staged:temporary.unlink(missing_ok=True)
    print('trueos-rust-std-fs: native objects, shared offsets, cross-owner locks, committed writes, retained unlinked identities')

def main():
    parser=argparse.ArgumentParser();parser.add_argument('--check',action='store_true');parser.add_argument('rust_src',type=Path);args=parser.parse_args();root=args.rust_src.resolve()
    if root.name=='library':root=root.parent
    if not (root/'library/std/src/sys/fs/mod.rs').is_file():raise SystemExit('expected rust source root containing library/std')
    install(root,args.check)
if __name__=='__main__':main()
