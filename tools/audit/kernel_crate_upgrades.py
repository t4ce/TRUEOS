#!/usr/bin/env python3
"""Audit the selected kernel graph, registry releases, and vendored source deltas.

Plans Cargo units without compiling or executing build scripts. Downloads public
registry indexes/archives into /tmp; never edits manifests, locks, or vendors.
Three-way merges are estimates against published sources, not build validation.
"""
import argparse
import collections
import csv
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import difflib
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile
import tomllib
import urllib.error
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
SKIP = {'.git', 'target', 'tgt', 'bld', '__pycache__', 'node_modules'}
NONCODE = {'tests', 'test', 'examples', 'benches', 'docs', 'doc', 'tools', '.github'}
EXTENSIONS = {'.rs', '.c', '.h', '.S', '.s', '.asm', '.inc', '.wgsl', '.vert', '.frag'}


def run(args):
    return subprocess.run(args, cwd=ROOT, text=True, capture_output=True, check=True).stdout


def version(v):
    core, _, pre = v.split('+')[0].partition('-')
    return (*map(int, core.split('.')), not bool(pre),
            tuple((1, p) if not p.isdigit() else (0, int(p)) for p in pre.split('.')))


def same_line(a, b):
    a, b = version(a)[:3], version(b)[:3]
    return a[0] == b[0] if a[0] else a[:2] == b[:2] if a[1] else a == b


def manifest_at(src):
    for p in Path(src).parents:
        if (p / 'Cargo.toml').is_file():
            return p / 'Cargo.toml'
    raise ValueError(src)


def graph(features):
    args = ['cargo', 'build', '-p', 'TRUEOS', '--offline', '--locked',
            '-Z', 'unstable-options', '--unit-graph']
    if features:
        args += ['--features', features]
    g = json.loads(run(args))
    packages, edges = {}, collections.defaultdict(set)
    for u in g['units']:
        ident = u['pkg_id']
        if ident not in packages:
            manifest = manifest_at(u['target']['src_path'])
            p = tomllib.loads(manifest.read_text())['package']
            packages[ident] = {'id': ident, 'name': p['name'], 'version': p['version'],
                               'manifest': str(manifest), 'features': set(), 'roles': set()}
        packages[ident]['features'].update(u['features'])
        packages[ident]['roles'].add('host' if u['platform'] is None else 'kernel')
        for d in u['dependencies']:
            target = g['units'][d['index']]['pkg_id']
            if target != ident:
                edges[ident].add(target)
    root = g['units'][g['roots'][0]]['pkg_id']
    depths, queue = {root: 0}, collections.deque([root])
    while queue:
        ident = queue.popleft()
        for child in sorted(edges[ident]):
            if child not in depths:
                depths[child] = depths[ident] + 1
                queue.append(child)
    for ident, p in packages.items():
        p['features'], p['roles'] = sorted(p['features']), sorted(p['roles'])
        p['depth'] = depths.get(ident)
        p['parents'] = sorted(packages[k]['name'] for k in edges if ident in edges[k])
        p['dependencies'] = sorted(edges[ident])
        path = Path(p['manifest'])
        p['source'] = ('registry' if ident.startswith('registry+') else
                       'git' if ident.startswith('git+') else
                       'vendor' if path.is_relative_to(ROOT / 'vendor') else
                       'local' if path.is_relative_to(ROOT.parent) else 'sysroot')
    return {'command': args, 'units': len(g['units']), 'root': root,
            'packages': sorted(packages.values(), key=lambda p: (p['name'], p['version']))}


def fetch(url, destination):
    req = urllib.request.Request(url, headers={'User-Agent': 'TRUEOS-kernel-crate-audit/1.0'})
    with urllib.request.urlopen(req, timeout=45) as r:
        data = r.read()
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(data)
    return data


def registry(name, cache, refresh):
    folder = '1' if len(name) == 1 else '2' if len(name) == 2 else '3/' + name[0] if len(name) == 3 else name[:2] + '/' + name[2:4]
    url = 'https://index.crates.io/' + folder + '/' + name
    path = cache / 'index' / name
    try:
        data = fetch(url, path) if refresh or not path.exists() else path.read_bytes()
        return {'url': url, 'versions': [json.loads(line) for line in data.splitlines()]}
    except (urllib.error.URLError, TimeoutError) as e:
        return {'url': url, 'error': str(e), 'versions': []}


def release_info(p, record):
    stable = [v for v in record['versions'] if not v['yanked'] and '-' not in v['vers']]
    releases = [v for v in record['versions'] if not v['yanked']]
    current = p['version'].split('-trueos')[0]
    latest = max(stable, key=lambda v: version(v['vers']), default=None)
    line = max((v for v in stable if same_line(current, v['vers'])),
               key=lambda v: version(v['vers']), default=None)
    newest = max(releases, key=lambda v: version(v['vers']), default=None)
    def brief(v):
        return {k: v.get(k) for k in ('vers', 'rust_version', 'pubtime')} if v else None
    return {'index_url': record['url'], 'error': record.get('error'),
            'latest_stable': brief(latest), 'latest_same_semver_line': brief(line),
            'latest_including_prerelease': brief(newest),
            'newer_stable': bool(latest and version(latest['vers']) > version(current))}


def archive(name, release, cache):
    v = release['vers']
    dest = cache / 'archives' / f'{name}-{v}.crate'
    url = f'https://static.crates.io/crates/{name}/{name}-{v}.crate'
    if not dest.exists():
        cached = list((Path.home() / '.cargo/registry/cache').glob(f'*/{name}-{v}.crate'))
        if cached:
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(cached[0].read_bytes())
        else:
            fetch(url, dest)
    if hashlib.sha256(dest.read_bytes()).hexdigest() != release['cksum']:
        raise ValueError('archive checksum mismatch: ' + str(dest))
    folder = cache / 'sources' / f'{name}-{v}'
    if not folder.exists():
        folder.parent.mkdir(parents=True, exist_ok=True)
        with tarfile.open(dest) as tar:
            tar.extractall(folder.parent, filter='data')
    return folder, url


def codefiles(root):
    return {str(p.relative_to(root)): p.read_bytes() for p in root.rglob('*')
            if p.is_file() and not p.is_symlink() and p.suffix in EXTENSIONS
            and not (set(p.relative_to(root).parts) & (SKIP | NONCODE))}


def compare(a, b):
    rows = []
    for name in sorted(a.keys() | b.keys()):
        old, new = a.get(name, b''), b.get(name, b'')
        if old == new:
            continue
        delta = list(difflib.unified_diff(old.decode('utf-8', errors='replace').splitlines(),
                                        new.decode('utf-8', errors='replace').splitlines(), n=0))
        rows.append({'file': name, 'added': sum(s.startswith('+') for s in delta[2:]),
                     'removed': sum(s.startswith('-') for s in delta[2:]),
                     'status': 'added' if name not in a else 'deleted' if name not in b else 'modified'})
    return rows


def manifest_delta(a, b):
    def flatten(value, prefix=''):
        out = {}
        for k, v in value.items():
            key = prefix + '.' + k if prefix else k
            if isinstance(v, dict):
                out.update(flatten(v, key))
            else:
                out[key] = v
        return out
    a, b = flatten(a), flatten(b)
    return [{'key': k, 'upstream': a.get(k), 'local': b.get(k)}
            for k in sorted(a.keys() | b.keys()) if a.get(k) != b.get(k)]


def declarations(parent):
    path = Path(parent['manifest'])
    manifest = tomllib.loads(path.read_text())
    sections = [('dependencies', manifest.get('dependencies', {})),
                ('build-dependencies', manifest.get('build-dependencies', {}))]
    for target, tables in manifest.get('target', {}).items():
        for kind in ('dependencies', 'build-dependencies'):
            sections.append((f'target.{target}.{kind}', tables.get(kind, {})))
    result = []
    for section, dependencies in sections:
        for alias, spec in dependencies.items():
            spec = {'version': spec} if isinstance(spec, str) else dict(spec)
            if spec.get('workspace'):
                for folder in path.parents:
                    candidate = folder / 'Cargo.toml'
                    if candidate.is_file():
                        workspace = tomllib.loads(candidate.read_text()).get('workspace', {}).get('dependencies', {})
                        if alias in workspace:
                            inherited = workspace[alias]
                            inherited = {'version': inherited} if isinstance(inherited, str) else inherited
                            spec = dict(inherited, **spec)
                            break
            result.append({'name': spec.get('package', alias), 'requirement': spec.get('version'),
                           'path': spec.get('path'), 'git': spec.get('git'),
                           'parent': parent['name'], 'manifest': str(path), 'section': section})
    return result


def merge_delta(base, local, new, cache):
    details = []
    work = cache / 'merge'
    work.mkdir(exist_ok=True)
    for name in sorted(base.keys() | local.keys()):
        b, l, n = base.get(name), local.get(name), new.get(name)
        if b == l:
            continue
        if b is None or l is None or n is None:
            conflict = l != n and b != n
            details.append({'file': name, 'conflict_hunks': int(conflict),
                            'status': 'add/delete conflict' if conflict else 'clean add/delete'})
            continue
        if l == n:
            details.append({'file': name, 'conflict_hunks': 0, 'status': 'already upstream'})
            continue
        paths = [work / k for k in ('local', 'base', 'new')]
        for p, data in zip(paths, (l, b, n)):
            p.write_bytes(data)
        r = subprocess.run(['git', 'merge-file', '-p', '--diff3', *map(str, paths)], capture_output=True)
        if r.returncode < 0 or r.returncode > 127:
            raise RuntimeError(r.stderr.decode(errors='replace'))
        details.append({'file': name, 'conflict_hunks': r.stdout.count(b'<<<<<<< '),
                        'status': 'conflict' if r.returncode else 'clean'})
    return {'conflict_files': sum(bool(d['conflict_hunks']) for d in details),
            'conflict_hunks': sum(d['conflict_hunks'] for d in details), 'files': details}


def vendor_delta(p, record, cache):
    baseline = p['version'].split('-trueos')[0]
    release = next((v for v in record['versions'] if v['vers'] == baseline), None)
    if not release:
        return {'error': 'No published release matches local version; historical baseline requires investigation.'}
    base, url = archive(p['name'], release, cache)
    local = Path(p['manifest']).parent
    a, b = codefiles(base), codefiles(local)
    delta = compare(a, b)
    manifest = tomllib.loads((base / 'Cargo.toml').read_text())
    local_manifest = tomllib.loads((local / 'Cargo.toml').read_text())
    result = {'baseline_version': baseline, 'baseline_archive_url': url,
              'baseline_sha256': release['cksum'], 'code_files': len(delta),
              'added_lines': sum(d['added'] for d in delta),
              'removed_lines': sum(d['removed'] for d in delta), 'code_delta': delta,
              'manifest_delta': manifest_delta(manifest, local_manifest), 'upgrades': []}
    current_vcs = local / '.cargo_vcs_info.json'
    base_vcs = base / '.cargo_vcs_info.json'
    result['vcs_revision_matches_release'] = (json.loads(current_vcs.read_text()) == json.loads(base_vcs.read_text())
                                              if current_vcs.exists() and base_vcs.exists() else None)
    releases = release_info(p, record)
    versions = {v['vers']: v for v in record['versions']}
    candidates = {releases[k]['vers'] for k in ('latest_stable', 'latest_same_semver_line',
                                               'latest_including_prerelease') if releases[k]}
    for candidate in sorted(candidates, key=version):
        if version(candidate) <= version(baseline):
            continue
        folder, new_url = archive(p['name'], versions[candidate], cache)
        new = codefiles(folder)
        touched = {d['file'] for d in compare(a, new)}
        newer_manifest = tomllib.loads((folder / 'Cargo.toml').read_text())
        upstream_manifest_delta = {d['key']: d for d in manifest_delta(manifest, newer_manifest)}
        manifest_disagreements = [d['key'] for d in result['manifest_delta']
                                  if d['key'] in upstream_manifest_delta
                                  and d['local'] != upstream_manifest_delta[d['key']]['local']]
        result['upgrades'].append({'version': candidate, 'archive_url': new_url,
                                   'sha256': versions[candidate]['cksum'],
                                   'same_semver_line': same_line(baseline, candidate),
                                   'prerelease': '-' in candidate,
                                   'manifest_disagreement_keys': manifest_disagreements,
                                   'upstream_changed_code_files': len(touched),
                                   'overlapping_code_files': sorted(touched & {d['file'] for d in delta}),
                                   'merge': merge_delta(a, b, new, cache)})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, required=True)
    parser.add_argument('--cache', type=Path, default=Path('/tmp/trueos-kernel-crate-audit'))
    parser.add_argument('--cached-index', action='store_true', help='reuse previously fetched registry records')
    parser.add_argument('--validation-json', type=Path, help='attach separately performed probe results')
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    graphs = {'default': graph(''), 'ttstt': graph('trueos_ttstt')}
    packages = {p['id']: dict(p, graphs=[]) for g in graphs.values() for p in g['packages']}
    for name, g in graphs.items():
        for p in g['packages']:
            packages[p['id']]['graphs'].append(name)
    for p in packages.values():
        p['declared_requirements'] = []
    for parent in packages.values():
        for decl in declarations(parent):
            for ident in parent['dependencies']:
                child = packages[ident]
                if child['name'] == decl['name']:
                    child['declared_requirements'].append(decl)
    third_party = [p for p in packages.values() if p['source'] in {'vendor', 'registry', 'git'}]
    names = sorted({p['name'] for p in third_party})
    print(f'Querying {len(names)} crate indexes for {len(third_party)} selected third-party packages.', flush=True)
    with ThreadPoolExecutor(max_workers=4) as pool:
        records = dict(zip(names, pool.map(lambda n: registry(n, args.cache, not args.cached_index), names)))
    for p in third_party:
        p['registry'] = release_info(p, records[p['name']])
        if p['source'] == 'vendor':
            print(f'Comparing {p["name"]} {p["version"]}', flush=True)
            try:
                p['vendor_delta'] = vendor_delta(p, records[p['name']], args.cache)
            except (urllib.error.URLError, TimeoutError, ValueError) as e:
                p['vendor_delta'] = {'error': str(e)}
    report = {'checked_at_utc': datetime.now(timezone.utc).isoformat(),
              'git_head': run(['git', 'rev-parse', 'HEAD']).strip(),
              'rustc': run(['rustc', '--version']).strip(),
              'method': 'Target-specific Cargo units; published non-yanked releases; SHA256-verified archives; code-only three-way textual merges.',
              'limitations': ['A same-semver-line version can still violate exact or upper-bound manifest constraints.',
                              'Textual conflicts do not measure API, feature, dependency, target, or runtime compatibility.',
                              'Published version is a comparison baseline, not proof of the original fork revision.',
                              'Code counts exclude tests/examples/docs/tools directories and non-code assets; inline test modules remain counted. Normalized manifest deltas are separate.',
                              'Sysroot crates are excluded from upgrade recommendations; optional features other than trueos_ttstt are not expanded.'],
              'graphs': {name: {**{k: v for k, v in g.items() if k != 'packages'},
                                'package_ids': [p['id'] for p in g['packages']],
                                'source_counts': dict(collections.Counter(p['source'] for p in g['packages']))}
                         for name, g in graphs.items()},
              'packages': sorted(packages.values(), key=lambda p: (p['name'], p['version']))}
    if args.validation_json:
        report['validation'] = json.loads(args.validation_json.read_text())
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.with_suffix('.json').write_text(json.dumps(report, indent=2) + '\n')
    with args.out.with_suffix('.tsv').open('w') as f:
        writer = csv.writer(f, delimiter='\t')
        writer.writerow(['name', 'current', 'source', 'depth', 'roles', 'latest_stable',
                         'latest_same_semver_line', 'latest_including_prerelease',
                         'local_code_files', 'local_added_lines', 'local_removed_lines',
                         'upgrade_conflicts_version:files:hunks', 'registry_error'])
        for p in sorted(third_party, key=lambda p: (p['name'], p['version'])):
            r, d = p['registry'], p.get('vendor_delta', {})
            writer.writerow([p['name'], p['version'], p['source'], p['depth'], ','.join(p['roles']),
                             *[(r[k] or {}).get('vers', '') for k in ('latest_stable', 'latest_same_semver_line', 'latest_including_prerelease')],
                             *[d.get(k, '') for k in ('code_files', 'added_lines', 'removed_lines')],
                             ';'.join(f'{u["version"]}:{u["merge"]["conflict_files"]}:{u["merge"]["conflict_hunks"]}'
                                      for u in d.get('upgrades', [])), r.get('error') or d.get('error', '')])
    print('Wrote ' + str(args.out.with_suffix('.json')), flush=True)


if __name__ == '__main__':
    main()
