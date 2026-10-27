#!/usr/bin/env bash
# Bound the whole build process tree, including native linkers and host tools.
set -euo pipefail
if [[ $# == 0 ]]; then
    echo "usage: $0 COMMAND [ARGS...]" >&2
    exit 2
fi
for tool in systemd-run taskset nice python3; do
    command -v "$tool" >/dev/null || { echo "Missing build containment tool: $tool" >&2; exit 1; }
done
cpus=${TRUEOS_BUILD_CPUS:-}
if [[ -z "$cpus" ]]; then
    cpus=$(python3 - <<'PY'
import os
from pathlib import Path
allowed = os.sched_getaffinity(0)
atom = Path('/sys/devices/cpu_atom/cpus')
efficient = set()
if atom.is_file():
    for field in atom.read_text().strip().split(','):
        bounds = list(map(int, field.split('-')))
        efficient.update(range(bounds[0], bounds[-1] + 1))
print(min(allowed & efficient or allowed))
PY
)
fi
echo "Build scope: CPUs=$cpus, CPU quota=1 core, RAM limit=6 GiB, swap disabled, jobs=1"
# Scope failure is fatal. Never retry the build without containment.
exec systemd-run --user --scope --quiet --expand-environment=no \
    --property=MemoryHigh=5G --property=MemoryMax=6G --property=MemorySwapMax=0 \
    --property=CPUQuota=100% --property=TasksMax=128 \
    -- taskset -c "$cpus" nice -n 15 env TRUEOS_BUILD_SCOPE=1 CARGO_BUILD_JOBS=1 RAYON_NUM_THREADS=1 "$@"
