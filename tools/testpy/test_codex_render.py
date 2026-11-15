#!/usr/bin/env python3
"""Exercise the production asynchronous Spirit character cache on the host."""
import json
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    with tempfile.TemporaryDirectory(prefix="codex-render-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text(
            '[package]\nname="codex-render-tests"\nversion="0.1.0"\n'
            'edition="2024"\n[workspace]\n[dependencies]\nspin="0.10"\n'
        )
        harness = (ROOT / "tools/testpy/codex_render/harness.rs").read_text()
        (work / "src/lib.rs").write_text(
            harness.replace("@TRUEOS_ROOT@", json.dumps(str(ROOT))[1:-1])
        )
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(ROOT / "tgt/host-codex-render-tests")
        build = subprocess.run(
            ["cargo", "test", "--release", "--offline", "--target",
             "x86_64-unknown-linux-gnu", "--no-run", "--message-format=json"],
            cwd=work, env=env, capture_output=True, text=True,
        )
        artifacts = [json.loads(line) for line in build.stdout.splitlines()
                     if line.startswith("{")]
        if build.returncode:
            for item in artifacts:
                if item.get("reason") == "compiler-message":
                    print(item["message"].get("rendered", item["message"]["message"]))
            print(build.stderr)
            build.check_returncode()
        binary, = [item["executable"] for item in artifacts
                   if item.get("reason") == "compiler-artifact"
                   and item.get("profile", {}).get("test") and item.get("executable")]
        listing = subprocess.check_output([binary, "--list"], text=True)
        names = [line.removesuffix(": test") for line in listing.splitlines()
                 if line.endswith(": test")]
        assert names, "no cache tests discovered"
        for name in names:
            subprocess.run([binary, name, "--exact"], check=True)


if __name__ == "__main__":
    main()
