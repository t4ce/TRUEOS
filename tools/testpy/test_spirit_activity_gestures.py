#!/usr/bin/env python3
"""Exercise the real Spirit sequencer on the host, without GPU hardware.

The kernel has test=false. This harness imports its production sequencer,
terminal parser, animation helpers and resident catalog, mocking only the
clock, random choices, terminal snapshot and hardware surfaces. Each test runs
in a fresh process because the real sequencer deliberately owns global slots.
Requires the repository toolchain and cached Cargo dependencies (offline).
"""

import json
from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
FIXTURES = Path(__file__).with_name("spirit_activity")
HOST = "x86_64-unknown-linux-gnu"


def main() -> None:
    toolchain = (ROOT / "rust-toolchain.toml").read_text()
    channel = re.search(r'^channel\s*=\s*"([^"]+)"', toolchain, re.MULTILINE)
    assert channel, "repository Rust toolchain channel missing"
    source = (ROOT / "src/spirit/lilly.rs").read_text()
    first = "#[derive(Copy, Clone, Debug, Eq, PartialEq)]\npub(crate) struct LillyResidentFrame"
    types = source[source.index(first):source.index("\nstruct LillyFrameEntry")]
    count = re.search(r"^const LILLY_FRAMES_PER_ASSET: usize = .*?;$", source, re.MULTILINE)
    assert count, "production frame-count constant missing"

    with tempfile.TemporaryDirectory(prefix="trueos-spirit-activity-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-spirit-activity-tests"
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
        (work / "src/lilly_types.rs").write_text(count.group() + "\n" + types)
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
        assert names, "no Spirit tests discovered"
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
