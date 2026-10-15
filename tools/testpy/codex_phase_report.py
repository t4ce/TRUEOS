#!/usr/bin/env python3
"""Offline Codex timing audit. Never emits raw log lines or arbitrary field values.

Only codex_otel.log_only is canonical; trace_safe mirrors are ignored. All output
strings are fixed enums, validated timestamps, or explicitly allowed executable
paths. Conversation identifiers are used only transiently to assign local labels.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import json
from pathlib import Path
import re

EVENTS = frozenset("api_request conversation_starts startup_phase step_context_phase websocket_connect websocket_request user_prompt sse_event turn_ttft tool_result".split())
STEP_PHASES = frozenset("capture active_turn_snapshot environment_ready instructions capability_roots capability_discovery mcp_ready recommendations selected_contributors build_router conversation_snapshot retain_context".split())
PHASES = frozenset("thread_start_create_thread thread_start_total startup_prewarm_create_turn_context startup_prewarm_build_tools startup_prewarm_build_prompt startup_prewarm_websocket_warmup startup_prewarm_total startup_prewarm_resolve".split())
STAGES = frozenset("account_status_begin account_status_done app_server_begin app_server_done arg0_cleanup_begin arg0_cleanup_done arg0_dispatch_done cli_begin cli_entering_tui cloud_auth_storage_begin cloud_auth_storage_done cloud_config_begin cloud_config_done crossterm_backend_created dotenv_begin dotenv_done editable_composer_visible first_frame_backend_flushed first_frame_render_begin first_frame_render_done full_config_begin full_config_done main_thread_entered main_thread_spawn_begin main_thread_spawn_done motion_begin motion_done path_aliases_begin path_aliases_done presentation_config_begin presentation_config_done process_main ratatui_terminal_begin ratatui_terminal_created runtime_begin runtime_done screen_reader_begin screen_reader_done session_prompt_ready state_db_begin state_db_done terminal_checks_begin terminal_checks_done terminal_init_begin terminal_init_done terminal_modes_begin terminal_modes_done terminal_queries_begin terminal_queries_done terminal_stderr_guard_done thread_start_begin thread_start_done tui_draft_begin tui_event_loop_created tui_preflight_begin".split())
SPAN_NAMES = frozenset("app_server.request app_server.thread_start.create_thread endpoint_session.execute_with list_models model_client.stream_responses_websocket model_client.websocket_connection responses_websocket.stream_request session_init session_loop submission_dispatch thread_spawn turn".split())
HOST_PATHS = frozenset({"/apps/codex/codex-code-mode-host"})
FIELD = re.compile(r'(?<![\w.])([\w.]+)=("(?:\\.|[^"\\])*"|[^\s\]]+)')
MARKER = re.compile(r'\[codex-startup\]\s+(\d+)\s+(\d+(?:\.\d+)?)\s+(\d+(?:\.\d+)?)\s+([a-z_0-9]+)\s*$')
TIMESTAMP = re.compile(r'\d{4}-\d\d-\d\dT\d\d:\d\d:\d\d(?:\.\d+)?Z\Z')


def _fields(line: str) -> dict[str, str]:
    # These values stay internal. Never return this dict as report data.
    # Tracing can print multiword payloads without quotes. Exclude their entire
    # region so payload text cannot impersonate telemetry fields.
    payload = re.search(r'\s(?:arguments|prompt)=', line)
    if payload:
        tail = line.rfind(' event.timestamp=')
        line = line[:payload.start()] + (line[tail:] if tail > payload.start() else '')
    return {k: v[1:-1] if v.startswith('"') else v for k, v in FIELD.findall(line)}


def _number(value: str | None) -> int | None:
    if value and re.fullmatch(r'\d{1,20}', value) and int(value) <= 2**64 - 1:
        return int(value)
    return None


def _ms(timestamp: str) -> float:
    return datetime.fromisoformat(timestamp.replace('Z', '+00:00')).timestamp() * 1000


def parse_log_text(text: str) -> dict:
    markers, events = [], []
    startup_runs = []
    startup_run = 0
    pid_elapsed: dict[int, float] = {}
    sessions: dict[str, str] = {}
    span_sessions: dict[tuple[int, str], str] = {}
    for line_no, line in enumerate(text.splitlines(), 1):
        marker = MARKER.search(line)
        if marker and marker[4] in STAGES:
            pid, elapsed = int(marker[1]), float(marker[2])
            explicit_start = marker[4] == 'process_main'
            reset = pid in pid_elapsed and elapsed < pid_elapsed[pid]
            if not startup_run or explicit_start or reset:
                startup_run += 1
                pid_elapsed.clear()
                span_sessions.clear()
                startup_runs.append(dict(run=startup_run, first_line=line_no, process_main_observed=explicit_start, boundary='process_main' if explicit_start else 'elapsed_reset' if reset else 'partial_capture'))
            pid_elapsed[pid] = elapsed
            markers.append(dict(line=line_no, run=startup_run, pid=pid, elapsed_ms=elapsed, delta_ms=float(marker[3]), stage=marker[4]))
        if 'codex_otel.log_only:' not in line:
            continue
        fields = _fields(line.split('codex_otel.log_only:', 1)[1])
        name = fields.get('event.name', '').removeprefix('codex.')
        if name not in EVENTS:
            continue
        if name == 'step_context_phase' and fields.get('phase') not in STEP_PHASES:
            continue
        event = dict(line=line_no, event='codex.' + name)
        if name == 'step_context_phase':
            event.update(run=startup_run or None, phase=fields['phase'])
            for key in ('capture_id', 'offset_ms'):
                value = _number(fields.get(key))
                if value is not None:
                    event[key] = value
            if fields.get('status') in {'complete', 'cancelled', 'error'}:
                event['status'] = fields['status']
        event['spans'] = [dict(id=int(sid), name=sname) for sid, sname in re.findall(r'\[span=(\d+) ([\w.]+)', line) if sname in SPAN_NAMES]
        conversation = fields.get('conversation.id')
        if conversation and not conversation.endswith(('...', '…')):
            if conversation not in sessions:
                sessions[conversation] = 'primary' if not sessions else f'background_{len(sessions)}'
            event['session'] = sessions[conversation]
        else:
            candidates = {span_sessions[(s['id'], s['name'])] for s in event['spans'] if (s['id'], s['name']) in span_sessions}
            event['session'] = candidates.pop() if len(candidates) == 1 else 'unassigned'
        if event['session'] != 'unassigned':
            for span in event['spans']:
                span_sessions[(span['id'], span['name'])] = event['session']
        timestamp = fields.get('event.timestamp', '')
        if TIMESTAMP.fullmatch(timestamp):
            try:
                _ms(timestamp)
                event['timestamp'] = timestamp
            except ValueError:
                pass
        for key in ('duration_ms', 'ttft_ms'):
            value = _number(fields.get(key))
            if value is not None:
                event[key] = value
        if fields.get('startup.phase') in PHASES:
            event['phase'] = fields['startup.phase']
        if fields.get('startup.status') in {'ready', 'error', 'cancelled'}:
            event['status'] = fields['startup.status']
        if fields.get('success') in {'true', 'false'}:
            event['success'] = fields['success'] == 'true'
        if name == 'websocket_request':
            event['request_kind'] = {'true': 'warmup', 'false': 'real'}.get(fields.get('websocket.warmup'), 'unknown')
        if name == 'sse_event' and fields.get('event.kind') in {'response.completed', 'response.created', 'response.output_text.delta'}:
            event['kind'] = fields['event.kind']
        if name == 'tool_result':
            if fields.get('tool_name') in {'exec', 'exec_command', 'write_stdin'}:
                event['tool'] = fields['tool_name']
            output = re.search(r'\soutput=(.*?)(?= mcp_server=| event.timestamp=|$)', line)
            diagnostic = output[1] if output else ''
            if re.search(r'\bENOSYS\b|\berrno 38\b|\bos error 38\b', diagnostic):
                event['error_class'] = 'ENOSYS'
            for host in HOST_PATHS:
                if re.search(r'(?<![\w/.-])' + re.escape(host) + r'(?![\w/.-])', diagnostic):
                    event['host_executable'] = host
        events.append(event)

    # These are marker-to-marker elapsed measurements, not exact critical-path
    # attribution. A run boundary, PID change, or repeated begin cannot supply a
    # missing end from another invocation.
    startup_spans = []
    pending = {}
    for marker in markers:
        stage = marker['stage']
        if stage.endswith('_begin'):
            span = dict(run=marker['run'], pid=marker['pid'], stage=stage[:-6], begin_line=marker['line'], end_line=None, duration_ms=None)
            startup_spans.append(span)
            pending[(marker['run'], marker['pid'], stage[:-6])] = (span, marker['elapsed_ms'])
        elif stage.endswith('_done'):
            pair = pending.pop((marker['run'], marker['pid'], stage[:-5]), None)
            if pair:
                span, begin_ms = pair
                span.update(end_line=marker['line'], duration_ms=marker['elapsed_ms'] - begin_ms)

    turns = []
    for prompt in (e for e in events if e['event'] == 'codex.user_prompt'):
        submission = next((s['id'] for s in prompt['spans'] if s['name'] == 'submission_dispatch'), None)
        boundary = next((e['line'] for e in events if e['event'] == 'codex.user_prompt' and e['session'] == prompt['session'] and e['line'] > prompt['line']), float('inf'))
        following = [e for e in events if prompt['line'] < e['line'] < boundary and e['session'] == prompt['session']]
        related = [e for e in following if submission is not None and any(s['id'] == submission and s['name'] == 'submission_dispatch' for s in e['spans'])]
        real = [e for e in related if e.get('request_kind') == 'real']
        ttft = next((e for e in related if e['event'] == 'codex.turn_ttft'), None)
        responses = []
        for index, request in enumerate(real):
            next_line = real[index + 1]['line'] if index + 1 < len(real) else boundary
            completion = next((e for e in following if request['line'] < e['line'] < next_line and e['event'] == 'codex.sse_event' and e.get('kind') == 'response.completed'), None)
            responses.append(dict(request_line=request['line'], request_duration_ms=request.get('duration_ms'), completion_line=completion['line'] if completion else None, sse_ttft_ms=completion.get('ttft_ms') if completion else None))
        first_request = real[0] if real else None
        gap = _ms(first_request['timestamp']) - _ms(prompt['timestamp']) if first_request and 'timestamp' in first_request and 'timestamp' in prompt else None
        turns.append(dict(session=prompt['session'], prompt_line=prompt['line'], submission_span=submission, turn_ttft_ms=ttft.get('duration_ms') if ttft else None, prompt_to_first_real_request_wall_ms=gap, responses=responses))
    captures = {}
    for event in events:
        if event['event'] != 'codex.step_context_phase' or 'capture_id' not in event:
            continue
        key = (event['run'], event['session'], event['capture_id'])
        capture = captures.setdefault(key, dict(run=event['run'], session=event['session'], capture_id=event['capture_id'], duration_ms=None, status=None, phases=[], unassigned_intervals_ms=None))
        offset, duration = event.get('offset_ms'), event.get('duration_ms')
        capture['phases'].append(dict(line=event['line'], phase=event['phase'], offset_ms=offset, duration_ms=duration, end_ms=offset + duration if offset is not None and duration is not None else None, status=event.get('status')))
        if event['phase'] == 'capture':
            capture.update(duration_ms=duration, status=event.get('status'))
    for capture in captures.values():
        capture['phases'].sort(key=lambda phase: (phase['offset_ms'] if phase['offset_ms'] is not None else float('inf'), phase['phase'] != 'capture', phase['line']))
        total = capture['duration_ms']
        if total is None:
            continue
        # Union child observations before computing uncovered intervals. Nested
        # and overlapping observations never imply additive latency attribution.
        intervals = sorted((max(0, p['offset_ms']), min(total, p['end_ms'])) for p in capture['phases'] if p['phase'] != 'capture' and p['offset_ms'] is not None and p['end_ms'] is not None and p['offset_ms'] < total)
        cursor, gaps = 0, []
        for start, end in intervals:
            if start > cursor:
                gaps.append([cursor, start])
            cursor = max(cursor, end)
        if cursor < total:
            gaps.append([cursor, total])
        capture['unassigned_intervals_ms'] = gaps
    return dict(schema=1, notes=[
        'Only canonical codex_otel.log_only events; trace_safe mirrors are ignored.',
        'Session labels follow first canonical conversation appearance; background labels do not prove purpose.',
        'Truncated conversation identifiers are resolved only through previously assigned numeric spans; otherwise unassigned.',
        'Wall timestamps in this capture have one-second granularity; cross-event differences are approximate.',
        'turn_ttft is end-to-end turn latency; SSE ttft is per response and is not equivalent.',
        'Response completions are correlated by session and line order between real requests, not by a logged response identifier.',
        'Measured spans overlap. Do not add them to infer a critical path.',
        'Prompt-to-request wall intervals are unassigned elapsed time, not a diagnosed cause; request duration measures sending, not full response latency.',
        'Missing ends and absent measurements are null, never zero.',
        'Startup run boundaries use process_main or a same-PID elapsed reset; captures without process_main are marked partial.',
        'Step-context offsets share one capture monotonic clock; overlapping child phases are unioned only to find unassigned intervals, never added as a critical path.',
        'Capture IDs are numeric and process-local; capture groups include observed startup run and session.',
    ], startup_runs=startup_runs, startup_markers=markers, startup_spans=startup_spans, events=events, turns=turns, captures=list(captures.values()))


def markdown(report: dict) -> str:
    lines = ['Codex timing audit', '', 'Startup markers:']
    lines += [f"- L{m['line']} run={m['run']} pid={m['pid']} {m['stage']}: elapsed {m['elapsed_ms']:g} ms, delta {m['delta_ms']:g} ms" for m in report['startup_markers']]
    lines += ['', 'Measured events:']
    for event in report['events']:
        details = {k: v for k, v in event.items() if k not in {'line', 'event', 'session', 'spans'}}
        lines.append(f"- L{event['line']} {event['session']} {event['event']}: {json.dumps(details, sort_keys=True)}")
    lines += ['', 'Turns:', json.dumps(report['turns'], indent=2), '', 'Step-context capture timelines:', json.dumps(report['captures'], indent=2), '', *report['notes']]
    return '\n'.join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('log', nargs='?', type=Path, default=Path('bld/baremetal-logs/LatestOfThree.logs'))
    parser.add_argument('--format', choices=('json', 'markdown'), default='json')
    args = parser.parse_args()
    try:
        report = parse_log_text(args.log.read_text(errors='replace'))
    except OSError:
        parser.exit(1, 'Could not read input log.\n')
    print(json.dumps(report, indent=2) if args.format == 'json' else markdown(report))


if __name__ == '__main__':
    main()
