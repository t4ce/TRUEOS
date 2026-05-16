#!/usr/bin/env bash
set -euo pipefail

cmd="${1:-start}"

host="${TRUEOS_BAREMETAL_LOG_HOST:-192.168.178.94}"
port="${TRUEOS_BAREMETAL_LOG_PORT:-1}"
delay="${TRUEOS_BAREMETAL_LOG_DELAY:-0}"
log_dir="${TRUEOS_BAREMETAL_LOG_DIR:-bld/baremetal-logs}"
pid_file="${TRUEOS_BAREMETAL_LOG_PID:-bld/baremetal-log-drain.pid}"
slot_file="${TRUEOS_BAREMETAL_LOG_SLOT:-bld/baremetal-log-drain.slot}"
slots="${TRUEOS_BAREMETAL_LOG_SLOTS:-3}"

kill_existing() {
    if [[ ! -f "$pid_file" ]]; then
        return 0
    fi

    local pid
    pid="$(cat "$pid_file" 2>/dev/null || true)"
    if [[ "$pid" =~ ^[0-9]+$ ]] && kill -0 "$pid" 2>/dev/null; then
        kill -TERM "-$pid" 2>/dev/null || kill -TERM "$pid" 2>/dev/null || true
        sleep 0.2
        if kill -0 "$pid" 2>/dev/null; then
            kill -KILL "-$pid" 2>/dev/null || kill -KILL "$pid" 2>/dev/null || true
        fi
    fi

    rm -f "$pid_file"
}

next_log_path() {
    mkdir -p "$log_dir" "$(dirname "$pid_file")" "$(dirname "$slot_file")"

    local previous next
    previous="$(cat "$slot_file" 2>/dev/null || echo -1)"
    if ! [[ "$previous" =~ ^-?[0-9]+$ ]]; then
        previous=-1
    fi

    next=$(( (previous + 1) % slots ))
    printf '%s\n' "$next" > "$slot_file"
    printf '%s/trueos-baremetal.%s.log\n' "$log_dir" "$next"
}

start() {
    if ! command -v nc >/dev/null 2>&1; then
        echo "baremetal-log-drain: nc not found" >&2
        exit 1
    fi

    if ! [[ "$slots" =~ ^[0-9]+$ ]] || [[ "$slots" -lt 1 ]]; then
        echo "baremetal-log-drain: TRUEOS_BAREMETAL_LOG_SLOTS must be >= 1" >&2
        exit 1
    fi

    kill_existing

    local log_path
    log_path="$(next_log_path)"
    : > "$log_path"
    ln -sfn "$(basename "$log_path")" "$log_dir/latest.log"

    setsid bash -c '
        set -euo pipefail
        delay="$1"
        host="$2"
        port="$3"
        log_path="$4"

        {
            printf "trueos baremetal log drain: delay=%ss target=%s:%s started_at=%s\n" "$delay" "$host" "$port" "$(date -Is)"
            if [[ "$delay" != "0" ]]; then
                sleep "$delay"
            fi
            printf "trueos baremetal log drain: connecting_at=%s\n" "$(date -Is)"
            exec nc "$host" "$port"
        } >> "$log_path" 2>&1
    ' baremetal-log-drain "$delay" "$host" "$port" "$log_path" &

    local pid=$!
    printf '%s\n' "$pid" > "$pid_file"
    echo "baremetal-log-drain: pid=$pid log=$(realpath "$log_path") latest=$(realpath "$log_dir/latest.log") target=$host:$port delay=${delay}s"
}

case "$cmd" in
    start)
        start
        ;;
    stop | snipe)
        kill_existing
        ;;
    *)
        echo "usage: $0 {start|stop|snipe}" >&2
        exit 2
        ;;
esac
