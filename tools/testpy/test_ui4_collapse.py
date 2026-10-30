#!/usr/bin/env python3
"""Run production terminal collapse code against mocked leases and UI4 storage."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/ui4/blueprint_ui4/collapse_api.rs").read_text()
    terminal = source[source.index("// Terminal icons use"):]
    with tempfile.TemporaryDirectory(prefix="trueos-ui4-collapse-") as directory:
        work = Path(directory)
        (work / "src").mkdir()
        (work / "Cargo.toml").write_text('''[package]
name = "trueos-ui4-collapse-tests"
version = "0.1.0"
edition = "2024"
[workspace]
[dependencies]
spin = "0.10.0"
''')
        (work / "src/lib.rs").write_bytes((ROOT / "tools/testpy/ui4_collapse/harness.rs").read_bytes())
        (work / "src/terminal.rs").write_text(terminal)
        shell = (ROOT / "src/shell3/tui.rs").read_text()
        binding = shell[shell.index("pub(super) fn bind_ui4_window"):shell.index("pub(super) fn new_frontend")]
        (work / "src/shell_binding.rs").write_text(binding)
        subprocess.run(["cargo", "test", "--offline", "--target", "x86_64-unknown-linux-gnu"],
                       cwd=work, check=True)


if __name__ == "__main__":
    main()
