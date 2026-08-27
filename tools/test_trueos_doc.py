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
        self.assertEqual(data["logs"]["baremetal_latest"], "bld/baremetal-logs/LatestOfThree.logs")
        self.assertEqual(data["make_iso"]["invocation"], "make iso")
        self.assertEqual(data["blueprints"]["invocation"], "!cargo bp <appname>")
        self.assertEqual(data["trueosfs_http"]["base_url"], "http://192.168.178.94")

    def test_live_registry_exposes_schema(self) -> None:
        data = run("command", "xhci")["data"]
        self.assertEqual(data["mode"], "cmd")
        self.assertEqual(data["parameters"]["type"], "object")
        self.assertIn("command", data["parameters"]["properties"])

    def test_os_command_carries_live_update_tui_contract(self) -> None:
        data = run("command", "os")["data"]
        self.assertEqual(data["invocation"], "os")
        self.assertIn("Down then Enter", data["live_update"]["selection"][1])
        self.assertIn("no disk installation", data["live_update"]["effect"])

    def test_shot_command_distinguishes_admission_from_persistence(self) -> None:
        data = run("command", "shot")["data"]
        self.assertIn("not PNG persistence", data["acknowledgement"])
        self.assertIn("do not silently substitute an old file", data["failure_diagnosis"])

    def test_it_runs_outside_the_repo_through_its_path(self) -> None:
        data = run("topic", "§", cwd=Path("/tmp"))["data"]
        self.assertEqual(data["name"], "headjack")

    def test_search_routes_agent_vocabulary(self) -> None:
        results = run("search", "testrig shell port")["data"]["results"]
        self.assertEqual(results[0]["name"], "rig")

    def test_logs_topic_explains_filtering_tiers_and_capture_files(self) -> None:
        data = run("topic", "logfiles")["data"]
        self.assertEqual(data["name"], "logs")
        self.assertEqual(data["levels"]["order"][0], "Error")
        self.assertIn("Never expect it in a logfile", data["filter_invariant"])
        self.assertEqual(data["captures"]["emulator"]["latest"], "bld/emulator-logs/latest.log")

    def test_make_iso_topic_documents_full_baremetal_redeploy(self) -> None:
        data = run("topic", "make iso")["data"]
        self.assertEqual(data["name"], "make-iso")
        self.assertEqual(data["invocation"], "make iso")
        self.assertIn("end-to-end bare-metal redeploy", data["summary"])
        self.assertIn("physically resets the rig", data["workflow"][-1])
        self.assertEqual(data["verification"]["latest_log"], "bld/baremetal-logs/LatestOfThree.logs")
        self.assertEqual(data["controls"]["START_BAREMETAL_LOG=0"], "Skip the physical deploy/reset/log-verification dispatch.")

    def test_search_discovers_make_iso_for_baremetal_redeploy_vocabulary(self) -> None:
        results = run("search", "baremetal redeploy iso")["data"]["results"]
        self.assertEqual(results[0]["name"], "make-iso")

    def test_blueprints_topic_documents_internal_named_app_publication(self) -> None:
        data = run("topic", "!cargo bp")["data"]
        self.assertEqual(data["name"], "blueprints")
        self.assertEqual(data["repository"], "../TRUEOS-Blueprints")
        self.assertEqual(data["build_and_publish"]["terminal_invocation"], "cd ../TRUEOS-Blueprints && cargo bp <appname>")
        self.assertIn("internal/local deployment", data["purpose"])
        self.assertIn("removes older versions", data["boundaries"][0])
        self.assertIn("TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1", data["boundaries"][-1])

    def test_search_discovers_blueprints_for_cargo_bp_vocabulary(self) -> None:
        results = run("search", "cargo bp app publish")["data"]["results"]
        self.assertEqual(results[0]["name"], "blueprints")

    def test_reference_topic_indexes_checked_in_html_facts(self) -> None:
        data = run("topic", "html")["data"]
        self.assertEqual(data["name"], "references")
        paths = {item["path"] for item in data["documents"]}
        self.assertEqual(
            paths,
            {
                "tools/docs/CompositorUI.html",
                "tools/docs/execution.html",
                "tools/docs/intel-uhd770-cpu-reference.html",
                "tools/docs/depgraph/index.html",
                "tools/docs/docs/HYPERVISOR_STATE_MACHINE.html",
            },
        )
        for path in paths:
            self.assertTrue((ROOT / path).is_file(), path)

    def test_trueosfs_http_topic_exposes_root_aware_download_api(self) -> None:
        data = run("topic", "filesystem-api")["data"]
        self.assertEqual(data["name"], "trueosfs-http")
        self.assertEqual(data["discovery"]["path"], "/")
        download = next(route for route in data["routes"] if route["path"].startswith("/dl/<root-id>"))
        self.assertFalse(download["mutation"])
        self.assertIn("do not silently substitute an old file", data["screenshot_pull"][-1])


if __name__ == "__main__":
    unittest.main()
