#!/usr/bin/env python3
"""Host-only checks for conservative outage bounds and complete HTTP probes."""

import asyncio
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import time
import unittest


SOURCE = Path(__file__).resolve().parents[1] / "live-update-downtime.py"
SPEC = importlib.util.spec_from_file_location("live_update_downtime", SOURCE)
PROBE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PROBE)


def observation(start, end, ok, sequence=0):
    return {
        "sequence": sequence, "start_monotonic_ns": round(start * 1e9),
        "end_monotonic_ns": round(end * 1e9), "start_utc": f"start:{start}",
        "end_utc": f"end:{end}", "ok": ok, "duration_ms": (end - start) * 1000,
    }


class SummaryTests(unittest.TestCase):
    def test_bound_keeps_polling_latency_and_manual_restart(self):
        rows = [observation(1, 1.01, True), observation(1.25, 1.45, False),
                observation(12, 12.2, False), observation(12.25, 12.28, True)]
        summary = PROBE.summarize_probe(rows)
        episode = summary["episodes"][0]
        self.assertAlmostEqual(episode["availability_gap_upper_bound_seconds"], 11.28)
        self.assertEqual(episode["failed_observations"], 2)
        self.assertEqual(episode["last_pre_failure_success"], PROBE.boundary(rows[0]))
        self.assertEqual(episode["first_recovered_success"], PROBE.boundary(rows[-1]))
        self.assertEqual(summary["baseline_success_durations"]["count"], 1)
        self.assertAlmostEqual(summary["maximum_start_to_start_seconds"], 10.75)

    def test_incomplete_boundaries_do_not_invent_a_bound(self):
        rows = [observation(1, 1.2, False), observation(1.25, 1.26, True),
                observation(1.5, 1.7, False)]
        episodes = PROBE.summarize_probe(rows)["episodes"]
        self.assertEqual(episodes[0]["status"], "recovered_without_pre_failure_success")
        self.assertIsNone(episodes[0]["availability_gap_upper_bound_seconds"])
        self.assertEqual(episodes[1]["status"], "unrecovered")
        self.assertIsNone(episodes[1]["availability_gap_upper_bound_seconds"])

    def test_overlap_uses_first_complete_recovery_and_handles_completion_order(self):
        rows = [observation(1, 1.01, True), observation(1.25, 1.75, False),
                observation(1.5, 1.95, True), observation(1.75, 1.8, True)]
        summary = PROBE.summarize_probe([rows[0], rows[1], rows[3], rows[2]])
        self.assertEqual(summary["episodes"][0]["first_recovered_success"],
                         PROBE.boundary(rows[3]))
        self.assertAlmostEqual(summary["episodes"][0]["availability_gap_upper_bound_seconds"], .8)

    def test_cli_rejects_sub_100ms_sampling(self):
        with self.assertRaises(SystemExit), redirect_stderr(io.StringIO()):
            PROBE.parse_args(["--output", "unused", "--interval", ".099"])


class NetworkTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.tasks = set()

        async def respond(reader, writer):
            task = asyncio.current_task()
            self.tasks.add(task)
            try:
                try:
                    request = await reader.readuntil(b"\r\n\r\n")
                except asyncio.IncompleteReadError:
                    return  # TCP acceptance probes intentionally send no HTTP.
                path = request.split(b" ", 2)[1]
                if path == b"/slow":
                    await asyncio.sleep(.3)
                if path == b"/chunked":
                    writer.write(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n"
                                 b"3\r\nweb\r\n3\r\nsys\r\n0\r\nX-Trailer: yes\r\n\r\n")
                elif path == b"/short":
                    writer.write(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nwebsys")
                else:
                    writer.write(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n"
                                 b"Content-Type: text/plain\r\n\r\nwebsys")
                await writer.drain()
            finally:
                writer.close()
                self.tasks.discard(task)

        self.server = await asyncio.start_server(respond, "127.0.0.1", 0)
        self.port = self.server.sockets[0].getsockname()[1]

    async def asyncTearDown(self):
        self.server.close()
        await self.server.wait_closed()
        tasks = list(self.tasks)
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)

    async def test_full_body_and_marker_are_required(self):
        for path in ("/", "/chunked"):
            result = await PROBE.http_request("127.0.0.1", self.port, path, "websys")
            self.assertTrue(result["ok"])
            self.assertEqual(result["body_bytes"], 6)
        mismatch = await PROBE.http_request("127.0.0.1", self.port, "/", "missing")
        self.assertFalse(mismatch["ok"])
        config = {"name": "http", "kind": "http", "host": "127.0.0.1",
                  "port": self.port, "path": "/short"}
        result = await PROBE.observe(config, 0, time.monotonic(), 0, .5)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error_type"], "IncompleteReadError")

    async def test_slow_http_does_not_block_fixed_schedule_or_tcp(self):
        with tempfile.TemporaryDirectory() as directory:
            args = PROBE.parse_args([
                "--output", directory, "--host", "127.0.0.1", "--port", str(self.port),
                "--ssh-port", str(self.port), "--path", "/slow", "--asset-path", "/",
                "--interval", ".1", "--timeout", ".2", "--duration", ".45",
            ])
            with redirect_stdout(io.StringIO()), redirect_stderr(io.StringIO()):
                summary = await PROBE.run(args)
            http = summary["probes"]["workload_http"]
            tcp = summary["probes"]["ssh_tcp"]
            self.assertGreaterEqual(http["observations"], 4)
            self.assertEqual(http["successes"], 0)
            self.assertEqual(tcp["successes"], tcp["observations"])
            self.assertLess(http["maximum_start_to_start_seconds"], .18)
            self.assertEqual(summary["probes"]["asset_http_1"]["failures"], 0)
            events = [json.loads(line) for line in
                      (Path(directory) / "observations.jsonl").read_text().splitlines()]
            self.assertEqual(events[0]["event"], "run_start")
            self.assertEqual(events[-1]["event"], "run_end")
            self.assertEqual(len([event for event in events if event["event"] == "observation"]),
                             sum(probe["observations"] for probe in summary["probes"].values()))
            self.assertTrue((Path(directory) / "summary.json").exists())


if __name__ == "__main__":
    unittest.main()
