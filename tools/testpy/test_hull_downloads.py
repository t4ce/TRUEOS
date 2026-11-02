#!/usr/bin/env python3
"""Run production download queue and HTTP framing/classification unit tests.

The kernel binary has no host test target. Compile its pure queue directly and
extract the unchanged header helpers and actual C ABI read entrypoint into
host harnesses. The ABI harness stubs VM context and disk normalization to
check virtual path routing. It does not exercise VMX admission, TLS or rendering.
"""
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def item(source, name, indent=""):
    match = re.search(r"(?m)^" + indent + r'(?:pub(?:\([^)]*\))? )?(?:unsafe extern "C" )?(?:struct|fn) '
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
    for name in ["HTTPS_PROBE_HEADER_MAX"]:
        harness += re.search(r"(?m)^const " + name + r":.*;$", https).group(0) + "\n"
    harness += "const NAVIGATION:u8=b'N'; const DOWNLOAD:u8=b'D';\n"
    for name in ["navigation_header_decision", "navigation_response_decision", "head_requires_get_fallback"]:
        harness += item(html, name)
    harness += "#[cfg(test)] mod tests { use super::*;\n"
    for source, names in [
        (https, ["head_response_stops_at_headers_and_ignores_representation_length",
                 "head_response_waits_for_complete_headers_only",
                 "headers_only_get_ignores_body_in_the_same_data_event",
                 "navigation_probe_accepts_large_github_style_redirect_headers",
                 "navigation_probe_rejects_unbounded_headers",
                 "response_body_limit_is_enforced_from_content_length",
                 "closed_truncated_response_is_rejected", "chunked_response_is_decoded",
                 "response_preserves_non_success_status_and_body"]),
        (html, ["navigation_header_probe_download_classification_is_conservative"]),
    ]:
        for name in names:
            harness += "#[test]\n" + item(source, name, "    ")
    harness += "}\n"
    abi_source = (ROOT / "src/r/io/async_fs_cabi.rs").read_text()
    production = ""
    for name in ["VMX_LAUNCH_SCRIPT_VFILE", "STARTUP_MANIFEST_VFILE", "TERMINAL_FRONTEND_VFILE",
                 "ASYNC_FS_MAX_VIRTUAL_READ_PATH"]:
        production += re.search(r"(?m)^const " + name + r":.*;$", abi_source).group(0) + "\n"
    for name in ["is_virtual_read", "is_virtual_write", "parse_path", "trueos_cabi_async_fs_read_start"]:
        production += item(abi_source, name)
    abi_harness = (ROOT / "tools/testpy/hull_downloads_abi.rs").read_text()
    abi_harness = abi_harness.replace("@CABI_CODES@", str(ROOT / "src/r/cabi_codes.rs"))
    abi_harness = abi_harness.replace("// @PRODUCTION@", production)
    with tempfile.TemporaryDirectory(prefix="hull-downloads-") as temporary:
        directory = Path(temporary)
        headers = directory / "headers.rs"
        headers.write_text(harness)
        abi = directory / "abi.rs"
        abi.write_text(abi_harness)
        for name, source in [("queue", ROOT / "src/r/services/download_queue.rs"),
                             ("headers", headers), ("abi", abi)]:
            binary = directory / name
            subprocess.run(["rustc", "--edition=2024", "--test", str(source), "-o", str(binary)],
                           cwd=ROOT, check=True)
            subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
