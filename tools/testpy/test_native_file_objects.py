#!/usr/bin/env python3
"""Exercise the unchanged production object authority over the real TRUEOSFS engine.

Only block storage, mount admission and index/cache maintenance boundaries are
host fixtures. File records, publication, overwrite, append, rename and deletion
use the actual trueos-fs crate. No rig or user database is touched.
"""
from pathlib import Path
import argparse,datetime,hashlib,json,re,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
def main():
    parser=argparse.ArgumentParser();parser.add_argument('--receipt',type=Path);args=parser.parse_args()
    source=ROOT/'src/r/fs/trueosfs_file_objects.rs';fixture=Path(__file__).with_name('fixtures')/'native_file_objects.rs'
    text=fixture.read_text().replace('@FILE_OBJECT_SOURCE@',str(source)).replace('@CABI_CODES_SOURCE@',str(ROOT/'src/r/cabi_codes.rs'))
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
spin="0.10"
''')
        command=['cargo','+'+toolchain,'test','--offline','--manifest-path',str(path/'Cargo.toml'),'--target','x86_64-unknown-linux-gnu','--target-dir','/tmp/trueos-native-file-object-tests','--','--test-threads=1']
        completed=subprocess.run(command,cwd=path,text=True,capture_output=True,timeout=180)
        print(completed.stdout,end='');print(completed.stderr,end='')
        if args.receipt:
            sources=[source,fixture,Path(__file__),ROOT/'crates/trueos-fs/src/lib.rs']
            args.receipt.write_text(json.dumps({'status':'passed' if completed.returncode==0 else 'failed','utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'command':command,'source_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sources},'stdout':completed.stdout,'stderr':completed.stderr,'scope':'Exact production native object authority + real TRUEOSFS engine, memory-backed BlockIo; no rig or platform block driver validation.'},indent=2)+'\n')
        if completed.returncode:raise SystemExit(completed.returncode)
if __name__=='__main__':main()
