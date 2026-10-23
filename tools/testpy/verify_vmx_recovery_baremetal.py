#!/usr/bin/env python3
"""Exercise real VM retirement on an already booted rig using Shell3 SSH PTYs.

Launches vm_recovery_peer, then repeatedly launches vm_recovery, kills parked
work or requests its explicit native panic, and checks scoped cleanup/reuse.
Build, publish, deployment and hardware reset belong to the caller.
"""
from __future__ import annotations

import argparse
import fcntl
from html.parser import HTMLParser
import json
import os
from pathlib import Path
import pty
import re
import select
import struct
import subprocess
import termios
import time
from urllib.parse import unquote, urljoin, urlsplit
from urllib.error import HTTPError
from urllib.request import ProxyHandler, build_opener

ROOT = Path(__file__).resolve().parents[2]
FIELDS = re.compile(r"([a-zA-Z0-9_]+)=([^\s]+)")
READY = re.compile(r"vm_recovery: READY ([^\r\n]+)")
RECOVERY = re.compile(r"vm-recovery: ([a-z0-9-]+) ([^\r\n]+)")
OFFLINE = re.compile(r"hv: vm(\d+) lifecycle: offline ([^\r\n]+)")
RIG_HTTP = build_opener(ProxyHandler({}))


def fields(text: str) -> dict:
    return {key: int(value) if value.isdecimal() else value for key, value in FIELDS.findall(text)}


def complete_lines(text: str) -> str:
    return text if text.endswith("\n") else text.rsplit("\n", 1)[0] if "\n" in text else ""


class Capture:
    def __init__(self, path: Path):
        self.path = path.resolve(strict=True)
        stat = self.path.stat()
        self.inode, self.offset = stat.st_ino, stat.st_size

    def read(self) -> str:
        stat = self.path.stat()
        if stat.st_ino != self.inode or stat.st_size < self.offset:
            raise RuntimeError("capture rotated during the test; choose the current physical capture")
        with self.path.open("rb") as handle:
            handle.seek(self.offset)
            return handle.read().decode("utf-8", errors="replace")


class Shell3:
    def __init__(self, args, name: str, output: Path):
        self.master, slave = pty.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
        self.transcript = (output / f"shell3-{name}.ansi").open("wb")
        self.raw = bytearray()
        command = ["ssh", "-F", "/dev/null", "-tt", "-p", "22", "-i", str(args.identity.expanduser()),
                   "-o", "BatchMode=yes", "-o", "ConnectTimeout=10",
                   "-o", "StrictHostKeyChecking=yes", f"{args.user}@{args.host}"]
        self.process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave, close_fds=True)
        os.close(slave)

    def pump(self) -> None:
        while select.select([self.master], [], [], 0)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError:
                break
            if not data:
                break
            self.raw.extend(data)
            self.transcript.write(data)
            self.transcript.flush()

    def send(self, text: str) -> None:
        os.write(self.master, text.encode())

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                self.process.kill()
                self.process.wait()
        self.pump()
        os.close(self.master)
        self.transcript.close()


class FileIndex(HTMLParser):
    def __init__(self, data: bytes):
        super().__init__()
        self.links: set[str] = set()
        self.roots: set[str] = set()
        self.feed(data.decode("utf-8"))

    def handle_starttag(self, tag, pairs):
        attrs = dict(pairs)
        if "data-root" in attrs:
            self.roots.add(attrs["data-root"])
        href = attrs.get("href", "")
        if tag == "a" and href.startswith("/dl/"):
            self.links.add(href)


def fetch(base: str, path: str, timeout: float = 5) -> bytes:
    target = urljoin(base + "/", path)
    if urlsplit(target).netloc != urlsplit(base).netloc:
        raise RuntimeError("filesystem index link crosses the selected rig origin")
    with RIG_HTTP.open(target, timeout=timeout) as response:
        if response.status != 200:
            raise RuntimeError(f"unexpected HTTP status {response.status}")
        return response.read()


def status(args, peer: bool = False, route: str = "/status") -> dict:
    return fields(fetch(f"http://{args.host}:{32380 if peer else 32379}", route).decode())


def wait_for(sessions: list[Shell3], predicate, timeout: float, description: str):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        for session in sessions:
            session.pump()
            if session.process.poll() is not None:
                raise RuntimeError("Shell3 SSH disconnected; see transcript")
        value = predicate()
        if value:
            return value
        time.sleep(0.05)
    raise RuntimeError(f"timed out waiting for {description}")


def launch(args, session: Shell3, sessions: list[Shell3], capture: Capture, peer: bool) -> dict:
    start = len(capture.read())
    package = ("vmpeer" if args.appdb else "vm_recovery_peer") if peer else "vm_recovery"
    # AppDB names auto-latch. The peer alias avoids vm_recovery matching before
    # the remaining '_peer' suffix has arrived. Fastspawn always uses Appstore.
    session.send(package if args.appdb else f"§§{package}\r")

    def observed():
        text = complete_lines(capture.read()[start:])
        failure = re.search(r"vm_recovery: FAIL ([^\r\n]*)", text)
        if failure:
            raise RuntimeError(f"probe setup failed: {failure[1]}")
        ready = [fields(match[1]) for match in READY.finditer(text)
                 if fields(match[1]).get("peer") == int(peer)]
        if not ready:
            return None
        prepared = re.findall(rf"apps: vm(\d+) preparing [^\r\n]*\b{package}\.bp", text)
        if len(set(prepared)) != 1:
            return None
        row = ready[-1]
        row["vm"] = int(prepared[-1])
        for key, expected in (("runtimes", 2), ("workers", 4), ("native_parked", 1),
                              ("ui4", 1), ("fs_stream", int(not peer))):
            if row.get(key) != expected:
                raise RuntimeError(f"READY lacks {key}={expected}: {row}")
        return row

    ready = wait_for(sessions, observed, args.timeout, f"{package} READY and owning VM")
    live = status(args, peer)
    if live.get("instance") != ready["instance"] or live.get("workers") != 4 or live.get("parked") != "true":
        raise RuntimeError(f"TCP status does not match READY: {live}")
    if min(live.get("ticks0", 0), live.get("ticks1", 0)) <= 0:
        raise RuntimeError("both Tokio runtimes must make progress before admission")
    ready["status"] = live
    print(f"READY {package} vm={ready['vm']} instance={ready['instance']}", flush=True)
    return ready


def cleanup(text: str, vm: int, require_stream: bool, native_terminal: bool) -> dict | None:
    if re.search(r"panic-context:|=== #(PF|GP|UD|DF)", text):
        raise RuntimeError("kernel panic/fault during guest retirement; inspect recovery.log")
    text = complete_lines(text)
    offline = [fields(match[2]) for match in OFFLINE.finditer(text) if int(match[1]) == vm]
    if not offline:
        return None
    terminal = offline[-1]
    generation = terminal.get("generation")
    if terminal.get("native_jobs") != 0 or terminal.get("carrier") != "released":
        raise RuntimeError(f"offline retained native jobs/carrier: {terminal}")
    events = {match[1]: fields(match[2]) for match in RECOVERY.finditer(text)
              if fields(match[2]).get("vm") == vm and fields(match[2]).get("generation") == generation}
    for name in ("filesystem-drained", "ui4-released", "network-released"):
        if name not in events:
            raise RuntimeError(f"offline lacks {name} witness for vm={vm} generation={generation}")
    fs, ui4, net = (events[name] for name in ("filesystem-drained", "ui4-released", "network-released"))
    if any(fs.get(name) != 0 for name in ("fs_pending", "fs_streams", "async_fs_pending")):
        raise RuntimeError(f"filesystem ownership remains: {fs}")
    if ui4.get("surfaces", 0) < 1 or ui4.get("surfaces_remaining") != 0:
        raise RuntimeError(f"owned UI4 frame was not revoked: {ui4}")
    if sum(net.get(name, 0) for name in ("blueprint_net", "mio", "socket_cabi")) < 1:
        raise RuntimeError(f"owned TCP listener was not closed: {net}")
    vgpu = re.search(rf"hv: vm{vm} lifecycle: vgpu cleanup released=(\d+) quarantined=(\d+)", text)
    if not vgpu or int(vgpu[1]) < 1 or int(vgpu[2]) != 0:
        raise RuntimeError("vGPU resources were not released without quarantine")
    if require_stream and not re.search(rf"hv: vm{vm} lifecycle: discard filesystem rollback requested fs_streams=[1-9]", text):
        raise RuntimeError("no evidence of an open filesystem stream at kill")
    if native_terminal and "native-terminal" not in events:
        raise RuntimeError("native panic/allocation failure did not reach the scoped terminal boundary")
    return {"vm": vm, "generation": generation, "offline": terminal, "events": events}


def fs_witness(args, instance: str) -> dict:
    base = f"http://{args.host}"
    index = FileIndex(fetch(base, "/", args.timeout))
    if not index.roots:
        raise RuntimeError("filesystem root index returned no mounted roots")
    paths = [href for href in index.links if unquote(href).endswith("/apps/common/vm-recovery/committed.txt")]
    if len(paths) != 1:
        raise RuntimeError(f"expected one committed probe file on discovered roots: {paths}")
    expected = f"vm_recovery committed instance={instance}\n".encode()
    if fetch(base, paths[0]) != expected:
        raise RuntimeError("committed file changed or belonged to an older probe instance")
    abandoned = f"/apps/common/vm-recovery/abandoned-{instance}.bin"
    if any(unquote(href).endswith(abandoned) for href in index.links):
        raise RuntimeError("abandoned partial stream appeared as a committed file")
    # The tree listing has a soft cap. Probe the exact sibling on the root
    # whose committed file we just read, rather than infer absence from HTML.
    abandoned_path = paths[0].rsplit("/", 1)[0] + f"/abandoned-{instance}.bin"
    try:
        fetch(base, abandoned_path)
    except HTTPError as error:
        with error:
            if error.code != 404 or error.read() != b"not found\n":
                raise RuntimeError(f"unexpected response for abandoned stream: HTTP {error.code}") from error
    else:
        raise RuntimeError("abandoned partial stream remains directly downloadable")
    return {"roots": sorted(index.roots), "committed": paths[0], "abandoned_absent": True,
            "abandoned": abandoned_path, "abandoned_http_status": 404}


def run(args) -> dict:
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    capture = Capture(args.log)
    sessions: list[Shell3] = []
    cases = []
    try:
        for name in ("peer", "victim"):
            session = Shell3(args, name, output)
            sessions.append(session)
            wait_for(sessions, lambda: b"AppDB[" in session.raw, 30, f"{name} Shell3 CMD strip")
        peer = launch(args, sessions[0], sessions, capture, True)
        previous = None
        identities = set()
        for number, action in enumerate(args.actions, 1):
            current = launch(args, sessions[1], sessions, capture, False)
            if current["instance"] in identities:
                raise RuntimeError("relaunch reused the previous instance identity")
            identities.add(current["instance"])
            if previous is not None and current["vm"] != previous["vm"]:
                raise RuntimeError("the fresh instance did not reuse the retired VMID")
            before = status(args, True)
            offset = len(capture.read())
            print(f"CASE {number} action={action} vm={current['vm']}", flush=True)
            if action in ("panic", "oom"):
                response = status(args, route=f"/{action}")
                if response.get("instance") != current["instance"] or response.get("command") != action:
                    raise RuntimeError(f"{action} acknowledgement belongs to a different instance")
            else:
                sessions[1].send("kill")  # VME exact-name control validates this session's lease.

            def retirement():
                text = capture.read()[offset:]
                failure = re.search(r"vm_recovery: FAIL ([^\r\n]*)", complete_lines(text))
                if failure:
                    raise RuntimeError(f"probe failure during {action}: {failure[1]}")
                return cleanup(text, current["vm"], True, action in ("panic", "oom"))

            retired = wait_for(sessions, retirement, args.timeout,
                               "all scoped retirement witnesses and offline slot")
            filesystem = fs_witness(args, current["instance"])

            def peer_progress():
                after = status(args, True)
                if after.get("instance") != peer["instance"]:
                    raise RuntimeError("the independent peer changed instance during recovery")
                return after if all(after.get(key, 0) > before.get(key, 0)
                                    for key in ("ticks0", "ticks1")) else None

            after = wait_for(sessions, peer_progress, 5, "both independent peer runtime tickers")
            if any(fields(match[2]).get("vm") == peer["vm"]
                   for match in RECOVERY.finditer(capture.read()[offset:])):
                raise RuntimeError("victim recovery touched the independent peer's VM ownership")
            cases.append({"action": action, "ready": current, "retirement": retired,
                          "filesystem": filesystem, "peer_before": before, "peer_after": after})
            previous = current
            sessions[1].send("§\r")  # Select the default slot before the next fastspawn.
            for session in sessions:
                session.pump()
            time.sleep(0.2)
        peer_offset = len(capture.read())
        sessions[0].send("kill")
        peer_retired = wait_for(sessions,
            lambda: cleanup(capture.read()[peer_offset:], peer["vm"], False, False),
            args.timeout, "test peer retirement")
        return {"status": "passed", "cases": cases, "peer": peer, "peer_retirement": peer_retired,
                "log": str(capture.path), "log_start_bytes": capture.offset,
                "scope": "cooperative parked work and explicit guest native terminal; no host spinner preemption"}
    finally:
        (output / "recovery.log").write_text(capture.read())
        (output / "completed-cases.json").write_text(json.dumps(cases, indent=2) + "\n")
        for session in sessions:
            session.close()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--user", default="t4ce")
    parser.add_argument("--identity", type=Path, default=Path("~/.ssh/id_ed25519_trueos"))
    parser.add_argument("--log", type=Path, default=ROOT / "bld/baremetal-logs/LatestOfThree.logs")
    parser.add_argument("--timeout", type=float, default=90)
    parser.add_argument("--appdb", action="store_true",
                        help="launch embedded vm_recovery.bp and vmpeer.bp through AppDB")
    parser.add_argument("--actions", nargs="+", choices=("kill", "panic", "oom"),
                        default=["kill", "panic", "oom", "kill"],
                        help="ordered victim cases; default: kill panic oom kill")
    parser.add_argument("--output", type=Path, default=ROOT / "bld" / f"vm-recovery-{time.time_ns()}")
    args = parser.parse_args()
    if args.timeout <= 0:
        parser.error("timeout must be positive")
    try:
        result = run(args)
    except (OSError, RuntimeError, ValueError) as error:
        result = {"status": "failed", "error": str(error)}
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / "summary.json").write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return int(result["status"] != "passed")


if __name__ == "__main__":
    raise SystemExit(main())
