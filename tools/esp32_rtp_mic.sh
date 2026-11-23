#!/usr/bin/env bash
set -euo pipefail

COMMAND="${1:-start}"
if [[ $# -gt 0 ]]; then
  shift
fi

ADDRESS="${1:-239.255.77.77}"
PORT="${2:-5004}"
RATE="${3:-48000}"

SINK_NAME="esp32_rtp_sink"
SOURCE_NAME="esp32_rtp_mic"
SINK_DESC="ESP32-S3 RTP Sink"
SOURCE_DESC="ESP32-S3 WiFi Microphone"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/tmp}/esp32-rtp-mic"
PID_FILE="${RUNTIME_DIR}/gst.pid"
MODULE_FILE="${RUNTIME_DIR}/pactl-modules"
LOG_FILE="${RUNTIME_DIR}/gst.log"

unload_own_modules() {
  if [[ -f "${MODULE_FILE}" ]]; then
    while read -r module_id; do
      [[ -n "${module_id}" ]] && pactl unload-module "${module_id}" >/dev/null 2>&1 || true
    done < "${MODULE_FILE}"
    rm -f "${MODULE_FILE}"
  fi

  pactl list short modules \
    | awk -v sink="${SINK_NAME}" -v source="${SOURCE_NAME}" \
        '$0 ~ "sink_name=" sink || $0 ~ "source_name=" source {print $1}' \
    | xargs -r -n1 pactl unload-module >/dev/null 2>&1 || true
}

stop_pipeline() {
  if [[ -f "${PID_FILE}" ]]; then
    pid="$(cat "${PID_FILE}")"
    if [[ -n "${pid}" ]] && kill -0 "${pid}" >/dev/null 2>&1; then
      kill "${pid}" >/dev/null 2>&1 || true
      sleep 0.2
      kill -9 "${pid}" >/dev/null 2>&1 || true
    fi
    rm -f "${PID_FILE}"
  fi
}

start_mic() {
  command -v pactl >/dev/null
  command -v gst-launch-1.0 >/dev/null
  mkdir -p "${RUNTIME_DIR}"

  stop_pipeline
  unload_own_modules

  null_module="$(
    pactl load-module module-null-sink \
      sink_name="${SINK_NAME}" \
      "sink_properties=\"device.description='${SINK_DESC}'\"" \
      rate="${RATE}" \
      channels=2 \
      channel_map=front-left,front-right
  )"

  # PipeWire implements module-remap-source with a passive monitor link. Keep
  # the public source attached to its driver so the first recording client can
  # cold-start it without needing another application (for example Discord) to
  # activate the graph first.
  source_module="$(
    pactl load-module module-remap-source \
      master="${SINK_NAME}.monitor" \
      source_name="${SOURCE_NAME}" \
      "source_properties=\"device.description='${SOURCE_DESC}' node.always-process=true session.suspend-timeout-seconds=0\"" \
      channels=2 \
      master_channel_map=front-left,front-right \
      channel_map=front-left,front-right \
      remix=no
  )"

  printf '%s\n%s\n' "${source_module}" "${null_module}" > "${MODULE_FILE}"
  pactl set-default-source "${SOURCE_NAME}" >/dev/null 2>&1 || true

  # Buffer Wi-Fi jitter, then let the audio device pace playback. Enabling
  # sink clock sync here introduced repeated 5–20 ms silence gaps.
  nohup gst-launch-1.0 -q \
    udpsrc address="${ADDRESS}" port="${PORT}" auto-multicast=true \
      caps="application/x-rtp,media=(string)audio,clock-rate=(int)${RATE},encoding-name=(string)L16,encoding-params=(string)2,channels=(int)2,payload=(int)96" \
    ! rtpjitterbuffer latency=60 drop-on-latency=true \
    ! rtpL16depay \
    ! audioconvert \
    ! audioresample \
    ! audio/x-raw,format=S16LE,rate="${RATE}",channels=2 \
    ! pulsesink device="${SINK_NAME}" sync=false buffer-time=80000 latency-time=10000 \
    > "${LOG_FILE}" 2>&1 &

  printf '%s\n' "$!" > "${PID_FILE}"
  printf 'Started %s from RTP %s:%s\n' "${SOURCE_DESC}" "${ADDRESS}" "${PORT}"
  pactl list short sources | awk -v source="${SOURCE_NAME}" '$2 == source {print}'
}

stop_mic() {
  stop_pipeline
  unload_own_modules
  printf 'Stopped ESP32-S3 RTP microphone\n'
}

status_mic() {
  if [[ -f "${PID_FILE}" ]] && kill -0 "$(cat "${PID_FILE}")" >/dev/null 2>&1; then
    printf 'RTP receiver running, pid %s\n' "$(cat "${PID_FILE}")"
  else
    printf 'RTP receiver not running\n'
  fi

  pactl list short sources | awk -v source="${SOURCE_NAME}" '$2 == source {print}'
}

case "${COMMAND}" in
  start)
    start_mic
    ;;
  stop)
    stop_mic
    ;;
  restart)
    stop_mic
    start_mic
    ;;
  status)
    status_mic
    ;;
  *)
    printf 'Usage: %s [start|stop|restart|status] [address] [port] [rate]\n' "$0" >&2
    exit 2
    ;;
esac
