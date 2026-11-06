#!/usr/bin/env python3
"""Exercise a built rpxe2 using loopback HTTP and mocked ip/dnsmasq.

Usage: python3 tests/smoke.py /absolute/or/relative/path/to/rpxe2
No DHCP or TFTP sockets are opened. Each test copies the executable into its
own temporary directory so --stop cannot target a development server.
"""

import http.client
import json
import os
from pathlib import Path
import shutil
import signal
import socket
import subprocess
import sys
import tempfile
import threading
import time
import unittest


FILES = ("EFI/BOOT/BOOTX64.EFI", "TRUEOS.elf", "EFI/BOOT/tgl_guc_70.bin")
ADDRESS = "127.0.0.23"
BINARY = None

IP_MOCK = r'''#!/usr/bin/env python3
import json, os, sys
with open(os.path.join(os.environ["RPXE_TEST_DIR"], "ip-calls.jsonl"), "a") as f:
    f.write(json.dumps(sys.argv[1:]) + "\n")
if "addr" in sys.argv:
    print(json.dumps([{"addr_info": [{"family": "inet", "scope": "global",
        "local": os.environ.get("RPXE_TEST_ADDRESS", "127.0.0.23")}]}]))
else:
    print('[{"dev":"lo"}]')
'''

DNSMASQ_MOCK = r'''#!/usr/bin/env python3
import json, os, signal, sys, threading, time
from pathlib import Path
root = Path(os.environ["RPXE_TEST_DIR"])
if "--test" in sys.argv:
    (root / "dnsmasq-test-args.json").write_text(json.dumps(sys.argv[1:]))
    if os.environ.get("RPXE_TEST_BAD_CONFIG"):
        print("mock dnsmasq configuration failure", file=sys.stderr)
        sys.exit(7)
    sys.exit(0)
(root / "dnsmasq-args.json").write_text(json.dumps(sys.argv[1:]))
(root / "dnsmasq-pid").write_text(str(os.getpid()))
done = threading.Event()
signal.signal(signal.SIGTERM, lambda *_: done.set())
signal.signal(signal.SIGINT, lambda *_: done.set())
if os.environ.get("RPXE_TEST_EARLY_EXIT"):
    print("mock dnsmasq exited before readiness", file=sys.stderr)
    sys.exit(int(os.environ["RPXE_TEST_EARLY_EXIT"]))
tftp = next(arg.split("=", 1)[1] for arg in sys.argv if arg.startswith("--tftp-root="))
(root / "tftp-root").write_text(tftp)
# A genuine child pipe can split this announcement at any byte boundary.
print("dnsmasq-tftp: TFTP root ", end="", file=sys.stderr, flush=True)
if done.wait(float(os.environ.get("RPXE_TEST_READY_DELAY", "0.15"))):
    sys.exit(0)
(root / "dnsmasq-ready").write_text("ready\n")
print("is " + tftp, file=sys.stderr, flush=True)
done.wait()
'''


def wait_until(predicate, description, timeout=6):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            if predicate():
                return
        except (FileNotFoundError, ProcessLookupError, ConnectionError, OSError):
            pass
        time.sleep(0.02)
    raise AssertionError(f"Timed out waiting for {description}")


def pid_exists(pid):
    try:
        os.kill(pid, 0)
        return True
    except ProcessLookupError:
        return False


def write_generation(directory, marker, archive=True):
    directory.mkdir(parents=True, exist_ok=True)
    for name in FILES:
        path = directory / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(f"{marker}:{name}".encode())
    (directory / "private.txt").write_text("must not be served")
    if archive:
        (directory / "TrueOS.tar.lz4").write_bytes(f"archive:{marker}".encode())


class Smoke(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="rpxe2-smoke-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.exe = self.root / "rpxe2"
        shutil.copy2(BINARY, self.exe)
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for name, body in (("ip", IP_MOCK), ("dnsmasq", DNSMASQ_MOCK)):
            path = self.bin / name
            path.write_text(body)
            path.chmod(0o755)
        self.env = dict(os.environ, PATH=f"{self.bin}:{os.environ['PATH']}",
                        RPXE_TEST_DIR=str(self.root), RPXE_TEST_ADDRESS=ADDRESS)
        self.first = self.root / "publication-a"
        write_generation(self.first, "a")
        self.source = self.root / "current"
        self.source.symlink_to(self.first.name)
        self.ready = self.root / "ready"
        self.processes = []
        self.addCleanup(self.cleanup_processes)

    def cleanup_processes(self):
        for proc, _ in self.processes:
            if proc.poll() is None:
                proc.terminate()
        for proc, log in self.processes:
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3)
            log.close()
        # A failing cleanup test must still not leave fake daemons behind.
        for pidfile in self.root.rglob("dnsmasq-pid"):
            try:
                pid = int(pidfile.read_text())
                argv = Path(f"/proc/{pid}/cmdline").read_bytes().split(b"\0")
                if os.fsencode(self.bin / "dnsmasq") in argv:
                    os.kill(pid, signal.SIGTERM)
            except (FileNotFoundError, ProcessLookupError, ValueError):
                pass

    def start(self, env=None, exe=None, ready=None, source=None, default_interface=False):
        env = self.env if env is None else env
        exe = self.exe if exe is None else exe
        ready = self.ready if ready is None else ready
        source = self.source if source is None else source
        log_path = Path(env["RPXE_TEST_DIR"]) / "server.log"
        log = log_path.open("wb")
        args = [str(exe), "--ready-file", str(ready)]
        if not default_interface:
            args.extend(["lo", str(source)])
        proc = subprocess.Popen(args, env=env, stdout=log, stderr=subprocess.STDOUT)
        self.processes.append((proc, log))
        return proc

    def diagnostics(self, directory=None):
        path = (self.root if directory is None else directory) / "server.log"
        return path.read_text(errors="replace") if path.exists() else ""

    def await_ready(self, proc, ready=None, directory=None):
        ready = self.ready if ready is None else ready
        directory = self.root if directory is None else directory
        try:
            wait_until(lambda: ready.exists() or proc.poll() is not None,
                       "server readiness")
            self.assertIsNone(proc.poll(), self.diagnostics(directory))
            self.assertTrue(ready.exists(), self.diagnostics(directory))
            self.assertEqual(ready.read_text().strip(), str(proc.pid))
            self.assertEqual(ready.stat().st_mode & 0o777, 0o644)
            self.assertTrue((directory / "dnsmasq-ready").exists(),
                            "readiness was published before dnsmasq was ready")
        except AssertionError as error:
            raise AssertionError(f"{error}\n{self.diagnostics(directory)}") from error

    def request(self, path="/TrueOS.tar.lz4", method="GET", address=ADDRESS):
        connection = http.client.HTTPConnection(address, 8080, timeout=3)
        try:
            connection.request(method, path)
            response = connection.getresponse()
            return response.status, dict(response.getheaders()), response.read()
        finally:
            connection.close()

    def assert_child_gone(self, directory=None):
        directory = self.root if directory is None else directory
        pidfile = directory / "dnsmasq-pid"
        if pidfile.exists():
            pid = int(pidfile.read_text())
            wait_until(lambda: not pid_exists(pid), f"fake dnsmasq {pid} to exit")

    def assert_failure(self, proc):
        try:
            code = proc.wait(timeout=6)
        except subprocess.TimeoutExpired as error:
            self.fail(f"server did not exit after startup failure\n{self.diagnostics()}")
        self.assertNotEqual(code, 0, self.diagnostics())
        self.assertFalse(self.ready.exists(), "failed startup published readiness")
        self.assert_child_gone()
        rootfile = self.root / "tftp-root"
        if rootfile.exists():
            self.assertFalse(Path(rootfile.read_text()).exists(), "snapshot leaked")

    def test_reload_http_and_sigterm_cleanup(self):
        proc = self.start()
        self.await_ready(proc)
        # The ready file promises that HTTP is already accepting connections.
        status, headers, body = self.request()
        self.assertEqual((status, body), (200, b"archive:a"))
        headers = {name.lower(): value for name, value in headers.items()}
        self.assertEqual(headers.get("content-type"), "application/octet-stream")
        self.assertEqual(headers.get("content-length"), str(len(body)))

        served = Path((self.root / "tftp-root").read_text())
        dnsmasq_args = json.loads((self.root / "dnsmasq-args.json").read_text())
        for argument in ("--conf-file=/dev/null", "--no-daemon", "--port=0",
                         "--interface=lo", "--bind-interfaces",
                         f"--listen-address={ADDRESS}", "--no-dhcpv6-interface=lo",
                         f"--dhcp-range={ADDRESS},proxy", "--dhcp-no-override",
                         "--dhcp-match=set:efi64,option:client-arch,7",
                         "--dhcp-match=set:efi64,option:client-arch,9",
                         "--dhcp-ignore=tag:!efi64",
                         f"--dhcp-boot=tag:efi64,{FILES[0]},,{ADDRESS}",
                         f"--pxe-service=tag:efi64,7,TRUEOS native UEFI,{FILES[0]},{ADDRESS}",
                         f"--pxe-service=tag:efi64,9,TRUEOS native UEFI,{FILES[0]},{ADDRESS}",
                         "--enable-tftp=lo", f"--tftp-root={served}", "--log-dhcp"):
            self.assertIn(argument, dnsmasq_args)
        previous = (served / "active").resolve()
        self.assertTrue(previous.is_dir())
        for name in FILES:
            self.assertEqual((served / name).read_bytes(), f"a:{name}".encode())
            self.assertEqual((served / name).stat().st_mode & 0o444, 0o444)
        self.assertFalse((served / "private.txt").exists())
        self.assertFalse((served / "TrueOS.tar.lz4").exists())

        for path, method in (("/TRUEOS.elf", "GET"), ("/private.txt", "GET"),
                             ("/", "GET"), ("/TrueOS.tar.lz4?x=1", "GET"),
                             ("/TrueOS.tar.lz4/", "GET"), ("/TrueOS.tar.lz4", "HEAD"),
                             ("/TrueOS.tar.lz4", "POST"), ("/TrueOS.tar.lz4", "OPTIONS")):
            with self.subTest(path=path, method=method):
                self.assertEqual(self.request(path, method)[0], 404)

        # A missing publication must leave the last complete TFTP generation live.
        pending = self.root / "pending-link"
        pending.symlink_to("not-published-yet")
        pending.replace(self.source)
        time.sleep(0.35)
        self.assertEqual((served / "active").resolve(), previous)
        self.assertEqual(self.request()[0], 503)

        incomplete = self.root / "incomplete-publication"
        incomplete.mkdir()
        (incomplete / "TRUEOS.elf").write_bytes(b"incomplete")
        pending.symlink_to(incomplete.name)
        pending.replace(self.source)
        time.sleep(0.35)
        self.assertEqual((served / "active").resolve(), previous)
        self.assertEqual((served / "TRUEOS.elf").read_bytes(), b"a:TRUEOS.elf")

        second = self.root / "publication-b"
        write_generation(second, "b")
        pending.symlink_to(second.name)
        pending.replace(self.source)
        wait_until(lambda: (served / "TRUEOS.elf").read_bytes() == b"b:TRUEOS.elf",
                   "complete second publication")
        selected = (served / "active").resolve()
        self.assertNotEqual(selected, previous)
        for name in FILES:
            self.assertEqual((selected / name).read_bytes(), f"b:{name}".encode())
            self.assertEqual((previous / name).read_bytes(), f"a:{name}".encode())
        self.assertFalse((served / "private.txt").exists())
        self.assertEqual(self.request()[::2], (200, b"archive:b"))
        (second / "TrueOS.tar.lz4").unlink()
        self.assertEqual(self.request()[0], 503)
        self.assertIsNone(proc.poll(), self.diagnostics())

        proc.send_signal(signal.SIGTERM)
        self.assertEqual(proc.wait(timeout=6), 0, self.diagnostics())
        self.assert_child_gone()
        self.assertFalse(served.exists(), "snapshot survived SIGTERM")
        self.assertFalse(self.ready.exists(), "ready file survived SIGTERM")

    def test_stop_is_scoped_to_executable_and_cleans_up(self):
        proc = self.start()
        self.await_ready(proc)
        served = Path((self.root / "tftp-root").read_text())

        other = self.root / "other-checkout"
        other.mkdir()
        other_exe = other / "rpxe2"
        shutil.copy2(BINARY, other_exe)
        other_env = dict(self.env, RPXE_TEST_DIR=str(other), RPXE_TEST_ADDRESS="127.0.0.24")
        other_ready = other / "ready"
        other_proc = self.start(env=other_env, exe=other_exe, ready=other_ready)
        self.await_ready(other_proc, ready=other_ready, directory=other)
        self.assertEqual(self.request(address="127.0.0.24")[0], 200)

        # Cargo replaces binaries atomically; the running process's old inode
        # then appears as '<path> (deleted)' in /proc/PID/exe.
        replacement = self.root / "replacement-rpxe2"
        shutil.copy2(BINARY, replacement)
        replacement.replace(self.exe)

        # --stop waits for disappearance; reap the original concurrently.
        waiter = threading.Thread(target=proc.wait)
        waiter.start()
        stopped = subprocess.run([str(self.exe), "--stop"], env=self.env,
                                 capture_output=True, text=True, timeout=15)
        waiter.join(timeout=3)
        self.assertEqual(stopped.returncode, 0, stopped.stdout + stopped.stderr)
        self.assertEqual(proc.poll(), 0, self.diagnostics())
        self.assert_child_gone()
        self.assertFalse(served.exists(), "snapshot survived --stop")
        self.assertFalse(self.ready.exists(), "ready file survived --stop")
        self.assertIsNone(other_proc.poll(), "--stop killed a different executable copy")
        self.assertEqual(self.request(address="127.0.0.24")[0], 200)
        other_proc.send_signal(signal.SIGINT)
        self.assertEqual(other_proc.wait(timeout=6), 0, self.diagnostics(other))
        self.assert_child_gone(other)
        self.assertFalse(Path((other / "tftp-root").read_text()).exists())
        self.assertFalse(other_ready.exists(), "ready file survived SIGINT")

    def test_dnsmasq_configuration_failure(self):
        proc = self.start(env=dict(self.env, RPXE_TEST_BAD_CONFIG="1"))
        self.assert_failure(proc)
        self.assertTrue((self.root / "dnsmasq-test-args.json").exists())
        self.assertFalse((self.root / "dnsmasq-pid").exists())

    def test_dnsmasq_exit_before_ready(self):
        for code in (0, 9):
            with self.subTest(child_exit=code):
                proc = self.start(env=dict(self.env, RPXE_TEST_EARLY_EXIT=str(code)))
                self.assert_failure(proc)

    def test_readiness_write_failure_reaps_child_and_removes_snapshot(self):
        self.ready = self.root / "missing-parent" / "ready"
        proc = self.start()
        self.assert_failure(proc)
        self.assertTrue((self.root / "dnsmasq-ready").exists(),
                        "did not reach readiness publication")

    def test_http_bind_failure_does_not_publish_ready_or_leak_child(self):
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as occupied:
            occupied.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
            occupied.bind((ADDRESS, 8080))
            occupied.listen()
            proc = self.start()
            self.assert_failure(proc)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        raise SystemExit(__doc__)
    BINARY = Path(sys.argv.pop(1)).resolve()
    if not BINARY.is_file() or not os.access(BINARY, os.X_OK):
        raise SystemExit(f"Not an executable binary: {BINARY}")
    unittest.main(verbosity=2)
