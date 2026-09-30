#!/usr/bin/env python3
"""Run XPAPP persistent-worker budget policy tests on the host."""
from pathlib import Path
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[1]
SOURCE = ROOT / "src/r/compute_budget.rs"
YIELD_POLICY = ROOT / "src/hv/execution_policy.rs"


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="trueos-xpapp-compute-budget-") as directory:
        root = Path(directory)
        harness = root / "test.rs"
        harness.write_text(
            f'#[path = "{SOURCE}"] mod compute_budget;\n'
            f'#[path = "{YIELD_POLICY}"] mod execution_policy;\n'
        )
        binary = root / "test"
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(harness), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
