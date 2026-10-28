#!/usr/bin/env python3
"""Test the production Spirit agent snapshot model, layouts and motion paths.

The kernel has test=false. This fixture imports the real wire contract and
presentation controller without hardware services. It exercises their inline
tests using the repository toolchain and a locked, offline Cargo fixture.
--preview FILE also exports model-generated placements and actual queued cubic
paths to JSON, without claiming a hardware-rendered screenshot.
"""

import argparse
import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("spirit_thoughts")
HOST = "x86_64-unknown-linux-gnu"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--preview", type=Path, help="export representative placements and paths as JSON")
    args = parser.parse_args()
    channel = re.search(r'^channel\s*=\s*"([^"]+)"',
                        (ROOT / "rust-toolchain.toml").read_text(), re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    vm_limit = re.search(r'pub const VM_ID_LIMIT: usize = (\d+);',
                         (ROOT / "src/allcaps.rs").read_text())
    assert vm_limit, "production VM source capacity missing"
    with tempfile.TemporaryDirectory(prefix="trueos-spirit-thoughts-") as directory:
        work = Path(directory)
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-spirit-thoughts-tests"
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
        (work / "Cargo.lock").write_bytes((FIXTURES / "Cargo.lock").read_bytes())
        root_literal = json.dumps(str(ROOT), ensure_ascii=False)[1:-1]
        (work / "lib.rs").write_text((FIXTURES / "harness.rs").read_text()
                                    .replace("@TRUEOS_ROOT@", root_literal)
                                    .replace("@VM_ID_LIMIT@", vm_limit.group(1)))
        build = subprocess.run([
            "cargo", "+" + channel.group(1), "test", "--offline", "--locked", "--lib",
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
        result = subprocess.run([binary], cwd=work)
        if result.returncode:
            raise SystemExit(result.returncode)
        if args.preview:
            import os
            output = args.preview.resolve()
            output.parent.mkdir(parents=True, exist_ok=True)
            subprocess.run([binary, "spirit_thoughts::tests::export_model_preview_when_requested", "--exact"],
                           cwd=work, env={**os.environ, "TRUEOS_SPIRIT_THOUGHTS_PREVIEW": str(output)},
                           check=True)
            print(f"Model layout preview: {output}")


if __name__ == "__main__":
    main()
