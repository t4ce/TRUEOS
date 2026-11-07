#!/usr/bin/env python3
"""Compile and exercise production native Tokio networking, without Unix sockets.

Host behavior tests execute complete Tokio, Mio and native scheduler sources
against a socket-service ABI simulator. Separate actual core+alloc builds use a
target with no Unix family and link TCP creation/bind/connect/listen, asynchronous
TCP/UDP, options and DNS. No builtin selection or boot publication is changed.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_native_mio import environment, manifest, info
from test_native_tokio import TOOLCHAIN, NATIVE_FEATURES
import audit_platform_contracts as contracts

ROOT = Path(__file__).resolve().parents[2]
BUILDINS = ROOT.parent / 'TRUEOS-Buildins'
FIXTURES = Path(__file__).with_name('native_tokio')
MIO = ROOT / 'vendor/mio-1.2.0'
TOKIO = BUILDINS / 'vendor/tokio-1.54.1'
TARGET = BUILDINS / 'tools/trueos-object-native.json'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    output = parser.parse_args().output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    checks = []
    features = NATIVE_FEATURES + ['net']
    cargo = ['rustup', 'run', TOOLCHAIN, 'cargo']
    with tempfile.TemporaryDirectory(prefix='trueos-tokio-network-') as directory:
        work = Path(directory)

        def run(label, command, env):
            log = output / (label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=240)
            checks.append({'label': label, 'exit': result.returncode,
                           'log': str(log), 'command': command})
            if result.returncode:
                raise RuntimeError(f'{label} failed\n{log.read_text()[-16000:]}')

        # Cargo selects host dependencies before rustc sees the explicit native
        # cfg used by this host harness. Supply Mio's native dependencies in a
        # temporary manifest pointing at the unchanged production sources.
        host = work / 'host'
        manifest(host / 'mio', f'''[package]
name="mio"
version="1.2.0"
edition="2024"
[lib]
path={json.dumps(str(MIO / 'src/lib.rs'))}
[features]
default=[]
std-compat=["dep:libc"]
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
        manifest(host, f'''[package]
name="tokio-native-network-behavior"
version="0.0.0"
edition="2024"
[workspace]
[[bin]]
name="tokio-native-network-behavior"
path={json.dumps(str(FIXTURES / 'network_behavior.rs'))}
[dependencies]
tokio={{path={json.dumps(str(TOKIO))},default-features=false,features={json.dumps(features)}}}
trueos-runtime={{path={json.dumps(str(ROOT / 'crates/trueos-runtime'))}}}
[patch.crates-io]
mio={{path="mio"}}
[profile.dev]
panic="abort"
[profile.release]
panic="abort"
''')
        for mode, args in [('debug', []), ('release', ['--release'])]:
            run('native-network-' + mode, cargo + ['run', '--offline', *args,
                '--manifest-path', str(host / 'Cargo.toml'), '--target', 'x86_64-unknown-linux-gnu'],
                environment(output / 'host-target', native_host=True))

        probe = work / 'probe'
        manifest(probe, f'''[package]
name="tokio-native-network-probe"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path={json.dumps(str(FIXTURES / 'network_probe.rs'))}
[[bin]]
name="tokio-native-network-probe"
path={json.dumps(str(FIXTURES / 'network_entry.rs'))}
[dependencies]
tokio={{path={json.dumps(str(TOKIO))},default-features=false,features={json.dumps(features)}}}
trueos={{path={json.dumps(str(BUILDINS / 'vendor/trueos'))},default-features=false,features=["default-global-allocator","default-panic-handler"]}}
[patch.crates-io]
mio={{path={json.dumps(str(MIO))}}}
[profile.dev]
panic="abort"
[profile.release]
panic="abort"
''')
        for mode, args in [('debug', []), ('release', ['--release'])]:
            run('native-object-' + mode, cargo + ['build', '--offline', *args,
                '--manifest-path', str(probe / 'Cargo.toml'), '--bin', 'tokio-native-network-probe',
                '--target', str(TARGET), '-Zbuild-std=core,compiler_builtins,alloc',
                '-Zbuild-std-features=compiler-builtins-mem', '-Zjson-target-spec'],
                environment(output / 'native-target'))

    module = output / 'native-target' / TARGET.stem / 'release/tokio-native-network-probe.o'
    symbols = sorted({line.split()[-1] for line in subprocess.check_output(
        ['nm', '-u', str(module)], text=True).splitlines() if line.strip()})
    no_std_dir = output / 'native-target' / TARGET.stem
    forbidden = [str(path) for pattern in ['libstd-*.rlib', 'liblibc-*.rlib', 'libsocket2-*.rlib']
                 for path in no_std_dir.rglob(pattern)]
    assert not forbidden, forbidden
    cfg = subprocess.check_output(['rustup', 'run', TOOLCHAIN, 'rustc', '-Zunstable-options',
        '--print', 'cfg', '--target', str(TARGET)], cwd=ROOT, text=True).splitlines()
    assert 'unix' not in cfg and 'target_env=""' in cfg and not any(x.startswith('target_family=') for x in cfg)
    known = {}
    for path in [ROOT / 'src/unix_compat.rs', ROOT / 'src/hv/blueprint/blueprint.rs']:
        known.update(contracts.resolver_arms(path))
    declared = set(re.findall(r'\bfn\s+(\w+)\(', (ROOT / 'crates/trueos-v/src/bp_abi.rs').read_text()))
    unknown = sorted(set(symbols) - known.keys() - declared)
    assert not unknown, unknown
    classifier = (ROOT / 'src/unix_compat.rs').read_text().split('pub(crate) fn is_unix_import', 1)[1].split('pub(crate) fn resolve_import', 1)[0]
    legacy = sorted(set(symbols).intersection(re.findall(r'"([^"\n]+)"', classifier)))
    assert not legacy, legacy
    # Count successful cases in each process; any panic aborts before a receipt.
    counts = {}
    for mode in ['debug', 'release']:
        log = (output / ('native-network-' + mode + '.log')).read_text()
        counts[mode] = len(re.findall(r'^ok ', log, re.M))
        assert counts[mode] == 16, counts
    receipt = {'scope': __doc__, 'checks': checks, 'behavior_tests': counts,
               'object': info(module), 'undefined': symbols, 'unix_imports': legacy,
               'unknown_source_contracts': unknown, 'target_cfg': cfg,
               'fixtures': [info(path) for path in sorted(FIXTURES.glob('network_*.rs'))],
               'limits': ['Behavioral service is simulated; physical networking and VM ownership require integration verification.',
                          'Resolver source membership does not prove the deployed kernel contains these hooks.']}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(f'Native Tokio networking: {counts}, native dev/release core+alloc links, {len(symbols)} imports, zero Unix/unknown contracts.')


if __name__ == '__main__':
    main()
