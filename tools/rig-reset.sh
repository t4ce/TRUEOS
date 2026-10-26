#!/usr/bin/env bash
# Reply once to the ESP32 reset controller; no kernel or SSH connection needed.
set -euo pipefail

python3 - <<'PY'
import socket
import sys
import time

print("Waiting up to 30 seconds for the ESP32 reset probe on UDP 7777…", flush=True)
deadline = time.monotonic() + 30
try:
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind(("0.0.0.0", 7777))
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise TimeoutError("No ESP32 probe received within 30 seconds")
            sock.settimeout(remaining)
            payload, source = sock.recvfrom(2048)
            if payload != b"probe":
                continue
            sock.sendto(b"ack", (source[0], 7777))
            print(f"Reset acknowledgement sent to ESP32 at {source[0]}:7777.", flush=True)
            break
except OSError as exc:
    print(f"Rig reset failed: {exc}", file=sys.stderr)
    sys.exit(1)
PY
