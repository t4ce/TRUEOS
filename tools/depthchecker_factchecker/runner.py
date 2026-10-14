#!/usr/bin/env python3
"""Run the stack experiment in a new, owned QEMU; never touch the physical rig."""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import pty
import re
import select
import signal
import shutil
import socket
import struct
import subprocess
import sys
import termios
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[2]


def digest(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def verify_artifact_binding(bp, debug_elf, packed_elf):
    path = bp.with_suffix(".debug.json")
    binding = json.loads(path.read_text())
    for name, artifact in (("blueprint", bp), ("debug_elf", debug_elf), ("packed_elf", packed_elf)):
        expected = binding[name]
        if expected["bytes"] != artifact.stat().st_size or expected["sha256"] != digest(artifact):
            raise ValueError(f"{name} differs from packer's diagnostic artifact binding")
    return {"path": str(path), "sha256": digest(path), "binding": binding}


def private_ports(count):
    sockets = [socket.socket() for _ in range(count)]
    try:
        for item in sockets:
            item.bind(("127.0.0.1", 0))
        return [item.getsockname()[1] for item in sockets]
    finally:
        for item in sockets:
            item.close()


def pin_host_key(path, port):
    # The kernel build embeds this development host seed. Pin its PUBLIC key;
    # do not disable host-key verification for the local experiment.
    from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PrivateKey
    from cryptography.hazmat.primitives import serialization
    public = Ed25519PrivateKey.from_private_bytes(
        (ROOT / ".local/ssh-host-seed.bin").read_bytes()
    ).public_key().public_bytes(serialization.Encoding.OpenSSH,
                              serialization.PublicFormat.OpenSSH)
    path.write_bytes(f"[127.0.0.1]:{port} ".encode() + public + b"\n")


class Terminal:
    def __init__(self, port, directory):
        self.master, slave = pty.openpty()
        import fcntl
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        attributes = termios.tcgetattr(slave)
        attributes[3] &= ~(termios.ECHO | termios.ICANON)
        termios.tcsetattr(slave, termios.TCSANOW, attributes)
        known = directory / "known_hosts"
        pin_host_key(known, port)
        self.process = subprocess.Popen([
            "ssh", "-F", "/dev/null", "-tt", "-p", str(port),
            "-i", str(Path.home() / ".ssh/id_ed25519_trueos"),
            "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
            "-o", f"UserKnownHostsFile={known}", "t4ce@127.0.0.1",
        ], stdin=slave, stdout=slave, stderr=slave)
        os.close(slave)
        self.log = (directory / "terminal.bin").open("wb", buffering=0)
        self.bytes = bytearray()

    def drain(self, seconds=.1):
        end = time.monotonic() + seconds
        while time.monotonic() < end:
            if select.select([self.master], [], [], min(.1, end-time.monotonic()))[0]:
                try:
                    data = os.read(self.master, 65536)
                except OSError:
                    data = b""
                if not data:
                    raise RuntimeError("SSH terminal closed")
                self.log.write(data)
                self.bytes.extend(data)
                for query, reply in ((b"\x1b[6n", b"\x1b[1;1R"),
                                     (b"\x1b[18t", b"\x1b[8;40;120t"),
                                     (b"\x1b[c", b"\x1b[?1;2c"),
                                     (b"\x1b[>c", b"\x1b[>0;0;0c")):
                    if query in data:
                        os.write(self.master, reply)

    def send(self, text):
        os.write(self.master, text.encode())

    def close(self):
        self.process.terminate()
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait()
        os.close(self.master)
        self.log.close()


def stop_owned(process):
    if process and process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("iso", "kernel", "debug-elf", "packed-elf", "bp", "plan", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--timeout", type=float, default=300)
    parser.add_argument("--smp", type=int, default=20)
    parser.add_argument("--memory", default="6000M")
    args = parser.parse_args()
    for name in ("iso", "kernel", "debug_elf", "packed_elf", "bp", "plan"):
        path = getattr(args, name).resolve()
        if not path.is_file():
            parser.error(f"missing {name}: {path}")
        setattr(args, name, path)
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=False)
    original_files = {name: str(getattr(args, name)) for name in
                      ("iso", "kernel", "debug_elf", "packed_elf", "bp", "plan")}
    original_binding = verify_artifact_binding(args.bp, args.debug_elf, args.packed_elf)
    inputs = args.output / "inputs"
    inputs.mkdir()
    # Reflink or independent copy; hardlinks would be truncated when the
    # packer replaces dist sidecars. Pin code, symbols and boot inputs now.
    names = {"iso": "trueos.iso", "kernel": "TRUEOS.full.elf",
             "debug_elf": "depthchecker_factchecker.debug.o",
             "packed_elf": "depthchecker_factchecker.packed.o",
             "bp": "depthchecker_factchecker.bp", "plan": "plan.json"}
    original_bp = args.bp
    for name, filename in names.items():
        destination = inputs / filename
        subprocess.run(["cp", "--reflink=auto", "--", str(getattr(args, name)), str(destination)], check=True)
        setattr(args, name, destination)
    shutil.copy2(original_bp.with_suffix(".debug.json"), args.bp.with_suffix(".debug.json"))
    overlay = original_bp.parent.parent / "probes/depthchecker_factchecker/target/trueos-blueprint/work/depthchecker_factchecker/source-overlay-app"
    if overlay.is_dir():
        shutil.copytree(overlay, inputs / "source-overlay-app")
    capture_tools = args.output / "capture-tools"
    capture_tools.mkdir()
    for name in ("gdb_capture.py", "elfmap.py", "vmx_memory.py"):
        shutil.copy2(Path(__file__).parent / name, capture_tools / name)
    ports = private_ports(3)
    serial = args.output / "serial.log"
    serial.touch()
    qmp = args.output / "qmp.sock"
    env = dict(os.environ, ISO_PATH=str(args.iso), QEMU_DISPLAY="egl-headless",
               QEMU_UEFI_FIRMWARE="/usr/share/ovmf/OVMF.fd", QEMU_MEMORY=args.memory,
               QEMU_SMP=str(args.smp), QEMU_SERIAL=f"file:{serial}",
               QEMU_DEBUG_LOG=str(args.output / "qemu-debug.log"),
               QEMU_NETDEV_USER=("user,id=net1,restrict=on,"
                   f"hostfwd=tcp:127.0.0.1:{ports[0]}-:22,"
                   f"hostfwd=tcp:127.0.0.1:{ports[1]}-:80"))
    command = [str(ROOT / "tools/qemu/run.sh"), "iso", "-snapshot", "-qmp",
               f"unix:{qmp},server=on,wait=off", "-gdb", f"tcp:127.0.0.1:{ports[2]}"]
    binding = verify_artifact_binding(args.bp, args.debug_elf, args.packed_elf)
    record = {"command": command, "artifact_binding": binding, "original_files": original_files,
              "original_artifact_binding_sha256": original_binding["sha256"],
              "capture_tool_sha256": {p.name: digest(p) for p in capture_tools.iterdir()},
              "environment": {k: v for k, v in env.items() if k.startswith("QEMU_") or k == "ISO_PATH"},
              "files": {name: {"path": str(getattr(args, name)), "sha256": digest(getattr(args, name))}
                        for name in ("iso", "kernel", "debug_elf", "packed_elf", "bp", "plan")},
              "ports": dict(zip(("ssh", "http", "gdb"), ports)), "outbound_network": False}
    result = {"status": "FAIL"}
    terminal = process = debugger = None
    deadline = time.monotonic() + args.timeout
    host_log = (args.output / "qemu.log").open("wb")
    debug_log = (args.output / "gdb.log").open("wb")

    def wait(predicate, label):
        result["stage"] = label
        while not predicate():
            if process.poll() is not None:
                raise RuntimeError(f"QEMU exited during {label}")
            if time.monotonic() >= deadline:
                raise TimeoutError(label)
            text = serial.read_text(errors="replace")
            panic = re.search(r"panicked at[^\r\n]*[\r\n]+([^\r\n]*)", text)
            if panic:
                raise RuntimeError("Probe/kernel panic: " + panic.group(0).strip())
            failure = re.search(r"depthchecker_factchecker: FAIL[^\r\n]*", text)
            if failure:
                raise RuntimeError(failure.group(0))
            if terminal:
                terminal.drain(.1)
            else:
                time.sleep(.1)

    def ssh_ready():
        try:
            with socket.create_connection(("127.0.0.1", ports[0]), .3) as connection:
                connection.settimeout(.3)
                connection.sendall(b"SSH-2.0-depthchecker_probe\r\n")
                return connection.recv(128).startswith(b"SSH-")
        except OSError:
            return False

    try:
        process = subprocess.Popen(command, cwd=ROOT, env=env, stdout=host_log,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        record["pid"] = process.pid
        (args.output / "run.json").write_text(json.dumps(record, indent=2) + "\n")
        wait(ssh_ready, "SSH boot readiness")
        terminal = Terminal(ports[0], args.output)
        wait(lambda: b"TrueOS" in terminal.bytes, "Shell3 repaint")
        terminal.drain(1)
        # ADM mode, RAM filesystem only. No NVMe formatting or external writes.
        terminal.send("\tdisc")
        wait(lambda: b"Choose an operation" in terminal.bytes, "disc picker")
        terminal.send("jj\r")
        wait(lambda: b"Creates and mounts TRUEOSFS" in terminal.bytes, "RAM disc confirmation")
        terminal.send("j\r")
        wait(lambda: b"RAM disk disc" in terminal.bytes, "RAM disc mount")
        listing = None
        def filesystem_ready():
            nonlocal listing
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{ports[1]}/", timeout=1) as response:
                    listing = response.read().decode()
                return bool(re.search(r'data-root="\d+"', listing))
            except (OSError, TimeoutError):
                return False
        wait(filesystem_ready, "HTTP RAM filesystem discovery")
        roots = set(re.findall(r'data-root="(\d+)"', listing))
        if len(roots) != 1:
            raise RuntimeError(f"expected one isolated RAM filesystem, found {roots}")
        result["root_id"] = roots.pop()
        # A fresh frontend starts in CMD. Closing the completed disc session
        # avoids sending app-name bytes into its asynchronously retiring TUI.
        terminal.close()
        terminal = None
        launch_directory = args.output / "launch-shell"
        launch_directory.mkdir()
        terminal = Terminal(ports[0], launch_directory)
        wait(lambda: b"TrueOS" in terminal.bytes, "launch Shell3 repaint")
        terminal.drain(.5)
        terminal.send("depthchecker_factchecker\r")
        wait(lambda: "depthchecker_factchecker: READY" in serial.read_text(errors="replace"), "probe debugger gate")
        matches = re.findall(r"hv: rel image loaded vm=(\d+) base=(0x[0-9A-Fa-f]+).*?used_len=(0x[0-9A-Fa-f]+)",
                             serial.read_text(errors="replace"))
        if len(matches) != 1:
            raise RuntimeError(f"expected exactly one probe module load, found {len(matches)}")
        result["vm_id"], base, result["image_bytes"] = matches[0]
        plan = json.loads(args.plan.read_text())
        plan.update(version=1, module={"debug_elf": str(args.debug_elf), "packed_elf": str(args.packed_elf),
                    "load_base": int(base, 16)}, output_dir=str(args.output / "captures"),
                    kernel_elf=str(args.kernel), markers={"gate": "DEPTHCHECKER_GATE"},
                    vmx_handoff_required=True, expected_vm_id=int(result["vm_id"]))
        sys.path.insert(0, str(capture_tools))
        from elfmap import prepare_manifest
        plan = prepare_manifest(plan)
        manifest = args.output / "capture-manifest.json"
        manifest.write_text(json.dumps(plan, indent=2) + "\n")
        script = args.output / "capture.gdb"
        script.write_text("set pagination off\nset confirm off\nset print pretty off\n"
            "set auto-load python-scripts off\nset debuginfod enabled off\n"
            f"file {args.kernel}\ntarget remote 127.0.0.1:{ports[2]}\n"
            f"source {capture_tools / 'gdb_capture.py'}\ndepthchecker-run\nquit\n")
        debugger = subprocess.Popen(["gdb", "-nx", "-batch", "-x", str(script)],
                                    env=dict(os.environ, DEPTHCHECKER_CAPTURE_MANIFEST=str(manifest)),
                                    stdout=debug_log, stderr=subprocess.STDOUT, start_new_session=True)
        wait(lambda: debugger.poll() is not None, "debugger first-hit captures")
        if debugger.returncode:
            raise RuntimeError(f"GDB failed with {debugger.returncode}; see gdb.log")
        summary = json.loads((args.output / "captures/capture-summary.json").read_text())
        result["capture_ok"] = summary["capture_ok"]
        result["probe_pass_mask"] = summary["probe_pass_mask"]
        if not summary["capture_ok"] or summary["probe_pass_mask"] != 0x7f:
            raise RuntimeError("probe/capture coverage incomplete; see captures/capture-summary.json")
        result["status"] = "PASS"
    except Exception as error:
        result["error"] = f"{type(error).__name__}: {error}"
    finally:
        if terminal:
            terminal.close()
        stop_owned(debugger)
        stop_owned(process)
        host_log.close()
        debug_log.close()
        result["owned_qemu_stopped"] = bool(process and process.poll() is not None)
        (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result))
    return 0 if result["status"] == "PASS" else 1


if __name__ == "__main__":
    raise SystemExit(main())
