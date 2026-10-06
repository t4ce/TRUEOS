#!/usr/bin/env python3
"""Check acceptance evidence and runner isolation without starting a VM."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
HELPER = ROOT / "tools/qemu/verify-tokio-platform.py"
spec = importlib.util.spec_from_file_location("tokio_platform_verify", HELPER)
verify = importlib.util.module_from_spec(spec)
spec.loader.exec_module(verify)
EVIDENCE = "\n".join((
    "tokio_mrt: std joined=2 detached=1 tls_destructors=3",
    "tokio_mrt: std scoped=2 borrowed_stack=PASS tls_destructors=2 nested_threads=2",
    "tokio_mrt: multi_thread wave=0 started=6 stopped=6 tls_destructors=6 blocking=16 socket=PASS",
    "tokio_mrt: multi_thread wave=1 started=6 stopped=6 tls_destructors=6 blocking=16 socket=PASS",
    "tokio_mrt: wave=0 counts=[512, 512] checksum=524800",
    "tokio_mrt: wave=1 counts=[512, 512] checksum=1573376",
    verify.PASS,
))


class ProbeEvidenceTests(unittest.TestCase):
    def test_complete_evidence_with_terminal_colours(self):
        self.assertEqual(verify.probe_result("\x1b[32m" + EVIDENCE + "\x1b[0m")[0], "PASS")

    def test_failure_dominates_later_pass(self):
        status, detail = verify.probe_result("tokio_mrt: FAIL stage=std.join\n" + EVIDENCE)
        self.assertEqual(status, "FAIL")
        self.assertIn("std.join", detail)

    def test_original_worker_panic_cannot_pass(self):
        self.assertEqual(verify.probe_result("OS can't spawn worker thread\n" + EVIDENCE)[0], "FAIL")

    def test_kernel_allocator_fault_cannot_pass(self):
        status, detail = verify.probe_result("=== #PF Page Fault ===\nCR2=0xffffffff00000008\n" + EVIDENCE)
        self.assertEqual(status, "FAIL")
        self.assertIn("#PF", detail)

    def test_veloren_requires_both_worker_counts(self):
        summary = "veloren_executor: PASS workers=1,2 ecs_ticks=64 borrowed=4096 scopes=64"
        self.assertEqual(verify.probe_result(summary, "veloren_executor")[0], "FAIL")
        waves = "\n".join(f"veloren_executor: wave workers={n} ticks=32 borrowed=64 scopes=32" for n in (1, 2))
        progress = (
            "veloren_executor: progress condvar_threads=8 handoffs=1024 hull_and_native=PASS",
            "veloren_executor: progress claimed_joins=256 callers=hull,native workers=4 PASS",
            "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=parallel-jobs",
            "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=async-yield",
            "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=worldgen-sites",
            "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=worldgen-map",
            "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=worldgen-lod",
        )
        evidence = waves + "\n" + summary + "\n" + "\n".join(progress)
        self.assertEqual(verify.probe_result(evidence, "veloren_executor")[0], "PASS")
        for record in progress:
            self.assertEqual(verify.probe_result(evidence.replace(record, ""), "veloren_executor")[0], "FAIL")

    def test_hypervisor_exception_fails_without_waiting_for_probe_timeout(self):
        status, detail = verify.probe_result(
            "tokio_mrt: start std-and-multi-thread\n"
            "[hv] [error] hv: vm0 fault-exc v=14 #PF Page Fault type=3(hw-exc) err=0x0 info=0x80000B0E\n"
        )
        self.assertEqual(status, "FAIL")
        self.assertIn("fault-exc v=14 #PF", detail)

    def test_summary_requires_actual_coverage(self):
        status, detail = verify.probe_result(verify.PASS)
        self.assertEqual(status, "FAIL")
        self.assertIn("scoped", detail)

    def test_nested_thread_coverage_is_required(self):
        status, detail = verify.probe_result(EVIDENCE.replace(" nested_threads=2", ""))
        self.assertEqual(status, "FAIL")
        self.assertIn("nested_threads=2", detail)

    def test_runtime_summary_requires_all_thread_cleanup_and_tcp(self):
        for field in ("started=6", "stopped=6", "tls_destructors=6", "blocking=16", "socket=PASS"):
            status, detail = verify.probe_result(EVIDENCE.replace(field, field.split("=")[0] + "=missing"))
            self.assertEqual(status, "FAIL")
            self.assertIn("multi_thread", detail)

    def test_incomplete_probe_keeps_waiting(self):
        self.assertEqual(verify.probe_result("tokio_mrt: start std-and-multi-thread"), (None, None))

    def test_stop_requires_worker_cleanup_and_actual_host_teardown(self):
        records = (
            "hv: vm0 lifecycle: stop requested cooperative=1 native_jobs=3 cleanup=guest-before-drain",
            "tokio_stop: observed host-stop",
            "tokio_stop: PASS started=3 stopped=3 tls_destructors=4 cpu=joined std=joined cleanup_blocking=42",
            "hv: vm0 lifecycle: offline native_jobs=0 carrier=released",
        )
        self.assertEqual(verify.probe_result("\n".join(records), "tokio_stop")[0], "PASS")
        for missing in range(len(records)):
            evidence = "\n".join(record for i, record in enumerate(records) if i != missing)
            self.assertEqual(verify.probe_result(evidence, "tokio_stop"), (None, None))

    def test_stop_cannot_pass_with_live_guest_jobs_or_missing_tls_destructors(self):
        evidence = "\n".join((
            "hv: vm0 lifecycle: stop requested cooperative=1",
            "tokio_stop: observed host-stop",
            "tokio_stop: PASS started=3 stopped=3 tls_destructors=4 cpu=joined std=joined cleanup_blocking=42",
            "hv: vm0 lifecycle: offline native_jobs=0 carrier=released",
        ))
        for old, new in (("native_jobs=0", "native_jobs=1"), ("tls_destructors=4", "tls_destructors=3")):
            self.assertEqual(verify.probe_result(evidence.replace(old, new), "tokio_stop"), (None, None))

    def test_stop_ready_ignores_repaint_from_previous_instance(self):
        old = "00000000-0000-4000-8000-000000000001"
        new = "00000000-0000-4000-8000-000000000002"
        ready = "tokio_stop: READY workers=2 std=1 cleanup_owner=1 instance="
        self.assertIsNone(verify.next_stop_instance(ready + old, {old}))
        self.assertEqual(verify.next_stop_instance(ready + old + "\n" + ready + new, {old}), new)

    def test_stop_relaunch_requires_fresh_done_and_serial_teardown(self):
        old = "00000000-0000-4000-8000-000000000001"
        new = "00000000-0000-4000-8000-000000000002"
        offline = "hv: vm0 lifecycle: offline native_jobs=0 carrier=released"
        evidence = "\n".join((
            "hv: vm0 lifecycle: stop requested cooperative=1",
            "tokio_stop: observed host-stop",
            "tokio_stop: PASS started=3 stopped=3 tls_destructors=4 cpu=joined std=joined cleanup_blocking=42",
            offline,
            f"tokio_stop: DONE instance={old}",
        ))
        self.assertEqual(verify.stop_wave_result(evidence, offline, new), (None, None, None))
        evidence += f"\ntokio_stop: DONE instance={new}"
        self.assertEqual(verify.stop_wave_result(evidence, "", new), (None, None, None))
        self.assertEqual(verify.stop_wave_result(evidence, offline, new)[::2], ("PASS", 0))

    def test_repaint_without_newlines_has_compact_distinct_probe_tail(self):
        frame = ("\x1b[2J" + "TRUE OS " * 2000 + "\x1b[12;1H" +
                 "tokio_mrt: start std-and-multi-thread" + "\x1b[13;1H" +
                 "tokio_mrt: std joined=2 detached=1 tls_destructors=3" + "\x1b[14;1H" +
                 "unrelated terminal contents " * 2000)
        self.assertEqual(verify.probe_tail(frame * 3), [
            "tokio_mrt: start std-and-multi-thread",
            "tokio_mrt: std joined=2 detached=1 tls_destructors=3",
        ])

    def test_failure_detail_is_bounded_without_terminal_line_breaks(self):
        status, detail = verify.probe_result("tokio_mrt: FAIL stage=scoped.borrow" + "repaint" * 2000)
        self.assertEqual(status, "FAIL")
        self.assertLessEqual(len(detail), 215)

    def test_fake_qemu_exercises_local_shell_qmp_and_cleanup(self):
        # This executable implements only QMP and terminal sockets, never a VM.
        # Running it through run.sh checks the exact final isolation arguments.
        with tempfile.TemporaryDirectory(prefix="tokio-verifier-test-") as directory:
            temp = Path(directory)
            fake = temp / "fake-qemu.py"
            fake.write_text('''#!/usr/bin/env python3
import json, pathlib, re, socket, sys, threading
args = sys.argv[1:]
def option(name): return args[args.index(name) + 1]
assert '-snapshot' in args
net = option('-netdev')
assert 'restrict=on' in net and '0.0.0.0' not in net
assert net.count('hostfwd=') == 2
serial = pathlib.Path(option('-serial').removeprefix('file:'))
pathlib.Path(option('-D')).write_text('isolated debug log\\n')
ports = dict((int(guest), int(host)) for host, guest in re.findall(r'hostfwd=tcp:127.0.0.1:(\\d+)-:(\\d+)', net))
def terminal(guest):
    listener = socket.socket()
    listener.bind(('127.0.0.1', ports[guest]))
    listener.listen()
    with listener:
        while True:
            connection, _ = listener.accept()
            with connection:
                connection.sendall(b'TRUE OS\\r\\n' if guest == 4245 else "TrueOS § 12:34\\r\\n§sh1 ".encode())
                pending = bytearray()
                while True:
                    data = connection.recv(4096)
                    if not data: break
                    pending.extend(data)
                    if b'help\\r' in pending:
                        connection.sendall(b'Shell3 recognizes names; command execution is not wired yet.\\r\\n')
                        pending.clear()
                    if b'tokio_mrt\\r' in pending:
                        serial.write_text(EVIDENCE + '\\n')
                        pending.clear()
for guest in ports:
    threading.Thread(target=terminal, args=(guest,), daemon=True).start()
qmp = socket.socket(socket.AF_UNIX)
qmp.bind(option('-qmp').split(',')[0].removeprefix('unix:'))
qmp.listen()
connection, _ = qmp.accept()
with connection:
    stream = connection.makefile('rwb', buffering=0)
    stream.write(b'{"QMP":{"version":{"qemu":{"major":10,"minor":0,"micro":0}},"capabilities":[]}}\\n')
    for line in stream:
        request = json.loads(line)
        result = {'return': {}, 'id': request['id']}
        if request['execute'] == 'query-status': result['return'] = {'running':True,'status':'running'}
        stream.write((json.dumps(result) + '\\n').encode())
        if request['execute'] == 'quit': break
'''.replace("import json, pathlib, re, socket, sys, threading", "import json, pathlib, re, socket, sys, threading\nEVIDENCE = " + repr(EVIDENCE)))
            fake.chmod(0o755)
            iso = temp / "test.iso"
            firmware = temp / "test.fd"
            iso.touch()
            firmware.touch()
            output = temp / "evidence"
            completed = subprocess.run([
                "python3", str(HELPER), "--iso", str(iso), "--output", str(output), "--timeout", "5",
            ], env=dict(os.environ, QEMU_BIN=str(fake), QEMU_UEFI_FIRMWARE=str(firmware)),
                capture_output=True, text=True, timeout=12)
            self.assertEqual(completed.returncode, 0, completed.stdout + completed.stderr)
            result = json.loads((output / "result.json").read_text())
            self.assertEqual(result["status"], "PASS")
            self.assertTrue(result["shell3"]["observed"])
            self.assertTrue(result["shell3"]["execution_unwired"])
            self.assertIn("qmp_final_status", result)
            self.assertIn('"execute": "quit"', (output / "qmp.jsonl").read_text())
            self.assertEqual((output / "qemu-debug.log").read_text(), "isolated debug log\n")
            with self.assertRaises(ProcessLookupError):
                os.kill(result["pid"], 0)
            # Output reuse must fail before starting the runner.
            again = subprocess.run([
                "python3", str(HELPER), "--iso", str(iso), "--output", str(output),
            ], env=dict(os.environ, QEMU_BIN=str(fake), QEMU_UEFI_FIRMWARE=str(firmware)),
                capture_output=True, text=True, timeout=3)
            self.assertNotEqual(again.returncode, 0)
            self.assertIn("fresh directory", again.stderr)


if __name__ == "__main__":
    unittest.main()
