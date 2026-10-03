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


def probe_tail(text, limit=4):
    """Extract bounded records before ANSI repaint removes line boundaries."""
    records = []
    for match in re.finditer(r"tokio_mrt:[^\r\n\x1b]{0,200}", text):
        record = plain(match.group(0)).strip()
        # Repaints repeatedly include the same retained transcript. Preserve
        # its most recent distinct records rather than whole screen redraws.
        if record in records:
            records.remove(record)
        records.append(record)
        records = records[-limit:]
    return records


def probe_result(text):
    """A PASS cannot hide an earlier probe failure or the reported worker panic."""
    text = plain(text)
    failure = re.search(r"tokio_mrt: FAIL[^\r\n]{0,200}", text)
    if failure:
        return "FAIL", failure.group(0)
    fault = re.search(r"hv: vm\d+ fault-exc[^\r\n]{0,180}", text)
    if fault:
        return "FAIL", fault.group(0)
    for marker in ("OS can't spawn worker thread", "thread '<unnamed>'", "panicked at"):
        if marker in text:
            return "FAIL", marker
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
    args = parser.parse_args()
    if not 0 < args.timeout <= 90:
        parser.error("--timeout must be greater than zero and at most 90 seconds")
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
              "shell2": {"guest_port": 4245, "host_port": ports[0], "command": "tokio_mrt"},
              "shell3": {"guest_port": 22, "host_port": ports[1], "observed": False}}
    started = time.monotonic()
    deadline = started + args.timeout
    process = None
    shell = None
    qmp = None
    with tempfile.TemporaryDirectory(prefix="trueos-tokio-qmp-") as socket_dir, \
            (args.output / "qemu.log").open("w") as host_log, \
            (args.output / "qmp.jsonl").open("w") as qmp_log:
        qmp_path = str(Path(socket_dir) / "qmp.sock")
        command = [str(ROOT / "tools/qemu/run.sh"), "iso", "-snapshot", "-qmp",
                   f"unix:{qmp_path},server=on,wait=off"]
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
            observed = serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
            status, detail = probe_result(observed)
            if status == "FAIL":
                raise RuntimeError(detail)
            if time.monotonic() >= deadline:
                raise TimeoutError("Boot/probe deadline expired; latest probe evidence: " +
                                   (" | ".join(probe_tail(observed)) or "no tokio_mrt output"))

        def drain(sock, log, seconds):
            end = min(deadline, time.monotonic() + seconds)
            while time.monotonic() < end:
                check_running()
                try:
                    data = sock.recv(65536)
                    if not data:
                        raise EOFError("Guest terminal closed")
                    log.extend(data)
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
            shell.sendall(b"tokio_mrt\r")
            result["shell2"]["mode"] = "Default (selected with §)"
            while True:
                check_running()
                observed = serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
                status, detail = probe_result(observed)
                if status:
                    result["status"] = status
                    result["detail"] = detail
                    if status == "FAIL":
                        raise RuntimeError(detail)
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
                serial_path.read_text(errors="replace") + shell_log.decode(errors="replace")
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
