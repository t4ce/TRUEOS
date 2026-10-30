#!/usr/bin/env python3
"""Exercise log-reader liveness and append-only reconnects without rig access."""

import importlib.util
import io
import os
from pathlib import Path
import socket
import subprocess
import tempfile
import threading
import time
import unittest
from unittest.mock import MagicMock, call, patch


ROOT = Path(__file__).resolve().parents[2]
READER = ROOT / "tools/baremetal-log-reader.py"
SPEC = importlib.util.spec_from_file_location("baremetal_log_reader", READER)
reader = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(reader)


class BaremetalLogDrain(unittest.TestCase):
    def test_reader_probes_quiet_peers_and_preserves_received_bytes(self):
        stream = MagicMock()
        stream.__enter__.return_value = stream
        stream.recv.side_effect = [b"first\x00", b"second\n", b""]
        output = io.BytesIO()
        with patch.object(reader.socket, "create_connection", return_value=stream) as connect:
            reader.copy_stream("rig", 1, output)
        connect.assert_called_once_with(("rig", 1), timeout=1.0)
        self.assertEqual(stream.setsockopt.call_args_list, [
            call(socket.SOL_SOCKET, socket.SO_KEEPALIVE, 1),
            call(socket.IPPROTO_TCP, socket.TCP_KEEPIDLE, 15),
            call(socket.IPPROTO_TCP, socket.TCP_KEEPINTVL, 5),
            call(socket.IPPROTO_TCP, socket.TCP_KEEPCNT, 3),
        ])
        stream.settimeout.assert_called_once_with(None)
        self.assertEqual(output.getvalue(), b"first\x00second\n")

    def test_lost_peer_error_reaches_collector_retry_loop(self):
        stream = MagicMock()
        stream.__enter__.return_value = stream
        stream.recv.side_effect = TimeoutError("keepalive peer timed out")
        with patch.object(reader.socket, "create_connection", return_value=stream):
            with self.assertRaises(TimeoutError):
                reader.copy_stream("rig", 1, io.BytesIO())
        stream.__exit__.assert_called_once()

    def test_reconnect_appends_same_capture_without_rotating_slots(self):
        with tempfile.TemporaryDirectory(prefix="trueos-log-drain-") as temporary:
            directory = Path(temporary)
            logs = directory / "logs"
            logs.mkdir()
            log = logs / "trueos-baremetal.1.log"
            log.write_bytes(b"preserved evidence\n")
            inode = log.stat().st_ino
            latest = logs / "LatestOfThree.logs"
            latest.symlink_to(log.name)
            slot = directory / "slot"
            slot.write_text("1\n")
            stop = threading.Event()
            errors = []
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                listener.listen(2)
                listener.settimeout(8)

                def serve():
                    try:
                        with listener.accept()[0] as stream:
                            stream.sendall(b"before disconnect\n")
                        with listener.accept()[0] as stream:
                            stream.sendall(b"after reconnect\n")
                            stop.wait(8)
                    except OSError as error:
                        errors.append(error)

                server = threading.Thread(target=serve)
                server.start()
                env = dict(os.environ,
                    TRUEOS_BAREMETAL_LOG_HOST="127.0.0.1",
                    TRUEOS_BAREMETAL_LOG_PORT=str(listener.getsockname()[1]),
                    TRUEOS_BAREMETAL_LOG_DELAY="0",
                    TRUEOS_BAREMETAL_LOG_RETRY_DELAY="1",
                    TRUEOS_BAREMETAL_LOG_DIR=str(logs),
                    TRUEOS_BAREMETAL_LOG_PID=str(directory / "pid"),
                    TRUEOS_BAREMETAL_LOG_SLOT=str(slot))
                collector = subprocess.Popen(
                    ["bash", str(ROOT / "tools/baremetal-log-drain.sh"),
                     "collect", "0123456789abcdef", str(log)],
                    env=env, start_new_session=True,
                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
                try:
                    deadline = time.monotonic() + 8
                    while b"after reconnect\n" not in log.read_bytes():
                        if collector.poll() is not None or time.monotonic() >= deadline:
                            self.fail("collector did not reconnect: " + log.read_text())
                        time.sleep(0.02)
                    captured = log.read_bytes()
                    self.assertTrue(captured.startswith(b"preserved evidence\n"))
                    self.assertIn(b"before disconnect\n", captured)
                    self.assertIn(b"connect_attempt=2", captured)
                    self.assertEqual(log.stat().st_ino, inode)
                    self.assertEqual(slot.read_text(), "1\n")
                    self.assertEqual(latest.readlink(), Path(log.name))
                    self.assertFalse(errors)
                finally:
                    collector.terminate()
                    try:
                        _, stderr = collector.communicate(timeout=3)
                    except subprocess.TimeoutExpired:
                        collector.kill()
                        _, stderr = collector.communicate(timeout=3)
                    stop.set()
                    server.join(timeout=3)
                self.assertEqual(collector.returncode, 0, stderr.decode())


if __name__ == "__main__":
    unittest.main()
