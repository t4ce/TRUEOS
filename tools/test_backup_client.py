#!/usr/bin/env python3
"""Host integration tests for encrypted TCP transfer and durable reconnects."""
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import socket
import struct
import tempfile
import threading
import unittest

spec = importlib.util.spec_from_file_location("backup_client", Path(__file__).with_name("backup-client.py"))
client = importlib.util.module_from_spec(spec)
spec.loader.exec_module(client)
KEY = bytes(range(32))
SESSION = bytes(range(16))
DATA = bytes(range(256)) * 13
CHUNK = 1024


class Server(threading.Thread):
    def __init__(self, drop=False, tamper=False, wrong_session=False, drop_eof=False):
        super().__init__(daemon=True)
        self.listener = socket.socket()
        self.listener.bind(("127.0.0.1", 0))
        self.port = self.listener.getsockname()[1]
        self.listener.listen(1)
        self.listener.settimeout(5)
        self.drop, self.tamper, self.wrong_session = drop, tamper, wrong_session
        self.drop_eof = drop_eof
        self.offsets = []
        self.error = None
        self.complete = False

    def run(self):
        try:
            for attempt in range(2 if self.drop else 1):
                conn, _ = self.listener.accept()
                with conn:
                    conn.settimeout(3)
                    session = b"X" * 16 if self.wrong_session else SESSION
                    hello = b"TBKP0001" + bytes([attempt + 1]) * 16 + session + struct.pack("<QII", len(DATA), 256, CHUNK)
                    conn.sendall(hello)
                    if self.wrong_session:
                        return
                    seq = 0
                    while True:
                        request = client.decrypt(client.receive(conn, 24), hello, client.nonce(hello, seq), KEY)
                        offset = struct.unpack("<Q", request)[0]
                        self.offsets.append(offset)
                        if offset == (1 << 64) - 1:
                            self.complete = True
                            return
                        if self.drop_eof and offset == len(DATA):
                            return
                        data = DATA[offset:offset + CHUNK]
                        encrypted = client.encrypt(data, hello, client.nonce(hello, seq, True), KEY)
                        frame = struct.pack("<I", len(encrypted)) + encrypted
                        if self.tamper:
                            conn.sendall(frame[:-1] + bytes([frame[-1] ^ 1]))
                            return
                        if self.drop and attempt == 0 and offset == CHUNK:
                            conn.sendall(frame[:111])  # Mid-chunk outage.
                            break
                        # Fragment framing as TCP is a stream, not message based.
                        conn.sendall(frame[:3])
                        conn.sendall(frame[3:])
                        seq += 1
        except Exception as error:
            self.error = error
        finally:
            self.listener.close()


class BackupClientTests(unittest.TestCase):
    def run_download(self, server, path):
        server.start()
        with contextlib.redirect_stdout(io.StringIO()):
            try:
                return client.download("127.0.0.1", server.port, path, SESSION, KEY, retry_delay=0)
            finally:
                server.join(6)
                self.assertFalse(server.is_alive(), "server failed to finish")
                if server.error:
                    raise server.error

    def test_full_image_and_reconnect_after_partial_chunk(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            server = Server(drop=True)
            state = self.run_download(server, path)
            self.assertEqual(path.read_bytes(), DATA)
            self.assertTrue(state["complete"])
            self.assertTrue(server.complete)
            self.assertEqual(server.offsets[:3], [0, CHUNK, CHUNK])
            self.assertEqual(state["sha256"], client.hashlib.sha256(DATA).hexdigest())

    def test_lost_terminal_response_does_not_retry_completed_image_forever(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            server = Server(drop_eof=True)
            state = self.run_download(server, path)
            self.assertEqual(path.read_bytes(), DATA)
            self.assertEqual(state["offset"], len(DATA))
            self.assertNotEqual(state.get("complete"), True)

    def test_tampered_chunk_never_reaches_image(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            with self.assertRaisesRegex(client.ProtocolError, "authentication"):
                self.run_download(Server(tamper=True), path)
            self.assertEqual(path.read_bytes(), b"")
            self.assertEqual(json.loads(path.with_name("disk.img.resume.json").read_text())["offset"], 0)

    def test_replaced_session_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            with self.assertRaisesRegex(client.ProtocolError, "expired/replaced"):
                self.run_download(Server(wrong_session=True), path)
            self.assertEqual(path.read_bytes(), b"")

    def test_restart_verifies_prefix_and_discards_uncommitted_tail(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            path.write_bytes(DATA[:CHUNK] + b"uncommitted")
            client.checkpoint(path.with_name("disk.img.resume.json"), {
                "session": SESSION.hex(), "offset": CHUNK,
                "sha256": client.hashlib.sha256(DATA[:CHUNK]).hexdigest(),
                "geometry": [len(DATA), 256, CHUNK],
            })
            server = Server()
            self.run_download(server, path)
            self.assertEqual(server.offsets[0], CHUNK)
            self.assertEqual(path.read_bytes(), DATA)

    def test_completed_checkpoint_returns_without_reconnecting(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            path.write_bytes(DATA)
            client.checkpoint(path.with_name("disk.img.resume.json"), {
                "session": SESSION.hex(), "offset": len(DATA), "complete": True,
                "sha256": client.hashlib.sha256(DATA).hexdigest(),
                "geometry": [len(DATA), 256, CHUNK],
            })
            # A retired server must not turn a durable completed image into an
            # endless reconnect loop.
            state = client.download("127.0.0.1", 1, path, SESSION, KEY, retry_delay=0)
            self.assertTrue(state["complete"])
            self.assertEqual(path.read_bytes(), DATA)

    def test_completed_checkpoint_requires_matching_geometry(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            path.write_bytes(DATA)
            client.checkpoint(path.with_name("disk.img.resume.json"), {
                "session": SESSION.hex(), "offset": len(DATA), "complete": True,
                "sha256": client.hashlib.sha256(DATA).hexdigest(),
                "geometry": [len(DATA) + 1, 256, CHUNK],
            })
            with self.assertRaisesRegex(client.ProtocolError, "completed checkpoint"):
                client.download("127.0.0.1", 1, path, SESSION, KEY)

    def test_corrupt_saved_prefix_and_existing_untracked_file_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "disk.img"
            path.write_bytes(b"keep me")
            with self.assertRaisesRegex(client.ProtocolError, "both"):
                client.download("127.0.0.1", 1, path, SESSION, KEY)
            self.assertEqual(path.read_bytes(), b"keep me")
            client.checkpoint(path.with_name("disk.img.resume.json"), {
                "session": SESSION.hex(), "offset": 7, "sha256": "bad",
            })
            with self.assertRaisesRegex(client.ProtocolError, "SHA-256"):
                client.download("127.0.0.1", 1, path, SESSION, KEY)


if __name__ == "__main__":
    unittest.main()
