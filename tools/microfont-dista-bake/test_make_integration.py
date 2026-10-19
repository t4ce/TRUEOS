#!/usr/bin/env python3
"""Exercise the production Make dependency graph with isolated mock tools."""
from __future__ import annotations

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


@unittest.skipUnless(shutil.which("make"), "GNU Make is required")
class MakeIntegrationTests(unittest.TestCase):
    def run_build(self, *, fail_bake: bool = False, mesa: str | None = None):
        with tempfile.TemporaryDirectory(prefix="dista-make-test-") as temporary:
            work = Path(temporary)
            bindir = work / "bin"
            bindir.mkdir()
            log = work / "events.jsonl"
            state = work / "fresh-shader"
            tool = """import json, os
from pathlib import Path
import sys
stage = Path(sys.argv[0]).name
with open(os.environ['DISTATEST_LOG'], 'a') as output:
    output.write(json.dumps({'stage': stage, 'args': sys.argv[1:]}) + '\\n')
state = Path(os.environ['DISTATEST_STATE'])
if stage == 'mock-python':
    assert sys.argv[1:3] == ['tools/microfont-dista-bake/bake.py', '--ensure-current']
    if os.environ['DISTATEST_FAIL'] == '1':
        sys.exit(17)
    state.write_text('rebaked')
elif stage == 'cargo':
    assert state.read_text() == 'rebaked', 'Cargo observed stale generated inputs'
"""
            for name in ("mock-python", "cargo", "mock-linked-check", "mock-artifacts", "mock-iso"):
                path = bindir / name
                path.write_text(f"#!{sys.executable}\n" + tool)
                path.chmod(0o755)
            # Keep the real iso -> artifacts -> kernel dependency edges. Only
            # replace publishing/packaging recipes; do not make an ISO or reset
            # hardware during a build graph test.
            fixture = work / "mock-packaging.mk"
            fixture.write_text("artifacts:\n\t@mock-artifacts\niso:\n\t@mock-iso\n")
            env = os.environ.copy()
            env.update(
                PATH=f"{bindir}{os.pathsep}{env['PATH']}",
                DISTATEST_LOG=str(log),
                DISTATEST_STATE=str(state),
                DISTATEST_FAIL=str(int(fail_bake)),
            )
            args = [
                shutil.which("make"), "--no-print-directory", "-j8",
                "-f", str(ROOT / "Makefile"), "-f", str(fixture),
                "-o", "images", "-o", "limine", "-o", "empty-libs", "iso",
                "ENABLE_BUILDINS=0", "INTEL_GPU_PREBUILD_VERIFY=",
                f"MICROFONT_DISTA_PYTHON={bindir / 'mock-python'}",
                f"MAKE={bindir / 'mock-linked-check'}",
            ]
            if mesa is not None:
                args.append(f"MICROFONT_DISTA_MESA_BUILD={mesa}")
            result = subprocess.run(args, cwd=work, env=env, text=True, capture_output=True)
            events = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return result, events

    def test_parallel_iso_refreshes_before_cargo_and_packaging(self):
        result, events = self.run_build()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual([event["stage"] for event in events], [
            "mock-python", "cargo", "mock-linked-check", "mock-artifacts", "mock-iso",
        ])
        self.assertEqual(events[0]["args"], ["tools/microfont-dista-bake/bake.py", "--ensure-current"])

    def test_failed_refresh_blocks_cargo_and_iso(self):
        result, events = self.run_build(fail_bake=True)
        self.assertNotEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual([event["stage"] for event in events], ["mock-python"])

    def test_configured_mesa_directory_is_one_argument(self):
        result, events = self.run_build(mesa="/tmp/compatible mesa/build")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertEqual(events[0]["args"], [
            "tools/microfont-dista-bake/bake.py", "--ensure-current",
            "--mesa-build", "/tmp/compatible mesa/build",
        ])


if __name__ == "__main__":
    unittest.main()
