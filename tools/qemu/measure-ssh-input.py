#!/usr/bin/env python3
"""Measure unsubmitted input echo over SSH, including transport and remote redraw.

The local PTY has echo disabled. A sample succeeds only when the remote output
contains its unique UTF-8 character. This measures returned terminal bytes, not
physical display latency. Use on an idle editable prompt; never sends Enter.
"""
import argparse
import fcntl
import json
import os
from pathlib import Path
import pty
import select
import statistics
import subprocess
import termios
import time
import struct


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host")
    parser.add_argument("--port", type=int, default=22)
    parser.add_argument("--identity")
    parser.add_argument("--known-hosts", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--samples", type=int, default=30)
    parser.add_argument("--settle", type=float, default=3)
    parser.add_argument("--timeout", type=float, default=3)
    args = parser.parse_args()
    if not 1 <= args.samples <= 80 or args.timeout <= 0 or args.settle < 0:
        parser.error("samples must be 1..80, timeout positive, settle nonnegative")
    args.output.mkdir(parents=True, exist_ok=False)
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 40, 120, 0, 0))
    attrs = termios.tcgetattr(slave)
    attrs[3] &= ~(termios.ECHO | termios.ICANON)
    termios.tcsetattr(slave, termios.TCSANOW, attrs)
    command = ["ssh", "-F", "/dev/null", "-tt", "-p", str(args.port),
               "-o", "BatchMode=yes", "-o", "ConnectTimeout=5",
               "-o", "StrictHostKeyChecking=accept-new",
               "-o", f"UserKnownHostsFile={args.known_hosts.resolve()}"]
    if args.identity:
        command += ["-o", "IdentitiesOnly=yes", "-i", args.identity]
    command.append(args.host)
    process = subprocess.Popen(command, stdin=slave, stdout=slave, stderr=slave)
    os.close(slave)
    samples = []
    started = time.monotonic()
    with (args.output / "terminal.bin").open("wb") as transcript, \
            (args.output / "chunks.jsonl").open("w") as chunks:
        def receive(timeout):
            if not select.select([master], [], [], max(0, timeout))[0]:
                return b""
            try:
                data = os.read(master, 65536)
            except OSError:
                data = b""
            transcript.write(data)
            chunks.write(json.dumps({"elapsed_ms": (time.monotonic() - started) * 1000,
                                     "bytes": len(data)}) + "\n")
            return data

        try:
            deadline = time.monotonic() + args.settle
            while time.monotonic() < deadline:
                receive(min(.1, deadline - time.monotonic()))
                if process.poll() is not None:
                    raise RuntimeError("SSH exited; inspect terminal.bin")
            for index in range(args.samples):
                # Distinct printable Cyrillic characters avoid matching ANSI numbers.
                marker = chr(0x410 + index).encode("utf-8")
                while receive(0):
                    pass
                sent = time.monotonic()
                os.write(master, marker)
                incoming = bytearray()
                deadline = sent + args.timeout
                matched = False
                while time.monotonic() < deadline:
                    incoming.extend(receive(min(.1, deadline - time.monotonic())))
                    if marker in incoming:
                        matched = True
                        break
                    if process.poll() is not None:
                        break
                samples.append({"index": index, "character": marker.decode(),
                                "latency_ms": (time.monotonic() - sent) * 1000 if matched else None,
                                "returned_bytes": len(incoming)})
                if not matched:
                    break
                # Erase only our just-entered character; leave the prompt untouched.
                os.write(master, b"\x7f")
                rest = time.monotonic() + .12 + (index % 7) * .003
                while time.monotonic() < rest:
                    receive(min(.03, rest - time.monotonic()))
        finally:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            os.close(master)
            latencies = sorted(s["latency_ms"] for s in samples if s["latency_ms"] is not None)
            result = {"host": args.host, "port": args.port, "samples": samples,
                      "measurement": "local PTY write to matching remote output bytes; includes SSH",
                      "requested_samples": args.samples, "matched_samples": len(latencies)}
            if latencies:
                result.update(median_ms=statistics.median(latencies),
                              p95_ms=latencies[max(0, (95 * len(latencies) + 99) // 100 - 1)],
                              max_ms=max(latencies))
            (args.output / "result.json").write_text(json.dumps(result, indent=2) + "\n")
            print(json.dumps({k: v for k, v in result.items() if k != "samples"}, indent=2))
    if len(latencies) != args.samples:
        raise SystemExit("Incomplete measurement; inspect terminal.bin and result.json")


if __name__ == "__main__":
    main()
