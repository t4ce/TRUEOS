#!/usr/bin/env python3
"""Conservative local-source census of Blueprint-visible kernel symbols.

An SDK/dependency reference keeps a symbol out of the zero-reference list even
when no application is known to call that wrapper. This avoids claiming that
the absence of a raw ABI name in application source proves non-use.
Package scanning requires nm, lz4, and 7z (for historical packages).
"""
import argparse
import collections
from datetime import datetime, timezone
import hashlib
import io
import json
import os
from pathlib import Path
import re
import struct
import subprocess
import tarfile
import tempfile
import tomllib

from blueprint_api import ARM, declarations

IDENT = re.compile(r'\b[A-Za-z_][A-Za-z_0-9]*\b')
FN_NAME = re.compile(r'\bfn\s+([A-Za-z_][A-Za-z_0-9]*)')
# Preserve strings: explicit dynamic-lookup names are consumer evidence too.
LEX = re.compile(r'r(\#*)"[\s\S]*?"\1|"(?:\\.|[^"\\])*"|//[^\n]*|/\*[\s\S]*?\*/')
SUFFIXES = {'.rs', '.c', '.h', '.cc', '.cpp', '.hpp', '.s', '.S', '.js', '.ts', '.mjs', '.odin'}
SKIP = {'.git', '.agents', '.codex', '.aws', 'target', 'node_modules', 'dist',
        'build', '.cache', '__pycache__', 'tests', 'test', 'benches', 'probes'}


def clean(source):
    return LEX.sub(lambda m: re.sub(r'[^\n]', ' ', m[0]) if m[0].startswith(('//', '/*')) else m[0], source)


def function_source(source, name):
    start = source.index('fn ' + name + '(')
    begin = source.index('{', start)
    depth = 1
    for end in range(begin + 1, len(source)):
        if source[end] == '{':
            depth += 1
        elif source[end] == '}':
            depth -= 1
            if depth == 0:
                return source[start:end + 1], source.count('\n', 0, start)
    raise ValueError(f'unclosed function {name}')


def worker_declarations(path):
    source = clean(path.read_text())
    return {m[1]: re.sub(r'\s+', '', source[m.start():source.index(';', m.end())])
            for m in re.finditer(r'pub fn (trueos_[A-Za-z_0-9]+)\s*\(', source)}


def walk(root, errors):
    def onerror(error):
        errors.append(str(error))
    for directory, dirs, files in os.walk(root, onerror=onerror):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith('.'))
        if str(Path(directory)).endswith('library/std/src/sys/pal'):
            dirs[:] = [d for d in dirs if d == 'unix']
        for filename in sorted(files):
            path = Path(directory) / filename
            if path.suffix in SUFFIXES:
                yield path


def walk_manifests(root, errors):
    for directory, dirs, files in os.walk(root, onerror=lambda error: errors.append(str(error))):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith('.'))
        if 'Cargo.toml' in files:
            yield Path(directory) / 'Cargo.toml'


def package_imports(path, tmp):
    blob = path.read_bytes()
    if len(blob) < 24 or blob[:4] != b'TRBP':
        raise ValueError('invalid TRBP header')
    version, flags, _, payload_len, raw_len = struct.unpack_from('<HHQII', blob, 4)
    if version != 1 or payload_len != len(blob) - 24:
        raise ValueError('unsupported version or payload length mismatch')
    if flags & 255 == 3:
        tar = subprocess.run(['lz4', '-dc'], input=blob[24:], capture_output=True, check=True).stdout
        with tarfile.open(fileobj=io.BytesIO(tar)) as archive:
            member = archive.extractfile('module.elf')
            if member is None:
                raise ValueError('missing module.elf')
            elf = member.read()
    elif flags & 255 == 1:
        elf = blob[24:]
    elif flags & 255 == 2:
        # Retain historical 7z packages as positive usage evidence even though
        # the current loader no longer accepts this payload encoding.
        archive = tmp.with_suffix('.7z')
        archive.write_bytes(blob[24:])
        elf = subprocess.run(['7z', 'e', '-so', str(archive)], capture_output=True, check=True).stdout
    else:
        raise ValueError(f'unsupported encoding {flags & 255}')
    if len(elf) != raw_len or elf[:4] != b'\x7fELF':
        raise ValueError('invalid ELF or raw length mismatch')
    tmp.write_bytes(elf)
    output = subprocess.run(['nm', '--undefined-only', '--format=posix', str(tmp)],
                            capture_output=True, text=True, check=True).stdout
    return sorted({line.split()[0] for line in output.splitlines() if line.split()}), hashlib.sha256(blob).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--kernel', type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument('--repos', type=Path)
    parser.add_argument('--out', type=Path, required=True, help='output prefix (.json, .md, .tsv)')
    args = parser.parse_args()
    kernel = args.kernel.resolve()
    started = datetime.now(timezone.utc).isoformat()
    repos = (args.repos or kernel.parent).resolve()
    bp = repos / 'TRUEOS-Blueprints'
    kd = declarations(kernel / 'crates/trueos-v/src/bp_abi.rs')
    sd = declarations(bp / 'crates/trueos-v/src/bp_abi.rs')
    sd.update(declarations(bp / 'crates/trueos-v/src/qjs_abi.rs'))
    errors = []
    exports = {}
    definitions = collections.defaultdict(list)
    kernel_files = list(walk(kernel / 'src', errors))
    kernel_files += list(walk(kernel / 'crates', errors))
    for path in kernel_files:
        source = clean(path.read_text(errors='replace'))
        for m in FN_NAME.finditer(source):
            if m[1].startswith('trueos_cabi_') and path.is_relative_to(kernel / 'src'):
                definitions[m[1]].append(f'{path.relative_to(repos)}:{source.count(chr(10), 0, m.start()) + 1}')

    def add(name, category, location):
        row = exports.setdefault(name, {'name': name, 'category': category, 'registration': [],
                                       'app_refs': [], 'sdk_dependency_refs': [], 'kernel_refs': [], 'packages': []})
        row['registration'].append(location)

    for name in sorted(set(kd) & set(definitions)):
        add(name, 'Native CABI', definitions[name][0])
    loader = kernel / 'src/hv/blueprint/blueprint.rs'
    for path, function in [(loader, 'resolve_known_import'), (loader, 'resolve_runtime_abi_import'),
                           (kernel / 'src/unix_compat.rs', 'resolve_import')]:
        body, offset = function_source(clean(path.read_text()), function)
        for match in ARM.finditer(body):
            for name in re.findall(r'"([^"\n]+)"', match[1]):
                category = ('Native CABI' if name.startswith('trueos_cabi_') else
                            'Kernel sys_*' if name.startswith('sys_') else
                            'Rust runtime' if name.startswith(('_RN', '__rust', '_Unwind')) else
                            'Platform / std bridge' if name.startswith(('trueos_', '_embassy')) else 'std / libc / math')
                add(name, category, f'{path.relative_to(repos)}:{offset + body.count(chr(10), 0, match.start()) + 1}')
    ring = kernel / 'vendor/ring-0.17.14/runtime-loader-symbols-x86_64.txt'
    for line in ring.read_text().splitlines():
        name = line.strip()
        if name and not name.startswith('#'):
            add(name, 'Ring runtime', str(ring.relative_to(repos)))
    unsupported_body, _ = function_source((kernel / 'src/unix_compat.rs').read_text(), 'unsupported_unix_import_reason')
    rejected = {n for m in ARM.finditer(unsupported_body) for n in re.findall(r'"([^"\n]+)"', m[1])}
    for name, row in exports.items():
        row['status'] = ('prebind rejected' if name in rejected or name.startswith(('trueos_cabi_fs_', 'trueos_cabi_trueosfs_')) else
                         'feature gated: gridpaper' if name.startswith('trueos_cabi_gridpaper_') else
                         'feature gated: wc3-x86' if name.startswith('trueos_cabi_x86_') else 'registered candidate')

    roots = {p.resolve() for parent in ['apps', 'buildins'] for p in (bp / parent).iterdir() if p.is_dir()}
    missing = []
    for entry in json.loads((bp / 'apps.json').read_text())['apps']:
        name = entry if isinstance(entry, str) else entry['name']
        path = bp / (f'apps/{name}' if isinstance(entry, str) else entry.get('path', f'apps/{name}'))
        if path.is_dir():
            roots.add(path.resolve())
        else:
            missing.append({'name': name, 'path': str(path), 'optional': isinstance(entry, dict) and entry.get('optional', False)})
    # Include unregistered local ports and frameworks. Do not count another
    # kernel checkout or host Rust compiler as application consumers.
    excluded = {'TRUEOS', 'TRUEOS-Blueprints', 'TRUEOS-x86-carrier'}
    for path in repos.iterdir():
        manifest = path / 'Cargo.toml'
        if path.name not in excluded and manifest.is_file() and 'trueos' in manifest.read_text().lower():
            roots.add(path.resolve())
    support = [bp / 'api', bp / 'crates', bp / 'vendor',
               repos / 'TRUEOS-Rust-Toolchain-nightly-2026-07-10/lib/rustlib/src/rust/library/std/src']
    # Workspace members can import most services on behalf of a tiny CLI.
    # Scan local dependency trees as evidence, without treating their mere
    # presence as proof that any particular app calls them.
    support_set = set(support)
    pending = list(roots) + [bp / 'api', bp / 'crates']
    visited_manifests = set()
    def local_paths(value):
        if isinstance(value, dict):
            if isinstance(value.get('path'), str):
                yield value['path']
            for child in value.values():
                yield from local_paths(child)
        elif isinstance(value, list):
            for child in value:
                yield from local_paths(child)
    while pending:
        root = pending.pop()
        manifests = {p for p in walk_manifests(root, errors)}
        for ancestor in root.parents:
            if ancestor == repos:
                break
            manifest = ancestor / 'Cargo.toml'
            if manifest.is_file():
                manifests.add(manifest)
        for manifest in sorted(manifests - visited_manifests):
            visited_manifests.add(manifest)
            metadata = tomllib.loads(manifest.read_text())
            # codex-rs/cli is only a frontend; include sibling workspace crates.
            if 'workspace' in metadata and manifest.parent not in {bp, kernel}:
                dependency = manifest.parent.resolve()
                if dependency not in support_set:
                    support_set.add(dependency)
                    pending.append(dependency)
            for relative in local_paths(metadata):
                dependency = (manifest.parent / relative).resolve()
                if (dependency.is_relative_to(repos) and dependency not in {kernel, bp}
                        and (not dependency.is_relative_to(kernel) or dependency.is_relative_to(kernel / 'crates'))
                        and not set(dependency.relative_to(repos).parts).intersection(SKIP)
                        and (dependency / 'Cargo.toml').is_file()
                        and dependency not in support_set and dependency not in roots):
                    support_set.add(dependency)
                    pending.append(dependency)
    support = sorted(support_set)
    names = set(exports)
    seen = set()
    source_counts = collections.Counter()
    source_hash = hashlib.sha256()
    registration_paths = {loader, kernel / 'src/unix_compat.rs'}
    for root, kind in [(p, 'app_refs') for p in sorted(roots)] + [(p, 'sdk_dependency_refs') for p in support] + [(kernel / 'src', 'kernel_refs'), (kernel / 'crates', 'kernel_refs')]:
        if not root.exists():
            continue
        for path in walk(root, errors):
            resolved = path.resolve()
            if resolved in seen:
                continue
            seen.add(resolved)
            data = path.read_bytes()
            source_hash.update(str(resolved).encode() + b'\0' + hashlib.sha256(data).digest())
            source_counts[kind] += 1
            source = clean(data.decode(errors='replace'))
            # A declared/defined symbol is not its own consumer.
            source = FN_NAME.sub(lambda m: m[0][:m.start(1) - m.start()] + ' ' * len(m[1]), source)
            # Resolver arms advertise exports; they are not internal callers.
            if path in registration_paths:
                continue
            file_kind = 'sdk_dependency_refs' if any(p in {'vendor', 'vendored', 'third_party', 'third-party'} for p in path.relative_to(repos).parts) else kind
            for number, line in enumerate(source.splitlines(), 1):
                for name in names.intersection(IDENT.findall(line)):
                    refs = exports[name][file_kind]
                    location = f'{path.relative_to(repos)}:{number}'
                    if len(refs) < 12:
                        refs.append(location)

    artifacts = []
    packages = set(bp.glob('dist/*.bp'))
    for root in roots:
        packages.update(root.glob('dist/*.bp'))
    packages.update((repos / 'voxy/target/asset-sync').glob('*.bp'))
    with tempfile.TemporaryDirectory(prefix='trueos-local-api-') as temp:
        for path in sorted(packages):
            try:
                imports, digest = package_imports(path, Path(temp) / 'module.elf')
                rel = str(path.relative_to(repos))
                artifacts.append({'path': rel, 'sha256': digest, 'imports': imports})
                for name in names.intersection(imports):
                    exports[name]['packages'].append(rel)
            except (ValueError, OSError, subprocess.SubprocessError, tarfile.TarError, KeyError) as error:
                errors.append(f'{path}: {error}')
    rows = sorted(exports.values(), key=lambda r: (r['category'], r['name']))
    for row in rows:
        row['consumer_evidence'] = ('app/package reference' if row['app_refs'] or row['packages'] else
                                    'SDK/dependency only' if row['sdk_dependency_refs'] else 'no consumer reference')
    groups = collections.defaultdict(collections.Counter)
    for row in rows:
        groups[row['category']][row['consumer_evidence']] += 1
        if row['consumer_evidence'] == 'no consumer reference' and not row['kernel_refs']:
            groups[row['category']]['no consumer or kernel reference'] += 1
    normalize = lambda signature: signature.replace(',)', ')')
    kw = worker_declarations(kernel / 'crates/trueos-v/src/worker_abi.rs')
    sw = worker_declarations(bp / 'crates/trueos-v/src/worker_abi.rs')
    diff = {'sdk_only': sorted(set(sd) - set(kd)), 'kernel_only': sorted(set(kd) - set(sd)),
            'signature_drift': sorted(n for n in set(sd) & set(kd) if normalize(sd[n]) != normalize(kd[n])),
            'declared_without_definition': sorted(set(kd) - set(definitions)),
            'rust_worker_sdk_only': sorted(set(sw) - set(kw)),
            'rust_worker_kernel_only': sorted(set(kw) - set(sw)),
            'rust_worker_signature_drift': sorted(n for n in set(sw) & set(kw) if normalize(sw[n]) != normalize(kw[n]))}
    scope = ('Lexical, conservative current-source census; Rust declaration/definition names and comments excluded. '
             'SDK/dependency references count even for dormant wrappers and unrelated features; short libc names can collide. '
             'App/package evidence is not proof of execution. No-consumer means no reference in this corpus, not compiler-proven dead code. '
             'Tests/probes/build directories are excluded; local Cargo path dependencies and std sources are included, with std PAL restricted to unix. cfg is not evaluated. '
             'Resolver candidates include gated/rejected names, marked individually; wildcard Rust runtime families are not finitely enumerated. '
             'Reference locations capped at 12 per symbol per source class. No rebuild or runtime test.')
    scope += (' This is not a complete unused-by-app call graph: dormant wrappers remain unresolved. '
              'Compiler-generated imports, token-pasted names, generated-only code, and dependency sources outside Repos '
              '(including the Cargo registry) can escape the source census; packages supply positive evidence for their particular builds.')
    report = {'started_utc': started, 'finished_utc': datetime.now(timezone.utc).isoformat(),
              'scope': scope, 'roots': [str(p.relative_to(repos)) for p in sorted(roots)],
              'support_roots': [str(p.relative_to(repos)) for p in support if p.exists()],
              'source_files': dict(source_counts), 'source_sha256': source_hash.hexdigest(),
              'missing_catalog_paths': missing, 'summary': dict(groups), 'abi_diff': diff,
              'exports': rows, 'artifacts': artifacts, 'errors': errors}
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    columns = ['category', 'name', 'status', 'consumer_evidence', 'kernel_refs', 'registration']
    args.out.with_suffix('.tsv').write_text('\t'.join(columns) + '\n' + '\n'.join(
        '\t'.join('; '.join(row[c]) if isinstance(row[c], list) else row[c] for c in columns) for row in rows) + '\n')
    md = ['# Local Blueprint ABI / std / kernel export usage', '',
          f'Scanned {len(roots)} local source roots, {sum(source_counts.values())} source files, and {len(artifacts)} packages. Errors: {len(errors)}.', '',
          '| Export surface | Symbols | App/package reference | SDK/dependency only | No consumer reference | Also no kernel reference |',
          '| --- | ---: | ---: | ---: | ---: | ---: |']
    for category, counts in sorted(groups.items()):
        total = sum(counts[k] for k in ['app/package reference', 'SDK/dependency only', 'no consumer reference'])
        md.append(f"| {category} | {total} | {counts['app/package reference']} | {counts['SDK/dependency only']} | {counts['no consumer reference']} | {counts['no consumer or kernel reference']} |")
    md += ['', 'SDK/dependency only is unresolved application usage, not an unused finding. '
           'A no-consumer row can still have kernel-internal callers (shown below).', '',
           '| ABI declaration diff | Count | Symbols |', '| --- | ---: | --- |']
    for key, values in diff.items():
        md.append(f"| {key} | {len(values)} | " + ', '.join(f'`{n}`' for n in values[:5]) + ('; full list in JSON' if len(values) > 5 else '') + ' |')
    md += ['', '## Exports with no consumer reference', '',
           '| Surface | Symbol | Kernel-internal reference | Availability |', '| --- | --- | --- | --- |']
    for row in rows:
        if row['consumer_evidence'] == 'no consumer reference':
            md.append(f"| {row['category']} | `{row['name']}` | {row['kernel_refs'][0] if row['kernel_refs'] else 'none found'} | {row['status']} |")
    md += ['', '## Scope', '', scope, '', 'Source roots: ' + ', '.join(f'`{p}`' for p in report['roots']) + '.', '',
           f"Support roots: {len(report['support_roots'])} local SDK, std and dependency paths (full inventory in JSON).", '',
           'Available packages: ' + ', '.join(f"`{p['path']}`" for p in artifacts) + '.', '',
           'Missing catalog paths: ' + (', '.join(f"`{p['name']}` (optional={p['optional']})" for p in missing) or 'none') + '.', '',
           'CABI declaration comparison includes SDK `bp_abi.rs` and `qjs_abi.rs`; native Rust `worker_abi.rs` is compared separately. Whitespace and trailing argument commas are ignored. Transitive struct layouts are not compared.', '',
           f"Snapshot: {report['started_utc']} to {report['finished_utc']}. Package files may be rebuilt independently; their hashes are recorded in JSON. Historical 7z packages count as usage evidence even though the current loader accepts raw ELF and tar/LZ4 only.", '',
           'Reproduce: `python3 tools/audit/local_api_usage.py --out tools/docs/local-api-usage-2026-10-21`', '']
    if errors:
        md += ['Errors:', ''] + [f'- {e}' for e in errors]
    args.out.with_suffix('.md').write_text('\n'.join(md))
    print(json.dumps({k: report[k] for k in ['source_files', 'summary', 'abi_diff', 'missing_catalog_paths', 'errors']}, indent=2))
    if errors:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
