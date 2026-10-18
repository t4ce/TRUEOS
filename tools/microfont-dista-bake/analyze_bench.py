#!/usr/bin/env python3
"""Summarize optional Microfont Dista benchmark JSON records from a rig log.

Sample zero is the first submission for a configuration. Samples one and above
are warm observations. P95 uses the nearest-rank definition; missing hardware
timestamps remain missing and are never replaced with host wall time.
"""
from __future__ import annotations

import argparse
from collections import Counter, defaultdict
import json
import math
from pathlib import Path
import statistics
import sys

PREFIX = "microfont-dista-bench: "
METRICS = ("wall_ns", "gpu_draw_ns", "gpu_target_ready_ns", "first_target_ready_ns",
           "gpu_clear_setup_ns", "gpu_writeback_ns")
LEGACY_METRICS = {"gpu_target_ready_ns":"gpu_rgba_ready_ns", "first_target_ready_ns":"first_rgba_ready_ns"}


def timing(record, metric):
    """Old RGBA-only captures used an output-specific name for target readiness."""
    return record.get(metric, record.get(LEGACY_METRICS.get(metric)))


def read_records(lines):
    records, malformed = [], []
    decoder = json.JSONDecoder()
    for number, line in enumerate(lines, 1):
        start = line.find(PREFIX)
        if start < 0:
            continue
        body = line[start+len(PREFIX):].lstrip()
        try:
            record, _ = decoder.raw_decode(body)
            if not isinstance(record, dict) or not isinstance(record.get("kind"), str):
                raise ValueError("expected object with string kind")
        except (ValueError, json.JSONDecodeError) as error:
            malformed.append({"line":number,"reason":str(error)})
            continue
        records.append(record)
    return records, malformed


def distribution(values):
    ordered = sorted(values)
    if not ordered:
        return {"count":0,"median_ns":None,"p95_ns":None,"min_ns":None,"max_ns":None}
    return {"count":len(ordered),"median_ns":statistics.median(ordered),
            "p95_ns":ordered[max(0,math.ceil(len(ordered)*0.95)-1)],
            "min_ns":ordered[0],"max_ns":ordered[-1]}


def valid_time(value):
    return isinstance(value,(int,float)) and not isinstance(value,bool) and math.isfinite(value) and value >= 0


def summarize(records, malformed=()):
    groups, invalid = defaultdict(list), []
    for record in records:
        if record["kind"] != "sample":
            continue
        sample = record.get("sample")
        output_format = record.get("output_format", "rgba8")
        stage = record.get("stage", "atlas-rebuild")
        if (not all(isinstance(record.get(key),str) for key in ("variant","quality","shape"))
                or output_format not in ("r8", "rgba8") or not isinstance(stage,str) or not stage
                or not isinstance(sample,int) or isinstance(sample,bool) or sample < 0
                or not valid_time(record.get("wall_ns"))
                or any(timing(record,metric) is not None and not valid_time(timing(record,metric)) for metric in METRICS[1:])
                or any(record.get(key) is not None and (not isinstance(record[key],int)
                    or isinstance(record[key],bool) or record[key] <= 0)
                    for key in ("destination_bytes", "destination_pitch_bytes"))):
            invalid.append(record)
            continue
        groups[(stage,output_format,record["variant"],record["quality"],record["shape"])].append(record)
    summaries = []
    for (stage,output_format,variant,quality,shape), samples in sorted(groups.items()):
        result = {"stage":stage,"output_format":output_format,
                  "variant":variant,"quality":quality,"shape":shape,"raw_count":len(samples),
                  "destination_bytes":sorted({s["destination_bytes"] for s in samples if s.get("destination_bytes") is not None}),
                  "destination_pitch_bytes":sorted({s["destination_pitch_bytes"] for s in samples if s.get("destination_pitch_bytes") is not None}),
                  "first_count":sum(s["sample"] == 0 for s in samples),
                  "warm_count":sum(s["sample"] >= 1 for s in samples)}
        for metric in METRICS:
            result[metric] = {
                phase:distribution([timing(s,metric) for s in samples
                                    if (s["sample"] == 0) == (phase == "first") and timing(s,metric) is not None])
                for phase in ("first","warm")
            }
        # Retain the old summary keys for RGBA-only report consumers.
        if output_format == "rgba8":
            for current, legacy in LEGACY_METRICS.items(): result[legacy] = result[current]
        summaries.append(result)
    builds = sorted({record["source_sha256"] for record in records
                     if record["kind"] == "begin" and isinstance(record.get("source_sha256"), str)})
    return {"schema":"microfont-dista-bench-summary/v2","percentile":"nearest-rank",
            "mixed_builds":len(builds) > 1, "source_sha256":builds,
            "record_counts":dict(Counter(record["kind"] for record in records)),
            "begin":[record for record in records if record["kind"] == "begin"],
            "setup":[record for record in records if record["kind"] == "setup"],
            "done":[record for record in records if record["kind"] == "done"],
            "validation":[record for record in records if record["kind"] == "validation"],
            "errors":[record for record in records if record["kind"] == "error"],
            "malformed":list(malformed),"invalid_samples":invalid,"groups":summaries}


def markdown(report):
    lines = ["Times in milliseconds: first / warm median / warm p95. P95 is nearest rank.",
             "Warm atlas-rebuild samples regenerate all glyphs; they do not measure retained atlas copies.",
             "First is a submission per configuration; it does not imply a cold GPU or empty caches.", "",
             "| Stage | Format | Variant | Quality | Shape | Target bytes | First / warm count | First end-to-end | Render wall | GPU draw | GPU target ready | GPU clear/setup | GPU writeback |",
             "| --- | --- | --- | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
    def shown(value):
        return "—" if value is None else f"{value/1_000_000:.6f}"
    for group in report["groups"]:
        values = []
        for metric in METRICS[:3] + METRICS[4:]:
            value = group[metric]
            values.append(" / ".join(shown(n) for n in (value["first"]["median_ns"],
                                                        value["warm"]["median_ns"],value["warm"]["p95_ns"])))
        lines.append(f"| {group['stage']} | {group['output_format']} | {group['variant']} | {group['quality']} | {group['shape']} | "
                     f"{', '.join(str(n) for n in group['destination_bytes']) or '—'} | "
                     f"{group['first_count']} / {group['warm_count']} | "
                     f"{shown(group['first_target_ready_ns']['first']['median_ns'])} | {' | '.join(values)} |")
    lines += ["",f"Records: {json.dumps(report['record_counts'],sort_keys=True)}."]
    if any(group["first_count"] > 1 for group in report["groups"]):
        lines.append("Multiple first samples were supplied; the first column reports their median. Keep captures with different builds separate.")
    if not report["done"]:
        lines.append("No done record found; this may be an incomplete capture.")
    if report["validation"]:
        lines.append(f"Post-timing format validation: {sum(v.get('passed') is True for v in report['validation'])}/{len(report['validation'])} pairs passed.")
    if report["mixed_builds"]:
        lines.append("Mixed shader builds: these grouped values are not a valid matched comparison; split the captures.")
    if report["errors"] or report["malformed"] or report["invalid_samples"]:
        lines.append(f"Errors: {len(report['errors'])}; malformed records: {len(report['malformed'])}; invalid samples: {len(report['invalid_samples'])}.")
    return "\n".join(lines)+"\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("logs", nargs="*", type=Path, help="log files; default stdin (or use -)")
    parser.add_argument("--json", action="store_true", help="emit the complete numeric report as JSON")
    args = parser.parse_args()
    records, malformed = [], []
    for path in args.logs or [Path("-")]:
        lines = sys.stdin if str(path) == "-" else path.read_text(errors="replace").splitlines()
        found, errors = read_records(lines)
        records.extend(found)
        malformed.extend({"source":str(path),**error} for error in errors)
    report = summarize(records,malformed)
    print(json.dumps(report,indent=2) if args.json else markdown(report),end="\n" if args.json else "")
    if (not report["groups"] or report["errors"] or report["malformed"] or report["invalid_samples"] or report["mixed_builds"]
            or any(v.get("passed") is not True for v in report["validation"])):
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
