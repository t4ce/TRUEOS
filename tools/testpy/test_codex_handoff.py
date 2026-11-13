#!/usr/bin/env python3
"""Exercise both production halves of the handoff; mock only hardware/services."""
from pathlib import Path
import json,os,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
with tempfile.TemporaryDirectory(prefix='codex-handoff-') as d:
    work=Path(d);(work/'src').mkdir()
    (work/'Cargo.toml').write_text('[package]\nname="codex-handoff-tests"\nversion="0.1.0"\nedition="2024"\n[workspace]\n[dependencies]\nspin="0.10"\n')
    source=(ROOT/'tools/testpy/codex_handoff/harness.rs').read_text().replace('@TRUEOS_ROOT@',json.dumps(str(ROOT))[1:-1])
    (work/'src/lib.rs').write_text(source)
    env=os.environ.copy();env['CARGO_TARGET_DIR']=str(ROOT/'tgt/host-codex-handoff-tests')
    subprocess.run(['cargo','test','--release','--offline','--target','x86_64-unknown-linux-gnu','--','--test-threads=1'],cwd=work,env=env,check=True,timeout=180)

