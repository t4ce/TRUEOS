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

    def setup_scope(self):
        # Mock every containment command: these tests never create a real scope
        # or change the test runner's CPU affinity and process priority.
        self.env['TRUEOS_BUILD_CPUS'] = '16'
        self.script(self.bin / 'systemd-run', '''
printf '%s\\0' "$@" > "$TEST_ROOT/scope-args"
echo scope >> "$TEST_ROOT/trace"
if [[ ${SCOPE_STATUS:-0} != 0 ]]; then exit "$SCOPE_STATUS"; fi
while [[ $# -gt 0 && $1 != -- ]]; do shift; done
test $# -gt 0
shift
exec "$@"
''')
        self.script(self.bin / 'taskset', '''
printf '%s\\0' "$@" > "$TEST_ROOT/affinity-args"
test "$1" = -c
shift 2
exec "$@"
''')
        self.script(self.bin / 'nice', '''
test "$1" = -n
test "$2" = 15
shift 2
exec "$@"
''')

    def setup_workflow(self):
        script = self.root / 'tools/rig-build.sh'
        script.parent.mkdir()
        shutil.copy(ROOT / 'tools/rig-build.sh', script)
        self.script(self.root / 'tools/trueos-boot/prepare.sh',
                    'echo "build $1" >> "$TEST_ROOT/trace"\n'
                    'printf "%s\\0" "${CARGO_BUILD_JOBS-unset}" "${RAYON_NUM_THREADS-unset}" '
                    '"${TRUEOS_BUILD_SCOPE-unset}" > "$TEST_ROOT/build-env"\n'
                    'exit "${BUILD_STATUS:-0}"\n')
        self.script(self.root / 'tools/rig-reset.sh',
                    'echo reset >> "$TEST_ROOT/trace"\n')
        for cmd in ('sudo', 'node', 'systemctl', 'systemd-run', 'taskset', 'nice'):
            self.script(self.bin / cmd, 'echo unexpected-server-command >> "$TEST_ROOT/trace"; exit 99\n')
        return script

    def workflow(self, mode, **env):
        script = self.setup_workflow()
        result = subprocess.run(['bash', str(script), mode], env=dict(self.env, **env),
                                capture_output=True, text=True, timeout=10)
        return result, (self.root / 'trace').read_text().splitlines()

    def test_dev_and_reset_once(self):
        result, trace = self.workflow('rig')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, ['build --dev', 'reset'])

    def test_installer_migrates_the_command_and_cargo_discovers_rig(self):
        self.setup_workflow()
        installer = self.root / 'tools/install-rig-commands.sh'
        shutil.copy(ROOT / 'tools/install-rig-commands.sh', installer)
        cargo_home = self.root / 'cargo-home'
        cargo_bin = cargo_home / 'bin'
        cargo_bin.mkdir(parents=True)
        legacy = cargo_bin / 'cargo-rigdev'
        legacy.symlink_to(self.root / 'tools/rig-build.sh')
        env = dict(self.env, CARGO_HOME=str(cargo_home),
                   PATH=f'{cargo_bin}:{self.env["PATH"]}')
        for _ in range(2):
            result = subprocess.run(['bash', str(installer)], env=env,
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(legacy.is_symlink())
        for name in ('rig', 'rigreal'):
            self.assertEqual((cargo_bin / f'cargo-{name}').resolve(),
                             self.root / 'tools/rig-build.sh')
        result = subprocess.run(['cargo', 'rig', '--help'], env=env, cwd=self.root,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('cargo rig: dev kernel', result.stdout)
        self.assertFalse((self.root / 'trace').exists())

        # A different checkout's command is outside this installer's migration.
        foreign = self.root / 'foreign-rig-build.sh'
        self.script(foreign, 'exit 0\n')
        legacy.symlink_to(foreign)
        result = subprocess.run(['bash', str(installer)], env=env,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(legacy.resolve(), foreign)

    def test_release_and_reset_once(self):
        result, trace = self.workflow('rigreal')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, ['build --release', 'reset'])

    def test_failed_build_does_not_touch_server_or_reset(self):
        result, trace = self.workflow('rig', BUILD_STATUS='1')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(trace, ['build --dev'])

    def test_workflow_preserves_caller_cargo_settings_without_entering_a_scope(self):
        result, trace = self.workflow('rig', CARGO_BUILD_JOBS='7', RAYON_NUM_THREADS='5',
                                      TRUEOS_BUILD_SCOPE='caller-value')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(trace, ['build --dev', 'reset'])
        self.assertEqual((self.root / 'build-env').read_bytes().split(b'\0')[:-1],
                         [b'7', b'5', b'caller-value'])

    def test_explicit_safe_wrapper_failure_does_not_run_its_payload(self):
        self.setup_scope()
        payload = self.bin / 'payload'
        self.script(payload, 'echo unexpected-payload >> "$TEST_ROOT/trace"\n')
        result = subprocess.run(['bash', str(ROOT / 'tools/build-safe.sh'), str(payload)],
                                env=dict(self.env, SCOPE_STATUS='42'),
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 42, result.stderr)
        self.assertEqual((self.root / 'trace').read_text().splitlines(), ['scope'])

    def test_safe_wrapper_limits_descendants_and_preserves_arguments(self):
        self.setup_scope()
        payload = self.bin / 'payload'
        self.script(payload, '''
printf '%s\\0' "$CARGO_BUILD_JOBS" "$RAYON_NUM_THREADS" "$TRUEOS_BUILD_SCOPE" "$@" > "$TEST_ROOT/payload-args"
''')
        arguments = ['literal $HOME', 'two words', '$(literal)']
        result = subprocess.run(['bash', str(ROOT / 'tools/build-safe.sh'),
                                 str(payload), *arguments],
                                env=dict(self.env, CARGO_BUILD_JOBS='64',
                                         RAYON_NUM_THREADS='64'),
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        scope = (self.root / 'scope-args').read_bytes().split(b'\0')[:-1]
        for option in ('--user', '--scope', '--expand-environment=no',
                       '--property=MemoryHigh=5G', '--property=MemoryMax=6G',
                       '--property=MemorySwapMax=0', '--property=CPUQuota=100%',
                       '--property=TasksMax=128'):
            self.assertIn(option.encode(), scope)
        self.assertEqual((self.root / 'affinity-args').read_bytes().split(b'\0')[:2],
                         [b'-c', b'16'])
        payload_args = (self.root / 'payload-args').read_bytes().split(b'\0')[:-1]
        self.assertEqual(payload_args, [b'1', b'1', b'1', *(arg.encode() for arg in arguments)])

    def prepare_fixture(self, profile='debug'):
        script = self.root / 'tools/trueos-boot/prepare.sh'
        script.parent.mkdir(parents=True)
        shutil.copy(ROOT / 'tools/trueos-boot/prepare.sh', script)
        buildins = self.root / 'buildins'
        (buildins / 'tools').mkdir(parents=True)
        (buildins / 'tools/build.py').touch()
        self.env['TRUEOS_BUILDINS_SOURCE'] = str(buildins)
        self.env['TEST_KERNEL_PROFILE'] = profile
        self.env['TEST_APP_PROFILE'] = 'dev' if profile == 'debug' else 'release'
        firmware = self.root / 'firmware/i915/tgl_guc_70.bin'
        firmware.parent.mkdir(parents=True)
        firmware.write_bytes(b'firmware')
        self.script(self.bin / 'python3', '''
if [[ $1 == */bundle.py ]]; then
    echo bundle >> "$TEST_ROOT/trace"
    test "$(cat "$2/TRUEOS.elf")" = runtime
    test "$(cat "$2/EFI/BOOT/BOOTX64.EFI")" = loader
    test "$(cat "$2/EFI/BOOT/tgl_guc_70.bin")" = firmware
    echo "bundle:$TEST_KERNEL_PROFILE" > "$2/TrueOS.tar.lz4"
    exit "${BUNDLE_STATUS:-0}"
fi
echo buildins >> "$TEST_ROOT/trace"
exit "${BUILDINS_STATUS:-0}"
''')
        self.script(self.bin / 'cargo', '''
test "$TRUEOS_BUILDINS_DIR" = "$TEST_ROOT/tgt/buildins/$TEST_APP_PROFILE"
echo "cargo $*" >> "$TEST_ROOT/trace"
mkdir -p "$TEST_ROOT/tgt/boot/x86_64-unknown-uefi/release" "$TEST_ROOT/tgt/x86_64-unknown-trueos/$TEST_KERNEL_PROFILE"
echo loader > "$TEST_ROOT/tgt/boot/x86_64-unknown-uefi/release/BOOTX64.efi"
echo kernel > "$TEST_ROOT/tgt/x86_64-unknown-trueos/$TEST_KERNEL_PROFILE/TRUEOS"
''')
        self.script(self.bin / 'rustc', '''
test "$*" = "--print sysroot"
echo "$TEST_ROOT/test sysroot"
''')
        self.script(self.root / 'test sysroot/lib/rustlib/x86_64-unknown-linux-gnu/bin/rust-objcopy', '''
test "$LD_LIBRARY_PATH" = "$TEST_ROOT/test sysroot/lib:inherited-libraries"
test "$1" = --strip-debug
test "$2" = "tgt/x86_64-unknown-trueos/$TEST_KERNEL_PROFILE/TRUEOS"
test "$(cat "$2")" = kernel
echo strip-debug >> "$TEST_ROOT/trace"
if [[ ${OBJCOPY_STATUS:-0} != 0 ]]; then exit "$OBJCOPY_STATUS"; fi
echo runtime > "$3"
''')
        self.env['LD_LIBRARY_PATH'] = 'inherited-libraries'
        return script

    def test_preparation_builds_apps_before_embedding_and_publishes_complete_kernel(self):
        script = self.prepare_fixture()
        result = subprocess.run(['bash', str(script), '--dev'], env=self.env,
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'trace').read_text().splitlines(),
                         ['buildins', 'cargo build', 'cargo boot', 'strip-debug', 'bundle'])
        self.assertEqual((self.root / 'tgt/pxe-current/TRUEOS.elf').read_text(), 'runtime\n')
        self.assertEqual((self.root / 'tgt/usb-debug/TRUEOS.elf').read_text(), 'runtime\n')
        self.assertEqual((self.root / 'tgt/x86_64-unknown-trueos/debug/TRUEOS').read_text(), 'kernel\n')
        self.assertEqual((self.root / 'tgt/rigdev.tar.lz4').read_text(), 'bundle:debug\n')
        self.assertEqual((self.root / 'tgt/TrueOS.tar.lz4').resolve(),
                         (self.root / 'tgt/pxe-current/TrueOS.tar.lz4').resolve())

    def test_dev_and_release_bundles_keep_separate_complete_generations(self):
        script = self.prepare_fixture()
        for flag, kernel_profile, app_profile in (('--dev', 'debug', 'dev'),
                                                  ('--release', 'release', 'release')):
            self.env.update(TEST_KERNEL_PROFILE=kernel_profile, TEST_APP_PROFILE=app_profile)
            result = subprocess.run(['bash', str(script), flag], env=self.env,
                                    capture_output=True, text=True, timeout=10)
            self.assertEqual(result.returncode, 0, result.stderr)
        dev = self.root / 'tgt/rigdev.tar.lz4'
        release = self.root / 'tgt/rigreal.tar.lz4'
        self.assertTrue(dev.is_symlink())
        self.assertTrue(release.is_symlink())
        self.assertEqual(dev.read_text(), 'bundle:debug\n')
        self.assertEqual(release.read_text(), 'bundle:release\n')
        self.assertNotEqual(dev.resolve().parent, release.resolve().parent)
        self.assertEqual((self.root / 'tgt/TrueOS.tar.lz4').resolve(), release.resolve())
        self.assertEqual((self.root / 'tgt/pxe-current/TRUEOS.elf').read_text(), 'runtime\n')
        self.assertEqual((self.root / 'trace').read_text().splitlines(),
                         ['buildins', 'cargo build', 'cargo boot', 'strip-debug', 'bundle',
                          'buildins', 'cargo build --release', 'cargo boot', 'strip-debug', 'bundle'])

    def test_failed_app_build_leaves_the_previous_publication_intact(self):
        script = self.prepare_fixture()
        current = self.root / 'tgt/pxe-current'
        current.mkdir(parents=True)
        (current / 'TRUEOS.elf').write_bytes(b'previous')
        result = subprocess.run(['bash', str(script), '--dev'],
                                env=dict(self.env, BUILDINS_STATUS='1'),
                                capture_output=True, text=True, timeout=10)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((self.root / 'trace').read_text().splitlines(), ['buildins'])
        self.assertEqual((current / 'TRUEOS.elf').read_bytes(), b'previous')

    def test_failed_debug_stripping_preserves_publication_and_prevents_reset(self):
        self.prepare_fixture()
        shutil.copy(ROOT / 'tools/rig-build.sh', self.root / 'tools/rig-build.sh')
        self.script(self.root / 'tools/rig-reset.sh', 'echo reset >> "$TEST_ROOT/trace"\n')
        current = self.root / 'tgt/pxe-current'
        current.mkdir(parents=True)
        (current / 'TRUEOS.elf').write_bytes(b'previous')
        result = subprocess.run(['bash', str(self.root / 'tools/rig-build.sh'), 'rig'],
                                env=dict(self.env, OBJCOPY_STATUS='23'),
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 23, result.stderr)
        self.assertEqual((self.root / 'trace').read_text().splitlines(),
                         ['buildins', 'cargo build', 'cargo boot', 'strip-debug'])
        self.assertEqual((current / 'TRUEOS.elf').read_bytes(), b'previous')
        self.assertFalse((self.root / 'tgt/usb-debug').exists())

    def test_failed_archive_preserves_the_previous_generation_and_prevents_reset(self):
        self.prepare_fixture()
        shutil.copy(ROOT / 'tools/rig-build.sh', self.root / 'tools/rig-build.sh')
        self.script(self.root / 'tools/rig-reset.sh', 'echo reset >> "$TEST_ROOT/trace"\n')
        previous = self.root / 'tgt/rig-images/previous'
        previous.mkdir(parents=True)
        (previous / 'TRUEOS.elf').write_bytes(b'previous kernel')
        (previous / 'TrueOS.tar.lz4').write_bytes(b'previous archive')
        current = self.root / 'tgt/pxe-current'
        current.symlink_to('rig-images/previous')
        profile = self.root / 'tgt/rigdev.tar.lz4'
        profile.symlink_to('rig-images/previous/TrueOS.tar.lz4')
        latest = self.root / 'tgt/TrueOS.tar.lz4'
        latest.symlink_to('pxe-current/TrueOS.tar.lz4')
        usb = self.root / 'tgt/usb-debug/TRUEOS.elf'
        usb.parent.mkdir()
        usb.write_bytes(b'previous USB kernel')
        result = subprocess.run(['bash', str(self.root / 'tools/rig-build.sh'), 'rig'],
                                env=dict(self.env, BUNDLE_STATUS='24'),
                                capture_output=True, text=True, timeout=10)
        self.assertEqual(result.returncode, 24, result.stderr)
        self.assertEqual((self.root / 'trace').read_text().splitlines(),
                         ['buildins', 'cargo build', 'cargo boot', 'strip-debug', 'bundle'])
        self.assertEqual(current.resolve(), previous)
        self.assertEqual((current / 'TRUEOS.elf').read_bytes(), b'previous kernel')
        self.assertEqual(profile.read_bytes(), b'previous archive')
        self.assertEqual(latest.read_bytes(), b'previous archive')
        self.assertEqual(usb.read_bytes(), b'previous USB kernel')

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
