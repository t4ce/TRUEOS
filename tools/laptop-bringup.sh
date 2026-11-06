#!/usr/bin/env bash
# Dedicated capture for the kitchen laptop; never resets or stops the rig drain.
set -euo pipefail
root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$root"
action=${1:-status}
export TRUEOS_BAREMETAL_LOG_HOST=${TRUEOS_LAPTOP_HOST:-192.168.178.90}
export TRUEOS_BAREMETAL_LOG_PORT=1
export TRUEOS_BAREMETAL_LOG_DELAY=0
export TRUEOS_BAREMETAL_LOG_DIR="$root/bld/laptop-bringup/logs"
export TRUEOS_BAREMETAL_LOG_PID="$root/bld/laptop-bringup/drain.pid"
export TRUEOS_BAREMETAL_LOG_SLOT="$root/bld/laptop-bringup/drain.slot"
export TRUEOS_BAREMETAL_LOG_LOCK="$root/bld/laptop-bringup/drain.lock"
export TRUEOS_BAREMETAL_BOOT_MARKER='log-profile=laptop-usb-gpu-bringup-v1'
if [[ -f bld/laptop-bringup/build-id ]]; then
    export TRUEOS_BAREMETAL_BOOT_MARKER="laptop-bringup-build-id=$(cat bld/laptop-bringup/build-id)"
fi
if [[ -f tgt/laptop-bringup/TRUEOS.elf ]]; then
    TRUEOS_BAREMETAL_EXPECTED_ELF_SHA256=$(sha256sum tgt/laptop-bringup/TRUEOS.elf | cut -d ' ' -f 1)
    export TRUEOS_BAREMETAL_EXPECTED_ELF_SHA256
fi
case "$action" in
    start|status|stop|wait)
        exec bash tools/baremetal-log-drain.sh "$action"
        ;;
    observe)
        # No mutations: preserve HTTP status, headers and bytes in a new capture.
        exec python3 - "$TRUEOS_BAREMETAL_LOG_HOST" <<'PY'
from datetime import datetime, timezone
from pathlib import Path
import json
import sys
import urllib.error
import urllib.request

host = sys.argv[1]
destination = Path('bld/laptop-bringup') / datetime.now(timezone.utc).strftime('observe-%Y%m%dT%H%M%S.%fZ')
destination.mkdir(parents=True)
opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
failed = False
for name, endpoint, is_json in (
    ('describe', '/usb/heal/describe', True),
    ('observe', '/usb/heal/observe', True),
    ('wifi', '/wifi/observe', False),
):
    url = f'http://{host}{endpoint}'
    try:
        with opener.open(url, timeout=10) as response:
            body = response.read()
            output = destination / f'{name}.{"json" if is_json else "txt"}'
            output.write_bytes(body)
            (destination / f'{name}.headers').write_text(str(response.headers))
            if is_json:
                json.loads(body)
            print(f'{response.status} {url} -> {output}')
    except (OSError, ValueError) as error:
        (destination / f'{name}.error').write_text(str(error))
        print(f'{url}: {error}', file=sys.stderr)
        failed = True
raise SystemExit(int(failed))
PY
        ;;
    *)
        echo "usage: bash tools/laptop-bringup.sh {start|status|wait|observe|stop}" >&2
        echo "set TRUEOS_LAPTOP_HOST if the displayed Wi-Fi last octet changes" >&2
        exit 2
        ;;
esac
