#!/usr/bin/env python3
"""Turn cargo JSON diagnostics into a reviewable dead-code table.

Run the kernel with `cargo rustc --offline --locked --bin TRUEOS
--message-format=json -- --force-warn dead_code`, capturing stdout to JSONL.
Compiler findings apply only to that target and feature set, not all builds.
"""
import argparse
import collections
import csv
import json
import os
from pathlib import Path


def group(path):
    parts = Path(path).parts
    if len(parts) >= 3 and parts[0] == 'src':
        return '/'.join(parts[:2])
    if len(parts) >= 2 and parts[0] == 'src':
        return path
    if len(parts) >= 2 and parts[0] == 'crates':
        return '/'.join(parts[:2])
    return 'dependency/generated'


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('diagnostics', type=Path)
    p.add_argument('--out', type=Path, required=True, help='output filename prefix')
    args = p.parse_args()
    rows = []
    errors = []
    finished = None
    for line in args.diagnostics.read_text().splitlines():
        try:
            event = json.loads(line)
        except ValueError:
            continue
        if event.get('reason') == 'build-finished':
            finished = event['success']
        if event.get('reason') != 'compiler-message':
            continue
        message = event['message']
        lint = (message.get('code') or {}).get('code', '')
        if message['level'] == 'error':
            errors.append(message['message'])
        if lint not in {'dead_code', 'unused_imports', 'unused_variables', 'unreachable_code'}:
            continue
        spans = message.get('spans', [])
        primary = [s for s in spans if s['is_primary']]
        # One diagnostic may name several fields/methods; preserve each primary
        # location, plus a diagnostic ID so totals cannot be mistaken for item counts.
        for span in primary:
            path = os.path.normpath(span['file_name'])
            rows.append({'diagnostic': None,
                         'lint': lint, 'file': path, 'line': span['line_start'],
                         'column': span['column_start'], 'group': group(path),
                         'message': message['message'], 'label': span.get('label') or '',
                         'package': event.get('package_id', ''),
                         'source': '\n'.join(s['text'] for s in span.get('text', []))})
        # Assign the same ID to all primary spans from this message.
        for row in rows[-len(primary):] if primary else []:
            row['diagnostic'] = event.get('package_id', '') + ':' + str(message.get('rendered', message['message']))
    # Use compact stable IDs, deduplicating cached/repeated Cargo diagnostics.
    ids = {}
    unique = {}
    for row in rows:
        key = row['diagnostic']
        if key not in ids:
            ids[key] = len(ids) + 1
        row['diagnostic'] = ids[key]
        unique[(row['diagnostic'], row['file'], row['line'], row['column'])] = row
    rows = sorted(unique.values(), key=lambda r: (r['file'], r['line'], r['column']))
    counts = collections.defaultdict(set)
    owned_counts = collections.defaultdict(set)
    for row in rows:
        counts[row['lint']].add(row['diagnostic'])
        if row['file'].startswith(('src/', 'crates/')):
            owned_counts[row['lint']].add(row['diagnostic'])
    result = {'build_finished': finished, 'errors': errors,
              'diagnostic_counts': {k: len(v) for k, v in sorted(counts.items())},
              'owned_diagnostic_counts': {k: len(v) for k, v in sorted(owned_counts.items())},
              'primary_locations': len(rows), 'findings': rows}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix('.json').write_text(json.dumps(result, indent=2) + '\n')
    with args.out.with_suffix('.csv').open('w') as output:
        writer = csv.DictWriter(output, fieldnames=['diagnostic', 'lint', 'file', 'line', 'column', 'group', 'message', 'label', 'package', 'source'])
        writer.writeheader()
        writer.writerows(rows)
    grouped = collections.defaultdict(list)
    for row in rows:
        if row['lint'] == 'dead_code':
            grouped[row['group']].append(row)
    md = ['# TRUEOS current-source dead-code check — 2026-10-18', '',
          'Compiler check of the TRUEOS kernel binary using the repository target and default features. '
          '`--force-warn dead_code` overrides local `allow`/`expect` suppression for the kernel crate. '
          'Dependency crates retain their configured lint levels. No code was removed.', '',
          f'Build success: **{finished}**. Compiler errors: **{len(errors)}**.', '',
          '| Lint | TRUEOS-owned diagnostics | Including dependency diagnostics |', '| --- | ---: | ---: |']
    md += [f'| `{lint}` | {len(owned_counts[lint])} | {len(values)} |' for lint, values in sorted(counts.items())]
    md += ['', 'Diagnostics can contain multiple unused fields/methods. The CSV preserves each primary source location; '
           'diagnostic IDs allow accurate deduplication.', '', '## Overview by source area', '',
           '| Source area | Dead-code diagnostics | Locations | Examples |', '| --- | ---: | ---: | --- |']
    for area, items in sorted(grouped.items(), key=lambda pair: (-len({r['diagnostic'] for r in pair[1]}), pair[0])):
        examples = list(dict.fromkeys(r['message'] for r in items))[:3]
        example = '; '.join(examples).replace('|', '\\|').replace('\n', ' ')
        md.append(f"| `{area}` | {len({r['diagnostic'] for r in items})} | {len(items)} | {example} |")
    categories = collections.defaultdict(set)
    for row in rows:
        if row['lint'] != 'dead_code' or not row['file'].startswith(('src/', 'crates/')):
            continue
        message = row['message']
        if message.startswith('function '):
            category = 'Functions'
        elif 'method' in message or message.startswith(('associated function', 'associated items', 'multiple associated')):
            category = 'Methods / associated items'
        elif message.startswith(('field ', 'fields ')):
            category = 'Fields (check layout before removal)'
        elif message.startswith(('constant ', 'static ')):
            category = 'Constants / statics'
        else:
            category = 'Types / variants / other'
        categories[category].add(row['diagnostic'])
    md += ['', '## TRUEOS-owned findings by item kind', '', '| Kind | Diagnostics |', '| --- | ---: |']
    md += [f'| {kind} | {len(values)} |' for kind, values in sorted(categories.items())]
    md += ['', '## Interpretation', '',
           '- **Compiler dead code:** unused in this configured kernel build. Review individual rows before deletion.',
           '- **Unused fields/constants/types:** can represent ABI layout, hardware register maps, debug visibility, or reserved extension points. Check layout and external contracts.',
           '- **Unused private functions/methods:** stronger removal candidates; verify alternate features, tests, and platform configurations.',
           '- **Unused imports/variables and unreachable code:** separate lint categories, not interchangeable with dead implementation code.',
           '- **Externally visible exports and public library APIs:** may escape this lint. Use the separate Blueprint API census for exported surfaces.',
           '- **Excluded modules/features:** not compiled, so absence here is not evidence of use or non-use. This is not an all-features or linker reachability audit.', '',
           '## Reproduce', '', '```sh',
           'cargo rustc --offline --locked --bin TRUEOS --message-format=json -- --force-warn dead_code > /tmp/trueos-deadcode.jsonl 2> /tmp/trueos-deadcode.stderr',
           'python3 tools/audit/deadcode_report.py /tmp/trueos-deadcode.jsonl --out /tmp/trueos-deadcode', '```', '',
           'Full findings are in the adjacent CSV and JSON files. They include file, line, source text, package, and compiler message.', '']
    if errors:
        md += ['## Build errors', ''] + [f'- {e}' for e in errors]
    args.out.with_suffix('.md').write_text('\n'.join(md))
    print(json.dumps({k:v for k,v in result.items() if k != 'findings'}, indent=2))


if __name__ == '__main__':
    main()
