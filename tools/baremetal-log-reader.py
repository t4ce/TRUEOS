#!/usr/bin/env python3
"""Copy one rig log connection, detecting peers lost without a TCP close."""

import socket
import sys


def copy_stream(host, port, output):
    with socket.create_connection((host, port), timeout=1.0) as stream:
        # A rig reset can leave a receive-only host connection ESTABLISHED
        # indefinitely. Probe quiet peers so the drain can reconnect and append
        # to the same capture after a missing FIN/RST. Healthy quiet streams
        # stay connected; inactivity alone is not a reason to rotate captures.
        stream.setsockopt(socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1)
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPIDLE, 15)
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPINTVL, 5)
        stream.setsockopt(socket.IPPROTO_TCP, socket.TCP_KEEPCNT, 3)
        stream.settimeout(None)
        while True:
            data = stream.recv(64 * 1024)
            if not data:
                return
            output.write(data)
            output.flush()


if __name__ == "__main__":
    copy_stream(sys.argv[1], int(sys.argv[2]), sys.stdout.buffer)
