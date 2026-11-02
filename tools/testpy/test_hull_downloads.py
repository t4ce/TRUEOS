#!/usr/bin/env python3
"""Run production download queue and HTTP framing/classification unit tests.

The kernel binary has no host test target. Compile its pure queue directly and
extract the unchanged header helpers plus their production tests into a small
host harness. This does not exercise VMX admission, TLS or browser rendering.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(source, name, indent=""):
    match = re.search(r"(?m)^" + indent + r"(?:pub )?(?:struct|fn) "
                      + re.escape(name) + r"(?=[<( {])", source)
    if not match:
        raise AssertionError(f"missing production item {name}")
    end = re.search(r"(?m)^" + indent + r"}", source[match.start():])
    if not end:
        raise AssertionError(f"unterminated production item {name}")
    return source[match.start():match.start() + end.end()] + "\n"


def main():
    https = (ROOT / "src/r/net/https.rs").read_text()
    html = (ROOT / "src/surfer/html_shack.rs").read_text()
    harness = ('extern crate alloc;\nuse alloc::{string::String, vec::Vec, format};\n'
               '#[derive(Debug)]\n' + item(https, "HttpsJsonResponse"))
    for name in ["find_http_header_end", "parse_http_status", "trim_ascii", "header_value",
                 "header_value_has_token", "decode_chunked", "final_response_bytes",
                 "bad_response_message", "complete_http_response", "complete_http_head_response",
                 "http_response_from_bytes", "success_body", "append_head_response_headers"]:
        harness += item(https, name)
    harness += "const HTTPS_RESPONSE_OVERHEAD_MAX:usize=4096;\n"
    harness += "const NAVIGATION:u8=b'N'; const DOWNLOAD:u8=b'D';\n"
    for name in ["navigation_header_decision", "navigation_response_decision", "head_requires_get_fallback"]:
        harness += item(html, name)
    harness += "#[cfg(test)] mod tests { use super::*;\n"
    for source, names in [
        (https, ["head_response_stops_at_headers_and_ignores_representation_length",
                 "head_response_waits_for_complete_headers_only",
                 "headers_only_get_ignores_body_in_the_same_data_event",
                 "response_body_limit_is_enforced_from_content_length",
                 "closed_truncated_response_is_rejected", "chunked_response_is_decoded",
                 "response_preserves_non_success_status_and_body"]),
        (html, ["navigation_header_probe_download_classification_is_conservative"]),
    ]:
        for name in names:
            harness += "#[test]\n" + item(source, name, "    ")
    harness += "}\n"
    with tempfile.TemporaryDirectory(prefix="hull-downloads-") as temporary:
        directory = Path(temporary)
        headers = directory / "headers.rs"
        headers.write_text(harness)
        for name, source in [("queue", ROOT / "src/r/services/download_queue.rs"),
                             ("headers", headers)]:
            binary = directory / name
            subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                           cwd=ROOT, check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
