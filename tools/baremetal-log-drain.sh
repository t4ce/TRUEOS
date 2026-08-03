#!/usr/bin/env bash
set -euo pipefail

cmd="${1:-start}"

host="${TRUEOS_BAREMETAL_LOG_HOST:-192.168.178.94}"
port="${TRUEOS_BAREMETAL_LOG_PORT:-1}"
delay="${TRUEOS_BAREMETAL_LOG_DELAY:-5}"
retry_delay="${TRUEOS_BAREMETAL_LOG_RETRY_DELAY:-1}"
log_dir="${TRUEOS_BAREMETAL_LOG_DIR:-bld/baremetal-logs}"
state_file="${TRUEOS_BAREMETAL_LOG_PID:-bld/baremetal-log-drain.pid}"
slot_file="${TRUEOS_BAREMETAL_LOG_SLOT:-bld/baremetal-log-drain.slot}"
lock_file="${TRUEOS_BAREMETAL_LOG_LOCK:-bld/baremetal-log-drain.lock}"
slots="${TRUEOS_BAREMETAL_LOG_SLOTS:-3}"
wait_timeout="${TRUEOS_BAREMETAL_LOG_WAIT_TIMEOUT:-180}"
boot_marker="${TRUEOS_BAREMETAL_BOOT_MARKER:-[service] [info] spawn-svc: started net-shell-listener}"
expected_elf_sha256="${TRUEOS_BAREMETAL_EXPECTED_ELF_SHA256:-unknown}"
expected_iso_sha256="${TRUEOS_BAREMETAL_EXPECTED_ISO_SHA256:-unknown}"
physical_reset_receipt="${TRUEOS_TESTRIG_PHYSICAL_RESET_RECEIPT:-none}"

script_path="$(realpath -m -- "${BASH_SOURCE[0]}")"
repo_root="$(realpath -m -- "$(dirname "$script_path")/..")"
log_dir="$(realpath -m -- "$log_dir")"
state_file="$(realpath -m -- "$state_file")"
slot_file="$(realpath -m -- "$slot_file")"
lock_file="$(realpath -m -- "$lock_file")"

die() {
    echo "baremetal-log-drain: error: $*" >&2
    exit 1
}

validate_config() {
    [[ "$host" != *$'\n'* && -n "$host" ]] ||
        die "TRUEOS_BAREMETAL_LOG_HOST must be one non-empty line"
    [[ "$port" =~ ^[0-9]+$ ]] && ((port >= 1 && port <= 65535)) ||
        die "TRUEOS_BAREMETAL_LOG_PORT must be in 1..65535"
    [[ "$delay" =~ ^[0-9]+$ ]] ||
        die "TRUEOS_BAREMETAL_LOG_DELAY must be a non-negative integer"
    [[ "$retry_delay" =~ ^[0-9]+$ ]] && ((retry_delay >= 1)) ||
        die "TRUEOS_BAREMETAL_LOG_RETRY_DELAY must be >= 1"
    [[ "$slots" =~ ^[0-9]+$ ]] && ((slots >= 1)) ||
        die "TRUEOS_BAREMETAL_LOG_SLOTS must be >= 1"
    [[ "$wait_timeout" =~ ^[0-9]+$ ]] && ((wait_timeout >= 1)) ||
        die "TRUEOS_BAREMETAL_LOG_WAIT_TIMEOUT must be >= 1"
    [[ "$boot_marker" != *$'\n'* && -n "$boot_marker" ]] ||
        die "TRUEOS_BAREMETAL_BOOT_MARKER must be one non-empty line"
}

state_field() {
    local key="$1"
    sed -n "s/^${key}=//p" "$state_file" 2>/dev/null | head -n 1
}

process_start_ticks() {
    local pid="$1"
    awk '{print $22}' "/proc/$pid/stat" 2>/dev/null
}

process_alive() {
    local pid="$1"
    local state
    [[ -r "/proc/$pid/stat" ]] || return 1
    state="$(awk '{print $3}' "/proc/$pid/stat" 2>/dev/null || true)"
    [[ -n "$state" && "$state" != "Z" ]]
}

signal_exact_pid() {
    local pid="$1"
    local signal_name="$2"
    process_alive "$pid" || return 0
    if kill "-$signal_name" -- "$pid" 2>/dev/null; then
        return 0
    fi
    command -v systemd-run >/dev/null 2>&1 ||
        die "could not signal exact pid=$pid with $signal_name and systemd-run is unavailable"
    systemd-run --user --wait --pipe --quiet \
        /bin/kill "-$signal_name" "$pid" ||
        die "user-manager exact-PID signal failed pid=$pid signal=$signal_name"
}

process_argv_has_identity() {
    local pid="$1"
    local run_id="$2"
    local -a argv=()
    local i

    [[ -r "/proc/$pid/cmdline" ]] || return 1
    mapfile -d '' -t argv < "/proc/$pid/cmdline" || return 1
    for ((i = 0; i + 2 < ${#argv[@]}; i++)); do
        if [[ "${argv[$i]}" == "$script_path" &&
            "${argv[$((i + 1))]}" == "collect" &&
            "${argv[$((i + 2))]}" == "$run_id" ]]; then
            return 0
        fi
    done
    return 1
}

tracked_process_owned() {
    [[ -f "$state_file" ]] || return 1

    local pid pgid run_id start_ticks actual_pgid actual_start_ticks
    pid="$(state_field pid)"
    pgid="$(state_field pgid)"
    run_id="$(state_field run_id)"
    start_ticks="$(state_field start_ticks)"

    [[ "$pid" =~ ^[0-9]+$ && "$pgid" =~ ^[0-9]+$ ]] || return 1
    [[ "$start_ticks" =~ ^[0-9]+$ && "$run_id" =~ ^[0-9a-f]{16,64}$ ]] || return 1
    # A collector is deliberately its own session/process-group leader. Refuse
    # a group signal if state corruption could point at somebody else's group.
    [[ "$pid" == "$pgid" ]] || return 1
    kill -0 "$pid" 2>/dev/null || return 1

    actual_pgid="$(ps -o pgid= -p "$pid" 2>/dev/null | tr -d '[:space:]')"
    actual_start_ticks="$(process_start_ticks "$pid")"
    [[ "$actual_pgid" == "$pgid" && "$actual_start_ticks" == "$start_ticks" ]] ||
        return 1
    process_argv_has_identity "$pid" "$run_id"
}

stop_tracked() {
    [[ -f "$state_file" ]] || return 0

    local pid pgid member
    local -a members=()
    pid="$(state_field pid)"
    pgid="$(state_field pgid)"
    if tracked_process_owned; then
        mapfile -t members < <(
            ps -eo pid=,pgid= |
                awk -v wanted="$pgid" '$2 == wanted { print $1 }'
        )
        ((${#members[@]} > 0)) ||
            die "tracked collector group has no visible members pid=$pid pgid=$pgid"
        # Signal only positive PIDs observed in the already validated private
        # collector group. This also handles legacy nc.openbsd children whose
        # AppArmor profile rejects a direct signal from this shell.
        for member in "${members[@]}"; do
            [[ "$member" == "$pid" ]] || signal_exact_pid "$member" TERM
        done
        signal_exact_pid "$pid" TERM
        for _ in {1..20}; do
            local alive=0
            for member in "${members[@]}"; do
                if process_alive "$member"; then
                    alive=1
                    break
                fi
            done
            ((alive == 0)) && break
            sleep 0.05
        done
        for member in "${members[@]}"; do
            process_alive "$member" && signal_exact_pid "$member" KILL
        done
        for _ in {1..40}; do
            local present=0
            for member in "${members[@]}"; do
                if [[ -e "/proc/$member" ]]; then
                    present=1
                    break
                fi
            done
            ((present == 0)) && break
            sleep 0.05
        done
        for member in "${members[@]}"; do
            [[ -e "/proc/$member" ]] &&
                die "tracked collector member still has /proc state after teardown pid=$member pgid=$pgid"
        done
        echo "baremetal-log-drain: stopped tracked collector pid=$pid pgid=$pgid"
    else
        echo "baremetal-log-drain: discarded stale state without signalling an unowned process: $state_file" >&2
    fi
    rm -f -- "$state_file"
}

legacy_process_owned() {
    local pid="$1"
    local stdout_path cwd_path
    local -a argv=()

    [[ -r "/proc/$pid/cmdline" ]] || return 1
    mapfile -d '' -t argv < "/proc/$pid/cmdline" || return 1
    [[ ${#argv[@]} -eq 3 ]] || return 1
    [[ "${argv[0]##*/}" == "nc" || "${argv[0]##*/}" == "netcat" ]] || return 1
    [[ "${argv[1]}" == "$host" && "${argv[2]}" == "$port" ]] || return 1

    cwd_path="$(readlink -f -- "/proc/$pid/cwd" 2>/dev/null || true)"
    stdout_path="$(readlink -f -- "/proc/$pid/fd/1" 2>/dev/null || true)"
    [[ "$cwd_path" == "$repo_root" ]] || return 1
    [[ "$(dirname -- "$stdout_path")" == "$log_dir" ]] || return 1
    [[ "$(basename -- "$stdout_path")" =~ ^trueos-baremetal\.[0-9]+\.log$ ]]
}

stop_legacy_collectors() {
    local proc pid
    local -a owned=()

    # Older versions recorded GNU setsid's short-lived wrapper PID. Recover
    # only collectors that have all of our identifying traits: exact nc
    # endpoint, repository cwd, and stdout attached to one of our slot logs.
    for proc in /proc/[0-9]*; do
        pid="${proc##*/}"
        if legacy_process_owned "$pid"; then
            owned+=("$pid")
        fi
    done
    ((${#owned[@]} > 0)) || return 0

    for pid in "${owned[@]}"; do
        signal_exact_pid "$pid" TERM
    done
    for _ in {1..20}; do
        local alive=0
        for pid in "${owned[@]}"; do
            if process_alive "$pid"; then
                alive=1
                break
            fi
        done
        ((alive == 0)) && break
        sleep 0.05
    done
    for pid in "${owned[@]}"; do
        process_alive "$pid" && signal_exact_pid "$pid" KILL
    done
    for _ in {1..40}; do
        local present=0
        for pid in "${owned[@]}"; do
            if [[ -e "/proc/$pid" ]]; then
                present=1
                break
            fi
        done
        ((present == 0)) && break
        sleep 0.05
    done
    for pid in "${owned[@]}"; do
        [[ -e "/proc/$pid" ]] &&
            die "scoped legacy collector still has /proc state after exact-PID teardown pid=$pid"
    done
    echo "baremetal-log-drain: stopped ${#owned[@]} scoped legacy collector(s)"
}

with_exclusive_lock() {
    mkdir -p -- "$(dirname "$lock_file")"
    exec 9>"$lock_file"
    flock -x 9
}

next_log_path() {
    mkdir -p -- "$log_dir" "$(dirname "$state_file")" "$(dirname "$slot_file")"

    local previous next slot_tmp
    previous="$(cat "$slot_file" 2>/dev/null || echo -1)"
    [[ "$previous" =~ ^-?[0-9]+$ ]] || previous=-1
    next=$(((previous + 1) % slots))
    slot_tmp="${slot_file}.$$.tmp"
    printf '%s\n' "$next" > "$slot_tmp"
    mv -f -- "$slot_tmp" "$slot_file"
    printf '%s/trueos-baremetal.%s.log\n' "$log_dir" "$next"
}

write_child_state() {
    local run_id="$1"
    local log_path="$2"
    local pid="$BASHPID"
    local pgid start_ticks state_tmp

    pgid="$(ps -o pgid= -p "$pid" | tr -d '[:space:]')"
    start_ticks="$(process_start_ticks "$pid")"
    [[ "$pid" == "$pgid" ]] ||
        die "collector did not become a process-group leader (pid=$pid pgid=$pgid)"
    [[ "$start_ticks" =~ ^[0-9]+$ ]] ||
        die "could not read collector process start time"

    state_tmp="${state_file}.${run_id}.tmp"
    umask 077
    {
        printf 'version=2\n'
        printf 'pid=%s\n' "$pid"
        printf 'pgid=%s\n' "$pgid"
        printf 'start_ticks=%s\n' "$start_ticks"
        printf 'run_id=%s\n' "$run_id"
        printf 'log_path=%s\n' "$log_path"
    } > "$state_tmp"
    mv -f -- "$state_tmp" "$state_file"
}

collect() {
    local run_id="${2:-}"
    local log_path="${3:-}"
    local child_pid=""
    local attempt=0

    [[ "$run_id" =~ ^[0-9a-f]{16,64}$ ]] || die "invalid collector run id"
    [[ "$log_path" == "$log_dir"/trueos-baremetal.*.log ]] ||
        die "collector log path is outside the configured slot set"

    write_child_state "$run_id" "$log_path"

    terminate_child() {
        if [[ -n "$child_pid" ]] && kill -0 "$child_pid" 2>/dev/null; then
            kill -TERM "$child_pid" 2>/dev/null || true
            wait "$child_pid" 2>/dev/null || true
        fi
        exit 0
    }
    trap terminate_child INT TERM HUP

    if ((delay > 0)); then
        printf 'trueos baremetal log drain: run_id=%s waiting_delay_seconds=%s\n' \
            "$run_id" "$delay" >> "$log_path"
        sleep "$delay"
    fi

    while :; do
        attempt=$((attempt + 1))
        printf 'trueos baremetal log drain: run_id=%s connect_attempt=%s target=%s:%s at=%s\n' \
            "$run_id" "$attempt" "$host" "$port" "$(date -Is)" >> "$log_path"
        python3 -u -c '
import socket
import sys

with socket.create_connection((sys.argv[1], int(sys.argv[2])), timeout=5.0) as stream:
    stream.settimeout(None)
    while True:
        data = stream.recv(64 * 1024)
        if not data:
            break
        sys.stdout.buffer.write(data)
        sys.stdout.buffer.flush()
' "$host" "$port" >> "$log_path" 2>&1 &
        child_pid=$!
        set +e
        wait "$child_pid"
        local status=$?
        set -e
        child_pid=""
        printf 'trueos baremetal log drain: run_id=%s disconnected_status=%s retry_seconds=%s at=%s\n' \
            "$run_id" "$status" "$retry_delay" "$(date -Is)" >> "$log_path"
        sleep "$retry_delay"
    done
}

start() {
    command -v python3 >/dev/null 2>&1 || die "python3 not found"
    command -v setsid >/dev/null 2>&1 || die "setsid not found"
    command -v flock >/dev/null 2>&1 || die "flock not found"
    validate_config
    with_exclusive_lock
    stop_tracked
    stop_legacy_collectors

    local log_path run_id
    log_path="$(next_log_path)"
    run_id="$(
        printf '%s:%s:%s:%s\n' "$BASHPID" "$RANDOM" "$(date +%s%N)" "$log_path" |
            sha256sum |
            cut -c1-32
    )"
    : > "$log_path"
    ln -sfn -- "$(basename "$log_path")" "$log_dir/LatestOfThree.logs"
    {
        printf 'trueos baremetal log drain: version=2 run_id=%s target=%s:%s started_at=%s\n' \
            "$run_id" "$host" "$port" "$(date -Is)"
        printf 'trueos baremetal log drain: expected_runtime_elf_sha256=%s expected_iso_sha256=%s\n' \
            "$expected_elf_sha256" "$expected_iso_sha256"
        printf 'trueos baremetal log drain: testrig_physical_reset_receipt=%s\n' \
            "$physical_reset_receipt"
    } >> "$log_path"

    rm -f -- "$state_file"
    setsid -f "$script_path" collect "$run_id" "$log_path" \
        9>&- </dev/null >/dev/null 2>&1

    local ready=0
    for _ in {1..100}; do
        if [[ "$(state_field run_id)" == "$run_id" ]] && tracked_process_owned; then
            ready=1
            break
        fi
        sleep 0.05
    done
    ((ready == 1)) || die "collector failed to publish owned PID/process-group state"

    local pid pgid
    pid="$(state_field pid)"
    pgid="$(state_field pgid)"
    echo "baremetal-log-drain: pid=$pid pgid=$pgid run_id=$run_id log=$log_path latest_of_three=$log_dir/LatestOfThree.logs target=$host:$port delay=${delay}s"
}

marker_present() {
    local log_path="$1"
    local line
    local lines=0

    while IFS= read -r line; do
        [[ "$line" == *"$boot_marker"* ]] && return 0
        lines=$((lines + 1))
        ((lines >= 512)) && break
    done < "$log_path"
    return 1
}

wait_for_marker() {
    validate_config
    local log_path deadline
    log_path="$(state_field log_path)"
    [[ -n "$log_path" && -f "$log_path" ]] ||
        die "no active collector log is recorded"
    [[ "$log_path" == "$log_dir"/trueos-baremetal.*.log ]] ||
        die "recorded collector log is outside the configured slot set"

    deadline=$((SECONDS + wait_timeout))
    while ((SECONDS <= deadline)); do
        if marker_present "$log_path"; then
            printf 'trueos baremetal log drain: boot_marker_verified=%s at=%s\n' \
                "$boot_marker" "$(date -Is)" >> "$log_path"
            echo "baremetal-log-drain: boot marker verified log=$log_path marker=$boot_marker"
            return 0
        fi
        tracked_process_owned ||
            die "collector exited before the fresh boot marker arrived (log=$log_path)"
        sleep 0.2
    done
    die "timed out after ${wait_timeout}s waiting for fresh boot marker in $log_path"
}

status() {
    if tracked_process_owned; then
        echo "baremetal-log-drain: running pid=$(state_field pid) pgid=$(state_field pgid) run_id=$(state_field run_id) log=$(state_field log_path)"
        return 0
    fi
    echo "baremetal-log-drain: stopped"
    return 1
}

case "$cmd" in
    collect)
        validate_config
        collect "$@"
        ;;
    start)
        start
        ;;
    wait)
        wait_for_marker
        ;;
    status)
        status
        ;;
    stop | snipe)
        validate_config
        with_exclusive_lock
        stop_tracked
        stop_legacy_collectors
        ;;
    *)
        echo "usage: $0 {start|wait|status|stop|snipe}" >&2
        exit 2
        ;;
esac
