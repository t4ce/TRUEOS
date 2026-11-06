#!/usr/bin/env python3
"""Exercise production Heal JSON routing/service with semantic host and clock fakes.

The real protocol, service, Serde parser, Signal, mutex and atomic cancellation
state run on the host. Only hardware and elapsed time are replaced.
"""
import os
import contextlib
import importlib.util
import io
import json
from http.server import BaseHTTPRequestHandler, HTTPServer
import threading
from unittest.mock import patch
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("usb_heal")


def production_guard():
    text = (ROOT / "vendor/CrabUSB/usb-host/src/backend/kmod/xhci/hub.rs").read_text()
    start = text.index("fn explicit_reset_rejection(")
    return text[start:].replace("fn explicit_reset_rejection(", "pub(crate) fn explicit_reset_rejection(", 1)


def client_regression():
    spec = importlib.util.spec_from_file_location("usb_heal_client", ROOT / "tools/usb-heal.py")
    client = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(client)
    calls = []

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_GET(self):
            calls.append(("GET", self.path, None))
            self.send_json([])

        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            calls.append(("POST", self.path, body))
            if body["op"] == "observe":
                self.send_json({"status": "ok", "revision": 41})
            else:
                self.send_json({"status": "rejected", "detail": "ever-probed port is protected"})

        def send_json(self, payload):
            body = json.dumps(payload).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    server = HTTPServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, daemon=True)
    worker.start()
    try:
        base = f"http://127.0.0.1:{server.server_port}"
        with patch.dict(os.environ, {"HTTP_PROXY": "http://127.0.0.1:1", "NO_PROXY": ""}):
            status, response = client.invoke(base, "observe")
            assert status == 200 and response == []
            assert calls == [("GET", "/usb/heal/observe", None)]
            with contextlib.redirect_stdout(io.StringIO()), patch("sys.argv", [
                "usb-heal.py", "--host", "127.0.0.1", "--http-port", str(server.server_port),
                "port-reset", "--controller", "1", "--port", "3", "--timeout-ms", "200",
            ]):
                assert client.main() == 1  # rejected JSON remains failure even with HTTP 200
        assert calls[1][2] == {"op": "observe", "api": 1, "controller": 1}
        assert calls[2][2] == {
            "op": "run", "api": 1, "controller": 1, "expected_revision": 41,
            "pattern": {"name": "usb2_port_reset_v1", "port": {"kind": "port", "port_id": 3}, "timeout_ms": 200},
        }
        print("Heal client LAN/proxy, read-only observe, revision/controller/port and rejection checks passed")
    finally:
        server.shutdown()
        worker.join()
        server.server_close()


def run():
    with tempfile.TemporaryDirectory(prefix="trueos-usb-heal-") as temporary:
        project = Path(temporary)
        (project / "src").mkdir()
        protocol = project / "src/protocol.rs"
        protocol.write_text((ROOT / "src/usb3/heal_protocol.rs").read_text() + "\n" + (FIXTURES / "tests.rs").read_text())
        source = (FIXTURES / "harness.rs").read_text()
        for token, path in {
            "@SERVICE@": ROOT / "src/usb3/heal_service.rs",
            "@PROTOCOL@": protocol,
            "@DIAG@": ROOT / "vendor/CrabUSB/usb-host/src/diag.rs",
            "@RECOVER@": ROOT / "vendor/CrabUSB/usb-host/src/recover.rs",
        }.items():
            source = source.replace(token, str(path))
        source = source.replace("@RESET_GUARD@", production_guard())
        (project / "src/lib.rs").write_text(source)
        (project / "Cargo.toml").write_text(
            '[package]\nname="usb-heal-regression"\nversion="0.0.0"\nedition="2024"\n'
            '[workspace]\n[dependencies]\n'
            'serde={version="1",features=["derive"]}\nserde_json="1"\n'
            'spin="0.10"\nembassy-sync="0.8"\n'
        )
        env = os.environ.copy()
        env.pop("RUSTFLAGS", None)
        env.pop("CARGO_ENCODED_RUSTFLAGS", None)
        channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text()).group(1)
        subprocess.run(["cargo", f"+{channel}", "test", "--offline", "--manifest-path", str(project / "Cargo.toml"), "--", "--test-threads=1"], cwd=project, env=env, check=True)


if __name__ == "__main__":
    run()
    client_regression()
