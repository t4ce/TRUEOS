#!/usr/bin/env python3
"""Replay overlapping, partial and corrupt startup captures through the real analyzer."""

import importlib.util
import json
from pathlib import Path
import unittest

SOURCE = Path(__file__).resolve().parents[1] / "codex_startup_report.py"
SPEC = importlib.util.spec_from_file_location("startup_report", SOURCE)
REPORT = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(REPORT)


def row(time, stage, pid=1, request=None):
    suffix = f" request={request}" if request is not None else ""
    return f"[blueprint] [info] [codex-startup] {pid}\t{time:.3f}\t0.000\t{stage}{suffix}\n"


def begin(time, span_id, name, request=None):
    return row(time, f"span_begin id={span_id} name={name}", request=request)


def end(time, span_id, name, duration, polls=1, total=1, maximum=1, outcome="complete", request=None):
    return row(time, f"span_end id={span_id} name={name} outcome={outcome} duration_ms={duration} "
                    f"poll_n={polls} poll_total_ms={total} poll_max_ms={maximum}", request=request)


class StartupReportTests(unittest.TestCase):
    def test_static_event_and_operation_names_are_preserved_without_payload(self):
        report = REPORT.parse_lines([row(1, "startup_app_event sample=3 category=0 event=CodexOp op=list_skills payload=private")])
        event = report["application_runs"][0]["startup_app_events"][0]
        self.assertEqual(event["event"], "CodexOp")
        self.assertEqual(event["op"], "list_skills")
        self.assertNotIn("private", json.dumps(report))

    def test_invalid_event_and_operation_labels_are_not_exported(self):
        report = REPORT.parse_lines([
            row(1, "startup_app_event sample=1 category=0 event=/private/path op=list_skills"),
            row(2, "startup_app_event sample=2 category=0 event=CodexOp op=private-operation"),
        ])
        self.assertNotIn("/private/path", json.dumps(report))
        self.assertNotIn("private-operation", json.dumps(report))
        self.assertEqual(report["application_runs"][0].get("startup_app_events", []), [])
        self.assertTrue(any("malformed" in warning for warning in report["warnings"]))

    def test_startup_skills_policy_and_executed_coalescing_are_separate(self):
        run = REPORT.parse_lines([
            row(1, "tui_startup_skills_coalescing_enabled"),
            row(2, "tui_startup_skills_override_invalid"),
            row(3, "tui_startup_skills_coalesced"),
        ])["application_runs"][0]
        self.assertTrue(run["startup_skills_coalescing"])
        self.assertTrue(run["startup_skills_override_invalid"])
        self.assertTrue(run["startup_skills_refresh_coalesced"])
        disabled = REPORT.parse_lines([row(1, "tui_startup_skills_coalescing_disabled")])["application_runs"][0]
        self.assertFalse(disabled["startup_skills_coalescing"])
        self.assertNotIn("startup_skills_refresh_coalesced", disabled)

    def test_post_handoff_budget_remains_separate_and_bounded(self):
        run = REPORT.parse_lines([
            row(1, "startup_app_event sample=9 category=0 event=RefreshSessionSkills op=none"),
            row(2, "startup_server_event sample=16 category=1"),
        ])["application_runs"][0]
        self.assertEqual(run["startup_app_events"][0]["phase"], "after_handoff")
        self.assertEqual(run["startup_server_events"][0]["phase"], "after_handoff")

    def test_server_event_names_use_a_fixed_payload_free_allowlist(self):
        report = REPORT.parse_lines([
            row(1, "startup_server_event sample=9 category=1 event=ServerNotification payload=private"),
            row(2, "startup_server_event sample=10 category=1 event=SecretNotification"),
        ])
        events = report["application_runs"][0]["startup_server_events"]
        self.assertEqual(len(events), 1)
        self.assertEqual(events[0]["event"], "ServerNotification")
        self.assertNotIn("private", json.dumps(report))
        self.assertNotIn("SecretNotification", json.dumps(report))

    def test_overlapping_spans_do_not_use_last_marker_delta(self):
        capture = [begin(0, 1, "outer"), begin(5, 2, "child"),
                   end(20, 2, "child", 15), end(30, 1, "outer", 30)]
        run = REPORT.parse_lines(capture, slow_ms=0)["application_runs"][0]
        self.assertEqual([item["duration_ms"] for item in run["slow_spans"]], [30, 15])
        self.assertEqual(len(run["spans"]), 2)
        self.assertEqual(run["open_spans"], [])

    def test_wait_and_blocking_poll_are_distinguished(self):
        capture = [begin(0, 1, "waiting"), end(1000, 1, "waiting", 1000, 2, 2, 1),
                   begin(1001, 2, "blocking"), end(1101, 2, "blocking", 100, 1, 100, 100)]
        run = REPORT.parse_lines(capture)["application_runs"][0]
        self.assertEqual([item["name"] for item in run["long_polls"]], ["blocking"])
        self.assertEqual(run["spans"][0]["outside_poll_ms"], 998)

    def test_editable_and_submit_ready_are_separate(self):
        capture = [row(0, "process_main"), row(20, "editable_composer_visible"),
                   begin(21, 1, "tui.state_db"), end(3000, 1, "tui.state_db", 2979),
                   row(3500, "session_prompt_ready")]
        run = REPORT.parse_lines(capture)["application_runs"][0]
        self.assertEqual(run["endpoints"], {"editable_ms": 20, "session_ready_frame_ms": 3500})
        self.assertEqual(run["spans"][0]["position"], "after_editable")

    def test_user_decision_is_not_ranked_as_automatic_bottleneck(self):
        run = REPORT.parse_lines([begin(0, 1, "tui.user_onboarding"),
                                  end(30000, 1, "tui.user_onboarding", 30000)])["application_runs"][0]
        self.assertTrue(run["spans"][0]["interactive"])
        self.assertEqual(run["slow_spans"], [])

    def test_missing_end_and_missing_begin_remain_explicit(self):
        run = REPORT.parse_lines([begin(10, 1, "pending"), row(30, "other_milestone"),
                                  end(40, 2, "partial", 10)])["application_runs"][0]
        self.assertEqual(run["open_spans"][0]["elapsed_lower_bound_ms"], 30)
        self.assertTrue(run["spans"][0]["partial"])
        self.assertIsNone(run["spans"][0]["start_ms"])

    def test_context_splits_reused_pid_and_discards_unrelated_telemetry(self):
        capture = [row(0, "process_context request=1 app_gen=7 archive_sha256=" + "a"*64),
                   begin(1, 1, "first", request=1), end(2, 1, "first", 1, request=1),
                   "unrelated private SQL auth payload must never be exported\n",
                   row(0, "process_context request=2 app_gen=8 secret=do_not_export"),
                   begin(1, 1, "second", request=2)]
        report = REPORT.parse_lines(capture)
        self.assertEqual(len(report["application_runs"]), 2)
        self.assertEqual(report["application_runs"][1]["context"], {"request": 2, "app_gen": 8})
        self.assertNotIn("do_not_export", json.dumps(report))
        self.assertNotIn("private SQL", json.dumps(report))

    def test_duplicate_rows_are_not_counted_twice(self):
        capture = [begin(0, 1, "test"), end(1, 1, "test", 1)]
        run = REPORT.parse_lines(capture+capture)["application_runs"][0]
        self.assertEqual(len(run["spans"]), 1)

    def test_corrupt_end_does_not_hide_open_span(self):
        report = REPORT.parse_lines([begin(0, 1, "pending"),
            row(1, "span_end id=1 name=pending outcome=complete duration_ms=nan poll_n=1 poll_total_ms=1 poll_max_ms=1")])
        self.assertEqual(len(report["application_runs"][0]["open_spans"]), 1)
        self.assertTrue(any("malformed" in item for item in report["warnings"]))

    def test_partial_capture_retains_per_record_request(self):
        run = REPORT.parse_lines([begin(10, 1, "known", request=7),
                                  end(20, 1, "known", 10, request=7)])["application_runs"][0]
        self.assertEqual(run["context"], {"request": 7})

    def test_plain_span_does_not_claim_poll_time_was_measured(self):
        run = REPORT.parse_lines([begin(0, 1, "db.state.open"),
            end(100, 1, "db.state.open", 100, polls=0, total=0, maximum=0)])["application_runs"][0]
        self.assertIsNone(run["spans"][0]["outside_poll_ms"])

    def test_corrupt_host_end_keeps_real_open_span(self):
        report = REPORT.parse_lines([
            "trueos-startup-v1 request=1 vm=255 gen=0 phase=prebind event=begin status=pending mono_ms=1 elapsed_ms=0\n",
            "trueos-startup-v1 request=1 vm=255 gen=0 phase=prebind event=end status=ok mono_ms=2 elapsed_ms=nan\n"])
        self.assertEqual(report["host_requests"][0]["open_spans"][0]["name"], "prebind")

    def test_reused_host_request_warns_about_mixed_boots(self):
        line = "trueos-startup-v1 request=1 vm=255 gen=0 phase=prebind event=begin status=pending mono_ms=1 elapsed_ms=0\n"
        report = REPORT.parse_lines([line, line])
        self.assertTrue(any("one boot" in warning for warning in report["warnings"]))

    def test_host_and_app_epochs_are_not_subtracted(self):
        capture = ["[blueprint] [info] trueos-startup-v1 request=4 vm=255 gen=0 phase=prebind event=begin status=pending mono_ms=90000 elapsed_ms=0\n",
                   "[blueprint] [info] trueos-startup-v1 request=4 vm=0 gen=9 phase=prebind event=end status=ok mono_ms=91000 elapsed_ms=1000\n",
                   row(0, "process_context request=4 app_gen=2"), row(10, "editable_composer_visible", request=4)]
        report = REPORT.parse_lines(capture)
        self.assertEqual(report["host_requests"][0]["spans"][0]["duration_ms"], 1000)
        self.assertEqual(report["application_runs"][0]["endpoints"]["editable_ms"], 10)
        self.assertEqual(report["application_runs"][0]["context"]["app_gen"], 2)

    def test_legacy_tsv_pairs_are_supported(self):
        report = REPORT.parse_lines(["3\t0.000\t0.000\tstate_db_begin\n",
                                     "3\t120.000\t120.000\tstate_db_done\n"])
        self.assertEqual(report["application_runs"][0]["milestone_intervals"][0]["duration_ms"], 120)
        self.assertIn("no poll measurements", REPORT.render_text(report))

    def test_interleaved_native_pid_one_uses_request_on_every_record(self):
        capture = [row(0, "process_context request=1"), row(0, "process_context request=2"),
                   begin(1, 1, "first", request=1), begin(1, 1, "second", request=2),
                   end(20, 1, "second", 19, request=2), end(30, 1, "first", 29, request=1)]
        runs = REPORT.parse_lines(capture)["application_runs"]
        self.assertEqual(len(runs), 2)
        self.assertEqual([item["spans"][0]["name"] for item in runs], ["first", "second"])

    def test_request_survives_hull_to_native_pid_change(self):
        capture = [row(0, "process_context request=3", pid=1001),
                   row(10, "runtime_begin", pid=1, request=3),
                   row(20, "runtime_done", pid=1, request=3)]
        runs = REPORT.parse_lines(capture)["application_runs"]
        self.assertEqual(len(runs), 1)
        self.assertEqual(runs[0]["pids"], [1001, 1])
        self.assertEqual(runs[0]["context"]["request"], 3)

    def test_placement_is_retained_without_other_attributes(self):
        run = REPORT.parse_lines([row(0, "thread_placement_configured mode=1 background_cap=2")])["application_runs"][0]
        self.assertEqual(run["placement"], {"mode": 1, "background_cap": 2})

    def test_runtime_workers_are_retained_with_bounded_numeric_fields(self):
        report = REPORT.parse_lines([row(0, "runtime_workers_configured async_workers=2 blocking_cap=8 secret=hidden")])
        self.assertEqual(report["application_runs"][0]["runtime"], {"async_workers": 2, "blocking_cap": 8})
        self.assertNotIn("hidden", json.dumps(report))
        self.assertIn("runtime=", REPORT.render_text(report))
        for workers, blocking in [(0, 8), (5, 8), (2, 0), (2, 9)]:
            report = REPORT.parse_lines([row(0, f"runtime_workers_configured async_workers={workers} blocking_cap={blocking}")])
            self.assertNotIn("runtime", report["application_runs"][0])
            self.assertTrue(any("malformed" in warning for warning in report["warnings"]))

    def test_no_timing_records_does_not_mean_fast_startup(self):
        report = REPORT.parse_lines(["codex-progress ready_max_ms=18254 slice_max_ms=28705\n"])
        self.assertEqual(report["application_runs"], [])
        self.assertIn("cannot be recovered", REPORT.render_text(report))

    def test_handoff_diagnostics_decode_only_bounded_numeric_fields(self):
        report = REPORT.parse_lines([
            row(10, "startup_handoff_state change=1 mask=139 secret=hidden"),
            row(11, "startup_app_event sample=1 category=2 payload=private"),
            row(11, "startup_server_event sample=1 category=2 payload=private"),
            row(12, "startup_handoff_frame_rendered"),
        ])
        run = report["application_runs"][0]
        state = run["startup_handoff_states"][0]["state"]
        self.assertEqual({name for name, present in state.items() if present},
                         {"pending_frame", "session_configured", "app_queue_nonempty", "draft_pending"})
        self.assertEqual(run["startup_app_events"][0]["category"], "backend_fallback")
        self.assertEqual(run["startup_server_events"][0]["category"], "request")
        self.assertEqual(run["endpoints"]["startup_handoff_frame_ms"], 12)
        self.assertIn("handoff state: change=1 mask=139", REPORT.render_text(report))
        for private in ("hidden", "private", "payload"):
            self.assertNotIn(private, json.dumps(report))

    def test_handoff_diagnostics_reject_out_of_range_or_noninteger_fields(self):
        for marker in (
            "startup_handoff_state change=0 mask=1", "startup_handoff_state change=17 mask=1",
            "startup_handoff_state change=1 mask=1024", "startup_handoff_state change=1 mask=-1",
            "startup_handoff_state change=1 mask=1.0", "startup_handoff_state change=1 mask=１",
            "startup_app_event sample=0 category=0", "startup_app_event sample=17 category=0",
            "startup_app_event sample=1 category=4", "startup_app_event sample=1 category=private",
            "startup_server_event sample=1 category=3", "startup_server_event sample=17 category=0",
        ):
            with self.subTest(marker=marker):
                report = REPORT.parse_lines([row(0, marker)])
                run = report["application_runs"][0]
                self.assertFalse(run.get("startup_handoff_states"))
                self.assertFalse(run.get("startup_app_events"))
                self.assertFalse(run.get("startup_server_events"))
                self.assertTrue(any("malformed" in warning for warning in report["warnings"]))

    def test_prewarm_policy_is_reported_without_override_values(self):
        for marker, expected in REPORT.PREWARM_SELECTIONS.items():
            with self.subTest(marker=marker):
                report = REPORT.parse_lines([
                    row(0, "core_startup_prewarm_override_invalid value=private"),
                    row(1, marker + " provider=hidden"),
                ])
                run = report["application_runs"][0]
                self.assertEqual(run["startup_websocket_prewarm"], expected)
                self.assertTrue(run["startup_prewarm_override_invalid"])
                self.assertIn("startup websocket prewarm: " + expected, REPORT.render_text(report))
                self.assertNotIn("private", json.dumps(report))
                self.assertNotIn("hidden", json.dumps(report))


if __name__ == "__main__":
    unittest.main()
