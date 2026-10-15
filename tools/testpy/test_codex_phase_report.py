#!/usr/bin/env python3
"""Synthetic privacy and correlation regressions for the offline timing report."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parent))
from codex_phase_report import markdown, parse_log_text


def event(name, fields='', spans='', target='log_only', session='primary-secret'):
    return (f'[apps] [info] codex_otel.{target}: {spans} '
            f'event.name="codex.{name}" {fields} conversation.id={session} '
            'user.account_id=ACCOUNT_SECRET user.email=EMAIL_SECRET\n')


class CodexPhaseReportTests(unittest.TestCase):
    def test_whitelist_and_canonical_deduplication(self):
        raw = event('tool_result', 'tool_name=exec duration_ms=4854 success=false arguments=ARGS_SECRET output=OUTPUT_SECRET failed to spawn code-mode host /apps/codex/codex-code-mode-host: trueos errno 38 (os error 38) event.timestamp=2026-10-15T07:35:27.000Z')
        raw += raw.replace('codex_otel.log_only:', 'codex_otel.trace_safe:')
        raw += event('user_prompt', 'prompt="PROMPT_SECRET" prompt_length=13')
        raw += '[codex-startup] 1 12.000 2.000 PRIVATE_SECRET\n'
        report = parse_log_text(raw)
        self.assertEqual(len(report['events']), 2)
        self.assertEqual(report['events'][0]['error_class'], 'ENOSYS')
        self.assertEqual(report['events'][0]['host_executable'], '/apps/codex/codex-code-mode-host')
        for serialized in (json.dumps(report), markdown(report)):
            for secret in ('ACCOUNT_SECRET', 'EMAIL_SECRET', 'ARGS_SECRET', 'OUTPUT_SECRET', 'PROMPT_SECRET', 'primary-secret', 'PRIVATE_SECRET'):
                self.assertNotIn(secret, serialized)

    def test_primary_background_and_warmup_are_separate(self):
        span = '[span=10 submission_dispatch] [span=1 session_loop]'
        turn = '[span=20 turn] ' + span
        raw = event('user_prompt', 'event.timestamp=2026-10-15T07:31:16.000Z', span)
        raw += event('websocket_request', 'duration_ms=4 websocket.warmup=true event.timestamp=2026-10-15T07:31:17.000Z')
        raw += event('sse_event', 'event.kind="response.completed" ttft_ms=3')
        raw += event('user_prompt', 'event.timestamp=2026-10-15T07:31:18.000Z', '[span=30 submission_dispatch]', session='background-secret')
        raw += event('turn_ttft', 'duration_ms=100', '[span=30 submission_dispatch]', session='background-secret')
        raw += event('websocket_request', 'duration_ms=72 websocket.warmup=false event.timestamp=2026-10-15T07:32:45.000Z', turn, session='primary-...')
        raw += event('turn_ttft', 'duration_ms=89256', turn)
        raw += event('sse_event', 'event.kind="response.completed" ttft_ms=960')
        report = parse_log_text(raw)
        primary, background = report['turns']
        self.assertEqual(primary['turn_ttft_ms'], 89256)
        self.assertEqual(primary['prompt_to_first_real_request_wall_ms'], 89000)
        self.assertEqual(len(primary['responses']), 1)
        self.assertEqual(primary['responses'][0]['sse_ttft_ms'], 960)
        self.assertEqual(background['session'], 'background_1')
        self.assertEqual(background['turn_ttft_ms'], 100)
        self.assertEqual(background['responses'], [])

    def test_missing_end_is_unknown_and_startup_keeps_line_numbers(self):
        raw = '[codex-startup] 1000 0.000 0.000 process_main\n'
        raw += '[codex-startup] 1 2.000 2.000 runtime_begin\n'
        raw += '[codex-startup] 1 5.000 3.000 runtime_done\n'
        raw += '[codex-startup] 1 6.000 1.000 thread_start_begin\n'
        raw += event('user_prompt', '', '[span=10 submission_dispatch]')
        raw += event('websocket_request', 'websocket.warmup=false duration_ms=0', '[span=10 submission_dispatch]')
        report = parse_log_text(raw)
        self.assertEqual(report['startup_markers'][1]['line'], 2)
        self.assertEqual(report['startup_spans'][0]['duration_ms'], 3)
        self.assertIsNone(report['startup_spans'][1]['duration_ms'])
        self.assertIsNone(report['turns'][0]['turn_ttft_ms'])
        self.assertIsNone(report['turns'][0]['responses'][0]['sse_ttft_ms'])
        self.assertEqual(report['turns'][0]['responses'][0]['request_duration_ms'], 0)

    def test_tool_arguments_cannot_impersonate_measurements_or_errors(self):
        raw = event('tool_result', 'duration_ms=5 tool_name=exec arguments=ENOSYS /apps/codex/codex-code-mode-host duration_ms=999 output=ok event.timestamp=2026-10-15T07:35:27.000Z')
        parsed = parse_log_text(raw)['events'][0]
        self.assertEqual(parsed['duration_ms'], 5)
        self.assertNotIn('error_class', parsed)
        self.assertNotIn('host_executable', parsed)

    def test_startup_pairs_do_not_cross_launch_pid_or_repeated_begin(self):
        raw = ('[codex-startup] 1 0 0 process_main\n'
               '[codex-startup] 1 10 10 runtime_begin\n'
               '[codex-startup] 1 0 0 process_main\n'
               '[codex-startup] 1 20 20 runtime_done\n'
               '[codex-startup] 1 25 5 thread_start_begin\n'
               '[codex-startup] 2 30 30 thread_start_done\n'
               '[codex-startup] 1 40 15 runtime_begin\n'
               '[codex-startup] 1 45 5 runtime_begin\n'
               '[codex-startup] 1 50 5 runtime_done\n')
        report = parse_log_text(raw)
        spans = report['startup_spans']
        self.assertEqual([s['duration_ms'] for s in spans], [None, None, None, 5])
        self.assertEqual([r['process_main_observed'] for r in report['startup_runs']], [True, True])
        self.assertEqual(spans[0]['run'], 1)
        self.assertEqual(spans[-1]['run'], 2)

    def test_partial_startup_and_elapsed_reset_remain_explicit(self):
        raw = ('[codex-startup] 1 100 10 runtime_begin\n'
               '[codex-startup] 1 5 5 runtime_done\n')
        report = parse_log_text(raw)
        self.assertEqual([r['boundary'] for r in report['startup_runs']], ['partial_capture', 'elapsed_reset'])
        self.assertFalse(any(r['process_main_observed'] for r in report['startup_runs']))
        self.assertIsNone(report['startup_spans'][0]['duration_ms'])

    def test_capture_timeline_unions_overlap_and_excludes_private_fields(self):
        raw = ''
        for phase, offset, duration in [('instructions', 0, 60), ('capability_discovery', 20, 50), ('build_router', 80, 10), ('capture', 0, 100)]:
            line = event('step_context_phase', f'capture_id=18446744073709551615 phase={phase} offset_ms={offset} duration_ms={duration} status=complete plugin_path=PATH_SECRET prompt=PROMPT_SECRET')
            raw += line + line.replace('codex_otel.log_only:', 'codex_otel.trace_safe:')
        raw += event('step_context_phase', 'capture_id=18446744073709551615 phase=PRIVATE_PHASE offset_ms=0 duration_ms=10 status=PRIVATE_STATUS')
        raw += event('step_context_phase', 'capture_id=3 phase=mcp_ready offset_ms=1 duration_ms=2 status=cancelled')
        report = parse_log_text(raw)
        capture, unfinished = report['captures']
        self.assertEqual(len(capture['phases']), 4)
        self.assertEqual(capture['unassigned_intervals_ms'], [[70, 80], [90, 100]])
        self.assertEqual(capture['duration_ms'], 100)
        self.assertIsNone(unfinished['duration_ms'])
        self.assertIsNone(unfinished['unassigned_intervals_ms'])
        self.assertEqual(unfinished['phases'][0]['status'], 'cancelled')
        for output in (json.dumps(report), markdown(report)):
            for secret in ('PRIVATE_PHASE', 'PRIVATE_STATUS', 'PATH_SECRET', 'PROMPT_SECRET', 'ACCOUNT_SECRET'):
                self.assertNotIn(secret, output)

    def test_capture_ids_are_scoped_to_startup_run(self):
        phase = event('step_context_phase', 'capture_id=1 phase=capture offset_ms=0 duration_ms=5 status=complete')
        raw = '[codex-startup] 1 0 0 process_main\n' + phase
        raw += '[codex-startup] 1 0 0 process_main\n' + phase
        report = parse_log_text(raw)
        self.assertEqual([c['run'] for c in report['captures']], [1, 2])

    def test_cli_json_and_markdown(self):
        script = Path(__file__).with_name('codex_phase_report.py')
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'synthetic.log'
            path.write_text(event('startup_phase', 'startup.phase="startup_prewarm_build_tools" duration_ms=62309'))
            result = subprocess.run([sys.executable, str(script), str(path)], capture_output=True, text=True, check=True)
            self.assertEqual(json.loads(result.stdout)['events'][0]['duration_ms'], 62309)
            result = subprocess.run([sys.executable, str(script), str(path), '--format', 'markdown'], capture_output=True, text=True, check=True)
            self.assertIn('startup_prewarm_build_tools', result.stdout)
            self.assertNotIn('ACCOUNT_SECRET', result.stdout)


if __name__ == '__main__':
    unittest.main()
