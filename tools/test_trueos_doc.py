#!/usr/bin/env python3

from __future__ import annotations

import json
import subprocess
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CLI = ROOT / "tools/trueos-doc"


def run(*args: str, cwd: Path | None = None) -> dict:
    completed = subprocess.run(
        [str(CLI), *args],
        cwd=cwd or ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(completed.stdout)


class TrueosDocTests(unittest.TestCase):
    def test_context_contains_high_value_navigation_and_rig_facts(self) -> None:
        data = run()["data"]
        self.assertEqual(data["shell2"]["default_mode"], "cmd")
        self.assertEqual(data["rig"]["shell2_tcp_port"], 4245)
        self.assertIn("§<slot>§", [item["input"] for item in data["headjack"]])

    def test_live_registry_exposes_schema(self) -> None:
        data = run("command", "xhci")["data"]
        self.assertEqual(data["mode"], "cmd")
        self.assertEqual(data["parameters"]["type"], "object")
        self.assertIn("command", data["parameters"]["properties"])

    def test_it_runs_outside_the_repo_through_its_path(self) -> None:
        data = run("topic", "§", cwd=Path("/tmp"))["data"]
        self.assertEqual(data["name"], "headjack")

    def test_search_routes_agent_vocabulary(self) -> None:
        results = run("search", "testrig shell port")["data"]["results"]
        self.assertEqual(results[0]["name"], "rig")


if __name__ == "__main__":
    unittest.main()
