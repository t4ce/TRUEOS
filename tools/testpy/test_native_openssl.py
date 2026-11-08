#!/usr/bin/env python3
"""Run actual WebAuthn/OpenSSL crypto against its native OS primitive provider.

Native-selected C and the unchanged Rust boundary execute with host std in
debug and release. No authenticator, browser, credentials or live data is used.
Native module admission is checked separately by the Blueprint package guard.
"""
import argparse
import json
import re
import subprocess
from pathlib import Path
from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest

SDK=ROOT.parent/'TRUEOS-Blueprints'

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    checks=[]
    def run(label,command,cwd,env=None):
        log=out/(label+'.log')
        with log.open('w')as stream:result=subprocess.run(command,cwd=cwd,env=env,stdout=stream,stderr=subprocess.STDOUT,timeout=1200)
        checks.append({'label':label,'exit':result.returncode,'log':str(log)})
        if result.returncode:raise RuntimeError(f'{label}:\n{log.read_text()[-14000:]}')
        return log.read_text()
    cargo=['rustup','run',TOOLCHAIN,'cargo']
    text=run('stage-production-openssl',cargo+['test','--offline','native_openssl_overlay::tests::stages_exact_original_openssl_sources_with_native_boundary','--','--nocapture'],SDK)
    staged=Path(re.search(r'native OpenSSL proof stage: (.+)',text)[1])/'source-overlay-crates'
    sys=staged/'openssl-sys-0.9.117-trueos-native';source=staged/'openssl-src-300.6.1+3.6.3-trueos'
    # Only the fixture build script selects native C while Rust retains host
    # std. Production source and boundary bytes otherwise remain unchanged.
    build=sys/'build/main.rs';original=build.read_text();assert original.count('fn main() {')==1
    build.write_text(original.replace('fn main() {','''fn main() {
    if std::env::var_os("CARGO_FEATURE_TRUEOS_NATIVE_TESTING").is_some() {
        unsafe { std::env::set_var("TARGET","x86_64-unknown-trueos"); std::env::set_var("CARGO_CFG_TARGET_OS","trueos"); }
    }
'''))
    app=out/'fixture'
    manifest(app,f'''[package]
name="trueos-native-openssl-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
openssl={{version="=0.10.81",features=["vendored"]}}
openssl-sys={{path={json.dumps(str(sys))},features=["vendored","trueos-native-testing"]}}
webauthn-rs-core="=0.5.5"
webauthn-attestation-ca="=0.5.5"
[patch.crates-io]
openssl-sys={{path={json.dumps(str(sys))}}}
openssl-src={{path={json.dumps(str(source))}}}
[[bin]]
name="trueos-native-openssl-proof"
path="main.rs"
''')
    (app/'main.rs').write_bytes(Path(__file__).with_name('native_openssl').joinpath('behavior.rs').read_bytes())
    env=environment(out/'host-target');env['CARGO_BUILD_JOBS']='2';env['CFLAGS']='-fno-stack-protector';env['CC_x86_64_unknown_trueos']='cc';env['AR_x86_64_unknown_trueos']='ar'
    counts={}
    for mode,flags in [('debug',[]),('release',['--release'])]:
        text=run('actual-webauthn-'+mode,cargo+['run','--offline',*flags,'--target','x86_64-unknown-linux-gnu','--manifest-path',str(app/'Cargo.toml')],staged.parent,env)
        counts[mode]=len(re.findall(r'^ok ',text,re.M));assert counts[mode]==6,counts
    for mode,optimization in [('debug','0'),('release','3')]:
        binary=out/('boundary-tests-'+mode)
        run('compile-boundary-tests-'+mode,['rustup','run',TOOLCHAIN,'rustc','--edition=2024','--test','-C','opt-level='+optimization,str(SDK/'src/native_openssl_boundary.rs'),'-o',str(binary)],out)
        run('boundary-contracts-'+mode,[str(binary),'--test-threads=1'],out)
    archive,=(out/'host-target/x86_64-unknown-linux-gnu/release/build').glob('openssl-sys-*/out/openssl-build/install/lib/libcrypto.a')
    imports=run('native-crypto-archive-imports',['nm','-u',str(archive)],out)
    forbidden=set(re.findall(r'\bU (\S+)',imports))&{'malloc','calloc','realloc','free','posix_memalign','__errno_location','__xpg_strerror_r','getenv','getpid','stat','time','gettimeofday','clock_gettime','nanosleep','sched_yield','getentropy','getrandom','syscall'}
    assert not forbidden,forbidden
    assert not re.search(r'\bU pthread_\w+',imports),imports
    (out/'receipt.json').write_text(json.dumps({'scope':__doc__,'checks':checks,'behavior_cases':counts,'archive':info(archive),'sources':[info(SDK/'src'/name)for name in ['native_openssl_overlay.rs','native_openssl_boundary.rs','native_openssl_boundary.h']],'limits':['Host std executes the real native boundary; native Blueprint admission requires its separate production package guard.']},indent=2)+'\n')
    print(out/'receipt.json')
if __name__=='__main__':main()
