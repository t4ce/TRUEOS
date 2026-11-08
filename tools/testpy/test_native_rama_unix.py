#!/usr/bin/env python3
"""Prove staged Rama Unix factories reject native use before filesystem effects.

The exact factory methods, listener fields and cleanup implementation are taken
from the production-staged pinned source. Only the surrounding Rama socket type
name is supplied by a real Socket2 alias. Tokio and host Unix sockets are real;
the native cfg is applied to the test crate alone, not its host dependencies.
This checks capability refusal, not a native Unix-domain transport provider.
"""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_native_mio import ROOT, TOOLCHAIN, environment, info, manifest
from test_native_rama_tcp import STAGE_DRIVER
from test_sdk_metadata_v2 import item

SDK = ROOT.parent / 'TRUEOS-Blueprints'
FIXTURE = Path(__file__).with_name('fixtures') / 'native_rama_unix.rs'


def selected_source(text: str) -> str:
    signatures = [
        '#[cfg(target_os = "trueos")]\n    pub async fn bind_path',
        '#[cfg(not(target_os = "trueos"))]\n    pub async fn bind_path',
        '#[cfg(target_os = "trueos")]\n    pub fn bind_socket',
        '#[cfg(not(target_os = "trueos"))]\n    pub fn bind_socket',
    ]
    for signature in signatures:
        assert text.count(signature) == 1, signature
    return '\n'.join([
        'use std::{io, path::{Path, PathBuf}};',
        'use std::os::unix::net::UnixListener as StdUnixListener;',
        'use tokio::net::UnixListener as TokioUnixListener;',
        'use tracing;',
        'mod rama_net { pub mod socket { pub mod core { pub use socket2::Socket; } } }',
        'pub struct UnixListenerBuilder;',
        'impl UnixListenerBuilder {',
        *(item(text, signature) for signature in signatures),
        '}',
        '#[derive(Debug)]', item(text, 'pub struct UnixListener {'),
        '#[derive(Debug)]', item(text, 'struct UnixSocketCleanup {'),
        item(text, 'impl Drop for UnixSocketCleanup {'),
    ])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    sources = sorted((Path.home() / '.cargo/registry/src').glob('*/rama-unix-0.3.0-alpha.4'))
    assert len(sources) == 1, sources
    source = sources[0]
    upstream = source / 'src/unix/server/listener.rs'
    original = info(upstream)
    checks = []
    counts = {}
    cargo = ['rustup', 'run', TOOLCHAIN, 'cargo']
    with tempfile.TemporaryDirectory(prefix='trueos-rama-unix-') as temporary:
        work = Path(temporary)

        def run(label, command, env=None):
            log = output / (label + '.log')
            with log.open('w') as stream:
                result = subprocess.run(command, cwd=work, env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=180)
            checks.append({'label': label, 'command': command, 'exit': result.returncode, 'log': str(log)})
            if result.returncode:
                raise RuntimeError(f'{label} failed\n{log.read_text()[-16000:]}')
            return log.read_text()

        driver = work / 'stage.rs'
        driver.write_text(STAGE_DRIVER.replace('@MODULE@', json.dumps(str(SDK / 'src/codex_native_socket_overlay.rs')))
                          .replace('overlay::stage(', 'overlay::stage_rama_unix('))
        run('compile-production-stage', ['rustup', 'run', TOOLCHAIN, 'rustc', '--edition=2024',
                                        str(driver), '-o', str(work / 'stage')])
        staged = Path(run('stage-rama-unix', [str(work / 'stage'), str(work / 'stage-work'), str(source)]).strip())
        selected = staged / 'src/unix/server/listener.rs'
        selected_text = selected.read_text()
        # Preserve an auditable exact staged source after the temporary tree retires.
        (output / 'staged-listener.rs').write_text(selected_text)
        fixture_source = selected_source(selected_text)
        (output / 'selected-factories.rs').write_text(fixture_source)
        app = work / 'app'
        manifest(app, f'''[package]
name="rama-unix-factory-proof"
version="0.0.0"
edition="2024"
[workspace]
[dependencies]
tokio={{path={json.dumps(str(ROOT.parent / 'TRUEOS-Buildins/vendor/tokio-1.54.1'))},features=["rt","net","fs","io-util"]}}
socket2="=0.6.3"
tracing="0.1"
[patch.crates-io]
mio={{path={json.dumps(str(ROOT / 'vendor/mio-1.2.0'))}}}
[[test]]
name="factories"
path="factories.rs"
''')
        (app / 'selected.rs').write_text(fixture_source)
        (app / 'factories.rs').write_bytes(FIXTURE.read_bytes())
        for profile, flags in [('debug', []), ('release', ['--release'])]:
            for mode in ['native', 'host']:
                extra = ['--cfg', 'target_os="trueos"', '-Aexplicit_builtin_cfgs_in_flags'] if mode == 'native' else []
                text = run(f'{mode}-{profile}-compile', cargo + ['rustc', '--offline', *flags,
                    '--manifest-path', str(app / 'Cargo.toml'), '--test', 'factories', '--message-format=json',
                    '--', *extra, '--check-cfg=cfg(target_os,values("trueos"))'], environment(output / 'target'))
                artifacts = []
                for line in text.splitlines():
                    if line.startswith('{'):
                        event = json.loads(line)
                        if event.get('reason') == 'compiler-artifact' and event.get('executable'):
                            artifacts.append(event['executable'])
                assert len(artifacts) == 1, artifacts
                result = run(f'{mode}-{profile}', [artifacts[0], '--test-threads=1'])
                count = int(re.search(r'test result: ok\. (\d+) passed', result)[1])
                assert count == (5 if mode == 'native' else 2), (mode, count)
                counts[f'{mode}-{profile}'] = count
        assert info(upstream) == original, 'registry source changed'
        receipt = {'scope': __doc__, 'checks': checks, 'cases': counts,
                   'production_stage': info(SDK / 'src/codex_native_socket_overlay.rs'),
                   'upstream': original, 'registry_source_unchanged': True,
                   'staged_source': info(output / 'staged-listener.rs'),
                   'selected_factories': info(output / 'selected-factories.rs'), 'fixture': info(FIXTURE),
                   'limits': ['Exact staged factory bodies are tested in an isolated harness; unrelated Rama service methods are excluded.',
                              'Native factory refusal is executed on host std; no native UDS transport is claimed.',
                              'Full graph native typechecking is independently performed by normal cargo bp codex --check.']}
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print(json.dumps(counts))
    print(output / 'receipt.json')


if __name__ == '__main__':
    main()
