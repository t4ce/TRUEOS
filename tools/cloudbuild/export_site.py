#!/usr/bin/env python3
"""Package the selector-only Python runtime into an existing TRUEOS website.

This copies code and the public catalog, never state, credentials or artifacts.
It does not restart a website or enable cloud build submissions.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[2]
RUNTIME = ('cloudbuild.py', 'build_plan.py', 'service.py', 'identity.py', 'github_actions.py',
           'ci.py', 'buildins.py', 'worker.py', 'catalog.json')


def export(site):
    source = ROOT / 'tools/cloudbuild'
    if not (site / 'App.js').is_file():
        raise ValueError('Choose an existing TRUEOS_SITE directory containing App.js')
    destination = site / 'tools/cloudbuild'
    destination.mkdir(parents=True, exist_ok=True)
    for name in RUNTIME:
        shutil.copyfile(source / name, destination / name)
    for name in ('web', 'measurements'):
        for path in (source / name).rglob('*'):
            if path.is_file():
                target = destination / name / path.relative_to(source / name)
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(path, target)
    revision = subprocess.check_output(['git', '-C', str(ROOT), 'rev-parse', 'HEAD'], text=True).strip()
    (destination / 'runtime.json').write_text(json.dumps({
        'schema': 1, 'mode': 'selector-only', 'source_commit': revision,
    }, indent=2) + '\n')
    for original, target in [('site-proxy.cjs', 'cloudbuild-proxy.js'), ('site-host.cjs', 'cloudbuild-host.cjs')]:
        shutil.copyfile(source / 'deploy' / original, site / 'tools' / target)
    print('Packaged selector-only runtime. Restart Node to load it. No Rust compiler is needed.')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--site', type=Path, required=True)
    args = parser.parse_args()
    export(args.site.resolve())
