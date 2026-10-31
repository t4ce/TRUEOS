#!/usr/bin/env python3
"""Read a bounded capture window; export only fixed native-FS numeric fields.

Use one boot per window: numeric launch/request IDs are scoped to a kernel boot.
Missing/expired observations never establish request completion.
"""
import argparse
import hashlib
import json
import os
import re

ROW_KEYS = frozenset(('vm','generation','launch','request','op','phase','status','queued_ms','service_ms','phase_ms','age_ms','full','missed','expired','released'))
COUNTER_KEYS = ('full','missed','expired','released')
SUMMARY_KEYS = frozenset(COUNTER_KEYS)
MARKER = re.compile(rb'\bcodex-fs-native(?P<summary>-summary)?\b(?P<fields>[^\n]*)')
FIELDS = re.compile(rb'(?:[ \t]+[a-z_]+=[0-9]+)+[ \t\r]*\Z')
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]')
MAX_LINE = 65_536
U64_MAX = (1 << 64)-1
PHASES = {1:'queued',2:'service',3:'filesystem',4:'result',5:'publish',6:'published',7:'retire',8:'finished',9:'range_scratch',10:'range_placement',11:'range_record_cache',12:'range_index',13:'range_read'}
OPS = {1:'prepare-volatile-files',2:'read-file',3:'read-file-len',4:'read-file-range',5:'stat',6:'typed-stat',7:'write-file-begin',8:'write-file-begin-typed',9:'create-dir-all',10:'write-file-chunk',11:'write-file-finish',12:'write-file-abort',13:'html-tree',14:'json-all',15:'list-dir',16:'typed-list-dir',17:'remove',18:'exists'}


def parse_line(line):
    match = MARKER.search(ANSI.sub(b'', line))
    if match is None:
        return None
    tail = match['fields']
    if FIELDS.fullmatch(tail) is None:
        return False
    pairs = [token.split(b'=', 1) for token in tail.split()]
    if len(pairs) != len({key for key, _ in pairs}):
        return False
    try:
        fields = {key.decode('ascii'):int(value) for key, value in pairs}
    except (ValueError, UnicodeError):
        return False
    expected = SUMMARY_KEYS if match['summary'] else ROW_KEYS
    if fields.keys() != expected or any(value > U64_MAX for value in fields.values()):
        return False
    if not match['summary'] and not (fields['vm'] < 64 and fields['launch'] > 0
            and 0 < fields['request'] < U64_MAX and fields['op'] in OPS
            and fields['phase'] in PHASES and fields['status'] <= 3
            and (fields['phase'] <= 8 or fields['op'] == 4)):
        return False
    return {'summary':bool(match['summary']),'fields':fields}


def read_window(stream, start, end, snapshot_size):
    if start < 0 or end < start or end > snapshot_size:
        raise ValueError('invalid byte window')
    stream.seek(start)
    remaining = end-start
    digest = hashlib.sha256()
    buffer = b''
    discard = False
    offset = start
    line_offset = start
    rows = []
    summaries = []
    malformed = 0
    oversized = 0
    def accept(line, at):
        nonlocal malformed
        parsed = parse_line(line)
        if parsed is False:
            malformed += 1
        elif parsed is not None:
            fields = {'offset':at, **parsed['fields']}
            (summaries if parsed['summary'] else rows).append(fields)
    while remaining:
        chunk = stream.read(min(65_536, remaining))
        if not chunk:
            raise ValueError('capture shrank during snapshot read')
        digest.update(chunk)
        remaining -= len(chunk)
        for piece in chunk.splitlines(keepends=True):
            ended = piece.endswith(b'\n') or piece.endswith(b'\r')
            if not discard:
                if len(buffer)+len(piece) > MAX_LINE:
                    buffer = b''
                    discard = True
                    oversized += 1
                else:
                    buffer += piece
            offset += len(piece)
            if ended:
                if not discard:
                    accept(buffer, line_offset)
                buffer = b''
                discard = False
                line_offset = offset
    if buffer and not discard:
        accept(buffer, line_offset)
    groups = {}
    for row in rows:
        key = tuple(row[name] for name in ('vm','generation','launch','request'))
        group = groups.setdefault(key, {'vm':key[0],'generation':key[1],'launch':key[2],
            'request':key[3],'op':row['op'],'op_name':OPS[row['op']], 'observations':[],
            'max_age_ms':0,'max_queued_ms':0,'max_service_ms':0,'max_phase_ms':0})
        group['observations'].append(row)
        for metric in ('age_ms','queued_ms','service_ms','phase_ms'):
            group['max_'+metric] = max(group['max_'+metric],row[metric])
        group['last_phase'] = row['phase']
        group['last_phase_name'] = PHASES[row['phase']]
        group['last_status'] = row['status']
    ordered = sorted(rows+summaries,key=lambda row:row['offset'])
    first = {key:ordered[0][key] for key in COUNTER_KEYS} if ordered else None
    last = {key:ordered[-1][key] for key in COUNTER_KEYS} if ordered else None
    decreased = any(later[key]<earlier[key] for earlier,later in zip(ordered,ordered[1:]) for key in COUNTER_KEYS)
    delta = {key:last[key]-first[key] for key in COUNTER_KEYS} if ordered and not decreased else None
    return {'schema':1,'capture_window':{'start_offset':start,'end_offset':end,
        'snapshot_size':snapshot_size,'bytes':end-start,'sha256':digest.hexdigest()},
        'rows':rows,'summaries':summaries,'requests':list(groups.values()),
        'counter_first':first,'counter_last':last,'counter_delta':delta,
        'counter_decrease_detected':decreased,'malformed_marker_lines':malformed,
        'oversized_lines_skipped':oversized,
        'qualifiers':['Use one boot per byte window; IDs can repeat after reboot.',
            'At most 16 pending observations, 128 selected attempts per measured launch, 180-second deadline and four records per five-second tick.',
            'Full/missed counters qualify incomplete coverage; expiration removes observation only and never cancels disk work.',
            'Released counts dropped observation owners; absence or a release count alone never proves disk completion.',
            'Rows are sampled snapshots; brief phases may be missed and BSP starvation may suppress samples.']}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('capture')
    parser.add_argument('--start-offset',type=int,default=0)
    parser.add_argument('--end-offset',type=int)
    args = parser.parse_args()
    try:
        with open(args.capture,'rb') as stream:
            size = os.fstat(stream.fileno()).st_size
            report = read_window(stream,args.start_offset,args.end_offset if args.end_offset is not None else size,size)
    except (OSError,ValueError):
        parser.exit(2,'cannot read the requested capture byte window\n')
    print(json.dumps(report,indent=2))


if __name__ == '__main__':
    main()
