#!/usr/bin/env python3
"""Exercise deployment ordering and PXE readiness without network or rig access."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]


class RigCommands(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        self.env = dict(os.environ, PATH=f'{self.bin}:{os.environ["PATH"]}',
                        TEST_ROOT=str(self.root))

    def script(self, path, text):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('#!/usr/bin/env bash\nset -eu\n' + text)
        path.chmod(0o755)

    def setup_workflow(self):
        script = self.root / 'tools/rig-build.sh'
        script.parent.mkdir()
        shutil.copy(ROOT / 'tools/rig-build.sh', script)
        self.script(self.root / 'tools/trueos-boot/prepare.sh',
                    'echo "build $1" >> "$TEST_ROOT/trace"\nexit "${BUILD_STATUS:-0}"\n')
        self.script(self.root / 'tools/rig-reset.sh',
                    'echo reset >> "$TEST_ROOT/trace"\n')
        for cmd in ('sudo', 'node', 'systemctl', 'systemd-run'):
            self.script(self.bin / cmd, 'echo unexpected-server-command >> "$TEST_ROOT/trace"; exit 99\n')
        return script

    def workflow(self, mode, **env):
        script = self.setup_workflow()
        result = subprocess.run(['bash', str(script), mode], env=dict(self.env, **env),
                                capture_output=True, text=True, timeout=10)
        return result, (self.root / 'trace').read_text().splitlines()

    def test_dev_and_reset_once(self):
        result, trace = self.workflow('rigdev')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, ['build --dev', 'reset'])

    def test_release_and_reset_once(self):
        result, trace = self.workflow('rigreal')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, ['build --release', 'reset'])

    def test_failed_build_does_not_touch_server_or_reset(self):
        result, trace = self.workflow('rigdev', BUILD_STATUS='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(trace, ['build --dev'])

    def test_pxe_ready_and_scoped_stop(self):
        # A private copy ensures --stop cannot match the actual LAN PXE server.
        pxe = self.root / 'pxe3.js'
        shutil.copy(ROOT / 'tools/netboot/pxe3.js', pxe)
        self.script(self.bin / 'ip', '''
 if [[ " $* " == *" addr "* ]]; then
 echo '[{"addr_info":[{"family":"inet","scope":"global","local":"127.0.0.1"}]}]'
 else echo '[{"dev":"lo"}]'; fi
''')
        self.script(self.bin / 'dnsmasq', '''
 if [[ $1 == --test ]]; then exit 0; fi
 for arg in "$@"; do
   if [[ $arg == --tftp-root=* ]]; then
     echo "${arg#*=}" > "$TEST_ROOT/tftp-root"
     echo "dnsmasq-tftp: TFTP root is ${arg#*=}" >&2
   fi
 done
 trap 'exit 0' TERM INT
 while :; do sleep 0.1; done
''')
        artifacts = self.root / 'artifacts'
        for name in ('EFI/BOOT/BOOTX64.EFI', 'TRUEOS.elf', 'EFI/BOOT/tgl_guc_70.bin'):
            f = artifacts / name
            f.parent.mkdir(parents=True, exist_ok=True)
            f.write_bytes(b'test artifact')
        ready = self.root / 'ready'
        proc = subprocess.Popen(['node', str(pxe), '--ready-file', str(ready), 'lo',
                                 str(artifacts)], env=self.env,
                                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.addCleanup(lambda: proc.poll() is None and proc.kill())
        deadline = time.monotonic() + 5
        while not ready.exists() and proc.poll() is None and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertTrue(ready.exists(), 'PXE never signalled TFTP readiness')
        served_root = Path((self.root / 'tftp-root').read_text().strip())
        self.assertEqual((served_root / 'TRUEOS.elf').read_bytes(), b'test artifact')
        (artifacts / 'TRUEOS.elf').write_bytes(b'updated kernel')
        deadline = time.monotonic() + 5
        while (served_root / 'TRUEOS.elf').read_bytes() != b'updated kernel' and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual((served_root / 'TRUEOS.elf').read_bytes(), b'updated kernel')
        self.assertIsNone(proc.poll(), 'Reload restarted the server')
        # Reap concurrently: stop waits for the old process to disappear from /proc.
        import threading
        waiter = threading.Thread(target=proc.wait)
        waiter.start()
        stopped = subprocess.run(['node', str(pxe), '--stop'], env=self.env,
                                 capture_output=True, text=True, timeout=15)
        waiter.join(timeout=5)
        self.assertEqual(stopped.returncode, 0, stopped.stderr)
        self.assertIsNotNone(proc.poll())
        self.assertIn('Stopping this repository', stopped.stdout)


if __name__ == '__main__':
    unittest.main()
