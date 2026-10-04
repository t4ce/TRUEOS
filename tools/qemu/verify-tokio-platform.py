#!/usr/bin/env python3
"""Run the embedded Tokio platform probe in an isolated local QEMU instance.

Build the ISO first, then pass --iso and a new --output directory. This does
not fetch apps or contact a physical rig. The existing runner uses a temporary
disk snapshot; every forwarded port is private to this run and loopback only.
"""
import argparse
import json
import os
from pathlib import Path
import re
import signal
import socket
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
ANSI = re.compile(r"\x1b\][^\x07]*(?:\x07|\x1b\\)|\x1b\[[0-?]*[ -/]*[@-~]")
PASS = "tokio_mrt: PASS std_threads=2 scoped_threads=2 tokio_workers=2 rebuilds=2 native_lanes=2 waves=2 tasks=16 rounds=32"


def plain(text):
    return ANSI.sub("", text)


def probe_tail(text, limit=4, probe="tokio_mrt"):
    """Extract bounded records before ANSI repaint removes line boundaries."""
    records = []
    for match in re.finditer(rf"{re.escape(probe)}:[^\r\n\x1b]{{0,200}}", text):
        record = plain(match.group(0)).strip()
        # Repaints repeatedly include the same retained transcript. Preserve
        # its most recent distinct records rather than whole screen redraws.
        if record in records:
            records.remove(record)
        records.append(record)
        records = records[-limit:]
    return records


def probe_result(text, probe="tokio_mrt"):
    """A PASS cannot hide an earlier probe failure or the reported worker panic."""
    text = plain(text)
    failure = re.search(rf"{re.escape(probe)}: FAIL[^\r\n]{{0,200}}", text)
    if failure:
        return "FAIL", failure.group(0)
    fault = re.search(r"hv: vm\d+ fault-exc[^\r\n]{0,180}", text)
    if fault:
        return "FAIL", fault.group(0)
    for marker in ("OS can't spawn worker thread", "thread '<unnamed>'", "panicked at",
                   "=== #PF Page Fault ===", "=== #GP General Protection Fault ==="):
        if marker in text:
            return "FAIL", marker
    if probe == "veloren_executor":
        marker = "veloren_executor: PASS workers=1,2 ecs_ticks=64 borrowed=4096 scopes=64"
        if marker in text:
            for workers in (1, 2):
                if f"veloren_executor: wave workers={workers} ticks=32 borrowed=64 scopes=32" not in text:
                    return "FAIL", f"PASS lacks workers={workers} ECS evidence"
            for record in (
                "veloren_executor: progress condvar_threads=8 handoffs=1024 hull_and_native=PASS",
                "veloren_executor: progress claimed_joins=256 callers=hull,native workers=4 PASS",
                "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=parallel-jobs",
                "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=async-yield",
                "veloren_executor: progress cpu_jobs=2 heartbeat_peers=2 bounded_turns=PASS mode=worldgen-sites",
            ):
                if record not in text:
                    return "FAIL", "PASS lacks execution progress evidence: " + record
            return "PASS", marker
        return None, None
    if probe == "tokio_stop":
        marker = "tokio_stop: PASS started=3 stopped=3 tls_destructors=4 cpu=joined std=joined cleanup_blocking=42"
        if marker in text:
            required = ("tokio_stop: observed host-stop",
                        "lifecycle: stop requested cooperative=1",
                        "lifecycle: offline native_jobs=0 carrier=released")
            if all(record in text for record in required):
                return "PASS", marker
        return None, None
    if PASS in text:
        required = (
            "tokio_mrt: std joined=2 detached=1 tls_destructors=3",
            "tokio_mrt: std scoped=2 borrowed_stack=PASS tls_destructors=2 nested_threads=2",
            "tokio_mrt: multi_thread wave=0 started=6 stopped=6 tls_destructors=6 blocking=16 socket=PASS",
            "tokio_mrt: multi_thread wave=1 started=6 stopped=6 tls_destructors=6 blocking=16 socket=PASS",
            "tokio_mrt: wave=0 counts=[512, 512]", "tokio_mrt: wave=1 counts=[512, 512]",
        )
        missing = [marker for marker in required if marker not in text]
        if missing:
            return "FAIL", "PASS lacks required coverage evidence: " + ", ".join(missing)
        return "PASS", PASS
    return None, None


def private_ports():
    sockets = [socket.socket(socket.AF_INET, socket.SOCK_STREAM) for _ in range(2)]
    try:
        for sock in sockets:
            sock.bind(("127.0.0.1", 0))
        return [sock.getsockname()[1] for sock in sockets]
    finally:
        for sock in sockets:
            sock.close()


def next_stop_instance(text, seen):
    """A retained Matrix repaint must never stop a not-yet-ready relaunch."""
    for ready in re.finditer(r"tokio_stop: READY workers=2 std=1 cleanup_owner=1 instance=([0-9a-f-]{36})", plain(text)):
        if ready.group(1) not in seen:
            return ready.group(1)
    return None


def stop_wave_result(text, serial_text, instance):
    """Require this guest's completion and this launch's host drain evidence."""
    status, detail = probe_result(text, "tokio_stop")
    if status == "FAIL":
        return status, detail, None
    vm = re.search(r"hv: vm(\d+) lifecycle: offline native_jobs=0 carrier=released", plain(serial_text))
    if status == "PASS" and instance and vm and f"tokio_stop: DONE instance={instance}" in plain(text):
        return status, detail, int(vm.group(1))
    return None, None, None


class Qmp:
    def __init__(self, path, transcript):
        self.sock = socket.socket(socket.AF_UNIX)
        self.sock.settimeout(2)
        self.sock.connect(path)
        self.stream = self.sock.makefile("rwb", buffering=0)
        self.transcript = transcript
        self.sequence = 0
        self.receive()
        self.command("qmp_capabilities")

    def receive(self):
        line = self.stream.readline()
        if not line:
            raise EOFError("QMP closed")
        result = json.loads(line)
        self.transcript.write(json.dumps({"received": result}) + "\n")
        self.transcript.flush()
        return result

    def command(self, name):
        self.sequence += 1
        request = {"execute": name, "id": self.sequence}
        self.transcript.write(json.dumps({"sent": request}) + "\n")
        self.transcript.flush()
        self.stream.write((json.dumps(request) + "\n").encode())
        while True:
            result = self.receive()
            if result.get("id") == self.sequence:
                if "error" in result:
                    raise RuntimeError(f"QMP {name}: {result['error']}")
                return result["return"]

    def close(self):
        self.stream.close()
        self.sock.close()


def stop_owned_process(process):
    """The runner execs QEMU; kill only this newly created process group."""
    if process.poll() is None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
    try:
        process.wait(timeout=5)
    except subprocess.TimeoutExpired:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--iso", type=Path, required=True, help="Already built ISO containing tokio_mrt")
    parser.add_argument("--output", type=Path, required=True, help="Fresh evidence directory (must not exist)")
    parser.add_argument("--timeout", type=float, default=90, help="Total boot and probe deadline, at most 90 seconds")
    parser.add_argument("--gdb-port", type=int, help="Expose this private QEMU instance to loopback GDB for diagnostics")
    parser.add_argument("--probe", choices=("tokio_mrt", "veloren_executor", "tokio_stop"), default="tokio_mrt", help="Embedded probe to run (tokio_stop checks stop and VM-slot reuse)")
    parser.add_argument("--veloren-executor", action="store_true", help="Also run the vendored Veloren Tokio executor probe")
    args = parser.parse_args()
    if not 0 < args.timeout <= 90:
        parser.error("--timeout must be greater than zero and at most 90 seconds")
    if args.gdb_port is not None and not 0 < args.gdb_port <= 65535:
        parser.error("--gdb-port must be between 1 and 65535")
    args.iso = args.iso.resolve()
    if not args.iso.is_file():
        parser.error(f"ISO does not exist: {args.iso}")
    firmware = os.environ.get("QEMU_UEFI_FIRMWARE") or next((str(path) for path in (
        Path("/usr/share/ovmf/OVMF.fd"), ROOT / "bld/trueos-release/ovmf-code-x86_64.fd"
    ) if path.is_file()), None)
    if not firmware or not Path(firmware).is_file():
        parser.error("Set QEMU_UEFI_FIRMWARE to an existing combined OVMF image")
    args.output = args.output.resolve()
    try:
        args.output.mkdir(parents=True, exist_ok=False)
    except FileExistsError:
        parser.error(f"--output must be a fresh directory: {args.output}")

    serial_path = args.output / "serial.log"
    shell_path = args.output / "shell.log"
    serial_path.touch()
    shell_log = bytearray()
    shell3_log = bytearray()
    ports = private_ports()
    env = dict(os.environ, ISO_PATH=str(args.iso), QEMU_SERIAL=f"file:{serial_path}",
               QEMU_UEFI_FIRMWARE=firmware, QEMU_DEBUG_LOG=str(args.output / "qemu-debug.log"),
               QEMU_HOST_TCP_PORT_NET_SHELL=str(ports[0]),
               QEMU_NETDEV_USER=("user,id=net1,restrict=on,"
                                f"hostfwd=tcp:127.0.0.1:{ports[0]}-:4245,"
                                f"hostfwd=tcp:127.0.0.1:{ports[1]}-:22"))
    env.setdefault("QEMU_DISPLAY", "egl-headless")
    result = {"status": "FAIL", "iso": str(args.iso), "timeout_seconds": args.timeout,
              "qemu_cpu_slots": int(env.get("QEMU_SMP", "14")),
              "shell2": {"guest_port": 4245, "host_port": ports[0], "command": args.probe},
              "shell3": {"guest_port": 22, "host_port": ports[1], "observed": False}}
    started = time.monotonic()
    deadline = started + args.timeout
    process = None
    shell = None
    active_probe = args.probe
    qmp = None
    with tempfile.TemporaryDirectory(prefix="trueos-tokio-qmp-") as socket_dir, \
            (args.output / "qemu.log").open("w") as host_log, \
            (args.output / "qmp.jsonl").open("w") as qmp_log:
        qmp_path = str(Path(socket_dir) / "qmp.sock")
        command = [str(ROOT / "tools/qemu/run.sh"), "iso", "-snapshot", "-qmp",
                   f"unix:{qmp_path},server=on,wait=off"]
        if args.gdb_port:
            command.extend(["-gdb", f"tcp:127.0.0.1:{args.gdb_port}"])
        (args.output / "run.json").write_text(json.dumps({
            "command": command, "iso": str(args.iso), "firmware": firmware,
            "iso_size": args.iso.stat().st_size, "iso_mtime_ns": args.iso.stat().st_mtime_ns,
            "netdev": env["QEMU_NETDEV_USER"], "display": env["QEMU_DISPLAY"],
            "gpu": env.get("QEMU_GPU", "virtio-gpu-gl-pci,xres=1920,yres=1080"),
            "snapshot": True, "shell_route": "legacy Shell2 TCP 4245; Shell3 TCP 22 does not execute apps",
        }, indent=2) + "\n")

        def check_running():
            if process.poll() is not None:
                raise RuntimeError(f"QEMU exited {process.returncode}; see qemu.log")
            observed = (serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
                        + (args.output / "qemu.log").read_text(errors="replace"))
            status, detail = probe_result(observed, active_probe)
            if status == "FAIL":
                raise RuntimeError(detail)
            if time.monotonic() >= deadline:
                raise TimeoutError("Boot/probe deadline expired; latest probe evidence: " +
                                   (" | ".join(probe_tail(observed, probe=active_probe))
                                    or f"no {active_probe} output"))

        def drain(sock, log, seconds):
            end = min(deadline, time.monotonic() + seconds)
            while time.monotonic() < end:
                check_running()
                try:
                    data = sock.recv(65536)
                    if not data:
                        raise EOFError("Guest terminal closed")
                    log.extend(data)
                    if log is shell_log:
                        shell_path.write_bytes(log)
                    if b"\x1b[18t" in log[-len(data)-8:]:
                        sock.sendall(b"\x1b[8;40;140t")
                except socket.timeout:
                    pass

        try:
            process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=host_log,
                                       stderr=subprocess.STDOUT, start_new_session=True)
            result["pid"] = process.pid
            while not Path(qmp_path).exists():
                check_running()
                time.sleep(.05)
            qmp = Qmp(qmp_path, qmp_log)
            result["qmp_boot_status"] = qmp.command("query-status")
            while shell is None:
                check_running()
                # QEMU accepts a forward before the guest listener is ready;
                # require a real legacy Shell2 repaint before submitting input.
                candidate = None
                try:
                    candidate = socket.create_connection(("127.0.0.1", ports[0]), .3)
                    candidate.settimeout(.1)
                    before = len(shell_log)
                    drain(candidate, shell_log, .5)
                    text = plain(shell_log[before:].decode(errors="replace"))
                    if "Shell3" in text:
                        raise RuntimeError("Guest TCP 4245 returned Shell3; it cannot execute Blueprint names")
                    if "TRUE OS" in text:
                        shell = candidate
                    else:
                        candidate.close()
                        time.sleep(.1)
                except (ConnectionError, socket.timeout, EOFError):
                    if candidate:
                        candidate.close()
                    time.sleep(.1)
            result["shell2"]["banner"] = "TRUE OS"
            result["shell2"]["connected_seconds"] = round(time.monotonic() - started, 3)
            # Record the separate Shell3 route when ready, without depending on it
            # for execution. Its checked-in banner explicitly says plaintext.
            try:
                with socket.create_connection(("127.0.0.1", ports[1]), .3) as shell3:
                    shell3.settimeout(.1)
                    drain(shell3, shell3_log, .4)
                    if b"Shell3 plaintext terminal" in shell3_log:
                        result["shell3"]["observed"] = True
                        shell3.sendall(b"help\r")
                        drain(shell3, shell3_log, .3)
                        result["shell3"]["execution_unwired"] = b"command execution is not wired yet" in shell3_log
            except (ConnectionError, socket.timeout, EOFError):
                pass
            # Matrix navigation returns Shell2 to Default; the next bare name
            # launches the embedded AppDB entry without an online lookup.
            shell.sendall("§\r".encode())
            drain(shell, shell_log, .3)
            serial_begin = serial_path.stat().st_size
            shell_begin = len(shell_log)
            shell.sendall((args.probe + "\r").encode())
            result["shell2"]["mode"] = "Default (selected with §)"
            if args.probe == "tokio_stop":
                result["stop_waves"] = []
                instances = set()
                for wave in range(2):
                    if wave:
                        shell.sendall("§\r".encode())
                        drain(shell, shell_log, .3)
                        serial_begin = serial_path.stat().st_size
                        shell_begin = len(shell_log)
                        shell.sendall(b"tokio_stop\r")
                    sent_stop = False
                    instance = None
                    stop_sends = 0
                    while True:
                        check_running()
                        serial_observed = serial_path.read_bytes()[serial_begin:].decode(errors="replace")
                        observed = (serial_observed
                                    + shell_log[shell_begin:].decode(errors="replace"))
                        if not sent_stop:
                            instance = next_stop_instance(observed, instances)
                            if instance:
                                instances.add(instance)
                                shell.sendall(b"vmx_stop\r")
                                sent_stop = True
                                stop_sends += 1
                        status, detail, vm = stop_wave_result(observed, serial_observed, instance)
                        if status == "FAIL":
                            raise RuntimeError(detail)
                        if status == "PASS":
                            result["stop_waves"].append({"wave": wave, "vmid": vm, "instance": instance, "stop_sends": stop_sends, "detail": detail})
                            break
                        drain(shell, shell_log, .1)
                if result["stop_waves"][0]["vmid"] != result["stop_waves"][1]["vmid"]:
                    raise RuntimeError("Stop probe did not reuse the same VM slot")
                result["status"] = "PASS"
                result["detail"] = "tokio_stop: PASS graceful_stop=2 same_vm_slot=1 native_jobs=0"
            while True:
                if args.probe == "tokio_stop":
                    break
                check_running()
                observed = serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
                status, detail = probe_result(observed, args.probe)
                if status:
                    result["status"] = status
                    result["detail"] = detail
                    if status == "FAIL":
                        raise RuntimeError(detail)
                    break
                drain(shell, shell_log, .1)
            if args.veloren_executor:
                # A Blueprint's entry loop retains its terminal route after
                # main returns. Select Default before launching another app.
                shell.sendall("§\r".encode())
                drain(shell, shell_log, .3)
                active_probe = "veloren_executor"
                shell.sendall(b"veloren_executor\r")
                while True:
                    check_running()
                    observed = serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
                    status, detail = probe_result(observed, "veloren_executor")
                    if status == "FAIL":
                        raise RuntimeError(detail)
                    if status == "PASS":
                        result["veloren_executor"] = detail
                        break
                    drain(shell, shell_log, .1)
            result["qmp_final_status"] = qmp.command("query-status")
        except Exception as error:
            result["status"] = "FAIL"
            result["detail"] = str(error)
            if qmp:
                try:
                    result["qmp_final_status"] = qmp.command("query-status")
                except Exception as qmp_error:
                    result["qmp_final_error"] = str(qmp_error)
        finally:
            shell_path.write_bytes(shell_log)
            (args.output / "shell3.log").write_bytes(shell3_log)
            result["probe_tail"] = probe_tail(
                serial_path.read_text(errors="replace") + shell_log.decode(errors="replace"),
                probe=active_probe,
            )
            if shell:
                shell.close()
            if qmp:
                try:
                    qmp.command("quit")
                except (OSError, EOFError, RuntimeError):
                    pass
                qmp.close()
            if process:
                stop_owned_process(process)
                result["qemu_exit_code"] = process.returncode
            result["elapsed_seconds"] = round(time.monotonic() - started, 3)
            (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"{result['status']}: {result.get('detail', 'No result')}")
    print(f"Evidence: {args.output}")
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
