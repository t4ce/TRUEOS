#!/usr/bin/env bash
set -euo pipefail

qemu_pid="${1:-}"
host="${TRUEOS_EMULATOR_LOG_HOST:-127.0.0.1}"
port="${TRUEOS_EMULATOR_LOG_PORT:-5555}"
log_dir="${TRUEOS_EMULATOR_LOG_DIR:-bld/emulator-logs}"
slot_file="${TRUEOS_EMULATOR_LOG_SLOT:-bld/emulator-log-capture.slot}"
slots="${TRUEOS_EMULATOR_LOG_SLOTS:-3}"

if ! [[ "$qemu_pid" =~ ^[0-9]+$ ]]; then
    echo "emulator-log-capture: qemu pid is required" >&2
    exit 2
fi
if ! command -v nc >/dev/null 2>&1; then
    echo "emulator-log-capture: nc not found" >&2
    exit 1
fi
if ! command -v tee >/dev/null 2>&1; then
    echo "emulator-log-capture: tee not found" >&2
    exit 1
fi
if ! [[ "$slots" =~ ^[0-9]+$ ]] || [[ "$slots" -lt 1 ]]; then
    echo "emulator-log-capture: TRUEOS_EMULATOR_LOG_SLOTS must be >= 1" >&2
    exit 1
fi

mkdir -p "$log_dir" "$(dirname "$slot_file")"

previous="$(cat "$slot_file" 2>/dev/null || echo -1)"
if ! [[ "$previous" =~ ^-?[0-9]+$ ]]; then
    previous=-1
fi

next=$(( (previous + 1) % slots ))
printf '%s\n' "$next" > "$slot_file"

log_path="$log_dir/trueos-emulator.$next.log"
: > "$log_path"
ln -sfn "$(basename "$log_path")" "$log_dir/latest.log"

echo "emulator-log-capture: log=$(realpath "$log_path") latest=$(realpath "$log_dir/latest.log") target=$host:$port"
printf 'trueos emulator log capture: target=%s:%s started_at=%s\n' \
    "$host" "$port" "$(date -Is)" >> "$log_path"

while :; do
    set +e
    nc -d "$host" "$port" 2>/dev/null | tee -a "$log_path"
    pipeline_status=("${PIPESTATUS[@]}")
    nc_status=${pipeline_status[0]}
    tee_status=${pipeline_status[1]}
    set -e

    if [[ "$tee_status" -ne 0 ]]; then
        exit "$tee_status"
    fi
    if [[ "$nc_status" -eq 0 ]]; then
        break
    fi

    qemu_state="$(ps -o stat= -p "$qemu_pid" 2>/dev/null || true)"
    if [[ -z "$qemu_state" || "$qemu_state" == Z* ]]; then
        break
    fi
    sleep 0.1
done
