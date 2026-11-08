#!/usr/bin/env python3
"""Prove normal native check-only staging never packages or publishes stale output."""
from pathlib import Path
import argparse,tempfile,subprocess,json
from test_native_mio import ROOT,info
parser=argparse.ArgumentParser(description='Exercise actual check-only success/failure without touching an existing BP');parser.add_argument('--output',type=Path,required=True);out=parser.parse_args().output.resolve();out.mkdir(parents=True,exist_ok=True);sdk=ROOT.parent/'TRUEOS-Blueprints'
with tempfile.TemporaryDirectory(prefix='trueos-check-only-e2e-')as temporary:
 p=Path(temporary);(p/'src').mkdir();(p/'dist').mkdir();(p/'Cargo.toml').write_text('[package]\nname="check-only-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n');(p/'src/main.rs').write_text('#![no_std]\n#![no_main]\n#[panic_handler]fn panic(_: &core::panic::PanicInfo)->!{loop{}}\n#[unsafe(no_mangle)]pub extern "C"fn main(){}\n'.replace('"C"fn','"C" fn'));bp=p/'dist/check-only-proof.bp';bp.write_bytes(b'old-dist-package-sentinel');old=(bp.read_bytes(),bp.stat().st_mtime_ns)
 command=['cargo','run','--offline','--bin','trueos-blueprint','--',str(p),'--check'];log=out/'check-only-e2e.log'
 with log.open('w')as f:r=subprocess.run(command,cwd=sdk,stdout=f,stderr=subprocess.STDOUT)
 print('exit',r.returncode);print(log.read_text()[-6000:]);assert r.returncode==0
 assert old==(bp.read_bytes(),bp.stat().st_mtime_ns)
 receipts=list((p/'target').rglob('native-check.json'));assert len(receipts)==1;receipt=json.loads(receipts[0].read_text());assert receipt['status']=='checked-no-package'and receipt['package_produced']==False
 assert list(p.rglob('*.bp'))==[bp]
 (p/'src/main.rs').write_text((p/'src/main.rs').read_text()+'\ncompile_error!("intentional check-only failure");\n')
 failure_log=out/'check-only-e2e-failure.log'
 with failure_log.open('w')as f:failed=subprocess.run(command,cwd=sdk,stdout=f,stderr=subprocess.STDOUT)
 assert failed.returncode!=0
 assert not receipts[0].exists(),"an unsuccessful check retained its old success receipt"
 assert old==(bp.read_bytes(),bp.stat().st_mtime_ns)
 assert list(p.rglob('*.bp'))==[bp]
 (out/'check-only-e2e-receipt.json').write_text(json.dumps({'status':'passed','command':command,'native_check':receipt,'stale_package_bytes_and_mtime_preserved':True,'additional_bp_files':0,'failed_check_removed_old_receipt':True,'sources':[info(sdk/'src/check_only.rs'),info(sdk/'src/main.rs')]},indent=2)+'\n')
