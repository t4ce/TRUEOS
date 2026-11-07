#!/usr/bin/env python3
"""Stage the production Rama overlay and exercise real Rama/Tokio/Mio code.

Native socket service and wake boundaries are simulated. These checks cover the
pinned provider's local bind, NODELAY, partial I/O, owned listener/client paths,
and explicit rejection of unsupported options. Ordinary-host control uses real
Linux TCP. No packages are published and no rig or shared BP cache is changed.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_native_mio import ROOT, TOOLCHAIN, environment, manifest, info

SDK = ROOT.parent / 'TRUEOS-Blueprints'
TOKIO = ROOT.parent / 'TRUEOS-Buildins/vendor/tokio-1.54.1'
FIXTURES = Path(__file__).with_name('native_tokio')

STAGE_DRIVER = r'''
use std::{path::{Path,PathBuf},fs};
#[path=@MODULE@] mod overlay;
fn package_value(path:&Path,key:&str)->Result<Option<String>,String>{
 let text=fs::read_to_string(path).map_err(|e|e.to_string())?;let mut package=false;
 for line in text.lines(){let line=line.trim();if line.starts_with('['){package=line=="[package]";}
  if package {if let Some((name,value))=line.split_once('='){if name.trim()==key{return Ok(Some(value.trim().trim_matches('"').to_owned()));}}}}
 Ok(None)
}
fn package_name(path:&Path)->Result<String,String>{package_value(path,"name")?.ok_or("missing package name".into())}
fn package_version(path:&Path)->Result<Option<String>,String>{package_value(path,"version")}
fn reset_dir(path:&Path)->Result<(),String>{if path.exists(){fs::remove_dir_all(path).map_err(|e|e.to_string())?;}fs::create_dir_all(path).map_err(|e|e.to_string())}
fn copy_app_tree(source:&Path,dest:&Path)->Result<(),String>{
 fs::create_dir_all(dest).map_err(|e|e.to_string())?;
 for entry in fs::read_dir(source).map_err(|e|e.to_string())?{let entry=entry.map_err(|e|e.to_string())?;let target=dest.join(entry.file_name());
  if entry.file_type().map_err(|e|e.to_string())?.is_dir(){copy_app_tree(&entry.path(),&target)?;}else{fs::copy(entry.path(),target).map_err(|e|e.to_string())?;}}
 Ok(())
}
fn main(){let args:Vec<String>=std::env::args().collect();let staged=overlay::stage(&PathBuf::from(&args[1]),&PathBuf::from(&args[2])).unwrap();println!("{}",staged.display());}
'''

HOST = r'''
use rama_tcp::{server::TcpListener,client::{TcpStreamConnector,default_tcp_connect}};
use rama_net::{socket::{Interface,SocketOptions},address::HostWithPort};
use std::sync::Arc;
use tokio::io::{AsyncReadExt,AsyncWriteExt};
fn main(){tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async{
 let listener=TcpListener::bind_address("127.0.0.1:0").await.unwrap();let address=listener.local_addr().unwrap();let extensions=rama_core::extensions::Extensions::new();let(client,_)=default_tcp_connect(&extensions,HostWithPort::from((address.ip(),address.port()))).await.unwrap();let(peer,_)=listener.accept().await.unwrap();drop((client,peer,listener));println!("ok ordinary host Rama listener/client");
 let mut options=SocketOptions::default_tcp();options.address=Some("127.0.0.1:0".parse().unwrap());options.tcp_no_delay=Some(true);let options=Arc::new(options);let listener=TcpListener::build().bind(Interface::Socket(options.clone())).await.unwrap();let mut client=options.connect(listener.local_addr().unwrap()).await.unwrap();let(mut peer,_)=listener.accept().await.unwrap();client.write_all(b"host").await.unwrap();let mut bytes=[0;4];peer.read_exact(&mut bytes).await.unwrap();assert_eq!(&bytes,b"host");assert!(client.stream.nodelay().unwrap());println!("ok ordinary host Rama SocketOptions and real IO");
});}
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    candidates = sorted((Path.home() / '.cargo/registry/src').glob('*/rama-tcp-0.3.0-alpha.4'))
    assert len(candidates) == 1, candidates
    source = candidates[0]
    before = {str(path.relative_to(source)): info(path) for path in sorted((source / 'src').rglob('*.rs'))}
    checks = []
    counts = {}
    cargo = ['rustup', 'run', TOOLCHAIN, 'cargo']
    with tempfile.TemporaryDirectory(prefix='trueos-native-rama-') as temporary:
        work = Path(temporary)

        def run(label, command, env=None):
            log = output / (label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=180)
            checks.append({'label': label, 'exit': result.returncode, 'command': command, 'log': str(log)})
            if result.returncode:
                raise RuntimeError(f'{label} failed\n{log.read_text()[-14000:]}')
            return log.read_text()

        driver = work / 'stage.rs'
        driver.write_text(STAGE_DRIVER.replace('@MODULE@', json.dumps(str(SDK / 'src/rama_tcp_overlay.rs'))))
        run('compile-production-overlay', ['rustup', 'run', TOOLCHAIN, 'rustc', '--edition=2024',
            str(driver), '-o', str(work / 'stage')])
        staged = Path(run('stage-production-overlay', [str(work / 'stage'), str(work / 'stage-work'), str(source)]).strip())
        assert (staged / 'src/trueos_native.rs').read_bytes() == (SDK / 'src/rama_tcp_native.rs').read_bytes()
        after = {str(path.relative_to(source)): info(path) for path in sorted((source / 'src').rglob('*.rs'))}
        assert before == after, 'staging changed the registry source'
        mio = work / 'mio'
        manifest(mio, f'''[package]
name="mio"
version="1.2.0"
edition="2024"
[lib]
path={json.dumps(str(ROOT / 'vendor/mio-1.2.0/src/lib.rs'))}
[features]
default=[]
std-compat=["dep:libc"]
native-net=[]
log=["dep:log"]
net=[]
os-poll=[]
os-ext=["os-poll"]
[dependencies]
core3={{version="0.1.2",default-features=false,features=["alloc"]}}
trueos-io={{path={json.dumps(str(ROOT / 'crates/trueos-io'))}}}
spin="0.10"
libc={{version="0.2.183",optional=true}}
log={{version="0.4.8",optional=true}}
''')
        app = work / 'app'
        manifest(app, f'''[package]
name="rama-native-provider-check"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
rama-tcp={{path={json.dumps(str(staged))}}}
rama-net="=0.3.0-alpha.4"
rama-core="=0.3.0-alpha.4"
rama-error="=0.3.0-alpha.4"
rama-macros="=0.3.0-alpha.4"
rama-utils="=0.3.0-alpha.4"
tokio={{path={json.dumps(str(TOKIO))},features=["native-net","full"]}}
[patch.crates-io]
tokio={{path={json.dumps(str(TOKIO))}}}
mio={{path={json.dumps(str(mio))}}}
rama-dns={{path={json.dumps(str(SDK / 'vendor/rama-dns-0.3.0-alpha.4-trueos'))}}}
[[bin]]
name="rama-native-provider-check"
path="main.rs"
[profile.dev]
panic="abort"
[profile.release]
panic="abort"
''')
        lock = ROOT.parent / 'codex/codex-rs/Cargo.lock'
        (app / 'Cargo.lock').write_bytes(lock.read_bytes())
        for name in ['boundary.rs', 'network_boundary.rs']:
            text = (FIXTURES / name).read_text().replace('crate::boundary::trueos_cabi_thread_current_id()<u32::MAX as usize',
                'std::thread::current().name().is_some_and(|name|name.starts_with("tokio-rt-worker"))')
            (app / name).write_text(text)
        behavior = (FIXTURES / 'rama_behavior.rs').read_text().replace('@RAMA_NATIVE@', json.dumps(str(SDK / 'src/rama_tcp_native.rs')))
        (app / 'main.rs').write_text(behavior)
        env = environment(output / 'native-target', native_host=True)
        for mode, flags in [('debug', []), ('release', ['--release'])]:
            text = run('rama-native-' + mode, cargo + ['run', '--offline', *flags, '--manifest-path',
                str(app / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu'], env)
            counts['native-' + mode] = len(re.findall(r'^ok ', text, re.M))
            assert counts['native-' + mode] == 14, counts
        # The same staged crate's ordinary host branches must still exercise
        # real sockets when the explicit native-net feature is enabled.
        (app / 'main.rs').write_text(HOST)
        text = run('rama-ordinary-host', cargo + ['run', '--offline', '--manifest-path',
            str(app / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu'], environment(output / 'host-target'))
        counts['ordinary-host'] = len(re.findall(r'^ok ', text, re.M))
        assert counts['ordinary-host'] == 2
    receipt = {'scope': __doc__, 'checks': checks, 'behavior_cases': counts,
               'production_overlay': info(SDK / 'src/rama_tcp_overlay.rs'),
               'production_provider': info(SDK / 'src/rama_tcp_native.rs'), 'codex_lock': info(lock),
               'fixture': info(FIXTURES / 'rama_behavior.rs'), 'registry_source_unchanged': before == after,
               'limits': ['Native service and generation waits are simulated; no physical packet or guest ownership proof.',
                          'Core host staging I/O helpers are supplied by the isolated driver; production overlay source runs unchanged.']}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(counts))
    print(output / 'receipt.json')


if __name__ == '__main__':
    main()
