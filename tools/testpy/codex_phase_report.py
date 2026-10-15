#!/usr/bin/env python3
"""Offline Codex timing audit. Never emits raw log lines or arbitrary field values.

Only codex_otel.log_only is canonical for OTel; fixed MCP/SQLx/kernel diagnostic
targets are accepted separately and trace_safe mirrors are ignored. All output
strings are fixed enums, validated timestamps, or explicitly allowed executable
paths. Conversation identifiers are used only transiently to assign local labels.
"""
from __future__ import annotations

import argparse
from datetime import datetime
import json
from pathlib import Path
import re

EVENTS = frozenset("api_request conversation_starts startup_phase step_context_phase mcp_phase websocket_connect websocket_request user_prompt sse_event turn_ttft tool_result".split())
MCP_PHASES = frozenset("prepare headers body first_body_byte json_decode first_sse_event operation".split())
MCP_METHODS = frozenset({"initialize", "notifications/initialized", "tools/list", "server/discover"})
MCP_STATES = frozenset({"started", "complete", "cancelled", "error"})
SQLX_SETUP_PHASES = frozenset("setup_total connect_headers service_ready connect_headers_retry upgrade open".split())
SQLX_RPC_PHASES = frozenset("serialize write read_header read_body parse complete".split())
SLICE_REASONS = frozenset({"Yield", "Sleep", "Wait", "Complete"})
SLICE_STOPS = frozenset({"End", "Bounds", "Unaligned", "NonIncreasing", "NonExecutable", "Limit"})
STEP_PHASES = frozenset("capture active_turn_snapshot environment_ready instructions capability_roots capability_discovery mcp_ready recommendations selected_contributors build_router conversation_snapshot retain_context turn_record_context turn_display_roots turn_skills_plugins turn_start_hooks turn_hooks_inputs sampling_base_instructions sampling_incremental_tools sampling_build_prompt sampling_metadata sampling_stream".split())
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
    payload = re.search(r'\s(?:arguments|prompt|sql|database|endpoint|url|authorization|output)=', line)
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


def _optional_number(value: str | None) -> int | None:
    if value and (match := re.fullmatch(r'Some\((\d{1,20})\)', value)):
        value = match[1]
    return _number(value)


def _sqlx_record(line: str, line_no: int, run: int) -> tuple[str, dict] | None:
    if 'sqlx_sqlite::remote_timing:' not in line:
        return None
    body = line.split('sqlx_sqlite::remote_timing:', 1)[1]
    fields = _fields(body)
    phase = fields.get('phase')
    if 'SQLite setup phase' in body and phase in SQLX_SETUP_PHASES:
        kind = 'setup'
    elif 'SQLite RPC timing' in body and phase in SQLX_RPC_PHASES:
        kind = 'rpc'
    else:
        return None
    connection = _number(fields.get('connection_id'))
    if connection is None:
        return None
    record = dict(line=line_no, run=run or None, connection_id=connection, phase=phase)
    for key in ('request_id', 'queued_ms', 'elapsed_ms'):
        value = _number(fields.get(key))
        if value is not None:
            record[key] = value
    if fields.get('completed') in {'true', 'false'}:
        record['completed'] = fields['completed'] == 'true'
    if kind == 'rpc':
        for key in ('serialize_ms', 'write_ms', 'header_ms', 'body_ms', 'parse_ms'):
            record[key] = _optional_number(fields.get(key))
    return kind, record


def _slice_record(line: str, line_no: int, run: int) -> dict | None:
    match = re.search(r'(?:^|\]\s+)codex-slice-exit\s+(vm=.*)', line)
    if not match:
        return None
    body = match[1]
    fields = _fields(body)
    if fields.get('reason') not in SLICE_REASONS or fields.get('stop') not in SLICE_STOPS or fields.get('kind') != 'exit-callstack' or fields.get('cpu_attribution') != 'none':
        return None
    record = dict(line=line_no, run=run or None, reason=fields['reason'], stop=fields['stop'], kind='exit-callstack', cpu_attribution='none')
    for key in ('vm', 'id', 'carrier', 'ended_ms', 'duration_ms'):
        value = _number(fields.get(key))
        if value is None:
            return None
        record[key] = value
    frames = re.search(r'(?<![\w.])frames=\[([^\]]*)\]', body)
    addresses = None
    if frames:
        values = [part.strip() for part in frames[1].split(',')] if frames[1].strip() else []
        if len(values) <= 12 and all(re.fullmatch(r'(?:0x)?[0-9a-fA-F]{1,16}', value) for value in values):
            addresses = [int(value, 16) for value in values]
    record['frames'] = addresses
    return record


def _mcp_captures(events: list[dict]) -> list[dict]:
    captures = {}
    pending = {}
    for event in events:
        if event['event'] != 'codex.mcp_phase' or 'sequence' not in event:
            continue
        key = (event['run'], event['sequence'], event['layer'], event['method'])
        capture = captures.setdefault(key, dict(run=event['run'], sequence=event['sequence'], layer=event['layer'], method=event['method'], duration_ms=None, deadline_ms=None, phases=[]))
        if 'deadline_ms' in event:
            capture['deadline_ms'] = event['deadline_ms']
        phase_key = (key, event['phase'], event.get('offset_ms'))
        if event['state'] == 'started':
            phase = dict(phase=event['phase'], begin_line=event['line'], end_line=None, offset_ms=event.get('offset_ms'), duration_ms=None, end_ms=None, state='started', bytes=None, status_code=event.get('status_code'))
            capture['phases'].append(phase)
            pending[phase_key] = phase
            continue
        phase = pending.pop(phase_key, None)
        if phase is None:
            phase = dict(phase=event['phase'], begin_line=None, offset_ms=event.get('offset_ms'))
            capture['phases'].append(phase)
        duration, offset = event.get('duration_ms'), event.get('offset_ms')
        phase.update(end_line=event['line'], duration_ms=duration, end_ms=offset + duration if offset is not None and duration is not None else None, state=event['state'], bytes=event.get('bytes'), status_code=event.get('status_code'))
        if event['layer'] == 'operation' and event['phase'] == 'operation':
            capture['duration_ms'] = duration
    return list(captures.values())


def _sqlx_connections(setup: list[dict], rpc: list[dict]) -> list[dict]:
    connections = {}
    for kind, records in (('setup', setup), ('rpc', rpc)):
        for record in records:
            key = (record['run'], record['connection_id'])
            connection = connections.setdefault(key, dict(run=record['run'], connection_id=record['connection_id'], setup_duration_ms=None, setup_completed=None, rpc_records=0, max_queue_ms=None, max_wire_ms=None))
            if kind == 'setup' and record['phase'] == 'setup_total':
                connection.update(setup_duration_ms=record.get('elapsed_ms'), setup_completed=record.get('completed'))
            elif kind == 'rpc':
                connection['rpc_records'] += 1
                for source, target in (('queued_ms', 'max_queue_ms'), ('elapsed_ms', 'max_wire_ms')):
                    value = record.get(source)
                    if value is not None:
                        connection[target] = max(value, connection[target] if connection[target] is not None else value)
    return list(connections.values())


def _slice_summaries(records: list[dict]) -> list[dict]:
    groups = {}
    for record in records:
        key = (record['run'], record['vm'], record['id'], record['carrier'])
        group = groups.setdefault(key, dict(run=record['run'], vm=record['vm'], id=record['id'], carrier=record['carrier'], records=0, max_duration_ms=0, reasons=[]))
        group['records'] += 1
        group['max_duration_ms'] = max(group['max_duration_ms'], record['duration_ms'])
        if record['reason'] not in group['reasons']:
            group['reasons'].append(record['reason'])
    return list(groups.values())


def parse_log_text(text: str) -> dict:
    markers, events = [], []
    sqlx_setup, sqlx_rpc, slice_exits = [], [], []
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
        if 'codex_otel.trace_safe:' in line:
            continue
        if 'codex_otel.log_only:' in line:
            body = line.split('codex_otel.log_only:', 1)[1]
        elif 'codex_rmcp_client::phase:' in line:
            body = line.split('codex_rmcp_client::phase:', 1)[1]
            if _fields(body).get('event.name') != 'codex.mcp_phase':
                continue
        else:
            sqlx = _sqlx_record(line, line_no, startup_run)
            if sqlx:
                (sqlx_setup if sqlx[0] == 'setup' else sqlx_rpc).append(sqlx[1])
            trace = _slice_record(line, line_no, startup_run)
            if trace:
                slice_exits.append(trace)
            continue
        fields = _fields(body)
        name = fields.get('event.name', '').removeprefix('codex.')
        if name not in EVENTS:
            continue
        if name == 'step_context_phase' and fields.get('phase') not in STEP_PHASES:
            continue
        if name == 'mcp_phase' and (fields.get('phase') not in MCP_PHASES or fields.get('method') not in MCP_METHODS or fields.get('layer') not in {'http', 'operation'} or fields.get('state') not in MCP_STATES):
            continue
        event = dict(line=line_no, event='codex.' + name)
        if name == 'mcp_phase':
            event.update(run=startup_run or None, phase=fields['phase'], method=fields['method'], layer=fields['layer'], state=fields['state'])
            for key in ('sequence', 'offset_ms', 'bytes', 'deadline_ms'):
                value = _number(fields.get(key))
                if value is not None:
                    event[key] = value
            status = _number(fields.get('status_code'))
            event['status_code'] = status if status is not None and 100 <= status <= 599 else None
        if name == 'step_context_phase':
            event.update(run=startup_run or None, phase=fields['phase'])
            for key in ('capture_id', 'offset_ms'):
                value = _number(fields.get(key))
                if value is not None:
                    event[key] = value
            if fields.get('status') in {'started', 'complete', 'cancelled', 'error'}:
                event['status'] = fields['status']
        span_prefix = body.split('event.name=', 1)[0]
        event['spans'] = [dict(id=int(sid), name=sname) for sid, sname in re.findall(r'\[span=(\d+) ([\w.]+)', span_prefix) if sname in SPAN_NAMES]
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
        if name == 'step_context_phase' and event.get('status') == 'started':
            # A started-only observation has no measured duration. The wire's
            # zero is only a placeholder, never a completed zero-length phase.
            event.pop('duration_ms', None)
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
        'OTel uses canonical codex_otel.log_only; exact MCP/SQLx/kernel diagnostic schemas are separately allowlisted. trace_safe mirrors are ignored.',
        'Session labels follow first canonical conversation appearance; background labels do not prove purpose.',
        'Truncated conversation identifiers are resolved only through previously assigned numeric spans; otherwise unassigned.',
        'Wall timestamps in this capture have one-second granularity; cross-event differences are approximate.',
        'turn_ttft is end-to-end turn latency; SSE ttft is per response and is not equivalent.',
        'Response completions are correlated by session and line order between real requests, not by a logged response identifier.',
        'Measured spans overlap. Do not add them to infer a critical path.',
        'Prompt-to-request wall intervals are unassigned elapsed time, not a diagnosed cause; request duration measures sending, not full response latency.',
        'Missing ends and absent measurements are null, never zero.',
        'Step phase started records precede work and do not prove completion; their duration and end remain null until a terminal observation exists.',
        'Startup run boundaries use process_main or a same-PID elapsed reset; captures without process_main are marked partial.',
        'Step-context offsets share one capture monotonic clock; overlapping child phases are unioned only to find unassigned intervals, never added as a critical path.',
        'Capture IDs are numeric and process-local; capture groups include observed startup run and session.',
        'MCP HTTP and operation sequences are separate captures, not an inferred parent/child or additive critical path. Started-only phases have null duration.',
        'SQLx setup connection IDs and slow-RPC request IDs are process-local. Records lack an authoritative Codex session mapping; no session association is inferred.',
        'SQLx wire elapsed includes serialization and parsing; queued time is separately measured. Stages are not added to whole-turn latency. Fast RPC summaries are intentionally absent.',
        'Slice frames are bounded return addresses from the exit call stack, not sampled current RIPs or evidence of where CPU time was spent. These traces appear only after a continuation yields or finishes.',
        'Diagnostic groups use observed startup runs; without lifecycle markers, reused process-local IDs cannot be reliably separated across restarts.',
        'SQLx records carry no VM identity; connection summaries must not be treated as authoritative cross-process joins when several emitters share a capture.',
    ], startup_runs=startup_runs, startup_markers=markers, startup_spans=startup_spans, events=events, turns=turns, captures=list(captures.values()), mcp_captures=_mcp_captures(events), sqlx_setup=sqlx_setup, sqlx_rpc=sqlx_rpc, sqlx_connections=_sqlx_connections(sqlx_setup, sqlx_rpc), slice_exits=slice_exits, slice_summaries=_slice_summaries(slice_exits))


def markdown(report: dict) -> str:
    lines = ['Codex timing audit', '', 'Startup markers:']
    lines += [f"- L{m['line']} run={m['run']} pid={m['pid']} {m['stage']}: elapsed {m['elapsed_ms']:g} ms, delta {m['delta_ms']:g} ms" for m in report['startup_markers']]
    lines += ['', 'Measured events:']
    for event in report['events']:
        details = {k: v for k, v in event.items() if k not in {'line', 'event', 'session', 'spans'}}
        lines.append(f"- L{event['line']} {event['session']} {event['event']}: {json.dumps(details, sort_keys=True)}")
    lines += ['', 'Turns:', json.dumps(report['turns'], indent=2), '', 'Step-context capture timelines:', json.dumps(report['captures'], indent=2)]
    for label, key in [('MCP phase timelines', 'mcp_captures'), ('SQLx connection summaries', 'sqlx_connections'), ('SQLx setup phases', 'sqlx_setup'), ('SQLx RPC phases', 'sqlx_rpc'), ('Continuation exit-callstack summaries', 'slice_summaries'), ('Continuation exit call stacks', 'slice_exits')]:
        lines += ['', label + ':', json.dumps(report[key], indent=2)]
    lines += ['', *report['notes']]
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
