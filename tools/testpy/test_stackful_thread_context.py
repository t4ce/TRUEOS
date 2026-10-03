#!/usr/bin/env python3
"""Exercise the production x86-64 continuation assembly on the host."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def main():
    with tempfile.TemporaryDirectory(prefix="trueos-thread-context-") as directory:
        path = Path(directory)
        source = path / "context_test.rs"
        source.write_text('extern crate alloc;\n#[path="' + str(ROOT / "src/r/threads/stack.rs") + '"] mod stack;\n#[path="' + str(ROOT / "src/r/threads/context.rs") + '"] mod context;\n')
        binary = path / "context_test"
        subprocess.run(["rustc", "--edition=2024", "--test", "--target", "x86_64-unknown-linux-gnu", str(source), "-o", str(binary)], check=True, cwd=ROOT)
        subprocess.run([str(binary)], check=True)

if __name__ == "__main__":
    main()
