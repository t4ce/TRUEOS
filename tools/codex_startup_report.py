#!/usr/bin/env python3
"""Summarize source-stamped Codex/TRUEOS startup records without exporting other logs.

Accepts resident logger captures or CODEX_STARTUP_TIMING_FILE TSV. Durations
overlap and must not be summed. TCP arrival times and CPU time are never inferred.
"""

import argparse
import hashlib
import io
import json
import math
import os
import re
from pathlib import Path


ANSI = re.compile(r"\x1b\[[0-?]*[ -/]*[@-~]")
ROW = re.compile(r"(?:\[codex-startup\]\s*|^)(\d+)\t([\d.]+)\t([\d.]+)\t(.+?)\s*$")
LABEL = re.compile(r"[A-Za-z0-9_.-]{1,96}\Z")
ENDPOINTS = {
    "first_frame_backend_flushed": "first_flush_ms",
    "editable_composer_visible": "editable_ms",
    "onboarding_provisional_frame": "onboarding_frame_ms",
    "picker_provisional_frame": "picker_frame_ms",
    "session_prompt_ready": "session_ready_frame_ms",
    "startup_handoff_frame_rendered": "startup_handoff_frame_ms",
}
HANDOFF_STATE_BITS = (
    "pending_frame", "session_configured", "thread_start_pending", "app_queue_nonempty",
    "queued_protected", "active_thread_queue_nonempty", "waiting_initial_session",
    "draft_pending", "active_modal", "protected_input_pending",
)
STARTUP_EVENT_CATEGORIES = ("other", "rate_limits_loaded", "backend_fallback", "history")
STARTUP_SERVER_CATEGORIES = ("other", "notification", "request")
PREWARM_SELECTIONS = {
    "core_startup_websocket_selected": "selected",
    "core_startup_websocket_deferred": "deferred",
    "core_startup_websocket_unavailable": "unavailable",
}


def fields(text):
    return dict(part.split("=", 1) for part in text.split() if "=" in part)


def number(value):
    result = float(value)
    if not math.isfinite(result) or result < 0:
        raise ValueError("expected a finite nonnegative number")
    return result


def byte_offset(value):
    try:
        if not value.isascii() or not value.isdigit():
            raise ValueError
        return int(value)
    except ValueError:
        raise argparse.ArgumentTypeError("expected a nonnegative integer byte offset") from None


class _CaptureWindow(io.RawIOBase):
    """Read and hash one frozen binary range without following later appends."""

    def __init__(self, capture, start, end):
        super().__init__()
        capture.seek(start)
        self.capture = capture
        self.remaining = end - start
        self.bytes_read = 0
        self.sha256 = hashlib.sha256()

    def readable(self):
        return True

    def readinto(self, buffer):
        if not self.remaining or not len(buffer):
            return 0
        view = memoryview(buffer)[:min(len(buffer), self.remaining)]
        size = self.capture.readinto(view)
        if not size:
            raise OSError("capture was truncated before the selected end offset")
        self.sha256.update(view[:size])
        self.bytes_read += size
        self.remaining -= size
        return size


def bounded_integer(value, minimum, maximum):
    if not value.isascii() or not value.isdigit() or not minimum <= int(value) <= maximum:
        raise ValueError("invalid bounded integer")
    return int(value)


def context_fields(values):
    result = {}
    for key in ("request", "app_gen"):
        value = values.get(key, "")
        if value.isascii() and value.isdigit() and int(value) <= 2**64 - 1:
            result[key] = int(value)
    instance = values.get("instance", "")
    if re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", instance):
        result["instance"] = instance.lower()
    digest = values.get("archive_sha256", "")
    if re.fullmatch(r"[0-9a-fA-F]{64}", digest):
        result["archive_sha256"] = digest.lower()
    return result


def parse_lines(lines, slow_ms=500, poll_ms=50):
    runs, hosts, warnings = [], {}, []
    current = {}
    invalid = 0

    def new_run(pid, request):
        run = {"pid": pid, "pids": [pid], "context": {"request": request} if request is not None else {},
               "milestones": [], "spans": [],
               "endpoints": {}, "last_elapsed_ms": 0.0, "_open": {}, "_seen": set()}
        runs.append(run)
        current[("request", request) if request is not None else ("pid", pid)] = run
        return run

    for line_no, raw in enumerate(lines, 1):
        line = ANSI.sub("", raw).strip("\r\n")
        try:
            match = ROW.search(line)
            if match:
                pid, elapsed, delta, stage = match.groups()
                pid, elapsed, delta = int(pid), number(elapsed), number(delta)
                name, _, attributes = stage.partition(" ")
                values = fields(attributes)
                request = context_fields(values).get("request")
                key = ("request", request) if request is not None else ("pid", pid)
                run = current.get(key) or new_run(pid, request)
                identity = (elapsed, stage)
                if identity in run["_seen"]:
                    continue
                if ((name == "process_context" and run["_seen"])
                        or elapsed < run["last_elapsed_ms"]):
                    run = new_run(pid, request)
                run["_seen"].add(identity)
                if pid not in run["pids"]:
                    run["pids"].append(pid)
                run["last_elapsed_ms"] = elapsed
                if name == "process_context":
                    run["context"] = context_fields(values)
                    continue
                if name == "span_begin":
                    span_id = int(values["id"])
                    label = values["name"]
                    if span_id < 1 or not LABEL.fullmatch(label):
                        raise ValueError("invalid span identifier")
                    if span_id in run["_open"]:
                        warnings.append(f"app pid={pid}: repeated open span id={span_id}")
                    run["_open"][span_id] = {"id": span_id, "name": label,
                                             "start_ms": elapsed, "line": line_no}
                elif name == "span_end":
                    span_id = int(values["id"])
                    label, outcome = values["name"], values["outcome"]
                    if not LABEL.fullmatch(label) or not LABEL.fullmatch(outcome):
                        raise ValueError("invalid span label")
                    span = run["_open"].get(span_id)
                    partial = span is None
                    span = span or {"id": span_id, "name": label, "start_ms": None}
                    if span["name"] != label:
                        raise ValueError("span labels disagree")
                    duration = number(values["duration_ms"])
                    polls = int(values["poll_n"])
                    total, maximum = number(values["poll_total_ms"]), number(values["poll_max_ms"])
                    if polls < 0:
                        raise ValueError("invalid poll count")
                    run["_open"].pop(span_id, None)
                    span.update(end_ms=elapsed, duration_ms=duration, outcome=outcome,
                                poll_n=polls, poll_total_ms=total, poll_max_ms=maximum,
                                outside_poll_ms=round(max(0.0, duration - total), 3) if polls else None,
                                partial=partial)
                    if maximum > total + 0.005 or total > duration + 0.005:
                        warnings.append(f"app pid={pid}: inconsistent poll times for {label}")
                    run["spans"].append(span)
                elif LABEL.fullmatch(name):
                    if name == "startup_handoff_state":
                        change = bounded_integer(values["change"], 1, 16)
                        mask = bounded_integer(values["mask"], 0, (1 << len(HANDOFF_STATE_BITS)) - 1)
                        run.setdefault("startup_handoff_states", []).append({
                            "elapsed_ms": elapsed, "change": change, "mask": mask,
                            "state": {label: bool(mask & (1 << bit))
                                      for bit, label in enumerate(HANDOFF_STATE_BITS)},
                        })
                    if name in ("startup_app_event", "startup_server_event"):
                        sample = bounded_integer(values["sample"], 1, 16)
                        categories = (STARTUP_EVENT_CATEGORIES if name == "startup_app_event"
                                      else STARTUP_SERVER_CATEGORIES)
                        category = bounded_integer(values["category"], 0, len(categories) - 1)
                        event = {
                            "elapsed_ms": elapsed, "sample": sample,
                            "category": categories[category],
                        }
                        if sample > 8:
                            event["phase"] = "after_handoff"
                        if name == "startup_app_event":
                            if "event" in values:
                                if not re.fullmatch(r"[A-Z][A-Za-z0-9_]{0,95}", values["event"]):
                                    raise ValueError("invalid static event name")
                                event["event"] = values["event"]
                            if "op" in values:
                                if values["op"] not in ("list_skills", "other", "none"):
                                    raise ValueError("invalid static operation name")
                                event["op"] = values["op"]
                        elif "event" in values:
                            if values["event"] not in ("ServerNotification", "ServerRequest", "Lagged", "Disconnected"):
                                raise ValueError("invalid static server event name")
                            event["event"] = values["event"]
                        run.setdefault(name + "s", []).append(event)
                    run["milestones"].append({"name": name, "elapsed_ms": elapsed, "delta_ms": delta})
                    if name == "thread_placement_configured":
                        mode, cap = int(values["mode"]), int(values["background_cap"])
                        if mode not in (0, 1, 2) or not 1 <= cap <= 4:
                            raise ValueError("invalid placement")
                        run["placement"] = {"mode": mode, "background_cap": cap}
                    if name == "runtime_workers_configured":
                        workers, blocking = int(values["async_workers"]), int(values["blocking_cap"])
                        if not 1 <= workers <= 4 or not 1 <= blocking <= 8:
                            raise ValueError("invalid runtime worker limits")
                        run["runtime"] = {"async_workers": workers, "blocking_cap": blocking}
                    if name in PREWARM_SELECTIONS:
                        run["startup_websocket_prewarm"] = PREWARM_SELECTIONS[name]
                    if name == "core_startup_prewarm_override_invalid":
                        run["startup_prewarm_override_invalid"] = True
                    if name in ("tui_startup_skills_coalescing_enabled", "tui_startup_skills_coalescing_disabled"):
                        run["startup_skills_coalescing"] = name.endswith("_enabled")
                    if name == "tui_startup_skills_override_invalid":
                        run["startup_skills_override_invalid"] = True
                    if name == "tui_startup_skills_coalesced":
                        run["startup_skills_refresh_coalesced"] = True
                    if name in ENDPOINTS:
                        run["endpoints"].setdefault(ENDPOINTS[name], elapsed)
                continue

            if "trueos-startup-v1 " in line:
                values = fields(line.split("trueos-startup-v1 ", 1)[1])
                request = int(values["request"])
                phase, event = values["phase"], values["event"]
                mono = number(values["mono_ms"])
                if request < 1 or request > 2**64 - 1 or not LABEL.fullmatch(phase) or event not in ("begin", "end"):
                    raise ValueError("invalid host span")
                host = hosts.setdefault(request, {"request": request, "spans": [], "_open": {},
                                                   "last_mono_ms": mono, "_phases": set()})
                host["last_mono_ms"] = max(host["last_mono_ms"], mono)
                metadata = {"vm": int(values["vm"]), "gen": int(values["gen"])}
                if not 0 <= metadata["vm"] <= 255 or not 0 <= metadata["gen"] <= 2**64 - 1:
                    raise ValueError("invalid host identity")
                if event == "begin":
                    if phase in host["_phases"]:
                        warnings.append(f"host request={request}: duplicate phase; use one boot per capture")
                    host["_phases"].add(phase)
                    host["_open"][phase] = {"name": phase, "start_ms": mono, **metadata}
                else:
                    status = values["status"]
                    if not LABEL.fullmatch(status):
                        raise ValueError("invalid host status")
                    elapsed = number(values["elapsed_ms"])
                    span = host["_open"].pop(phase, None)
                    partial = span is None
                    span = span or {"name": phase, "start_ms": None}
                    span.update(end_ms=mono, duration_ms=elapsed,
                                outcome=status, partial=partial, **metadata)
                    host["spans"].append(span)
        except (ValueError, KeyError, OverflowError):
            invalid += 1

    for run in runs:
        legacy = {}
        run["milestone_intervals"] = []
        for marker in run["milestones"]:
            name = marker["name"]
            if name.endswith("_begin"):
                legacy[name[:-6]] = marker["elapsed_ms"]
            elif name.endswith(("_done", "_failed")):
                suffix = "_failed" if name.endswith("_failed") else "_done"
                label = name[:-len(suffix)]
                start = legacy.pop(label, None)
                if start is not None:
                    run["milestone_intervals"].append({
                        "name": label, "start_ms": start, "end_ms": marker["elapsed_ms"],
                        "duration_ms": marker["elapsed_ms"] - start,
                    })
        run["open_milestone_intervals"] = [{"name": name, "start_ms": start} for name, start in legacy.items()]
        run["milestone_gaps"] = sorted([
            {"after": before["name"], "before": after["name"],
             "duration_ms": after["elapsed_ms"] - before["elapsed_ms"]}
            for before, after in zip(run["milestones"], run["milestones"][1:])
        ], key=lambda item: item["duration_ms"], reverse=True)
        run["open_spans"] = list(run.pop("_open").values())
        run.pop("_seen")
        for span in run["open_spans"]:
            span["elapsed_lower_bound_ms"] = max(0, run["last_elapsed_ms"] - span["start_ms"])
        editable = run["endpoints"].get("editable_ms")
        for span in run["spans"]:
            span["interactive"] = span["name"].startswith("tui.user_")
            start = span["start_ms"]
            span["position"] = (
                "unknown" if editable is None or start is None else
                "before_editable" if span["end_ms"] <= editable else
                "after_editable" if start >= editable else "crosses_editable"
            )
        run["slow_spans"] = sorted(
            (span for span in run["spans"] if span["duration_ms"] >= slow_ms and not span["interactive"]),
            key=lambda span: span["duration_ms"], reverse=True)
        run["long_polls"] = sorted(
            (span for span in run["spans"] if span["poll_n"] and span["poll_max_ms"] >= poll_ms),
            key=lambda span: span["poll_max_ms"], reverse=True)
        if not run["context"]:
            warnings.append(f"app pid={run['pid']}: missing launch identity; repeated PIDs may be ambiguous")
        if run["open_spans"]:
            warnings.append(f"app pid={run['pid']}: unfinished phases; capture may be partial or work may still be pending")
        if any(item["name"] == "timing_span_budget_exhausted" for item in run["milestones"]):
            warnings.append(f"app pid={run['pid']}: startup span budget exhausted; later phases were not measured")
    for host in hosts.values():
        host.pop("_phases")
        host["open_spans"] = list(host.pop("_open").values())
        for span in host["open_spans"]:
            span["elapsed_lower_bound_ms"] = max(0, host["last_mono_ms"] - span["start_ms"])
    if invalid:
        warnings.append(f"ignored {invalid} malformed timing record(s)")
    if not runs and not hosts:
        warnings.append("no startup timing records found; elapsed time cannot be recovered from TCP arrival or lifetime maxima")
    return {"schema": 1, "application_runs": runs, "host_requests": list(hosts.values()),
            "warnings": list(dict.fromkeys(warnings)),
            "limits": ["application epoch begins at its first timing call, after loading",
                       "host mono_ms and application elapsed_ms have different origins; join only by request identity",
                       "app_gen is instance generation; host gen is VM run generation",
                       "nested/concurrent durations overlap: do not sum them",
                       "poll durations are wall elapsed, including descheduling, not CPU time",
                       "no records does not mean a phase was fast or absent"]}


def render_text(report):
    output = ["Startup timings (wall elapsed; nested phases overlap)"]
    if window := report.get("capture_window"):
        output.append(f"Capture bytes [{window['start_offset']}, {window['end_offset']}): "
                      f"{window['bytes']} bytes, sha256={window['sha256']}")
    for run in report["application_runs"]:
        context = run["context"]
        output.append(f"\nApplication pid={run['pid']} request={context.get('request', 'unknown')}")
        if context.get("archive_sha256"):
            output.append(f"  archive_sha256={context['archive_sha256']}")
        if run.get("placement"):
            output.append(f"  placement={run['placement']}")
        if run.get("runtime"):
            output.append(f"  runtime={run['runtime']}")
        if run.get("startup_websocket_prewarm"):
            output.append(f"  startup websocket prewarm: {run['startup_websocket_prewarm']}")
        if "startup_skills_coalescing" in run:
            output.append(f"  startup skills coalescing: {run['startup_skills_coalescing']}")
        if run.get("startup_handoff_states"):
            latest = run["startup_handoff_states"][-1]
            output.append(f"  handoff state: change={latest['change']} mask={latest['mask']}")
        for name, elapsed in run["endpoints"].items():
            output.append(f"  {name}: {elapsed:.3f} ms from application timing origin")
        for span in run["slow_spans"][:12]:
            output.append(f"  slow {span['name']}: {span['duration_ms']:.3f} ms ({span['position']}, {span['outcome']})")
        for span in run["long_polls"][:12]:
            output.append(f"  long poll {span['name']}: max={span['poll_max_ms']:.3f} ms, polls={span['poll_n']}")
        if not run["spans"]:
            for span in sorted(run["milestone_intervals"], key=lambda item: item["duration_ms"], reverse=True)[:8]:
                output.append(f"  milestone interval {span['name']}: {span['duration_ms']:.3f} ms (no poll measurements)")
            for gap in run["milestone_gaps"][:3]:
                output.append(f"  milestone gap {gap['after']} -> {gap['before']}: {gap['duration_ms']:.3f} ms (may include waits/user interaction)")
        for span in run["open_spans"]:
            output.append(f"  OPEN {span['name']} (end not captured)")
    for host in report["host_requests"]:
        output.append(f"\nHost launch request={host['request']}")
        for span in sorted(host["spans"], key=lambda item: item["duration_ms"], reverse=True)[:12]:
            output.append(f"  {span['name']}: {span['duration_ms']:.3f} ms ({span['outcome']}, vm={span['vm']} gen={span['gen']})")
        for span in host["open_spans"]:
            output.append(f"  OPEN {span['name']} (end not captured)")
    output.extend("Warning: " + warning for warning in report["warnings"])
    return "\n".join(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__, epilog=(
        "Use one boot per byte window: host request IDs can be reused after a reboot. "
        "Offsets may cut records; missing phase boundaries remain partial. "
        "The default end is the file size observed when replay starts, excluding later appends."))
    parser.add_argument("capture", type=Path)
    parser.add_argument("--json", action="store_true", help="emit sanitized structured measurements")
    parser.add_argument("--slow-ms", type=number, default=500)
    parser.add_argument("--poll-ms", type=number, default=50)
    parser.add_argument("--start-offset", type=byte_offset, default=0,
                        help="first byte to replay, inclusive (default: 0); select one boot")
    parser.add_argument("--end-offset", type=byte_offset,
                        help="last byte position, exclusive (default: snapshotted file size)")
    args = parser.parse_args()
    try:
        with args.capture.open("rb") as capture:
            snapshot_size = os.fstat(capture.fileno()).st_size
            end = snapshot_size if args.end_offset is None else args.end_offset
            if not 0 <= args.start_offset <= end <= snapshot_size:
                parser.error("byte offsets must satisfy 0 <= start <= end <= "
                             f"snapshotted file size ({snapshot_size} bytes)")
            window = _CaptureWindow(capture, args.start_offset, end)
            with io.TextIOWrapper(io.BufferedReader(window), encoding="utf-8", errors="replace") as lines:
                report = parse_lines(lines, args.slow_ms, args.poll_ms)
            report["capture_window"] = {
                "start_offset": args.start_offset, "end_offset": end,
                "bytes": window.bytes_read, "sha256": window.sha256.hexdigest(),
            }
    except OSError as error:
        parser.error(f"cannot read capture: {error.strerror or error}")
    print(json.dumps(report, indent=2) if args.json else render_text(report))


if __name__ == "__main__":
    main()
