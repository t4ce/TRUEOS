#!/usr/bin/env python3
"""Summarize the UI4 video RGBA ownership/SURFLIVE probe."""

from __future__ import annotations

import argparse
import bisect
import re
import statistics
from collections import Counter, defaultdict
from pathlib import Path


EVENT_MARKER = "ui4 video-surface-lifecycle "
FIELD_RE = re.compile(r"([a-zA-Z0-9_]+)=([^\s]+)")
READERS_RE = re.compile(r"start_readers=\[([^\]]*)\]")


def integer(fields: dict[str, str], key: str, default: int = 0) -> int:
    value = fields.get(key)
    if value is None:
        return default
    return int(value.rstrip(","), 0)


def percentile(values: list[float], fraction: float) -> float:
    if not values:
        return 0.0
    ordered = sorted(values)
    index = round((len(ordered) - 1) * fraction)
    return ordered[index]


def metric(label: str, values: list[float], unit: str = "us") -> str:
    if not values:
        return f"{label}=n/a"
    return (
        f"{label}=avg:{statistics.fmean(values):.3f}{unit},"
        f"p50:{percentile(values, 0.50):.3f}{unit},"
        f"p95:{percentile(values, 0.95):.3f}{unit},"
        f"max:{max(values):.3f}{unit}"
    )


def parse_logs(paths: list[Path]) -> dict[int, dict[str, list[dict[str, object]]]]:
    frames: dict[int, dict[str, list[dict[str, object]]]] = defaultdict(
        lambda: defaultdict(list)
    )
    for path in paths:
        with path.open("r", encoding="utf-8", errors="replace") as stream:
            for line_number, line in enumerate(stream, 1):
                marker = line.find(EVENT_MARKER)
                if marker < 0:
                    continue
                payload = line[marker + len(EVENT_MARKER) :]
                fields = dict(FIELD_RE.findall(payload))
                event = fields.get("event")
                frame = integer(fields, "frame")
                if not event or frame == 0:
                    continue
                record: dict[str, object] = {
                    "path": str(path),
                    "line": line_number,
                    "event": event,
                }
                for key, value in fields.items():
                    if key in {"event", "boundary", "probe", "ownership_unchanged"}:
                        record[key] = value
                    else:
                        try:
                            record[key] = int(value.rstrip(","), 0)
                        except ValueError:
                            record[key] = value
                readers = READERS_RE.search(payload)
                if readers is not None:
                    record["start_readers"] = tuple(
                        int(value.strip(), 0)
                        for value in readers.group(1).split(",")
                        if value.strip()
                    )
                frames[frame][event].append(record)
    return frames


def summarize_frame(frame: int, events: dict[str, list[dict[str, object]]]) -> None:
    acquisitions = events.get("rgba-acquired", [])
    publications = events.get("published", [])
    surflive = events.get("surflive-observed", [])
    releases = events.get("display-release", [])
    busy = [record for record in acquisitions if record.get("busy") == 1]
    waits_us = [float(record["wait_us"]) for record in busy]

    # A same-surface geometry/opacity transaction retains and releases another
    # lease on the same allocation; that successful API release does not make
    # the allocation producer-reusable. Count only a SURFLIVE replacement
    # whose old and new buffers differ.
    replacement_events = [
        record
        for record in surflive
        if int(record.get("previous_frame", 0)) == frame
        and int(record.get("previous_buffer", 255)) != 255
        and int(record["previous_buffer"]) != int(record["buffer"])
    ]
    release_times: dict[int, list[int]] = defaultdict(list)
    for record in replacement_events:
        release_times[int(record["previous_buffer"])].append(int(record["observed_ns"]))
    for times in release_times.values():
        times.sort()

    matched_release = 0
    blocked_until_release_us: list[float] = []
    release_to_acquire_us: list[float] = []
    no_display_release = 0
    for record in busy:
        buffer = int(record["buffer"])
        acquired_ns = int(record["observed_ns"])
        wait_ns = int(record["wait_us"]) * 1_000
        wait_started_ns = acquired_ns - wait_ns
        times = release_times.get(buffer, [])
        index = bisect.bisect_right(times, acquired_ns) - 1
        if index < 0 or times[index] < wait_started_ns:
            no_display_release += 1
            continue
        release_ns = times[index]
        matched_release += 1
        blocked_until_release_us.append((release_ns - wait_started_ns) / 1_000)
        release_to_acquire_us.append((acquired_ns - release_ns) / 1_000)

    blocker_shapes: Counter[str] = Counter()
    target_blockers: Counter[str] = Counter()
    for record in busy:
        front = int(record.get("start_front", 255))
        acquired_mask = int(record.get("start_acquired_mask", 0))
        reader_mask = int(record.get("start_reader_mask", 0))
        occupied_mask = acquired_mask | reader_mask
        if front != 255:
            occupied_mask |= 1 << front
        blocker_shapes[
            f"occupied=0x{occupied_mask:X}/front={front}/"
            f"acquired=0x{acquired_mask:X}/readers=0x{reader_mask:X}"
        ] += 1

        buffer = int(record["buffer"])
        labels: list[str] = []
        if front == buffer:
            labels.append("front")
        if acquired_mask & (1 << buffer):
            labels.append("producer")
        readers = record.get("start_readers", ())
        if isinstance(readers, tuple) and buffer < len(readers) and readers[buffer]:
            labels.append(f"readers:{readers[buffer]}")
        target_blockers["+".join(labels) if labels else "other-buffer-freed"] += 1

    publication_times = {
        (int(record["buffer"]), int(record["frame_serial"])): int(record["observed_ns"])
        for record in publications
    }
    unique_surflive: list[dict[str, object]] = []
    seen_publish_serials: set[int] = set()
    for record in surflive:
        publish_serial = int(record["publish_serial"])
        if publish_serial == 0 or publish_serial in seen_publish_serials:
            continue
        seen_publish_serials.add(publish_serial)
        unique_surflive.append(record)
    publish_to_surflive_us: list[float] = []
    for record in unique_surflive:
        key = (int(record["buffer"]), int(record["publish_serial"]))
        published_ns = publication_times.get(key)
        if published_ns is not None:
            publish_to_surflive_us.append(
                (int(record["observed_ns"]) - published_ns) / 1_000
            )

    transaction_us = [float(record["transaction_us"]) for record in unique_surflive]
    pre_flip_us = [float(record["pre_flip_us"]) for record in unique_surflive]
    flip_wait_us = [float(record["flip_wait_us"]) for record in unique_surflive]
    flip_polls = [float(record["flip_polls"]) for record in unique_surflive]
    coupled_batches = sum(
        int(record.get("batch_planes", 0)) > 1
        or int(record.get("batch_guc_jobs", 0)) > 0
        for record in unique_surflive
    )

    print(f"frame={frame}")
    print(
        f"  acquire samples={len(acquisitions)} busy={len(busy)} "
        f"busy_rate={(100.0 * len(busy) / len(acquisitions)) if acquisitions else 0:.1f}% "
        f"{metric('busy_wait', waits_us)}"
    )
    print(
        f"  release_match matched={matched_release} "
        f"no_display_release_in_wait={no_display_release} "
        f"{metric('wait_start_to_release', blocked_until_release_us)} "
        f"{metric('release_to_acquire', release_to_acquire_us)}"
    )
    print(
        "  target_blockers "
        + (" ".join(f"{key}:{count}" for key, count in target_blockers.most_common()) or "none")
    )
    print(
        "  initial_ownership "
        + (" ".join(f"{key}:{count}" for key, count in blocker_shapes.most_common()) or "none")
    )
    print(
        f"  display publications={len(publications)} unique_surflive={len(unique_surflive)} "
        f"effective_replacements={len(replacement_events)} raw_releases={len(releases)} "
        f"coupled_batches={coupled_batches} "
        f"{metric('publish_to_surflive', publish_to_surflive_us)}"
    )
    print(
        f"  display_phases {metric('transaction', transaction_us)} "
        f"{metric('pre_flip', pre_flip_us)} {metric('flip_wait', flip_wait_us)} "
        f"{metric('flip_polls', flip_polls, unit='')}"
    )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Analyze ui4 video-surface-lifecycle markers from bare-metal logs."
    )
    parser.add_argument("logs", nargs="+", type=Path)
    args = parser.parse_args()
    missing = [path for path in args.logs if not path.is_file()]
    if missing:
        parser.error("not a file: " + ", ".join(str(path) for path in missing))
    frames = parse_logs(args.logs)
    if not frames:
        print("no ui4 video-surface-lifecycle markers found")
        return 1
    for frame in sorted(frames):
        summarize_frame(frame, frames[frame])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
