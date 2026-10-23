import hashlib
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HELPER = Path(__file__).with_name("download_locked_package.sh").resolve()


class DownloadTests(unittest.TestCase):
    def test_resume_fallback_checksum_and_cache(self):
        for mode in ("resume", "fallback", "corrupt", "failure"):
            with self.subTest(mode=mode), tempfile.TemporaryDirectory() as temporary:
                root = Path(temporary)
                curl = root / "curl"
                curl.write_text('''#!/usr/bin/env python3
import os, sys
from pathlib import Path
args = sys.argv[1:]
assert args[args.index('--continue-at') + 1] == '-'
count = Path('count')
n = int(count.read_text()) + 1 if count.exists() else 1
count.write_text(str(n))
out = Path(args[args.index('--output') + 1])
mode = os.environ['TEST_DOWNLOAD_MODE']
if mode == 'failure':
    out.write_bytes(b'wrong')
    sys.exit(0)
if mode == 'fallback' and n <= 2:
    sys.exit(22)
if mode == 'fallback':
    assert 'azure.archive.ubuntu.com' in args[-1]
if mode == 'resume' and n == 1:
    out.write_bytes(b'pay')
    sys.exit(28)
if mode == 'resume':
    assert out.read_bytes() == b'pay'
if mode == 'corrupt' and n == 1:
    out.write_bytes(b'wrong')
    sys.exit(0)
if mode == 'corrupt':
    assert not out.exists()
out.write_bytes(b'payload')
''')
                curl.chmod(0o755)
                env = dict(os.environ, PATH=f"{root}:{os.environ['PATH']}", TEST_DOWNLOAD_MODE=mode)
                digest = hashlib.sha256(b"payload").hexdigest()
                command = ['bash', '-c', 'set -euo pipefail; source "$1"; download_locked_package sha256 "$2" https://de.archive.ubuntu.com/ubuntu/pool/test.deb package.deb', 'test', str(HELPER), digest]
                result = subprocess.run(command, cwd=root, env=env, capture_output=True)
                self.assertEqual(result.returncode, 1 if mode == 'failure' else 0, result.stderr)
                archive = root / 'package.deb'
                if mode == 'failure':
                    self.assertFalse(archive.exists())
                    continue
                self.assertEqual(archive.read_bytes(), b'payload')
                calls = (root / 'count').read_text()
                subprocess.run(command, cwd=root, env=env, check=True, capture_output=True)
                self.assertEqual((root / 'count').read_text(), calls)


if __name__ == '__main__':
    unittest.main()
