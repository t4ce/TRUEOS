#!/usr/bin/env python3
"""Exercise the actual Spirit agent ABI ingress and Hull dispatch boundary on the host."""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("spirit_agent_transport")


def body(source: str, signature: str) -> str:
    start = source.index(signature)
    opening = source.index("{", start)
    depth = 1
    for index in range(opening + 1, len(source)):
        depth += (source[index] == "{") - (source[index] == "}")
        if depth == 0:
            return source[start:index + 1]
    raise AssertionError(f"unclosed production body: {signature}")


def main() -> None:
    source = (ROOT / "src/spirit/mod.rs").read_text()
    ingress = body(source, "pub(crate) fn publish_agent_snapshot_bytes(")
    export = body(source, 'pub unsafe extern "C" fn trueos_cabi_spirit_agents_publish_v1(')
    dispatch = body((ROOT / "src/hv/vmcall.rs").read_text(), "OP_BP_SPIRIT_AGENTS_PUBLISH_V1 => {")
    dispatch = dispatch[dispatch.index("{") + 1:-1]
    vmcall = (ROOT / "crates/trueos-vm/src/vmcall.rs").read_text()
    opcode = re.search(r"pub const OP_BP_SPIRIT_AGENTS_PUBLISH_V1: u32 = (0x[0-9a-fA-F]+);", vmcall)
    comm_bytes = re.search(r"pub const COMM_PAGE_BYTES: usize = ([0-9* +]+);", vmcall)
    assert opcode and comm_bytes
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-agent-transport-") as directory:
        work = Path(directory)
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-spirit-agent-transport-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
heapless = "=0.9.1"
libm = "=0.2.16"
serde = { version = "=1.0.229", features = ["derive"] }
serde_json = "=1.0.154"
[lib]
path = "lib.rs"
''')
        lock = (ROOT / "tools/testpy/spirit_thoughts/Cargo.lock").read_text()
        (work / "Cargo.lock").write_text(lock.replace('name = "trueos-spirit-thoughts-tests"', 'name = "trueos-spirit-agent-transport-tests"'))
        root_literal = json.dumps(str(ROOT), ensure_ascii=False)[1:-1]
        harness = (FIXTURES / "harness.rs").read_text().replace("@TRUEOS_ROOT@", root_literal)
        harness = harness.replace("@INGRESS@", ingress).replace("@EXPORT@", export).replace("@DISPATCH@", dispatch)
        harness = harness.replace("@OPCODE@", opcode.group(1)).replace("@COMM_PAGE_BYTES@", comm_bytes.group(1))
        (work / "lib.rs").write_text(harness)
        result = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--locked", "--lib",
            "--target", "x86_64-unknown-linux-gnu",
        ], cwd=work)
        raise SystemExit(result.returncode)


if __name__ == "__main__":
    main()
