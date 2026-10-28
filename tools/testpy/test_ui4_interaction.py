#!/usr/bin/env python3
"""Exercise native UI4 access and input ownership without real UI hardware.

The kernel has test=false. The fixture imports the complete production bridge,
real wire constants, and its inline tests; it mocks only VM lifecycle, UI4
snapshots/captures, and paired input devices. Tests run in fresh processes.
Requires the repository toolchain and cached Cargo dependencies (offline).
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("ui4_interaction")
HOST = "x86_64-unknown-linux-gnu"


def main() -> None:
    toolchain = (ROOT / "rust-toolchain.toml").read_text()
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', toolchain, re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    abi_source = (ROOT / "crates/trueos-v/src/bp_abi.rs").read_text()
    abi_constants = re.findall(r"^pub const DOBBY_UI4_.*?;", abi_source, re.MULTILINE)
    vmcall_source = (ROOT / "crates/trueos-vm/src/vmcall.rs").read_text()
    vmcall_constants = re.findall(r"^pub const OP_BP_DOBBY_UI4_.*?;", vmcall_source, re.MULTILINE)
    assert abi_constants and vmcall_constants, "production UI4 wire constants missing"

    with tempfile.TemporaryDirectory(prefix="trueos-ui4-interaction-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-ui4-interaction-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
spin = "0.10"
''')
        (work / "Cargo.lock").write_bytes((FIXTURES / "Cargo.lock").read_bytes())
        # JSON escaping also handles quotes and backslashes in checkout paths.
        root_literal = json.dumps(str(ROOT), ensure_ascii=False)[1:-1]
        template = (FIXTURES / "harness.rs").read_text()
        (work / "src/lib.rs").write_text(template.replace("@TRUEOS_ROOT@", root_literal))
        (work / "src/bp_abi_constants.rs").write_text("\n".join(abi_constants))
        (work / "src/vmcall_constants.rs").write_text("\n".join(vmcall_constants))
        build = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--locked",
            "--target", HOST, "--no-run", "--message-format=json",
        ], cwd=work, text=True, capture_output=True)
        if build.returncode:
            print(build.stdout + build.stderr)
            raise SystemExit(build.returncode)
        artifacts = [json.loads(line) for line in build.stdout.splitlines() if line.startswith("{")]
        binaries = [item["executable"] for item in artifacts
                    if item.get("reason") == "compiler-artifact"
                    and item.get("profile", {}).get("test") and item.get("executable")]
        assert len(binaries) == 1, f"expected one test binary, found {binaries}"
        binary = binaries[0]
        listing = subprocess.check_output([binary, "--list"], text=True)
        names = [line.removesuffix(": test") for line in listing.splitlines() if line.endswith(": test")]
        assert names, "no UI4 interaction tests discovered"
        failed = []
        for name in names:
            result = subprocess.run([binary, name, "--exact"], text=True, capture_output=True)
            print(("PASS " if result.returncode == 0 else "FAIL ") + name)
            if result.returncode:
                print(result.stdout + result.stderr)
                failed.append(name)
        print(f"{len(names) - len(failed)} passed; {len(failed)} failed")
        raise SystemExit(bool(failed))


if __name__ == "__main__":
    main()
