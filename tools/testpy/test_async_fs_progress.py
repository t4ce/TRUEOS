#!/usr/bin/env python3
"""Exercise production atomic FS phases and check bounded privacy-safe wiring."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/r/io/async_fs_cabi.rs').read_text()
    assert source.count('ASYNC_FS_OPERATIONS.lock()')==1
    assert 'if !begin_request(&request)' in source
    assert 'PROGRESS.admit(owner, id, kind.progress_class()' in source
    assert 'PROGRESS.admit(owner, id, request.kind.progress_class()' in source
    assert 'PROGRESS.done(request.owner, request.id' in source
    assert 'PROGRESS.observe(owner, id' in source
    assert 'PROGRESS.release(owner, id)' in source
    observer=source.split('pub(crate) fn progress_lines(',1)[1].split('#[derive(Debug)]',1)[0]
    assert '.lock(' not in observer and '.lock()' not in observer
    for line in source.splitlines():
        if 'codex-progress-fs ' in line or 'blueprint-async-fs-timing ' in line:
            literal=re.search(r'"([^"\n]+)"',line).group(1)
            keys=re.findall(r'([a-z_]+)=',literal)
            assert keys and set(keys)<={'owner','id','class','phase','queued_ms','service_ms','consumer_ms','skipped','registry_waiters','registry_epoch_age_ms','pending','missed','admitted_age_ms','unobserved_ms'}
            assert not any(word in literal for word in ['path','payload','account','prompt'])
    with tempfile.TemporaryDirectory(prefix='trueos-fs-progress-') as directory:
        path=Path(directory)
        (path/'test.rs').write_text(f'#[path="{ROOT}/src/r/io/async_fs_progress.rs"] mod progress;')
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'test')],check=True)
        subprocess.run([str(path/'test')],check=True)
if __name__=='__main__':main()
