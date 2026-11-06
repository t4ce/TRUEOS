#!/usr/bin/env python3
r"""Observe a live-update outage from a separate host, without changing the rig.

Example:
    python3 tools/live-update-downtime.py --output /tmp/live-update-run \
        --expected-text websys --kernel-http-port 80

HTTP success requires status 200 and the complete response body. The reported
availability-gap bound runs from the last successful request's START to the
first recovered response's END. It includes polling uncertainty, service
startup and manual restart time; it is not an exact kernel-swap measurement.
The bound assumes one contiguous outage between those successful observations.
TCP port 22 success proves only that a TCP connection was accepted.
"""

import argparse
import asyncio
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
import math
from pathlib import Path
import signal
import statistics
import sys
import time


INTERPRETATION = (
    "Observed availability-gap upper bound, assuming a single contiguous "
    "outage: last pre-failure successful request start to first recovered "
    "complete response end. Includes polling, request latency, manual restart "
    "and service startup. No time is subtracted. Not exact kernel-swap downtime."
)


def utc_now():
    return datetime.now(timezone.utc).isoformat(timespec="microseconds")


def boundary(observation):
    if observation is None:
        return None
    return {
        key: observation[key]
        for key in ("sequence", "start_utc", "end_utc", "start_monotonic_ns",
                    "end_monotonic_ns", "duration_ms", "ok")
    }


def duration_stats(observations):
    values = [row["duration_ms"] for row in observations]
    if not values:
        return {"count": 0}
    return {
        "count": len(values),
        "minimum_ms": min(values),
        "median_ms": statistics.median(values),
        "mean_ms": statistics.mean(values),
        "maximum_ms": max(values),
    }


def summarize_probe(observations):
    """Group failures by request start; retain the earliest recovery completion."""
    rows = sorted(observations, key=lambda row: row["start_monotonic_ns"])
    episodes = []
    previous_success = None
    current = None
    recovered_episode = None
    baseline = []
    seen_failure = False
    for row in rows:
        if row["ok"]:
            if not seen_failure:
                baseline.append(row)
            if current is not None:
                current["first_recovered_success"] = boundary(row)
                if previous_success is not None:
                    current["availability_gap_upper_bound_seconds"] = (
                        row["end_monotonic_ns"]
                        - previous_success["start_monotonic_ns"]
                    ) / 1e9
                current["status"] = (
                    "recovered" if previous_success is not None
                    else "recovered_without_pre_failure_success"
                )
                episodes.append(current)
                recovered_episode = current
                current = None
            elif (recovered_episode is not None
                  and row["end_monotonic_ns"] < recovered_episode[
                      "first_recovered_success"]["end_monotonic_ns"]):
                # Overlapping requests may complete out of start order.
                recovered_episode["first_recovered_success"] = boundary(row)
                preceding = recovered_episode["last_pre_failure_success"]
                if preceding is not None:
                    recovered_episode["availability_gap_upper_bound_seconds"] = (
                        row["end_monotonic_ns"] - preceding["start_monotonic_ns"]
                    ) / 1e9
            previous_success = row
        else:
            seen_failure = True
            recovered_episode = None
            if current is None:
                current = {
                    "status": "unrecovered",
                    "last_pre_failure_success": boundary(previous_success),
                    "first_failure": boundary(row),
                    "last_failure": boundary(row),
                    "first_recovered_success": None,
                    "failed_observations": 0,
                    "availability_gap_upper_bound_seconds": None,
                }
            current["last_failure"] = boundary(row)
            current["failed_observations"] += 1
    if current is not None:
        episodes.append(current)
    fingerprints = Counter(
        (row["body_sha256"], row["body_bytes"], row.get("content_type", ""))
        for row in rows if row.get("body_sha256") is not None
    )
    return {
        "observations": len(rows),
        "successes": sum(row["ok"] for row in rows),
        "failures": sum(not row["ok"] for row in rows),
        "first_observation": boundary(rows[0]) if rows else None,
        "last_observation": boundary(rows[-1]) if rows else None,
        "maximum_start_to_start_seconds": max(
            ((b["start_monotonic_ns"] - a["start_monotonic_ns"]) / 1e9
             for a, b in zip(rows, rows[1:])), default=None,
        ),
        "maximum_start_lateness_ms": max(
            (row.get("start_lateness_ms", 0) for row in rows), default=None,
        ),
        "missed_scheduled_slots": sum(row.get("missed_slots", 0) for row in rows),
        "baseline_success_durations": duration_stats(baseline),
        "all_success_durations": duration_stats([row for row in rows if row["ok"]]),
        "failure_durations": duration_stats([row for row in rows if not row["ok"]]),
        "response_fingerprints": [
            {"sha256": digest, "bytes": size, "content_type": mime,
             "observations": count}
            for (digest, size, mime), count in sorted(fingerprints.items())
        ],
        "episodes": episodes,
    }


async def read_chunked_body(reader):
    chunks = []
    while True:
        line = await reader.readline()
        if not line.endswith(b"\r\n"):
            raise ValueError("incomplete chunk-size line")
        size = int(line.split(b";", 1)[0].strip(), 16)
        if size < 0:
            raise ValueError("negative chunk size")
        if size == 0:
            while True:
                trailer = await reader.readline()
                if trailer == b"\r\n":
                    return b"".join(chunks)
                if not trailer:
                    raise ValueError("incomplete chunk trailer")
        chunks.append(await reader.readexactly(size))
        if await reader.readexactly(2) != b"\r\n":
            raise ValueError("missing chunk terminator")


async def http_request(host, port, path, expected_text=None):
    writer = None
    try:
        reader, writer = await asyncio.open_connection(host, port)
        writer.write((
            f"GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\n"
            "Connection: close\r\nAccept-Encoding: identity\r\n"
            "User-Agent: TRUEOS-live-update-downtime/1\r\n\r\n"
        ).encode("ascii"))
        await writer.drain()
        header_block = await reader.readuntil(b"\r\n\r\n")
        lines = header_block[:-4].split(b"\r\n")
        status_parts = lines[0].split(b" ", 2)
        if len(status_parts) < 2 or not status_parts[0].startswith(b"HTTP/"):
            raise ValueError("invalid HTTP status line")
        status = int(status_parts[1])
        headers = {}
        for line in lines[1:]:
            name, value = line.split(b":", 1)
            headers[name.decode("ascii").lower()] = value.strip().decode("latin1")
        if "chunked" in headers.get("transfer-encoding", "").lower():
            body = await read_chunked_body(reader)
        elif "content-length" in headers:
            size = int(headers["content-length"])
            if size < 0:
                raise ValueError("negative Content-Length")
            body = await reader.readexactly(size)
        else:
            body = await reader.read()
        marker_ok = expected_text is None or expected_text.encode("utf-8") in body
        result = {
            "ok": status == 200 and marker_ok,
            "status_code": status,
            "body_bytes": len(body),
            "body_sha256": hashlib.sha256(body).hexdigest(),
            "content_type": headers.get("content-type", ""),
        }
        if expected_text is not None:
            result["expected_text_found"] = marker_ok
        if not result["ok"]:
            result["error"] = (
                f"HTTP status {status}" if status != 200 else "expected text absent"
            )
        return result
    finally:
        if writer is not None:
            writer.close()


async def tcp_request(host, port):
    _, writer = await asyncio.open_connection(host, port)
    writer.close()
    return {"ok": True}


async def observe(config, sequence, scheduled, missed_slots, timeout):
    start_ns, start_utc = time.monotonic_ns(), utc_now()
    try:
        request = (
            http_request(config["host"], config["port"], config["path"],
                         config.get("expected_text"))
            if config["kind"] == "http"
            else tcp_request(config["host"], config["port"])
        )
        result = await asyncio.wait_for(request, timeout)
    except Exception as error:
        result = {
            "ok": False,
            "error_type": type(error).__name__,
            "error": (
                f"request did not complete within {timeout:g} seconds"
                if isinstance(error, asyncio.TimeoutError) else str(error)
            ),
        }
    end_ns, end_utc = time.monotonic_ns(), utc_now()
    return {
        "event": "observation", "probe": config["name"], "sequence": sequence,
        "start_utc": start_utc, "end_utc": end_utc,
        "scheduled_monotonic_ns": round(scheduled * 1e9),
        "start_monotonic_ns": start_ns, "end_monotonic_ns": end_ns,
        "duration_ms": (end_ns - start_ns) / 1e6,
        "start_lateness_ms": max(0, (start_ns / 1e9 - scheduled) * 1000),
        "missed_slots": missed_slots,
        **result,
    }


async def probe_loop(config, interval, timeout, stop, emit, origin):
    """Launch on a fixed schedule; slow requests never block the next probe."""
    inflight = set()
    sequence = 0
    scheduled = origin

    async def one_probe(sequence, scheduled, missed_slots):
        emit(await observe(config, sequence, scheduled, missed_slots, timeout))

    while not stop.is_set():
        delay = scheduled - time.monotonic()
        if delay > 0:
            try:
                await asyncio.wait_for(stop.wait(), delay)
                break
            except asyncio.TimeoutError:
                pass
        if stop.is_set():
            break
        # If the observing host stalls, do not create a burst of catch-up probes.
        # Preserve the actual gap and record how many scheduled slots were lost.
        missed_slots = max(0, math.floor((time.monotonic() - scheduled) / interval))
        scheduled += missed_slots * interval
        task = asyncio.create_task(one_probe(sequence, scheduled, missed_slots))
        inflight.add(task)
        task.add_done_callback(inflight.discard)
        sequence += 1
        scheduled += interval
    if inflight:
        await asyncio.gather(*inflight)


async def run(args):
    output = Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    if (output / "summary.json").exists():
        raise FileExistsError(f"summary already exists: {output / 'summary.json'}")
    probes = [
        {"name": "workload_http", "kind": "http", "host": args.host,
         "port": args.port, "path": args.path, "expected_text": args.expected_text},
        {"name": "ssh_tcp", "kind": "tcp", "host": args.host,
         "port": args.ssh_port},
    ]
    if args.kernel_http_port is not None:
        probes.append({"name": "kernel_http", "kind": "http", "host": args.host,
                       "port": args.kernel_http_port, "path": "/"})
    for index, path in enumerate(args.asset_path, 1):
        probes.append({"name": f"asset_http_{index}", "kind": "http",
                       "host": args.host, "port": args.port, "path": path})
    stop = asyncio.Event()
    reason = "duration"
    loop = asyncio.get_running_loop()

    def request_stop(new_reason):
        nonlocal reason
        if not stop.is_set():
            reason = new_reason
            stop.set()

    for sig in (signal.SIGINT, signal.SIGTERM):
        loop.add_signal_handler(sig, request_stop, sig.name)
    timer = loop.call_later(args.duration, request_stop, "duration")
    observations = []
    last_state = {}
    started_utc = utc_now()
    origin = time.monotonic()
    config = {
        "interval_seconds": args.interval, "request_timeout_seconds": args.timeout,
        "requested_duration_seconds": args.duration, "probes": probes,
        "ssh_tcp_interpretation": "TCP acceptance only; does not prove shell readiness",
        "http_interpretation": "HTTP 200 plus complete body and optional expected text",
        "episode_order": (
            "Contiguous failures in request-start order; recovery is the earliest "
            "complete successful response among the following successful requests."
        ),
    }
    try:
        with (output / "observations.jsonl").open("x", encoding="utf-8", buffering=1) as log:
            def write(row):
                log.write(json.dumps(row, sort_keys=True) + "\n")
                log.flush()

            def emit(row):
                observations.append(row)
                write(row)
                if last_state.get(row["probe"]) != row["ok"]:
                    print(f"{row['end_utc']} {row['probe']}: "
                          f"{'UP' if row['ok'] else 'DOWN'} "
                          f"{row.get('error', '')}", file=sys.stderr, flush=True)
                    last_state[row["probe"]] = row["ok"]

            write({"event": "run_start", "utc": started_utc,
                   "monotonic_ns": round(origin * 1e9), "config": config,
                   "interpretation": INTERPRETATION})
            await asyncio.gather(*(
                probe_loop(probe, args.interval, args.timeout, stop, emit, origin)
                for probe in probes
            ))
            summary = {
                "started_utc": started_utc, "finished_utc": utc_now(),
                "elapsed_seconds": time.monotonic() - origin,
                "stop_reason": reason, "config": config,
                "interpretation": INTERPRETATION,
                "probes": {
                    probe["name"]: summarize_probe([
                        row for row in observations if row["probe"] == probe["name"]
                    ]) for probe in probes
                },
            }
            write({"event": "run_end", "utc": summary["finished_utc"],
                   "stop_reason": reason})
        with (output / "summary.json").open("x", encoding="utf-8") as stream:
            json.dump(summary, stream, indent=2, sort_keys=True)
            stream.write("\n")
        print(json.dumps(summary, indent=2, sort_keys=True))
        return summary
    finally:
        timer.cancel()
        for sig in (signal.SIGINT, signal.SIGTERM):
            loop.remove_signal_handler(sig)


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--host", default="192.168.178.94")
    parser.add_argument("--port", type=int, default=54321, help="workload HTTP port")
    parser.add_argument("--path", default="/", help="workload HTTP request path")
    parser.add_argument("--expected-text", help="require this UTF-8 text in workload response")
    parser.add_argument("--asset-path", action="append", default=[],
                        help="also monitor this workload HTTP path; repeat for more assets")
    parser.add_argument("--ssh-port", type=int, default=22, help="TCP acceptance probe port")
    parser.add_argument("--kernel-http-port", type=int, help="also probe kernel HTTP / (e.g. 80)")
    parser.add_argument("--interval", type=float, default=0.25, help="seconds, at least 0.1")
    parser.add_argument("--timeout", type=float, default=0.2, help="total timeout per request")
    parser.add_argument("--duration", type=float, default=180, help="seconds; SIGINT/TERM also finish")
    parser.add_argument("--output", required=True, help="directory for JSONL and summary.json")
    args = parser.parse_args(argv)
    if not math.isfinite(args.interval) or args.interval < 0.1:
        parser.error("--interval must be finite and at least 0.1 seconds")
    for option in ("timeout", "duration"):
        value = getattr(args, option)
        if not math.isfinite(value) or value <= 0:
            parser.error(f"--{option} must be positive and finite")
    for option in ("port", "ssh_port", "kernel_http_port"):
        value = getattr(args, option)
        if value is not None and not 1 <= value <= 65535:
            parser.error(f"--{option.replace('_', '-')} must be between 1 and 65535")
    for path in [args.path, *args.asset_path]:
        if not path.startswith("/") or any(char in path for char in "\r\n "):
            parser.error("HTTP paths must start with / and contain no spaces or line breaks")
    if any(char.isspace() for char in args.host):
        parser.error("--host must contain no whitespace")
    try:
        for path in [args.path, *args.asset_path]:
            path.encode("ascii")
        args.host.encode("ascii")
    except UnicodeEncodeError:
        parser.error("use ASCII host names and percent-encoded paths")
    return args


if __name__ == "__main__":
    try:
        asyncio.run(run(parse_args()))
    except (OSError, ValueError) as error:
        print(f"live-update-downtime: {error}", file=sys.stderr)
        sys.exit(1)
