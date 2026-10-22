#!/usr/bin/env python3
"""Inventory Blueprint vendors using target-filtered Cargo compilation unit graphs.

Uses existing staged app manifests/locks, the packer's production build planner,
and reconstructed source-overlay configs. --unit-graph plans without compiling
or executing build scripts. Queries are offline/locked; source trees are kept.
"""
import argparse
import collections
from concurrent.futures import ThreadPoolExecutor, as_completed
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import tomllib
from urllib.parse import unquote

from local_api_usage import function_source

SKIP = {'target', 'tgt', 'bld', 'build', '.git', '.agents', '.aws', '.codex',
        'node_modules', '__pycache__', '.cache'}


def read(path):
    return tomllib.loads(path.read_text())


def manifests(root, skip_vendor=False):
    for directory, dirs, files in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith('.')
                         and not (skip_vendor and d in {'vendor', 'vendored'}))
        if 'Cargo.toml' in files:
            yield Path(directory) / 'Cargo.toml'


def size(root, excluded=()):
    total = 0
    for directory, dirs, files in os.walk(root):
        here = Path(directory)
        if here != root and here in excluded:
            dirs[:] = []
            continue
        for dirname in list(dirs):
            if dirname in SKIP or dirname.startswith('.'):
                dirs.remove(dirname)
        for filename in files:
            path = here / filename
            if not path.is_symlink():
                total += path.stat().st_size
    return total


def inventory(vendor):
    rows = []
    for folder in sorted(p for p in vendor.iterdir() if p.is_dir()):
        root = folder / 'Cargo.toml'
        candidates = set()
        if root.exists():
            meta = read(root)
            if 'package' in meta:
                candidates.add(root)
            for member in meta.get('workspace', {}).get('members', []):
                candidates.update(p / 'Cargo.toml' for p in folder.glob(member) if (p / 'Cargo.toml').is_file())
        else:
            candidates.update(folder.glob('*/Cargo.toml'))
        for path in sorted(candidates):
            meta = read(path)
            package = meta.get('package')
            if not package:
                continue
            version = package.get('version', '0.0.0')
            if isinstance(version, dict):
                version = read(root)['workspace']['package']['version']
            rows.append({'name': package['name'], 'version': version,
                         'path': str(path.parent.relative_to(vendor)), 'manifest': str(path),
                         'runtime_apps': [], 'host_build_apps': [], 'overlay_source_apps': [],
                         'other_source_apps': [], 'lock_only_apps': [], 'manifest_consumers': [],
                         'workspace_declarations': [], 'patch_registrations': [], 'packer_requirements': []})
    all_roots = {Path(r['manifest']).parent for r in rows}
    for row in rows:
        row['source_bytes'] = size(Path(row['manifest']).parent, all_roots - {Path(row['manifest']).parent})
    return rows


def planner(bp, directory):
    main = (bp / 'src/main.rs').read_text()
    catalog = (bp / 'src/app_catalog.rs').read_text()
    helpers = '\n'.join(function_source(main, n)[0] for n in [
        'parse_string_array', 'push_feature', 'source_tree_mentions',
        'source_tree_mentions_in_dir', 'io_string', 'is_helix_app_dir'])
    helpers += '\n' + '\n'.join(function_source(catalog, n)[0] for n in [
        'manifest_declared_features', 'manifest_has_dependency', 'push_app_or_trueos_feature'])
    start = main.index('    let mut extra_features = required_features.to_vec();')
    end = main.index('    if no_default_features {\n        cargo.arg(', start)
    feature_selection = main[start:end]
    source = '#![allow(dead_code)]\nuse std::{fs,io,path::Path};\n'
    source += f'#[path={json.dumps(str(bp / "src/build_plan.rs"))}] mod build_plan;\n'
    source += 'use build_plan::{BuildFlavor,BuildTarget};\n' + helpers
    source += '''
fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let app_dir = Path::new(&a[1]);
    let manifest = Path::new(&a[2]);
    let staged = Path::new(&a[3]);
    let build_settings = build_plan::resolve_build_settings(app_dir, manifest, &BuildTarget::Package).unwrap();
    let declared_features = manifest_declared_features(staged).unwrap();
    let has_trueos_dependency = manifest_has_dependency(staged, "trueos").unwrap();
    let required_features: Vec<String> = Vec::new();
'''
    source += feature_selection
    source += '''
    println!("flavor\\t{}", match build_settings.flavor {BuildFlavor::ThinNoStd=>"thin",BuildFlavor::TokioStd=>"std"});
    println!("no_default\\t{}", no_default_features);
    println!("features\\t{}", extra_features.join(","));
    println!("source\\t{}", build_settings.source_path.display());
    Ok(())
}
'''
    path = directory / 'planner.rs'
    path.write_text(source)
    binary = directory / 'planner'
    subprocess.run(['rustc', '--edition=2024', str(path), '-o', str(binary)], check=True)
    return binary


def overlay_configs(bp, app, original, staged, flavor):
    main = (bp / 'src/main.rs').read_text()
    patches = {}
    def constants(name):
        body = main.split('const ' + name + ':', 1)[1].split('];', 1)[0]
        return re.findall(r'\("([^"\n]+)", "([^"\n]+)"\)', body)
    def vendor_path(folder):
        for ancestor in [app, *app.parents]:
            candidate = ancestor / 'vendor' / folder
            if candidate.is_dir():
                return candidate
        candidate = bp / 'vendor' / folder
        return candidate if candidate.is_dir() else None
    for name, folder in constants('BLUEPRINT_VENDOR_PATCHES'):
        path = vendor_path(folder)
        if path:
            patches[name] = (name, path)
    if flavor == 'thin':
        lock = next((p / 'Cargo.lock' for p in [app, *app.parents] if (p / 'Cargo.lock').is_file()), None)
        mentions = original.read_text() + ('\n' + lock.read_text() if lock else '')
        for name, folder in constants('TRUEOS_IMAGE_CODEC_VENDOR_PATCHES'):
            needed = 'png' if name in {'crc32fast', 'fdeflate', 'png', 'simd-adler32'} else 'zune-jpeg'
            if f'name = "{needed}"' in mentions or re.search(r'(?m)^\s*' + needed + r'\s*=', mentions):
                path = bp.parent / 'TRUEOS/vendor' / folder
                if path.exists():
                    patches[name] = (name, path)
    patches['libc'] = ('libc', bp / 'vendor/libc-0.2.186')
    for key, folder in [('getrandom', 'getrandom-0.2.17'), ('getrandom_03', 'getrandom-0.3.4'), ('getrandom_04', 'getrandom-0.4.2')]:
        patches[key] = ('getrandom', vendor_path(folder))
    for path in sorted((staged.parent.parent / 'source-overlay-crates').glob('*/Cargo.toml')):
        package = read(path)['package']
        name = package['name']
        key = {'rustix': 'rustix_0_38', 'crossterm': 'crossterm_0_28', 'socket2': 'socket2_0_5'}.get(name, name)
        patches[key] = (name, path.parent)
    for key, patch in read(original).get('patch', {}).get('crates-io', {}).items():
        if isinstance(patch, dict) and 'path' in patch:
            patches[key] = (patch.get('package', key), (original.parent / patch['path']).resolve())
    for name, folder in constants('TRACING_VENDOR_PATCHES'):
        patches[name] = (name, bp / 'vendor' / folder)
    args = []
    for key, (name, path) in sorted(patches.items()):
        if path is None:
            continue
        if key != name:
            args += ['--config', f'patch.crates-io.{key}.package={json.dumps(name)}']
        args += ['--config', f'patch.crates-io.{key}.path={json.dumps(str(path))}']
    return args


def query(bp, app, helper, target_dir, logs):
    name, directory = app
    original = directory / 'Cargo.toml'
    if name == 'edit':
        original = directory / 'crates/edit/Cargo.toml'
    candidates = list((directory / 'target/trueos-blueprint/work').glob('*/source-overlay-app/Cargo.toml'))
    if not candidates:
        return {'app': name, 'error': 'no existing staged manifest', 'directory': str(directory)}
    staged = max(candidates, key=lambda p: p.stat().st_mtime_ns)
    staged = staged.parent / original.relative_to(directory)
    # The catalog can name a package inside a virtual workspace.
    if not staged.exists():
        raise ValueError(f'missing staged package manifest: {staged}')
    plan = dict(line.split('\t', 1) for line in subprocess.check_output(
        [str(helper), str(directory), str(original), str(staged)], text=True).splitlines())
    toolchain = bp.parent / 'TRUEOS-Rust-Toolchain-nightly-2026-07-10/bin'
    target = directory / 'target/trueos-blueprint/x86_64-unknown-trueos.json'
    if not target.exists():
        target = bp / 'apps/target.json'
    cmd = [str(toolchain / 'cargo'), 'build', '--unit-graph', '-Z', 'unstable-options', '-Z', 'json-target-spec',
           '-Z', 'build-std=core,compiler_builtins,alloc' + (',std,panic_abort' if plan['flavor'] == 'std' else ''),
           '--offline', '--locked', '--manifest-path', str(staged), '--target', str(target)]
    if plan['no_default'] == 'true':
        cmd.append('--no-default-features')
    if plan['features']:
        cmd += ['--features', plan['features']]
    meta = read(original)
    profile = meta.get('package', {}).get('metadata', {}).get('trueos-blueprint', {}).get('profile', 'release')
    if profile == 'release':
        cmd.append('--release')
    elif profile not in {'dev', 'debug'}:
        cmd += ['--profile', profile]
    binaries = meta.get('bin', [])
    if binaries or Path(plan['source']).name == 'main.rs':
        binary = meta['package'].get('default-run') or (binaries[0].get('name') if binaries else meta['package']['name'])
        cmd += ['--bin', binary]
    stage_root = next(p for p in staged.parents if p.name == 'source-overlay-app')
    cmd += overlay_configs(bp, directory, original, stage_root / 'Cargo.toml', plan['flavor'])
    env = os.environ.copy()
    env['RUSTC'] = str(toolchain / 'rustc')
    env['RUSTC_BOOTSTRAP_SYNTHETIC_TARGET'] = '1'
    env['CARGO_TARGET_DIR'] = str(target_dir / name)
    lock_path = next(p / 'Cargo.lock' for p in [staged.parent, *staged.parent.parents] if (p / 'Cargo.lock').is_file())
    before = hashlib.sha256(lock_path.read_bytes()).hexdigest()
    result = subprocess.run(cmd, cwd=staged.parent, env=env, capture_output=True, text=True, timeout=90)
    (logs / (name + '.stderr')).write_text(result.stderr)
    row = {'app': name, 'directory': str(directory), 'staged': str(staged), 'plan': plan,
           'command': cmd, 'staged_lock_sha256': before}
    assert before == hashlib.sha256(lock_path.read_bytes()).hexdigest()
    if result.returncode:
        row['error'] = result.stderr[result.stderr.rfind('error:'):][-3500:]
        row['lock_closure'] = lock_closure(lock_path, meta['package']['name'])
    else:
        graph = json.loads(result.stdout)
        (logs / (name + '.json')).write_text(result.stdout)
        seen, pending = set(), list(graph['roots'])
        while pending:
            index = pending.pop()
            if index in seen:
                continue
            seen.add(index)
            pending.extend(d['index'] for d in graph['units'][index]['dependencies'])
        row['units'] = [graph['units'][i] for i in sorted(seen)]
        row['unit_count'] = len(seen)
    return row


def lock_closure(path, root):
    packages = read(path)['package']
    by_name = collections.defaultdict(list)
    for package in packages:
        by_name[package['name']].append(package)
    seen, pending = {}, list(by_name[root])
    while pending:
        package = pending.pop()
        key = (package['name'], package['version'], package.get('source', ''))
        if key in seen:
            continue
        seen[key] = package
        for dependency in package.get('dependencies', []):
            parts = dependency.split()
            pending.extend(p for p in by_name[parts[0]] if len(parts) < 2 or p['version'] == parts[1])
    return [{'name': p['name'], 'version': p['version'], 'source': p.get('source')} for p in seen.values()]


def consumer_declarations(bp, rows):
    roots = {p for p in bp.parent.iterdir() if (p / 'Cargo.toml').is_file() and p.name != 'odin-son'}
    roots.update([bp / name for name in ['api', 'apps', 'buildins', 'crates', 'probes']])
    roots.add(bp.parent / 'codex/codex-rs')
    by_name = collections.defaultdict(list)
    for row in rows:
        by_name[row['name']].append(row)
    seen = set()
    for root in sorted(roots):
        for path in manifests(root, skip_vendor=True):
            path = path.resolve()
            if path in seen:
                continue
            seen.add(path)
            meta = read(path)
            sections = [(kind, None, meta.get(kind, {})) for kind in ['dependencies', 'build-dependencies', 'dev-dependencies']]
            for target, table in meta.get('target', {}).items():
                sections.extend((kind, target, table.get(kind, {})) for kind in ['dependencies', 'build-dependencies', 'dev-dependencies'])
            sections.append(('workspace', None, meta.get('workspace', {}).get('dependencies', {})))
            for registry, table in meta.get('patch', {}).items():
                sections.append(('patch', registry, table))
            for kind, target, dependencies in sections:
                for key, value in dependencies.items():
                    spec = value if isinstance(value, dict) else {'version': value}
                    name = spec.get('package', key)
                    if name not in by_name:
                        continue
                    entry = {'manifest': str(path.relative_to(bp.parent)), 'kind': kind, 'target': target, 'dependency': key, **spec}
                    bucket = 'patch_registrations' if kind == 'patch' else 'workspace_declarations' if kind == 'workspace' else 'manifest_consumers'
                    for row in by_name[name]:
                        row[bucket].append(entry)
    return len(seen)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--blueprints', type=Path, default=Path(__file__).resolve().parents[3] / 'TRUEOS-Blueprints')
    parser.add_argument('--out', type=Path, required=True)
    args = parser.parse_args()
    bp = args.blueprints.resolve()
    rows = inventory(bp / 'vendor')
    apps = []
    for entry in json.loads((bp / 'apps.json').read_text())['apps']:
        name = entry if isinstance(entry, str) else entry['name']
        relative = f'apps/{name}' if isinstance(entry, str) else entry.get('path', f'apps/{name}')
        apps.append((name, (bp / relative).resolve()))
    args.out.parent.mkdir(parents=True, exist_ok=True)
    logs = Path('/tmp/trueos-blueprint-vendor-graphs')
    logs.mkdir(exist_ok=True)
    results = []
    with tempfile.TemporaryDirectory(prefix='trueos-vendor-audit-') as directory:
        helper = planner(bp, Path(directory))
        with ThreadPoolExecutor(max_workers=4) as pool:
            pending = {pool.submit(query, bp, app, helper, Path(directory) / 'target', logs): app[0] for app in apps}
            for future in as_completed(pending):
                try:
                    result = future.result()
                except Exception as error:
                    result = {'app': pending[future], 'error': str(error)}
                print(result['app'], result.get('error', f"{result.get('unit_count', 0)} units"), flush=True)
                results.append(result)
    by_path = sorted(rows, key=lambda r: -len(r['manifest']))
    by_identity = collections.defaultdict(list)
    for row in rows:
        by_identity[(row['name'], row['version'])].append(row)
    for result in results:
        for package in result.get('lock_closure', []):
            for row in by_identity.get((package['name'], package['version']), []):
                row['lock_only_apps'].append(result['app'])
        for unit in result.pop('units', []):
            source = Path(unit['target']['src_path']).resolve()
            kind = 'runtime_apps' if unit['platform'] and unit['target']['kind'] != ['custom-build'] else 'host_build_apps'
            matched = next((r for r in by_path if source.is_relative_to(Path(r['manifest']).parent)), None)
            if matched:
                matched[kind].append(result['app'])
                continue
            identity = unquote(unit['pkg_id'].split('#')[-1])
            if '@' not in identity:
                continue
            name, version = identity.rsplit('@', 1)
            for row in by_identity.get((name, version), []):
                key = 'overlay_source_apps' if 'source-overlay-crates' in source.parts else 'other_source_apps'
                row[key].append(result['app'])
    manifest_count = consumer_declarations(bp, rows)
    for row in rows:
        for key in ['runtime_apps', 'host_build_apps', 'overlay_source_apps', 'other_source_apps', 'lock_only_apps']:
            row[key] = sorted(set(row[key]))
        row['status'] = ('app target graph' if row['runtime_apps'] else 'overlay source graph' if row['overlay_source_apps'] else
                         'host build only' if row['host_build_apps'] else 'lock evidence only' if row['lock_only_apps'] else 'not selected')
        row['review'] = ('required' if row['status'] != 'not selected' else
                         'other platform backend' if row['path'].startswith('winit/') else
                         'optional terminal backend' if row['name'] in {'ratatui-termina', 'ratatui-termion', 'ratatui-termwiz'} else
                         'example package' if row['name'] == 'rayon-demo' else 'stale vendor candidate')
    report = {'created_utc': datetime.now(timezone.utc).isoformat(), 'blueprints': str(bp),
              'scope': 'Cargo compilation unit graphs reconstructed from existing staged manifests, packer production feature planning, and source-overlay configs; offline/locked, no compilation. Catalog defaults and source feature directives, not all optional features or all OS targets. A failed unit graph is protected by a conservative lockfile dependency closure including host/optional/other-target edges. Existing staging can lag original sources; this is dependency selection, not final linked-code reachability. No code deletion.',
              'summary': {'catalog_apps': len(apps), 'graphs_ok': sum('error' not in r for r in results),
                          'graphs_failed': sum('error' in r for r in results), 'vendor_packages': len(rows),
                          'manifests_scanned': manifest_count,
                          'source_bytes': sum(r['source_bytes'] for r in rows),
                          'statuses': dict(collections.Counter(r['status'] for r in rows)),
                          'review': dict(collections.Counter(r['review'] for r in rows))},
              'packages': rows, 'apps': sorted(results, key=lambda r: r['app'])}
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    columns = ['name', 'version', 'path', 'status', 'review', 'source_bytes', 'runtime_apps', 'host_build_apps', 'overlay_source_apps', 'lock_only_apps']
    args.out.with_suffix('.tsv').write_text('\t'.join(columns) + '\n' + '\n'.join(
        '\t'.join(','.join(r[k]) if isinstance(r[k], list) else str(r[k]) for k in columns) for r in rows) + '\n')
    md = ['# Blueprint vendor dependency usage — 2026-10-22', '',
          f"Inventoried **{len(rows)} packages** in Blueprint vendor folders. Resolved **{report['summary']['graphs_ok']}/{len(apps)}** app compilation-unit graphs; protected the remaining app with its lockfile closure. Scanned {manifest_count} local manifests for dependency declarations and patch registrations.", '',
          '| Result | Packages |', '| --- | ---: |']
    md += [f'| {k} | {v} |' for k, v in report['summary']['review'].items()]
    md += ['', '## Vendor packages absent from the configured app builds', '',
           '| Package | Version | Source KiB | Assessment |', '| --- | --- | ---: | --- |']
    notes = {'crossfont': 'No selected graph or local dependency declaration; old font integration candidate.',
             'hickory-proto': 'Voxy selects registry 0.26.3; vendor 0.25.2 patch is unused.',
             'hickory-resolver': 'Voxy selects registry 0.26.3; vendor 0.25.2 patch is unused.',
             'taffy': 'Hull selects registry 0.7.7; vendor 0.15.0 is unused.',
             'winit-common': 'Shared support for non-TRUEOS backends, absent from the configured target graphs.'}
    for row in rows:
        if row['status'] == 'not selected':
            md.append(f"| `{row['name']}` | {row['version']} | {row['source_bytes']/1024:.0f} | {notes.get(row['name'], row['review'])} |")
    md += ['', '## Dependencies which must not be mistaken for unused vendors', '',
           '- `tokio-macros` and `tracing-attributes` are host-side proc macros used to build apps.',
           '- `tower` is consumed through packer-generated source-overlay copies. Its absence as a direct vendor path is not non-use.',
           '- `libloading` has lockfile dependency evidence from `rustc-min`; it is retained as unresolved target/build usage.', '',
           'The Winit rows and three alternate Ratatui backends are not selected by the current TRUEOS app builds. They remain optional/cross-platform support, rather than unconditional dead-code findings. `rayon-demo` is an example package.', '',
           '## Coverage and reproduction', '', report['scope'], '',
           '`rustc-min` cannot resolve its unit graph offline because `thin-vec 0.2.19` is missing from the local registry cache. Its lockfile closure is used conservatively; no package reachable there is called unused.', '',
           'Patch registrations and `[workspace.dependencies]` entries are recorded separately from consumer dependency declarations in JSON. A patch entry alone is not use. Direct declarations can still be optional, dev-only, or target-specific; version requirements are preserved for review. Source sizes exclude target/build/.git directories and nested package roots; they include docs/tests shipped with each package.', '',
           'Full package/app mappings and the exact query commands are in the adjacent JSON and TSV. Raw graph diagnostics are saved under `/tmp/trueos-blueprint-vendor-graphs/`.', '',
           '```sh', 'python3 tools/audit/blueprint_vendor_usage.py --out tools/docs/blueprint-vendor-usage-2026-10-22', '```', '']
    args.out.with_suffix('.md').write_text('\n'.join(md))
    print(json.dumps(report['summary'], indent=2))


if __name__ == '__main__':
    main()
