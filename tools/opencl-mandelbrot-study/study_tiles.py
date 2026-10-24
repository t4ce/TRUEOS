#!/usr/bin/env python3
"""Compile three subdivision strategies and check their source on the CPU."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
TOOLS = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--out', type=Path, default=ROOT / 'bld/opencl-mandelbrot-recursion')
    args = parser.parse_args()
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    builds = {}
    for mode, define in [('recursive', 'RECURSIVE_PROBE=1'), ('stack', None), ('parallel', 'PARALLEL_LEAVES=1')]:
        command = [sys.executable, str(TOOLS / 'compile.py'), '--source', str(TOOLS / 'mandelbrot_tiles.cl'),
                   '--out', str(out / mode)]
        if define:
            command += ['--define', define]
        subprocess.run(command, cwd=ROOT, check=True, timeout=60)
        builds[mode] = json.loads((out / mode / 'results.json').read_text())
    command = ['g++', '-std=c++17', '-O2', '-ffp-contract=off', '-Wno-unknown-pragmas',
               '-fsanitize=address,undefined', '-fno-omit-frame-pointer',
               str(TOOLS / 'check_tiles.cpp'), '-o', str(out / 'check-tiles')]
    subprocess.run(command, cwd=ROOT, check=True, timeout=60)
    env = os.environ.copy()
    # LeakSanitizer cannot run under the execution environment's ptrace policy.
    env['ASAN_OPTIONS'] = 'detect_leaks=0'
    result = subprocess.run([str(out / 'check-tiles')], cwd=ROOT, env=env, check=True,
                            text=True, stdout=subprocess.PIPE, timeout=60)
    (out / 'cpu-check.log').write_text(result.stdout)
    print(result.stdout)
    (out / 'results.json').write_text(json.dumps({'baseline': {'width':512, 'height':512,
          'leaf_side':64, 'leaves':64, 'output_bytes':1048576, 'local_size':256,
          'origin':[-2.0,-1.5], 'step':3.0/512.0, 'max_iterations':64},
          'checks':'Offline native compilation plus CPU source simulation; no GPU execution.',
          'strategies':builds}, indent=2) + '\n')


if __name__ == '__main__':
    main()
