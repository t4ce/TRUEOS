#!/usr/bin/env python3
"""Measure a reviewed shortlist against a fresh local_api_usage.py census.

This is a candidate audit, not a Rust call-graph or a deletion tool.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess

from local_api_usage import clean, FN_NAME, LEX, walk
from blueprint_vendor_usage import size


def candidate(name, exports, functions, tokens, note, modules=(), extra_symbols=()):
    return dict(name=name, exports=exports, functions=functions, tokens=tokens,
                note=note, modules=modules, extra_symbols=extra_symbols)


CANDIDATES = [
    candidate('Native scheduled audio engine',
              ['trueos_cabi_audio_native_render_v' + str(i) for i in (1, 2, 3)] +
              ['trueos_cabi_audio_native_sample_register_v1', 'trueos_cabi_audio_native_sample_remove_v1'],
              {'src/aud/cabi.rs': ['native_error_code'] +
               [p + str(i) for p in ('render_native_host_v', 'guest_native_render_v') for i in (1, 2, 3)]},
              ['NativeEngine', 'NativeRenderCommandV1', 'NativeRenderCommandV2', 'NativeRenderCommandV3',
               'OP_BP_AUDIO_NATIVE_RENDER_V1', 'OP_BP_AUDIO_NATIVE_RENDER_V2', 'OP_BP_AUDIO_NATIVE_RENDER_V3'],
              'No app wrapper or direct named VMCALL caller found. Engine callers are its CABI and three VMCALL dispatch arms. Remove those together; preserve PCM playback, HDA and the separate synth.',
              ['src/aud/native_engine.rs'], ['TRUEOS::aud::native_engine::']),
    candidate('Virtual gamepad automation station',
              ['trueos_cabi_gamepad_control_' + s for s in ('request', 'release', 'submit', 'submit_json', 'idle', 'snapshot')],
              {'src/r/io/input_cabi.rs': ['gamepad_control_principal'],
               'src/r/services/spawn_service.rs': ['spawn_gamepad_control_service_task']},
              ['VGamepad', 'GamepadControlCommand', 'GamepadControlSnapshot'],
              'No app producer found. Kernel references register/spawn the station and clean up owners; those keep it live without an app. Module explicitly says gamepads are not routed into UI4 yet. Preserve physical HID/gamepad support.',
              ['src/r/services/gamepad_control_service.rs'],
              ['TRUEOS::r::services::gamepad_control_service::']),
    candidate('TUN CABI and its private handle table',
              ['trueos_cabi_tun_' + s for s in ('open', 'close', 'send', 'recv')],
              {'src/r/net/socket_cabi.rs': ['tun_open_host', 'tun_close_host', 'tun_send_host', 'tun_recv_host',
                                          'next_tun_id', 'with_tun_mut', 'pump_tun']},
              ['Tun', 'tun_open', 'tun_close', 'tun_send', 'tun_recv'],
              'Only the SDK Tun wrapper and kernel re-export refer to the CABI. Private TUN helpers feed that interface. Preserve TCP sockets and the shared VNet/OpenTun implementation.',
              extra_symbols=['TRUEOS::r::net::socket_cabi::TUNS', 'TRUEOS::r::net::socket_cabi::TUN_SEQ']),
    candidate('Guest vVideoMem registration interface',
              ['trueos_cabi_vgpu_vvideo_' + s for s in ('create', 'flush', 'invalidate')],
              {'src/r/io/vgpu_cabi.rs': ['guest_vvideo_range', 'broker_vvideo_create', 'broker_vvideo_flush', 'broker_vvideo_invalidate'],
               'src/gpu/vgpu.rs': ['create_vvideo_mem', 'flush_vvideo_mem', 'invalidate_vvideo_mem']},
              ['VVideoMem', 'allocate_vvideo_mem', 'OP_BP_VGPU_VVIDEO_CREATE', 'OP_BP_VGPU_VVIDEO_FLUSH', 'OP_BP_VGPU_VVIDEO_INVALIDATE'],
              'RETAIN: current boat and TRUEOS-Picasso-Example sources use VVideoMem and allocate_vvideo_mem despite no import in their existing packaged artifacts. This is a concrete stale-package false positive.'),
    candidate('Legacy zkVM-shaped sys_* exports',
              ['sys_alloc_words', 'sys_getenv', 'sys_argc', 'sys_argv', 'sys_sha_compress'], {},
              ['sys_alloc_words', 'sys_getenv', 'sys_argc', 'sys_argv', 'sys_sha_compress'],
              'Four are kept looking used by std files selected only for target_os=zkvm. No kernel caller beyond resolver registration. SHA export has no consumer and only copies state; preserve sys_alloc_aligned, sys_rand, sys_read and sys_write.'),
    candidate('Legacy Konsole row/cursor/end-frame exports',
              ['trueos_cabi_konsole_' + s for s in ('write_row', 'set_cursor', 'end_frame')], {},
              ['konsole_write_row', 'konsole_set_cursor', 'konsole_end_frame'],
              'No app or dependency caller beyond trueos-v wrappers found. begin_frame IS imported and must remain. Frame state/helpers are shared with begin_frame and excluded from the estimate; removing these is only a partial interface cleanup.'),
    candidate('Vendored Rayon demo package', [], {}, ['rayon_demo'],
              'Example package absent from the previous 58 resolved app unit graphs; current source is still a workspace member, not a production dependency. Remove the member entry with the directory; retain rayon/rayon-core. Source saving only; no linked-kernel saving claimed.'),
]


def function_span(source, name):
    # Mask strings as well as comments so braces in format strings cannot end a function.
    masked = LEX.sub(lambda m: re.sub(r'[^\n]', ' ', m[0]), source)
    match = re.search(r'\bfn\s+' + re.escape(name) + r'\b', masked)
    if not match:
        raise ValueError('missing function ' + name)
    start = masked.index('{', match.end())
    depth = 1
    end = start + 1
    while depth:
        depth += (masked[end] == '{') - (masked[end] == '}')
        end += 1
    return match.start(), end


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--census', type=Path, required=True)
    parser.add_argument('--kernel-elf', type=Path, default=Path('tgt/x86_64-unknown-trueos/debug/TRUEOS'))
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    kernel = Path(__file__).resolve().parents[2]
    repos = kernel.parent
    census = json.loads(args.census.read_text())
    exports = {e['name']: e for e in census['exports']}
    symbols = []
    for line in subprocess.check_output(['nm', '-S', '-C', '--defined-only', str(args.kernel_elf)], text=True).splitlines():
        match = re.match(r'([0-9a-f]+) ([0-9a-f]+) ([A-Za-z]) (.*)', line)
        if match:
            address, length, kind, name = match.groups()
            symbols.append(dict(address=int(address, 16), bytes=int(length, 16), kind=kind, name=name))
    rows = json.loads(json.dumps(CANDIDATES))
    already_removed = [row for row in rows if row['exports'] and not any(e in exports for e in row['exports'])]
    rows = [row for row in rows if row not in already_removed]
    for row in rows:
        if any(e not in exports for e in row['exports']):
            raise ValueError('partially removed candidate needs review: ' + row['name'])
    all_tokens = set(t for row in rows for t in row['tokens'] + row['exports'])
    token_re = re.compile(r'\b(?:' + '|'.join(map(re.escape, sorted(all_tokens))) + r')\b')
    hits = {token: [] for token in all_tokens}
    support_hits = {token: [] for token in all_tokens}
    seen = set()
    errors = []
    for root in census['roots']:
        for path in walk(repos / root, errors):
            if path in seen or {'vendor', 'vendored', 'third_party', 'third-party'} & set(path.parts):
                continue
            seen.add(path)
            source = FN_NAME.sub(lambda m: ' ' * len(m[0]), clean(path.read_text(errors='replace')))
            for number, line in enumerate(source.splitlines(), 1):
                for token in set(token_re.findall(line)):
                    hits[token].append(f'{path.relative_to(repos)}:{number}')
    app_file_count = len(seen)
    for root in census['support_roots']:
        for path in walk(repos / root, errors):
            if path in seen:
                continue
            seen.add(path)
            source = FN_NAME.sub(lambda m: ' ' * len(m[0]), clean(path.read_text(errors='replace')))
            for number, line in enumerate(source.splitlines(), 1):
                for token in set(token_re.findall(line)):
                    support_hits[token].append(f'{path.relative_to(repos)}:{number}')
    for row in rows:
        row['app_token_hits'] = {t: hits[t] for t in row['tokens'] + row['exports'] if hits[t]}
        row['support_token_hits'] = {t: support_hits[t] for t in row['tokens'] + row['exports'] if support_hits[t]}
        row['export_evidence'] = [exports[e] for e in row['exports']]
        row['package_imports'] = sorted({p for e in row['export_evidence'] for p in e['packages']})
        spans = {}
        for relative, names in row['functions'].items():
            source = (kernel / relative).read_text()
            spans[relative] = [function_span(source, name) for name in names]
        for name in row['exports']:
            definitions = []
            for path in (kernel / 'src').rglob('*.rs'):
                if re.search(r'\bfn\s+' + re.escape(name) + r'\s*\(', path.read_text()):
                    definitions.append(path)
            if len(definitions) != 1:
                raise ValueError((name, definitions))
            path = definitions[0]
            spans.setdefault(str(path.relative_to(kernel)), []).append(function_span(path.read_text(), name))
        for relative in row['modules']:
            spans[relative] = [(0, len((kernel / relative).read_text()))]
        row['source_components'] = []
        for relative, ranges in spans.items():
            source = (kernel / relative).read_text()
            row['source_components'].append(dict(path=relative, bytes=sum(len(source[a:b].encode()) for a, b in ranges),
                                                  lines=sum(source[a:b].count('\n') + 1 for a, b in ranges),
                                                  sha256=hashlib.sha256(source.encode()).hexdigest()))
        selectors = row['exports'] + [n for names in row['functions'].values() for n in names]
        name_re = re.compile(r'(?<![A-Za-z_0-9])(?:' + '|'.join(map(re.escape, selectors)) + r')(?![A-Za-z_0-9])') if selectors else None
        selected = [s for s in symbols if (name_re and name_re.search(s['name'])) or any(t in s['name'] for t in row['extra_symbols'])]
        row['linked_symbols'] = selected
        def footprint(kinds):
            # Union address ranges avoids counting aliases/overlapping symbols twice.
            ranges = sorted((s['address'], s['address'] + s['bytes']) for s in selected if s['kind'] in kinds)
            total, end = 0, 0
            for begin, stop in ranges:
                total += max(0, stop - max(begin, end))
                end = max(end, stop)
            return total
        row['linked_text_bytes'] = footprint('tT')
        row['linked_data_bss_bytes'] = footprint('bBdDrRsS')
        row['source_bytes'] = sum(c['bytes'] for c in row['source_components'])
        if row['name'] == 'Vendored Rayon demo package':
            demo = repos / 'TRUEOS-Blueprints/vendor/rayon/rayon-demo'
            row['source_bytes'] = size(demo)
            row['source_directory'] = str(demo)
    retained = [row for row in rows if row['app_token_hits'] or row['package_imports']]
    rows = [row for row in rows if not row['app_token_hits'] and not row['package_imports']]
    rows.sort(key=lambda row: row['linked_text_bytes'], reverse=True)
    report = dict(created_utc=datetime.now(timezone.utc).isoformat(), census=str(args.census),
                  census_sha256=hashlib.sha256(args.census.read_bytes()).hexdigest(),
                  census_started_utc=census['started_utc'], census_finished_utc=census['finished_utc'],
                  kernel_elf=str(args.kernel_elf.resolve()), kernel_elf_mtime=args.kernel_elf.stat().st_mtime,
                  app_source_files=app_file_count, support_source_files=len(seen)-app_file_count,
                  package_count=len(census['artifacts']), errors=errors,
                  candidates=rows, retained_after_review=retained,
                  already_removed=[row['name'] for row in already_removed])
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    md = ['# Reviewed removal candidates — 2026-10-22', '',
          f"Fresh census: {len(census['roots'])} app roots, {app_file_count} non-vendor app source files, {len(seen)-app_file_count} additional support source files, {len(census['artifacts'])} packages. Wrapper/type and named VMCALL tokens were checked in addition to raw exports. Errors: {len(errors) + len(census['errors'])}.", '',
          'Ranked by attributed linked kernel text, with repository-only cleanup last. Ratatui options and other Winit backends are intentionally retained. No implementation was deleted.', '',
          '| Rank | Candidate | Linked text KiB | Data/BSS KiB | Source KiB | App / package hits |',
          '| ---: | --- | ---: | ---: | ---: | --- |']
    for rank, row in enumerate(rows, 1):
        md.append(f"| {rank} | {row['name']} | {row['linked_text_bytes']/1024:.1f} | {row['linked_data_bss_bytes']/1024:.1f} | {row['source_bytes']/1024:.1f} | {len(row['app_token_hits'])} / {len(row['package_imports'])} |")
    md += ['', '## Evidence and removal boundaries', '']
    for rank, row in enumerate(rows, 1):
        md += [f"{rank}. **{row['name']}**: {row['note']}", '']
    md += ['## False positive caught by wrapper checks', '']
    for row in retained:
        md += [row['note'], '', 'App references: ' + ', '.join(sorted({p for refs in row['app_token_hits'].values() for p in refs})), '']
    if already_removed:
        md += ['Already absent from the current export census: ' + ', '.join(row['name'] for row in already_removed) + '.', '']
    md += ['## Interpretation and limits', '',
           'Linked sizes are attributed symbol footprints from the existing debug ELF, not measured savings from a removal/relink. They exclude shared callees and inlined VMCALL dispatch arms; generic symbols containing a module/type name can include common machinery. Data/BSS includes reserved runtime memory, which is not equivalent to on-disk file size. Source sizes count selected kernel function bodies/whole modules (including module tests); the Rayon row counts its shipped directory, excluding build caches. They exclude additional SDK declarations/wrappers and most registration/dispatch edits.', '',
           'Absence from these current sources and existing packages establishes candidates, not compiler-proven unreachability. Packages can lag sources; no fresh full-app builds, runtime trace or whole-program call graph was performed. Numeric VMCALL IDs and computed symbol names can escape named-token searches. The full export records, selected source hashes, per-symbol byte sizes, app-token checks and package evidence are in the JSON.', '',
           'The Unix/POSIX surface was included in the fresh census. TCP/Mio/wait bridges remain because imported POSIX functions call them. `__errno` and `fstatat64` were not promoted to removal candidates: portability macros/aliases need target-specific checking. Directory-stream rejection entries are not live exports to remove.', '',
           'Taffy 0.15 is now a direct Hull path dependency and is excluded. Crossfont and the old Hickory versions were already removed; Hickory 0.26.3 is retained. Rayon demo is the remaining small source-only package candidate from the earlier compilation-unit audit.', '']
    md += ['## Reproduce', '', '```sh',
           'python3 tools/audit/local_api_usage.py --out /tmp/local-api-candidates-current',
           'python3 tools/audit/removal_candidates.py --census /tmp/local-api-candidates-current.json --out tools/docs/removal-candidates-2026-10-22',
           '```', '']
    args.out.with_suffix('.md').write_text('\n'.join(md))
    print('\n'.join(md[:16]))


if __name__ == '__main__':
    main()
