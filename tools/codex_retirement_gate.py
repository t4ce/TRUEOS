#!/usr/bin/env python3
"""Prove exact-incarnation retirement from a bounded, append-only capture.

Accept a frozen schema-2 startup summary with a hashed window and file identity,
or schema-1 codex_startup_report.py CLI JSON with capture_window. The latter
binds the currently opened file only after its raw window hash matches. Reports
from parse_lines alone lack a source window and cannot establish this proof.
Never connects to a rig or controls an application or collector.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import time
import uuid

MAX_BYTES = 16 * 1024 * 1024
MAX_LINE = 65536
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')
CONTEXT = re.compile(rb'\[codex-startup\]\s*\d+\t[\d.]+\t[\d.]+\tprocess_context\s+(.+)$')
PAIR = re.compile(rb'([a-z_][a-z0-9_]*)=([^\s]+)')
VM_PREFIX = re.compile(rb'\bvm(\d+)\s+codex_startup:')
GUEST = re.compile(rb'trueos-startup-v1\s+(.+)$')
PLACEMENT = re.compile(rb'thread-placement-config vm=(\d+) gen=(\d+)\b')
FS = re.compile(rb'vm-recovery: filesystem-drained vm=(\d+) generation=(\d+) fs_pending=(\d+) fs_streams=(\d+) async_fs_pending=(\d+)\s*$')
OFFLINE = re.compile(rb'hv: vm(\d+) lifecycle: offline native_jobs=(\d+) carrier=([a-z-]+) wait_queues_retired=(\d+) generation=(\d+)\s*$')
ASSIGNED = re.compile(rb'hv: vm(\d+) lifecycle: generation=(\d+) assigned slot=(\d+)\b')
RETAINED = re.compile(rb'hv: vm(\d+) lifecycle: resource release retained\b')

class GateError(Exception):
    pass


def values(blob):
    result = {}
    for key, value in PAIR.findall(blob):
        key, value = key.decode('ascii'), value.decode('ascii')
        if key in result and result[key] != value:
            raise GateError('conflicting_duplicate_fields')
        result[key] = value
    return result


def context(blob):
    value = values(blob)
    result = {key: value[key] for key in ('request', 'app_gen', 'instance', 'archive_sha256')}
    result['request'], result['app_gen'] = int(result['request']), int(result['app_gen'])
    if result['request'] <= 0 or result['app_gen'] <= 0:
        raise GateError('invalid_application_identity')
    uuid.UUID(result['instance'])
    if not re.fullmatch(r'[0-9a-f]{64}', result['archive_sha256']):
        raise GateError('invalid_archive_identity')
    return result


def rows(raw, start):
    offset = start
    for line in raw.splitlines(keepends=True):
        if len(line) > MAX_LINE:
            raise GateError('collector_line_limit_exceeded')
        yield offset, ANSI.sub(b'', line.rstrip(b'\r\n'))
        offset += len(line)


def normalize_source(document):
    if document.get('schema') == 2:
        return document, 'startup_summary_v2'
    if document.get('schema') == 1 and 'application_runs' in document:
        if not isinstance(document.get('capture_window'), dict):
            raise GateError('requires_bounded_startup_report')
        return {'schema': 2,
                'applications': document['application_runs'],
                'window': document['capture_window']}, 'startup_report_v1'
    raise GateError('requires_startup_summary_or_bounded_report')


def launch_proof(summary, raw):
    apps = summary.get('applications', [])
    if summary.get('schema') != 2 or len(apps) != 1:
        raise GateError('requires_one_application_v2_summary')
    identity = apps[0]['context']
    contexts, entries, placements = [], [], []
    for offset, line in rows(raw, summary['window']['start_offset']):
        match = CONTEXT.search(line)
        if match:
            prefix = VM_PREFIX.search(line)
            if not prefix:
                raise GateError('context_missing_vm_prefix')
            contexts.append((offset, context(match[1]), int(prefix[1])))
        match = GUEST.search(line)
        if match:
            value = values(match[1])
            if value.get('request') == str(identity['request']) and value.get('phase') == 'guest_entry':
                entries.append((offset, value))
        match = PLACEMENT.search(line)
        if match:
            placements.append((offset, int(match[1]), int(match[2])))
    if len(contexts) != 1 or contexts[0][1] != identity:
        raise GateError('source_context_ambiguous_or_mismatch')
    begins = [(offset, value) for offset, value in entries if value.get('event') == 'begin']
    ends = [(offset, value) for offset, value in entries if value.get('event') == 'end' and value.get('status') == 'ok']
    if len(begins) != 1 or len(ends) != 1:
        raise GateError('source_guest_entry_incomplete_or_ambiguous')
    begin, end = begins[0], ends[0]
    vm, generation = int(end[1]['vm']), int(end[1]['gen'])
    if generation <= 0 or (begin[1]['vm'], begin[1]['gen']) != (str(vm), str(generation)):
        raise GateError('source_guest_entry_generation_mismatch')
    if not begin[0] < end[0] < contexts[0][0] or contexts[0][2] != vm:
        raise GateError('source_guest_entry_context_order_mismatch')
    if any(offset > contexts[0][0] and v == vm and g != generation for offset, v, g in placements):
        raise GateError('ambiguous_or_newer_placement_in_proof')
    return {'context': identity, 'vm': vm, 'generation': generation,
            'process_context_offset': contexts[0][0], 'guest_entry_begin_offset': begin[0],
            'guest_entry_end_offset': end[0],
            'placement_offsets': [o for o, v, g in placements if (v, g) == (vm, generation)]}


class RetirementState:
    def __init__(self, proof):
        self.proof = proof
        self.filesystem = None
        self.offline = None
        self.seen_context = False

    def accept(self, offset, line):
        p = self.proof
        if offset < p['process_context_offset']:
            return
        if b'Entering TrueOS kernel' in line:
            raise GateError('newer_boot_after_target_launch')
        match = CONTEXT.search(line)
        if match:
            if context(match[1]) != p['context'] or self.seen_context:
                raise GateError('newer_or_duplicate_application_context')
            self.seen_context = True
        match = ASSIGNED.search(line)
        if match and int(match[1]) == p['vm'] and int(match[2]) != p['generation']:
            raise GateError('target_vm_reassigned_to_another_generation')
        match = PLACEMENT.search(line)
        if match and int(match[1]) == p['vm'] and int(match[2]) != p['generation']:
            raise GateError('target_vm_reused_by_another_generation')
        match = GUEST.search(line)
        if match:
            value = values(match[1])
            if value.get('vm') == str(p['vm']) and value.get('gen') != str(p['generation']):
                raise GateError('target_vm_reused_by_another_guest_entry')
        match = RETAINED.search(line)
        if match and int(match[1]) == p['vm']:
            raise GateError('target_resource_release_retained')
        match = FS.search(line)
        if match and (int(match[1]), int(match[2])) == (p['vm'], p['generation']):
            if any(int(match[i]) != 0 for i in (3, 4, 5)):
                raise GateError('filesystem_drain_not_zero')
            if self.filesystem is not None:
                raise GateError('duplicate_filesystem_drain')
            self.filesystem = {'offset': offset, 'vm': p['vm'], 'generation': p['generation'],
                               'fs_pending': 0, 'fs_streams': 0, 'async_fs_pending': 0}
        match = OFFLINE.search(line)
        if match and (int(match[1]), int(match[5])) == (p['vm'], p['generation']):
            if int(match[2]) != 0 or match[3] != b'released':
                raise GateError('offline_owner_not_drained_or_carrier_retained')
            if self.filesystem is None or self.filesystem['offset'] >= offset:
                raise GateError('offline_missing_prior_filesystem_drain')
            if self.offline is not None:
                raise GateError('duplicate_exact_offline')
            self.offline = {'offset': offset, 'vm': p['vm'], 'generation': p['generation'],
                            'native_jobs': 0, 'carrier': 'released',
                            'wait_queues_retired': int(match[4])}


def observe(summary_path, expected_sha256, capture_path, timeout=60.0, poll=0.1, end_offset=None):
    started = time.monotonic()
    result = {'schema': 1, 'success': False, 'read_only': True,
              'summary_path': str(summary_path), 'expected_sha256': expected_sha256,
              'observation_kind': 'frozen_window' if end_offset is not None else 'append_follow',
              'observation_started_epoch': time.time(), 'timeout_seconds': timeout,
              'limits': ['No rig connection, stop, launch, or collector control is performed.',
                         'Success proves exact-generation filesystem/native drain and carrier release at the frozen collector endpoint.',
                         'The caller must serialize launch/control operations; this observer does not lock a future launcher.',
                         'The capture must be append-only; file identity and size checks do not detect arbitrary in-place rewrites.',
                         'Collector windows are bounded to 16 MiB, with 64 KiB maximum line length.',
                         'A failed pre-runtime application may retire; runtime/ready markers are not prerequisites.']}
    if end_offset is not None:
        result['limits'].append('A frozen replay proves only its historical endpoint, not that a launch is safe now.')
    state, data, start, device, inode, end = None, b'', None, None, None, None
    try:
        summary_bytes = Path(summary_path).read_bytes()
        result['summary_sha256'] = hashlib.sha256(summary_bytes).hexdigest()
        summary, source_kind = normalize_source(json.loads(summary_bytes))
        result['source_kind'] = source_kind
        window = summary['window']
        start, proof_end = window['start_offset'], window['end_offset']
        if not isinstance(start, int) or not isinstance(proof_end, int) or not 0 <= start < proof_end:
            raise GateError('invalid_proof_offsets')
        if proof_end - start > MAX_BYTES:
            raise GateError('proof_window_limit_exceeded')
        if window.get('bytes', proof_end-start) != proof_end-start:
            raise GateError('proof_window_byte_count_mismatch')
        with Path(capture_path).open('rb') as capture:
            st = os.fstat(capture.fileno())
            if source_kind == 'startup_summary_v2':
                device, inode = window['file_device'], window['file_inode']
            else:
                device, inode = st.st_dev, st.st_ino
            if (st.st_dev, st.st_ino) != (device, inode) or st.st_size < proof_end:
                raise GateError('collector_identity_changed_or_truncated')
            if end_offset is not None and (type(end_offset) is not int
                    or not proof_end <= end_offset <= st.st_size):
                raise GateError('invalid_frozen_end_offset')
            capture.seek(start)
            raw = capture.read(proof_end-start)
            if len(raw) != proof_end-start or hashlib.sha256(raw).hexdigest() != window['sha256']:
                raise GateError('proof_source_hash_mismatch')
            proof = launch_proof(summary, raw)
            if proof['context']['archive_sha256'] != expected_sha256:
                raise GateError('archive_mismatch')
            result['launch_proof'] = proof
            state = RetirementState(proof)
            # Parse the exact prefix whose hash was verified, then follow only
            # appended bytes on this same descriptor.
            data = raw
            consumed = 0
            while True:
                st, path_stat = os.fstat(capture.fileno()), Path(capture_path).stat()
                if ((st.st_dev, st.st_ino) != (device, inode)
                        or (path_stat.st_dev, path_stat.st_ino) != (device, inode)
                        or st.st_size < max(start+len(data), end_offset or 0)):
                    raise GateError('collector_identity_changed_or_truncated')
                end = st.st_size if end_offset is None else end_offset
                if end-start > MAX_BYTES:
                    raise GateError('collector_window_limit_exceeded')
                extra = capture.read(end-(start+len(data)))
                if len(extra) != end-(start+len(data)):
                    raise GateError('collector_truncated_during_snapshot')
                data += extra
                while True:
                    boundary = data.find(b'\n', consumed)
                    if boundary < 0:
                        if len(data)-consumed > MAX_LINE:
                            raise GateError('collector_line_limit_exceeded')
                        break
                    if boundary+1-consumed > MAX_LINE:
                        raise GateError('collector_line_limit_exceeded')
                    line = ANSI.sub(b'', data[consumed:boundary].rstrip(b'\r'))
                    state.accept(start+consumed, line)
                    consumed = boundary+1
                if state.offline is not None:
                    if consumed != len(data):
                        # Do not admit a launch based on a snapshot ending in an unparsed partial row.
                        if time.monotonic()-started >= timeout:
                            raise GateError('timeout_with_partial_collector_line')
                    else:
                        result['success'] = True
                        result['reason'] = 'exact_incarnation_fully_retired'
                        break
                if end_offset is not None:
                    raise GateError('frozen_window_missing_retirement_or_partial_line')
                if time.monotonic()-started >= timeout:
                    raise GateError('timeout_waiting_exact_incarnation_retirement')
                time.sleep(min(poll, max(0, timeout-(time.monotonic()-started))))
    except (GateError, KeyError, ValueError, TypeError, AttributeError, OSError) as exc:
        result['reason'] = str(exc) if isinstance(exc, GateError) else type(exc).__name__
    if state is not None:
        result['filesystem_drained'] = state.filesystem
        result['offline'] = state.offline
    if start is not None and end is not None:
        result['window'] = {'start_offset': start, 'end_offset': start+len(data),
                            'bytes': len(data), 'sha256': hashlib.sha256(data).hexdigest(),
                            'file_device': device, 'file_inode': inode}
    result['elapsed_ms'] = round((time.monotonic()-started)*1000, 3)
    result['observation_ended_epoch'] = time.time()
    return result


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--startup-summary', required=True, type=Path,
                        help='frozen schema-2 summary or bounded startup-report CLI JSON')
    parser.add_argument('--expected-sha256', required=True)
    parser.add_argument('--capture', required=True, type=Path,
                        help='append-only collector file containing the hashed source window')
    parser.add_argument('--output', required=True, type=Path,
                        help='new retirement receipt; existing files are never replaced')
    parser.add_argument('--timeout', type=float, default=60.0)
    parser.add_argument('--end-offset', type=int,
                        help='fixed exclusive byte endpoint for historical replay; does not follow appends')
    args = parser.parse_args(argv)
    if not re.fullmatch(r'[0-9a-f]{64}', args.expected_sha256):
        parser.error('invalid SHA256')
    if not 0 <= args.timeout <= 180:
        parser.error('timeout must be between 0 and 180 seconds')
    output = args.output
    if output.exists():
        parser.error('refusing to overwrite retirement receipt')
    result = observe(args.startup_summary, args.expected_sha256, args.capture, args.timeout,
                     end_offset=args.end_offset)
    try:
        with output.open('x') as handle:
            json.dump(result, handle, indent=2)
            handle.write('\n')
    except FileExistsError:
        parser.error('refusing to overwrite retirement receipt')
    except OSError:
        parser.error('cannot write retirement receipt')
    print(json.dumps({'output': str(output), **result}, indent=2))
    return 0 if result['success'] else 2

if __name__ == '__main__':
    raise SystemExit(main())
