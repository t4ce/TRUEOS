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

    def test_post_capture_started_boundaries_survive_without_invented_completion(self):
        phases = ('turn_record_context turn_display_roots turn_skills_plugins '
                  'turn_start_hooks turn_hooks_inputs sampling_base_instructions '
                  'sampling_incremental_tools sampling_build_prompt sampling_metadata '
                  'sampling_stream').split()
        raw = event('conversation_starts', spans='[span=1 session_loop]')
        for sequence, phase in enumerate(phases, 10):
            line = event('step_context_phase',
                         f'capture_id={sequence} phase={phase} offset_ms=7 '
                         'duration_ms=0 status=started path=PATH_SECRET prompt=PROMPT_SECRET',
                         spans='[span=1 session_loop]')
            raw += line + line.replace('codex_otel.log_only:', 'codex_otel.trace_safe:')
        raw += event('step_context_phase',
                     'capture_id=10 phase=turn_record_context offset_ms=7 '
                     'duration_ms=53 status=complete')
        report = parse_log_text(raw)
        self.assertEqual(len(report['captures']), len(phases))
        started = [e for e in report['events'] if e.get('status') == 'started']
        self.assertEqual(len(started), len(phases))
        self.assertTrue(all('duration_ms' not in e for e in started))
        for capture in report['captures']:
            first = capture['phases'][0]
            self.assertEqual(first['status'], 'started')
            self.assertIsNone(first['duration_ms'])
            self.assertIsNone(first['end_ms'])
        terminal = report['captures'][0]['phases'][1]
        self.assertEqual(terminal['duration_ms'], 53)
        self.assertEqual(terminal['end_ms'], 60)
        self.assertEqual(terminal['status'], 'complete')
        for output in (json.dumps(report), markdown(report)):
            for secret in ('PATH_SECRET', 'PROMPT_SECRET', 'ACCOUNT_SECRET', 'EMAIL_SECRET'):
                self.assertNotIn(secret, output)

    def test_mcp_exact_target_whitelist_and_missing_phase_end(self):
        def mcp(sequence, phase, state, offset=0, duration=0, method='tools/list', layer='http'):
            return (f'[apps] [info] codex_rmcp_client::phase: event.name="codex.mcp_phase" sequence={sequence} '
                    f'layer="{layer}" method="{method}" phase="{phase}" state="{state}" offset_ms={offset} '
                    f'duration_ms={duration} bytes=12 deadline_ms=30000 status_code=200 '
                    'account_id=ACCOUNT_SECRET url="https://URL_SECRET" body="BODY_SECRET"\n')
        raw = mcp(1, 'headers', 'started') + mcp(1, 'headers', 'cancelled', duration=30000)
        raw += mcp(1, 'body', 'started', offset=30000)
        raw += mcp(2, 'operation', 'started', layer='operation')
        raw += mcp(2, 'operation', 'error', duration=30001, layer='operation')
        raw += mcp(3, 'headers', 'complete', method='tools/call/PRIVATE_METHOD')
        raw += mcp(4, 'PRIVATE_PHASE', 'complete')
        raw += mcp(5, 'headers', 'PRIVATE_STATE')
        raw += mcp(6, 'headers', 'complete', layer='PRIVATE_LAYER')
        raw += event('mcp_phase', 'sequence=1 layer=http method=tools/list phase=headers state=cancelled duration_ms=30000', target='trace_safe')
        raw += mcp(7, 'headers', 'complete').replace('codex_rmcp_client::phase:', 'arbitrary_target:')
        report = parse_log_text(raw)
        http, operation = report['mcp_captures']
        self.assertEqual(len(report['events']), 5)
        self.assertEqual(http['phases'][0]['duration_ms'], 30000)
        self.assertEqual(http['phases'][0]['state'], 'cancelled')
        self.assertEqual(http['phases'][0]['end_line'], 2)
        self.assertIsNone(http['phases'][1]['duration_ms'])
        self.assertIsNone(http['duration_ms'], 'HTTP phases do not imply a measured operation total')
        self.assertEqual(operation['duration_ms'], 30001)
        self.assertEqual(operation['layer'], 'operation')
        self.assertEqual(operation['phases'][0]['end_ms'], 30001)
        self.assertEqual((operation['phases'][0]['begin_line'], operation['phases'][0]['end_line']), (4, 5))
        for output in (json.dumps(report), markdown(report)):
            for private in ('ACCOUNT_SECRET', 'URL_SECRET', 'BODY_SECRET', 'PRIVATE_METHOD', 'PRIVATE_PHASE', 'PRIVATE_STATE', 'PRIVATE_LAYER'):
                self.assertNotIn(private, output)

    def test_sqlx_fixed_schema_optional_stages_and_connection_summary(self):
        target = '[apps] [info] sqlx_sqlite::remote_timing: '
        raw = target + 'SQLite setup phase connection_id=3 phase="connect_headers" elapsed_ms=20 completed=true\n'
        raw += target + 'SQLite setup phase connection_id=3 phase="setup_total" elapsed_ms=30000 completed=false\n'
        raw += target + 'SQLite RPC timing connection_id=3 request_id=12 phase="read_body" queued_ms=150 elapsed_ms=400 serialize_ms=Some(1) write_ms=Some(2) header_ms=Some(397) body_ms=None parse_ms=None completed=false sql="SQL_SECRET" database=DATABASE_SECRET endpoint=URL_SECRET error=ERROR_SECRET\n'
        raw += target + 'SQLite RPC timing connection_id=3 request_id=13 phase="complete" queued_ms=0 elapsed_ms=200 serialize_ms=0 write_ms=2 header_ms=100 body_ms=90 parse_ms=8 completed=true\n'
        raw += target + 'SQLite RPC timing connection_id=3 phase="PRIVATE_PHASE" elapsed_ms=1\n'
        raw += (target + 'SQLite setup phase connection_id=4 phase="open" elapsed_ms=999\n').replace('sqlx_sqlite::remote_timing:', 'arbitrary_target:')
        report = parse_log_text(raw)
        self.assertEqual(len(report['sqlx_setup']), 2)
        self.assertEqual(len(report['sqlx_rpc']), 2)
        rpc = report['sqlx_rpc'][0]
        self.assertEqual(rpc['serialize_ms'], 1)
        self.assertIsNone(rpc['body_ms'])
        self.assertIsNone(rpc['parse_ms'])
        summary = report['sqlx_connections'][0]
        self.assertEqual(summary['setup_duration_ms'], 30000)
        self.assertFalse(summary['setup_completed'])
        self.assertEqual((summary['max_queue_ms'], summary['max_wire_ms']), (150, 400))
        self.assertNotIn('session', summary)
        for output in (json.dumps(report), markdown(report)):
            for private in ('SQL_SECRET', 'DATABASE_SECRET', 'URL_SECRET', 'ERROR_SECRET', 'PRIVATE_PHASE'):
                self.assertNotIn(private, output)

    def test_slice_exit_numeric_frames_are_bounded_and_not_cpu_attribution(self):
        prefix = '[blueprint] [info] codex-slice-exit vm=1 id=2147483650 carrier=19 ended_ms=12345 duration_ms=18576 reason=Wait kind=exit-callstack cpu_attribution=none '
        raw = prefix + 'frames=[FFFFFFFF8033F4AB, 0x1234] stop=Bounds account_id=ACCOUNT_SECRET\n'
        raw += prefix + 'frames=[FRAME_SECRET] stop=NonExecutable\n'
        raw += prefix + 'frames=[' + ', '.join(['1234'] * 13) + '] stop=Limit\n'
        raw += (prefix + 'frames=[] stop=End\n').replace('reason=Wait', 'reason=PRIVATE_REASON')
        raw += (prefix + 'frames=[] stop=End\n').replace('cpu_attribution=none', 'cpu_attribution=PRIVATE_CPU')
        raw += (prefix + 'frames=[] stop=End\n').replace('stop=End', 'stop=PRIVATE_STOP')
        report = parse_log_text(raw)
        self.assertEqual(len(report['slice_exits']), 3)
        self.assertEqual(report['slice_exits'][0]['frames'], [0xFFFFFFFF8033F4AB, 0x1234])
        self.assertIsNone(report['slice_exits'][1]['frames'])
        self.assertIsNone(report['slice_exits'][2]['frames'])
        self.assertEqual(report['slice_summaries'][0]['max_duration_ms'], 18576)
        self.assertEqual(report['slice_summaries'][0]['records'], 3)
        for output in (json.dumps(report), markdown(report)):
            for private in ('ACCOUNT_SECRET', 'FRAME_SECRET', 'PRIVATE_REASON', 'PRIVATE_CPU', 'PRIVATE_STOP'):
                self.assertNotIn(private, output)

    def test_payload_text_cannot_impersonate_numeric_span_context(self):
        raw = event('user_prompt', 'prompt="PRIVATE [span=987654321 turn]"', '[span=5 submission_dispatch]')
        report = parse_log_text(raw)
        self.assertEqual(report['events'][0]['spans'], [{'id': 5, 'name': 'submission_dispatch'}])
        self.assertNotIn('987654321', json.dumps(report))

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
