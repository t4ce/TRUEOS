#!/usr/bin/env python3
"""Verify native SDK DNS without C addrinfo/errno or a std dependency.

The complete SDK client and native scheduler run with host ABI transport stubs;
the actual kernel value adapter and guest/host routing bodies are unchanged.
Real DNS network traffic and physical carrier ownership remain integration work.
A separate no-Unix/core+alloc release object checks the real import boundary.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_native_mio_readiness import item
from test_native_mio import environment
from test_native_tokio import TOOLCHAIN
import audit_platform_contracts as contracts

ROOT=Path(__file__).resolve().parents[2]
BUILDINS=ROOT.parent/'TRUEOS-Buildins'
FIXTURE=Path(__file__).with_name('native_dns')

def function(source,name):
    return item(source, rf'(?:pub(?:\(crate\))? )?(?:extern "Rust" )?fn {name}\(')


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--output',type=Path,required=True)
    output=parser.parse_args().output.resolve();output.mkdir(parents=True,exist_ok=True)
    source=(ROOT/'src/r/net/vlayer.rs').read_text()
    body='#[derive(Clone,Copy,Debug)]\n'+item(source,'pub enum DnsResolveError')+'\n'+ '\n'.join(function(source,name) for name in ['dns_resolve_error_code','resolve_ipv4_for_sync_abi','trueos_platform_dns_resolve_ipv4'])
    # Extracted extern Rust adapter keeps its production unmangled export.
    body=body.replace('pub extern "Rust" fn trueos_platform_dns_resolve_ipv4','#[unsafe(no_mangle)]\npub extern "Rust" fn trueos_platform_dns_resolve_ipv4')
    behavior=(FIXTURE/'behavior.rs').read_text().replace('// @KERNEL_VALUE_ADAPTER@',body).replace('#[path = "../native_tokio/boundary.rs"]',f'#[path={json.dumps(str(ROOT/"tools/testpy/native_tokio/boundary.rs"))}]')
    (output/'production-behavior.rs').write_text(behavior)
    checks=[]
    with tempfile.TemporaryDirectory(prefix='trueos-native-dns-') as directory:
        work=Path(directory);(work/'src').mkdir();(work/'src/main.rs').write_text(behavior)
        manifest=f'''[package]\nname="native-dns-proof"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\ntrueos={{path={json.dumps(str(BUILDINS/'vendor/trueos'))},default-features=false,features=["native-runtime"]}}\n[patch.crates-io]\ntokio={{path={json.dumps(str(BUILDINS/'vendor/tokio-1.54.1'))}}}\nmio={{path={json.dumps(str(ROOT/'vendor/mio-1.2.0'))}}}\n[profile.dev]\npanic="abort"\n[profile.release]\npanic="abort"\n'''
        (work/'Cargo.toml').write_text(manifest)
        def run(label,command,env):
            log=output/(label+'.log')
            with log.open('w') as stream:r=subprocess.run(command,cwd=work,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=240)
            checks.append({'label':label,'exit':r.returncode,'log':str(log),'command':command})
            if r.returncode:raise RuntimeError(log.read_text()[-12000:])
        cargo=['rustup','run',TOOLCHAIN,'cargo']
        run('behavior',cargo+['run','--offline','--target','x86_64-unknown-linux-gnu','--manifest-path',str(work/'Cargo.toml')],environment(output/'target',native_host=True))
        assert 'Native DNS:' in (output/'behavior.log').read_text()
        (work/'src/main.rs').write_text((FIXTURE/'entry.rs').read_text())
        (work/'Cargo.toml').write_text(manifest.replace('features=["native-runtime"]','features=["native-runtime","default-global-allocator","default-panic-handler"]'))
        target=BUILDINS/'tools/trueos-object-native.json'
        run('native-object',cargo+['build','--offline','--release','--manifest-path',str(work/'Cargo.toml'),'--target',str(target),'-Zbuild-std=core,compiler_builtins,alloc','-Zbuild-std-features=compiler-builtins-mem','-Zjson-target-spec'],environment(output/'target'))
    module=output/'target/trueos-object-native/release/native-dns-proof.o'
    symbols=sorted({line.split()[-1] for line in subprocess.check_output(['nm','-u',str(module)],text=True).splitlines() if line.strip()})
    assert 'trueos_platform_dns_resolve_ipv4' in symbols and 'trueos_service_lane_submit_job' in symbols
    legacy=contracts.audit(ROOT/'tgt/buildins/release')['classifier']
    classifier=(ROOT/'src/unix_compat.rs').read_text().split('pub(crate) fn is_unix_import',1)[1].split('pub(crate) fn resolve_import',1)[0]
    legacy_symbols=set(re.findall(r'"([^"\n]+)"',classifier))
    assert not legacy_symbols.intersection(symbols)
    assert not list((output/'target/trueos-object-native/release/deps').glob('libstd-*.rlib'))
    known={}
    for path in [ROOT/'src/unix_compat.rs',ROOT/'src/hv/blueprint/blueprint.rs']:known.update(contracts.resolver_arms(path))
    cabi=set(re.findall(r'\bfn\s+(\w+)\(', (ROOT/'crates/trueos-v/src/bp_abi.rs').read_text()))
    unknown=sorted(set(symbols)-known.keys()-cabi);assert not unknown,unknown
    receipt={'scope':__doc__,'checks':checks,'undefined':symbols,'unix_imports':[],'unknown_source_contracts':unknown,'kernel_fragments_sha256':hashlib.sha256(body.encode()).hexdigest(),'kernel_source_sha256':hashlib.sha256(source.encode()).hexdigest(),'object_sha256':hashlib.sha256(module.read_bytes()).hexdigest(),'object_bytes':module.stat().st_size,'classifier':legacy}
    (output/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(f'Native DNS SDK/production-adapter behaviors and native release object passed; {len(symbols)} imports, zero Unix imports, zero unknown source contracts.')

if __name__=='__main__':main()
