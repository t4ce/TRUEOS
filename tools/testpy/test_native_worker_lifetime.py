#!/usr/bin/env python3
"""Run actual SDK worker and kernel lifetime tests without unrelated CABI linkage."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

ROOT=Path(__file__).resolve().parents[1]

def main():
    with tempfile.TemporaryDirectory(prefix='trueos-native-worker-') as directory:
        root=Path(directory);(root/'src').mkdir()
        (root/'Cargo.toml').write_text('[package]\nname="native-worker-tests"\nversion="0.1.0"\nedition="2024"\n[dependencies]\ntokio={version="=1.52.3",default-features=false,features=["sync"]}\n[workspace]\n')
        worker=ROOT.parent/'TRUEOS-Blueprints/api/src/worker.rs'
        lifetime=ROOT/'src/r/blocking/lifetime.rs'
        source=lifetime.read_text()
        start=source.index('impl State {');end=source.index('\n}',start)+2
        tests=item(str(lifetime),'tests')
        code=f'#![allow(dead_code,unexpected_cfgs)]\nextern crate alloc;\n#[path="{worker}"] mod worker;\n'
        code+='mod lifetime {\n'+item(str(lifetime),'State')+'\n'+source[start:end]+'\n'+tests+'\n}\n'
        (root/'src/lib.rs').write_text(code)
        subprocess.run(['cargo','test','--offline','--target','x86_64-unknown-linux-gnu','--target-dir',str(ROOT/'bld/native-worker-host-tests')],cwd=root,check=True)

if __name__=='__main__':main()
