#!/usr/bin/env python3
"""Exercise actual HTTP SDK, codec and kernel operation state without a rig."""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    kernel = (ROOT / 'src/r/net/http_request_cabi.rs').read_text().replace('#[trueos_executor::task]\n', '')
    fixture = (Path(__file__).with_name('fixtures') / 'native_http_request.rs').read_text()
    toolchain = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    for sdk in [ROOT / 'crates/trueos-v/src', ROOT.parent / 'TRUEOS-Buildins/vendor/trueos-v/src']:
        with tempfile.TemporaryDirectory(prefix='trueos-native-http-request-') as temporary:
            path = Path(temporary)
            (path / 'kernel.rs').write_text(kernel + '\n' + (Path(__file__).with_name('fixtures') / 'native_http_request_tests.rs').read_text())
            source = fixture.replace('@SDK@', str(sdk / 'http_request.rs')).replace('@KERNEL@', str(path / 'kernel.rs'))
            rust = path / 'tests.rs'; binary = path / 'tests'
            rust.write_text(source)
            subprocess.run(['rustc', '+' + toolchain, '--edition=2024', '--test', '-Dwarnings', str(rust), '-o', str(binary)], check=True, timeout=60)
            print(f'=== {sdk} ===', flush=True)
            subprocess.run([str(binary), '--test-threads=1'], check=True, timeout=60)


if __name__ == '__main__':
    main()
