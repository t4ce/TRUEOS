#!/usr/bin/env python3
"""Run production HTTPS response and redirect helpers without kernel hardware."""
from pathlib import Path
import subprocess
import tempfile
from test_clip_position3_uv_texture import item

source = "src/r/net/https.rs"
harness = "extern crate alloc;\nuse alloc::{format, string::String, vec::Vec};\n"
for name in ("FetchTarget", "HttpsJsonResponse", "valid_header_value", "parse_fetch_url",
             "fetch_target_url", "resolve_fetch_redirect", "find_http_header_end",
             "parse_http_status", "trim_ascii", "header_value", "header_value_has_token",
             "decode_chunked", "final_response_bytes", "bad_response_message", "complete_http_response",
             "http_response_from_bytes", "http_fetch_tests"):
    harness += item(source, name)
with tempfile.TemporaryDirectory(prefix="trueos-https-") as directory:
    root = Path(directory)
    (root / "test.rs").write_text(harness)
    subprocess.run(["rustc", "--edition=2024", "--test", "-A", "dead_code",
                    str(root / "test.rs"), "-o", str(root / "test")], check=True)
    subprocess.run([str(root / "test")], check=True)
