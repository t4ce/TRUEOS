#!/usr/bin/env python3
"""Export rustdoc's public Blueprint C ABI as a small, source-free AI reference."""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
TOOL = Path(__file__).resolve()
RULES = TOOL.with_name('api-prm') / 'usage.json'
FORMAT = 1


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def source_files() -> list[Path]:
    rules = json.loads(RULES.read_text())
    paths = {TOOL, RULES, ROOT / 'Cargo.lock', ROOT / 'crates/trueos-v/Cargo.toml'}
    paths.update((ROOT / 'crates/trueos-v/src').rglob('*.rs'))
    for topic in rules['topics'].values():
        paths.update(ROOT / name for name in topic['sources'])
    return sorted(paths)


def provenance() -> dict:
    files = {p.relative_to(ROOT).as_posix(): digest(p.read_bytes()) for p in source_files()}
    return {'sha256': digest(json.dumps(files, sort_keys=True).encode()), 'files': files}


def rust_type(t: dict | None) -> str:
    if t is None:
        return '()'
    if 'primitive' in t:
        return t['primitive']
    if 'raw_pointer' in t:
        p = t['raw_pointer']
        return ('*mut ' if p['is_mutable'] else '*const ') + rust_type(p['type'])
    if 'resolved_path' in t:
        p = t['resolved_path']
        if p['args'] is not None:
            raise ValueError(f'unsupported ABI path arguments: {p}')
        return p['path']
    if 'array' in t:
        p = t['array']
        return f'[{rust_type(p["type"])}; {p["len"]}]'
    if 'function_pointer' in t:
        p = t['function_pointer']
        if p['generic_params'] or 'C' not in p['header']['abi']:
            raise ValueError('non-C or generic function pointer in C ABI')
        return 'unsafe extern "C" fn(' + ', '.join(rust_type(x) for _, x in p['sig']['inputs']) + ') -> ' + rust_type(p['sig']['output'])
    raise ValueError(f'unsupported public ABI type: {t}')


def group(name: str) -> str:
    if name in ('trueos_cabi_log', 'trueos_cabi_write'):
        return 'logging'
    if any(x in name for x in ('terminal_', 'shell_attached_', 'konsole_', 'shell3_session_')):
        return 'terminal'
    if name.startswith('trueos_cabi_async_fs_') or name.startswith('trueos_cabi_archive_'):
        return 'filesystem'
    if name.startswith(('trueos_cabi_blueprint_', 'trueos_cabi_thread_')) or name in ('trueos_cabi_poll_once', 'trueos_cabi_sleep_ms', 'trueos_cabi_wls_current_slot') or name.startswith('trueos_time_'):
        return 'lifecycle'
    if name in ('trueos_cabi_alloc', 'trueos_cabi_calloc', 'trueos_cabi_realloc', 'trueos_cabi_free', 'sys_alloc_aligned', 'trueos_cabi_malloc_usable_size', 'trueos_cabi_heap_stats'):
        return 'memory'
    if name.startswith(('trueos_cabi_ui4_', 'trueos_cabi_vgpu_')):
        return 'graphics'
    return 'services'


def retired_symbols() -> set[str]:
    source = (ROOT / 'src/hv/blueprint/prebind.rs').read_text()
    source = source.split('fn retired_import_error', 1)[1].split('fn is_rayon_import', 1)[0]
    return set(re.findall(r'"([A-Za-z_][A-Za-z_0-9]*)"', source))


def references(value: object) -> set[int]:
    found = set()
    if isinstance(value, dict):
        if 'resolved_path' in value:
            found.add(value['resolved_path']['id'])
        for child in value.values():
            found.update(references(child))
    elif isinstance(value, list):
        for child in value:
            found.update(references(child))
    return found


def c_struct(item: dict, index: dict) -> dict:
    name = item['name']
    data = item['inner']['struct']
    plain = data['kind'].get('plain')
    if plain is None or plain['has_stripped_fields'] or data['generics']['params']:
        raise ValueError(f'incomplete or non-plain ABI struct {name}')
    representation = [a['repr'] for a in item['attrs'] if isinstance(a, dict) and 'repr' in a]
    if not representation or representation[0]['kind'] != 'c':
        raise ValueError(f'ABI struct {name} is not repr(C)')
    fields = []
    for field_id in plain['fields']:
        field = index[str(field_id)]
        fields.append({'name': field['name'], 'type': rust_type(field['inner']['struct_field']), 'docs': field['docs'] or ''})
    span = item['span']
    return {'name': name, 'repr': representation[0], 'fields': fields, 'docs': item['docs'] or '', 'source': f'{span["filename"]}:{span["begin"][0]}'}


def extract(document: dict) -> dict:
    if document['format_version'] != 60:
        raise ValueError(f'unsupported rustdoc JSON schema {document["format_version"]}; update exporter explicitly')
    index = document['index']
    module = next(x for x in index.values() if x['name'] == 'bp_abi' and x['crate_id'] == 0 and 'module' in x['inner'])
    records, types, constants, excluded = [], [], [], []
    retired = retired_symbols()
    pending, seen = set(), set()
    for ident in module['inner']['module']['items']:
        item = index[str(ident)]
        name, inner = item['name'], item['inner']
        if item['visibility'] != 'public':
            continue
        span = item['span']
        location = f'{span["filename"]}:{span["begin"][0]}' if span else ''
        if 'function' in inner:
            f = inner['function']
            if f['has_body'] or 'C' not in f['header']['abi']:
                continue
            if name in retired or name.startswith(('trueos_cabi_fs_', 'trueos_cabi_trueosfs_')):
                excluded.append(name)
                continue
            sig = f['sig']
            if sig['is_c_variadic'] or f['generics']['params']:
                raise ValueError(f'unsupported variadic/generic ABI function {name}')
            signature = f'extern "C" fn {name}(' + ', '.join(f'{n}: {rust_type(t)}' for n, t in sig['inputs']) + ') -> ' + rust_type(sig['output']) + ';'
            records.append({'name': name, 'topic': group(name), 'signature': signature, 'docs': item['docs'] or '', 'source': location})
            pending.update(references(sig))
        elif 'struct' in inner:
            types.append(c_struct(item, index))
            seen.add(item['id'])
            for field_id in inner['struct']['kind']['plain']['fields']:
                pending.update(references(index[str(field_id)]['inner']['struct_field']))
        elif 'constant' in inner:
            c = inner['constant']
            constants.append({'name': name, 'type': rust_type(c['type']), 'value': c['const']['value'] or c['const']['expr'], 'docs': item['docs'] or ''})
        else:
            raise ValueError(f'new ABI item kind: {name}: {list(inner)}')
    while pending - seen:
        ident = min(pending - seen)
        seen.add(ident)
        dependency = index.get(str(ident))
        if dependency is None or dependency['crate_id'] != 0:
            path = document['paths'].get(str(ident), {}).get('path', [])
            if path[:2] == ['core', 'ffi']:
                continue
            raise ValueError(f'unresolved ABI type dependency {ident}: {path}')
        if 'struct' not in dependency['inner']:
            raise ValueError(f'unsupported ABI dependency: {dependency["name"]}')
        types.append(c_struct(dependency, index))
        for field_id in dependency['inner']['struct']['kind']['plain']['fields']:
            pending.update(references(index[str(field_id)]['inner']['struct_field']))
    # Logging levels live in the Rust facade rather than bp_abi, but are wire values.
    for item in index.values():
        if item['crate_id'] == 0 and (item['name'] or '').startswith(('LOG_LEVEL_', 'LOG_FLAG_')) and 'constant' in item['inner']:
            c = item['inner']['constant']
            constants.append({'name': item['name'], 'type': rust_type(c['type']), 'value': c['const']['value'] or c['const']['expr'], 'docs': item['docs'] or ''})
    return {'functions': sorted(records, key=lambda x: x['name']), 'types': sorted(types, key=lambda x: x['name']), 'constants': sorted(constants, key=lambda x: x['name']), 'excluded': sorted(excluded)}


def render(data: dict, stamp: dict, document: dict) -> dict[str, bytes]:
    rules = json.loads(RULES.read_text())
    abi_hash = digest(json.dumps(data, sort_keys=True, separators=(',', ':')).encode())
    version = f'trueos-blueprint-api/1/{abi_hash[:16]}'
    header = f'API PRM {version}\nReference source fingerprint: {stamp["sha256"]}\n'
    files = {}
    lines = [header, '# Start here', *rules['overview'], '', '# Topic lookup']
    for topic, rule in rules['topics'].items():
        records = [r for r in data['functions'] if r['topic'] == topic]
        lines.append(f'- {topic}.prm: {len(records)} C ABI functions. {rule["notes"][0].split(". ")[0]}.')
        text = [header, '# ' + topic, *rule['notes'], '', '# C ABI declarations (Rust type notation)']
        for r in records:
            text.extend([f'\n## {r["name"]}', r['signature'], f'Declaration: {r["source"]}'])
            if r['docs']:
                text.append(r['docs'])
        if topic == 'logging':
            text.append('\n# Logging wire constants')
            text.extend(f'{c["name"]}: {c["type"]} = {c["value"]}' for c in data['constants'] if c['name'].startswith('LOG_'))
        files[f'{topic}.prm'] = ('\n\n'.join(text) + '\n').encode()
    lines.extend(['- types.prm: repr(C) field order, arrays, public ABI constants. No implementation bodies.', '- symbols.prm: searchable C symbol-to-topic index; consult when you know a function name.', '- manifest.json: version, source hashes, artifact hashes, counts, excluded retired declarations.'])
    files['trueos-api.prm'] = ('\n'.join(lines) + '\n').encode()
    files['symbols.prm'] = (header + '\n# C ABI symbol lookup\n' + '\n'.join(f'{r["name"]} -> {r["topic"]}.prm' for r in data['functions']) + '\n').encode()
    text = [header, '# Public C ABI data layouts', 'Fields are in repr(C) order. This is not a packed serialization: honor native alignment, pointers, padding and array lengths. Raw-pointer mutability indicates direction; explicit lengths remain authoritative. Numeric byte offsets are not synthesized by rustdoc.']
    for t in data['types']:
        text.extend(['\n## ' + t['name'], f'repr: {json.dumps(t["repr"], sort_keys=True)}', t['docs'], 'struct ' + t['name'] + ' {'])
        for field in t['fields']:
            text.append('  ' + field['name'] + ': ' + field['type'] + ',')
            if field['docs']:
                text.append('  // ' + field['docs'].replace('\n', ' '))
        text.extend(['}', 'Declaration: ' + t['source']])
    text.append('\n# Wire constants')
    text.extend(f'{c["name"]}: {c["type"]} = {c["value"]}' + (' // ' + c['docs'].replace('\n', ' ') if c['docs'] else '') for c in data['constants'])
    files['types.prm'] = ('\n'.join(text) + '\n').encode()
    manifest = {'scope': 'public v::bp_abi C declarations and data layouts; not every Rust facade or libc export', 'runtime_availability': 'not probed; running kernel features, service readiness and Blueprint scope apply', 'format_version': FORMAT, 'api_id': version, 'abi_sha256': abi_hash, 'source': stamp, 'rustdoc': {'crate': 'v', 'crate_version': document['crate_version'], 'format_version': document['format_version'], 'target': document['target']}, 'counts': {k: len(v) for k, v in data.items()}, 'excluded_retired_declarations': data['excluded'], 'files': {n: {'sha256': digest(b), 'bytes': len(b)} for n, b in sorted(files.items())}}
    files['manifest.json'] = (json.dumps(manifest, sort_keys=True, indent=2) + '\n').encode()
    return files


def current(output: Path, stamp: dict) -> bool:
    try:
        manifest = json.loads((output / 'manifest.json').read_text())
        expected = {f'{topic}.prm' for topic in json.loads(RULES.read_text())['topics']}
        expected.update(('trueos-api.prm', 'types.prm', 'symbols.prm'))
        return (manifest['format_version'] == FORMAT and manifest['source'] == stamp
                and set(manifest['files']) == expected
                and all(digest((output / name).read_bytes()) == info['sha256']
                        and (output / name).stat().st_size == info['bytes']
                        for name, info in manifest['files'].items()))
    except (OSError, ValueError, KeyError):
        return False


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=ROOT.parent / 'TRUEOS-Blueprints/crates/trueos-ide/api-prm')
    parser.add_argument('--rustdoc-json', type=Path, help='consume a fresh cargo rustdoc export instead of invoking Cargo')
    parser.add_argument('--check', action='store_true', help='fail if generated reference does not match current source hashes')
    parser.add_argument('--refresh', action='store_true', help='regenerate even when the source fingerprint matches')
    args = parser.parse_args()
    stamp = provenance()
    if not args.refresh and current(args.output, stamp):
        print(f'API PRM is current: {args.output}')
        return
    if args.check:
        raise ValueError('API PRM is stale or corrupted; regenerate with tools/build_api_prm.py')
    if args.rustdoc_json:
        document = json.loads(args.rustdoc_json.read_bytes())
    else:
        build_dir = ROOT / 'tgt/api-prm-rustdoc'
        command = ['cargo', 'rustdoc', '--locked', '-p', 'v', '--lib', '--target', 'x86_64-unknown-linux-gnu', '--target-dir', str(build_dir), '--', '-Z', 'unstable-options', '--output-format', 'json']
        subprocess.run(command, cwd=ROOT, check=True, env={k: v for k, v in os.environ.items() if k not in ('RUSTFLAGS', 'RUSTDOCFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_DIR')})
        document = json.loads((build_dir / 'x86_64-unknown-linux-gnu/doc/v.json').read_bytes())
    if stamp != provenance():
        raise ValueError('reference inputs changed during rustdoc generation; retry')
    files = render(extract(document), stamp, document)
    args.output.mkdir(parents=True, exist_ok=True)
    for name, payload in files.items():
        destination = args.output / name
        with tempfile.NamedTemporaryFile(dir=args.output, delete=False) as staged:
            staged.write(payload)
            temp = Path(staged.name)
        os.replace(temp, destination)
    print(f'Generated {len(files)} API PRM files, {sum(len(b) for b in files.values())} bytes: {args.output}')


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        sys.exit(str(error))
