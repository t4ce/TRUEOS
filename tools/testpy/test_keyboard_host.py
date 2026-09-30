#!/usr/bin/env python3
"""Run the actual keyboard module's pure tests without booting the kernel."""

import json
from pathlib import Path
import subprocess
import tempfile


def main():
    source = Path(__file__).resolve().parents[1] / "src/r/keyboard.rs"
    with tempfile.TemporaryDirectory(prefix="trueos-keyboard-tests-") as directory:
        root = Path(directory)
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text(
            '[package]\nname = "trueos-keyboard-host-tests"\n'
            'version = "0.1.0"\nedition = "2024"\n'
            '[dependencies]\nheapless = { version = "=0.9.3", default-features = false }\n'
            'spin = "=0.10.1"\n[workspace]\n'
        )
        (root / "src/lib.rs").write_text(
            '#![allow(dead_code, unused_variables, unfulfilled_lint_expectations)]\n'
            'extern crate alloc;\nextern crate self as embassy_time_driver;\n'
            'pub const TICK_HZ: u64 = 1_000;\npub fn now() -> u64 { 0 }\n'
            '#[macro_export]\nmacro_rules! log_info { ($($tokens:tt)*) => {{}}; }\n'
            f'#[path = {json.dumps(str(source))}]\nmod keyboard;\n'
        )
        subprocess.run(["cargo", "test", "--quiet"], cwd=root, check=True)


if __name__ == "__main__":
    main()
