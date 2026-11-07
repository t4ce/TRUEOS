#!/usr/bin/env python3
"""Exercise native socket IDs through the std-facing Mio/Tokio API.

The complete production libraries run against the existing in-memory socket ABI
simulator. Native readiness, cancellation, partial I/O and ownership are real
library code; packets, guest identity and the kernel scheduler are simulated.
A separate ordinary-host control exercises the opt-in feature on Linux.
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

BUILDINS = ROOT.parent / 'TRUEOS-Buildins'
FIXTURES = Path(__file__).with_name('native_tokio')

MIO_BOUNDARY_CASES = r'''
#[test]
fn native_net_rejects_unix_descriptors_before_registration() {
    let poll = Poll::new().unwrap();
    let error = poll.registry().register(&mut mio::unix::SourceFd(&0), Token(9), Interest::READABLE).unwrap_err();
    assert_eq!(error.kind(), ErrorKind::Unsupported);
}
#[test]
fn native_net_rejects_unix_pipe_creation() {
    assert_eq!(mio::unix::pipe::new().unwrap_err().kind(), ErrorKind::Unsupported);
}
#[test]
fn native_net_rejects_pathname_sockets_without_panicking() {
    assert_eq!(mio::net::UnixStream::connect("/native-is-not-a-unix-socket").unwrap_err().kind(), ErrorKind::Unsupported);
    assert_eq!(mio::net::UnixListener::bind("/native-is-not-a-unix-socket").unwrap_err().kind(), ErrorKind::Unsupported);
}
'''

TOKIO_BOUNDARY_CASES = r'''
 test("std-facing native sockets reject transfers to Unix descriptors",||{current().block_on(async{
  let listener=TcpListener::bind("127.0.0.1:0").await.unwrap();
  let stream=TcpStream::connect(listener.local_addr().unwrap()).await.unwrap();
  let (peer,_)=listener.accept().await.unwrap();
  assert_eq!(stream.into_std().unwrap_err().kind(),std::io::ErrorKind::Unsupported);
  assert_eq!(listener.into_std().unwrap_err().kind(),std::io::ErrorKind::Unsupported);
  drop(peer);
  let udp=UdpSocket::bind("127.0.0.1:0").await.unwrap();
  assert_eq!(udp.into_std().unwrap_err().kind(),std::io::ErrorKind::Unsupported);
 });});
 test("std-facing native sockets reject imported Unix descriptors",||{
  let listener=std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  assert_eq!(TcpListener::from_std(listener).unwrap_err().kind(),std::io::ErrorKind::Unsupported);
  let udp=std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
  assert_eq!(UdpSocket::from_std(udp).unwrap_err().kind(),std::io::ErrorKind::Unsupported);
 });
'''


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--mio', type=Path, default=ROOT / 'vendor/mio-1.2.0')
    parser.add_argument('--tokio', type=Path, default=BUILDINS / 'vendor/tokio-1.54.1')
    parser.add_argument('--full', action='store_true', help='Also exercise runtime startup with all Tokio features')
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    mio = args.mio.resolve()
    tokio = args.tokio.resolve()
    checks = []
    cargo = ['rustup', 'run', TOOLCHAIN, 'cargo']
    with tempfile.TemporaryDirectory(prefix='trueos-native-net-std-') as temporary:
        work = Path(temporary)

        def run(label, command, env, expected=0):
            log = output / (label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=180)
            checks.append({'label': label, 'exit': result.returncode, 'expected': expected,
                           'command': command, 'log': str(log)})
            if (expected == 0 and result.returncode != 0) or (expected != 0 and result.returncode == 0):
                raise RuntimeError(f'{label} failed\n{log.read_text()[-16000:]}')
            return log.read_text()

        native = work / 'native'
        # Explicit trueos cfg is applied after Cargo's host dependency selection.
        # Supply native target dependencies without replacing production sources.
        manifest(native / 'mio', f'''[package]
name="mio"
version="1.2.0"
edition="2024"
[lib]
path={json.dumps(str(mio / 'src/lib.rs'))}
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
[lints.rust]
unexpected_cfgs={{level="warn",check-cfg=['cfg(mio_unsupported_force_poll_poll)','cfg(mio_unsupported_force_waker_pipe)']}}
''')
        manifest(native, f'''[package]
name="std-facing-native-network"
version="0.0.0"
edition="2024"
[workspace]
[features]
full-signal-check=["tokio/full"]
native-fd-check=[]
[dependencies]
mio={{path="mio",default-features=false,features=["std-compat","native-net","os-poll","os-ext","net"]}}
tokio={{path={json.dumps(str(tokio))},default-features=false,features=["native-net","net","rt","rt-multi-thread","io-util","sync","time"]}}
trueos-runtime={{path={json.dumps(str(ROOT / 'crates/trueos-runtime'))}}}
[patch.crates-io]
mio={{path="mio"}}
[[test]]
name="mio_native_std"
path="mio_behavior.rs"
[[bin]]
name="tokio-native-std"
path="network_behavior.rs"
[[bin]]
name="native-fd-trait-rejection"
path="fd_trait.rs"
[profile.dev]
panic="abort"
[profile.release]
panic="abort"
''')
        original = (Path(__file__).with_name('native_mio') / 'native_tcp.rs').read_text()
        # Preserve all ABI/behavior cases; only the selected I/O trait identity
        # differs between the no_std suite and this std-facing suite.
        (native / 'mio_behavior.rs').write_text(original.replace('use trueos_io::{', 'use mio::io::{') + MIO_BOUNDARY_CASES)
        (native / 'boundary.rs').write_text((FIXTURES / 'boundary.rs').read_text())
        boundary = (FIXTURES / 'network_boundary.rs').read_text()
        old = 'crate::boundary::trueos_cabi_thread_current_id()<u32::MAX as usize'
        assert old in boundary
        # std host threads are not created by the native simulator's thread
        # spawn hook. Assert the real current thread is the Tokio worker.
        boundary = boundary.replace(old, 'std::thread::current().name().is_some_and(|name|name.starts_with("tokio-rt-worker"))')
        (native / 'network_boundary.rs').write_text(boundary)
        behavior = (FIXTURES / 'network_behavior.rs').read_text().replace('trueos_runtime::io::ErrorKind::', 'std::io::ErrorKind::')
        marker = ' println!("16 native-network behavior cases passed (host ABI simulation)");'
        assert marker in behavior
        signal_case = r'''
 #[cfg(feature="full-signal-check")]
 test("full runtime starts and rejects Unix signal subscriptions explicitly",||{current().block_on(async{
  use tokio::signal::unix::{signal,SignalKind};
  for kind in [SignalKind::interrupt(),SignalKind::child(),SignalKind::terminate(),SignalKind::window_change()] {
   assert_eq!(signal(kind).unwrap_err().kind(),std::io::ErrorKind::Unsupported);
  }
  assert_eq!(tokio::signal::ctrl_c().await.unwrap_err().kind(),std::io::ErrorKind::Unsupported);
 });});
'''
        behavior = behavior.replace(marker, TOKIO_BOUNDARY_CASES + signal_case + marker)
        (native / 'network_behavior.rs').write_text(behavior)
        (native / 'fd_trait.rs').write_text('#[cfg(feature="native-fd-check")]\nfn main() { fn fd<T:std::os::fd::AsRawFd>() {} fd::<tokio::net::TcpStream>(); }\n#[cfg(not(feature="native-fd-check"))]\nfn main() {}\n')
        counts = {}
        env = environment(output / 'native-host-target', native_host=True)
        for mode, flags in [('debug', []), ('release', ['--release'])]:
            text = run('mio-native-std-' + mode, cargo + ['test', '--offline', *flags,
                '--manifest-path', str(native / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu',
                '--test', 'mio_native_std', '--', '--test-threads=1'], env)
            counts['mio-' + mode] = int(re.search(r'test result: ok\. (\d+) passed', text)[1])
            assert counts['mio-' + mode] == 34
            text = run('tokio-native-std-' + mode, cargo + ['run', '--offline', *flags,
                '--manifest-path', str(native / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu',
                '--bin', 'tokio-native-std'], env)
            counts['tokio-' + mode] = len(re.findall(r'^ok ', text, re.M))
            assert counts['tokio-' + mode] == 18
        text = run('native-id-is-not-fd', cargo + ['check', '--offline', '--manifest-path',
            str(native / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu',
            '--features', 'native-fd-check', '--bin', 'native-fd-trait-rejection'], env, expected=1)
        assert 'AsRawFd' in text and 'TcpStream' in text and 'not satisfied' in text
        if args.full:
            text = run('native-std-full-startup', cargo + ['run', '--offline', '--manifest-path',
                str(native / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu', '--features',
                'full-signal-check', '--bin', 'tokio-native-std'], env)
            counts['tokio-full'] = len(re.findall(r'^ok ', text, re.M))
            assert counts['tokio-full'] == 19
        host = work / 'host'
        manifest(host, f'''[package]
name="native-net-host-control"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
tokio={{path={json.dumps(str(tokio))},default-features=false,features=["native-net","full","test-util"]}}
[patch.crates-io]
mio={{path={json.dumps(str(mio))}}}
[[bin]]
name="host-control"
path={json.dumps(str(FIXTURES / 'std_behavior.rs'))}
''')
        text = run('ordinary-host-control', cargo + ['run', '--offline', '--manifest-path',
            str(host / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu', '--bin', 'host-control'],
            environment(output / 'ordinary-host-target'))
        counts['host-control'] = len(re.findall(r'^ok ', text, re.M))
        assert counts['host-control'] == 10
    sources = []
    for source_root in [mio, tokio]:
        files = sorted((source_root / 'src').rglob('*.rs')) + [source_root / 'Cargo.toml']
        digest = hashlib.sha256()
        for path in files:
            digest.update(str(path.relative_to(source_root)).encode())
            digest.update(path.read_bytes())
        sources.append({'root': str(source_root), 'files': len(files), 'sha256': digest.hexdigest()})
    receipt = {'scope': __doc__, 'checks': checks, 'behavior_cases': counts, 'sources': sources,
               'limits': ['Socket service and native waits are simulated. No live packets, VM or rig reset.',
                          'Ordinary host control uses real Linux TCP/filesystem/process behavior.']}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(counts))
    print(output / 'receipt.json')


if __name__ == '__main__':
    main()
