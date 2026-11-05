#!/usr/bin/env python3
"""Correlate the newest finite native download-smoke run with HTTP evidence.

403/429 responses are complete diagnostic evidence and failed HTTP downloads.
An incomplete newest run never borrows evidence from an older successful run.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import sys
from urllib.parse import urljoin, urlsplit, urlunsplit

PREFIX = re.compile(r"(download-smoke-app|download-http):\s*")


def public_url(url: str) -> str:
    parts = urlsplit(url)
    host = (parts.hostname or "").lower()
    if ":" in host:
        host = f"[{host}]"
    port = parts.port
    if port is not None and port != {"https": 443, "http": 80}.get(parts.scheme):
        host += f":{port}"
    return urlunsplit((parts.scheme.lower(), host, parts.path or "/", "", ""))


def parse_records(contents: str) -> tuple[list[dict], list[str]]:
    records, issues, seen_app = [], [], set()
    decoder = json.JSONDecoder()
    for number, line in enumerate(contents.splitlines(), 1):
        match = PREFIX.search(line)
        if not match:
            continue
        source = "app" if match[1] == "download-smoke-app" else "http"
        try:
            record, _ = decoder.raw_decode(line[match.end():])
            expected = "download_smoke_app_v1" if source == "app" else "download_http_v1"
            if not isinstance(record, dict) or record.get("schema") != expected:
                raise ValueError("unexpected diagnostic schema")
            if source == "app":
                if not isinstance(record.get("run"), str) or not record["run"]:
                    raise ValueError("app record lacks run identifier")
                key = json.dumps(record, sort_keys=True, separators=(",", ":"))
                if key in seen_app:
                    continue
                seen_app.add(key)
            records.append(dict(record, _source=source, _line=number))
        except (ValueError, TypeError) as error:
            issues.append(f"line {number}: {source} record: {error}")
    return records, issues


def check_retry(record: dict, ordinal: int, previous: dict | None, route: str) -> list[str]:
    issues = []
    if route != "image" or record.get("scope") != "image" or record.get("status") != 429:
        issues.append("retry marker is not an image HTTP 429 retry")
    if type(record.get("attempt")) is not int or record.get("attempt") != ordinal or record.get("max_retries") != 2 or not 1 <= ordinal <= 2:
        issues.append("retry marker exceeds or disagrees with the two-retry sequence")
    fields = ("delay_ms", "retry_after_ms", "elapsed_since_response_ms", "elapsed_ms", "remaining_ms", "budget_ms")
    if any(type(record.get(field)) is not int or record[field] < 0 for field in fields):
        issues.append("retry marker lacks nonnegative integer budget/delay evidence")
        return issues
    delay, minimum = record["delay_ms"], record["retry_after_ms"]
    since_response = record["elapsed_since_response_ms"]
    elapsed, remaining, budget = record["elapsed_ms"], record["remaining_ms"], record["budget_ms"]
    if not 1 <= budget <= 15_000 or elapsed > budget or remaining > budget or abs(elapsed + remaining - budget) > 1:
        issues.append("retry marker violates the total 15-second request budget")
    if minimum < 1000 or since_response > elapsed or delay + since_response < minimum or delay >= remaining:
        issues.append("retry delay violates Retry-After minimum or remaining request budget")
    if previous is not None:
        if record["budget_ms"] != previous.get("budget_ms"):
            issues.append("retry marker changed the request budget")
        if all(type(previous.get(key)) is int for key in ("elapsed_ms", "delay_ms")) and elapsed + 1 < previous["elapsed_ms"] + previous["delay_ms"]:
            issues.append("retry marker did not account for the preceding wait")
    return issues


def summarize_run(records: list[dict], run: str | None = None, parse_issues: list[str] | None = None) -> dict:
    issues = list(parse_issues or [])
    app = [r for r in records if r["_source"] == "app"]
    selected = run or next((r["run"] for r in reversed(app)), None)
    app = [r for r in app if r["run"] == selected]
    starts = [r for r in app if r.get("event") == "start"]
    summaries = [r for r in app if r.get("event") == "summary"]
    if len(starts) != 1:
        issues.append(f"expected one app start, observed {len(starts)}")
    if len(summaries) != 1:
        issues.append(f"expected one final app summary, observed {len(summaries)}")
    start = starts[0] if starts else {}
    summary = summaries[0] if summaries else {}
    expected = start.get("requests", 8)
    if type(expected) is not int or expected < 1 or expected > 24:
        issues.append("invalid expected request count")
        expected = 8
    if start.get("urls") and start.get("routes"):
        if len(start["urls"]) * len(start["routes"]) != expected:
            issues.append("app start matrix does not match its request count")
    matrix = [(route, public_url(url)) for route in start.get("routes", []) for url in start.get("urls", [])]
    groups: dict[int, dict] = {}
    for record in app:
        event = record.get("event")
        if event not in ("begin", "request"):
            if event == "error":
                issues.append(f"app error at line {record['_line']}: {record.get('error')}")
            continue
        case = record.get("case")
        if type(case) is not int or not 0 <= case < expected:
            issues.append(f"invalid case identifier at line {record['_line']}")
            continue
        group = groups.setdefault(case, {})
        if event in group:
            issues.append(f"case {case}: conflicting duplicate {event}")
        else:
            group[event] = record
    cases = []
    for number in range(expected):
        group = groups.get(number, {})
        begin, result = group.get("begin"), group.get("request")
        case_issues = []
        evidence = []
        responses = []
        terminal = None
        retries = []
        retried_failures = []
        pending_retry = None
        if begin is None or result is None:
            case_issues.append("missing case begin or final request record")
        else:
            if number < len(matrix) and (begin.get("route"), public_url(begin.get("url", ""))) != matrix[number]:
                case_issues.append("case does not match the ordered app start matrix")
            for key in ("route", "url"):
                if begin.get(key) != result.get(key):
                    case_issues.append(f"case {key} changed during request")
            if begin["_line"] >= result["_line"]:
                case_issues.append("case result precedes begin")
            if result.get("completed") is not True:
                case_issues.append(f"app operation did not complete: {result.get('error')}")
            if type(result.get("code")) is not int:
                case_issues.append("missing integer API error code")
            elif result.get("api_success") is True and result["code"] != 0:
                case_issues.append("API success has a nonzero error code")
            allowed = {public_url(begin.get("url", ""))}
            for record in records:
                if record["_source"] != "http" or not begin["_line"] < record["_line"] < result["_line"]:
                    continue
                url = record.get("url")
                if not isinstance(url, str) or public_url(url) not in allowed:
                    continue
                evidence.append(record)
                if record.get("event") == "retry":
                    marker_issues = check_retry(record, len(retries) + 1, retries[-1] if retries else None, begin.get("route", ""))
                    case_issues.extend(marker_issues)
                    if terminal is None or terminal.get("status") != 429 or public_url(terminal["url"]) != public_url(url):
                        case_issues.append("retry marker does not follow a matching URL HTTP 429")
                    elif pending_retry is not None:
                        case_issues.append("multiple retry markers for one HTTP 429 response")
                    elif not marker_issues:
                        pending_retry = record
                    retries.append(record)
                    continue
                if record.get("event") != "response":
                    continue
                if terminal is not None:
                    if pending_retry is not None and public_url(url) == public_url(pending_retry["url"]):
                        retried_failures.append(terminal)
                        terminal = None
                        pending_retry = None
                    else:
                        case_issues.append("ambiguous multiple terminal HTTP responses in case window; matching retry marker required after HTTP 429")
                responses.append(record)
                status = record.get("status")
                if type(status) is not int or not 100 <= status <= 599:
                    case_issues.append("invalid HTTP status in response")
                    continue
                if not isinstance(record.get("user_agent"), str) or not record["user_agent"]:
                    case_issues.append("HTTP response lacks request User-Agent evidence")
                if not re.fullmatch(r"[0-9a-f]{64}", str(record.get("body_sha256", ""))):
                    case_issues.append("HTTP response lacks body SHA-256 evidence")
                if type(record.get("body_bytes")) is not int or record["body_bytes"] < 0:
                    case_issues.append("HTTP response lacks body byte count")
                if status in (301, 302, 303, 307, 308):
                    location = (record.get("headers") or {}).get("location")
                    if isinstance(location, str) and location:
                        allowed.add(public_url(urljoin(url, location)))
                    else:
                        case_issues.append("HTTP redirect lacks destination evidence")
                else:
                    terminal = record
            if pending_retry is not None:
                case_issues.append("retry marker lacks a subsequent matching URL HTTP response")
            if terminal is None:
                case_issues.append("missing terminal HTTP response evidence")
            if terminal is not None and result.get("api_success") is True:
                if not 200 <= terminal["status"] < 300:
                    case_issues.append("app reported API success despite unsuccessful HTTP status")
                encoding = (terminal.get("headers") or {}).get("content_encoding")
                if result.get("route", "").startswith("general") and encoding in (None, "", "identity"):
                    if result.get("sha256") != terminal.get("body_sha256") or result.get("bytes") != terminal.get("body_bytes"):
                        case_issues.append("general API bytes do not match terminal HTTP body fingerprint")
                if result.get("route", "").startswith("general") and result.get("final_url") is not None:
                    if public_url(result["final_url"]) != public_url(terminal["url"]):
                        case_issues.append("general final URL metadata does not match the terminal HTTP response")
        http_success = terminal is not None and type(terminal.get("status")) is int and 200 <= terminal["status"] < 300
        api_success = result is not None and result.get("api_success") is True
        recovered_failures = len(retried_failures) if http_success else 0
        classification = "incomplete"
        if terminal is not None:
            if not http_success:
                classification = "http_failure"
            elif api_success:
                classification = "download_succeeded"
            elif result and result.get("route") == "image":
                classification = "http_succeeded_image_rejected"
            else:
                classification = "http_succeeded_api_failed"
        cases.append({"case": number, "url": (begin or result or {}).get("url"),
                      "route": (begin or result or {}).get("route"), "diagnostic_complete": not case_issues,
                      "missing_http_evidence": terminal is None, "http_success": http_success,
                      "classification": classification, "http_status": terminal.get("status") if terminal else None,
                      "recovered_http_failures": recovered_failures, "retried_http_failures": len(retried_failures),
                      "http_failure_responses": sum(type(r.get("status")) is int and r["status"] >= 400 for r in responses),
                      "retries": retries, "api_result": result, "responses": responses, "http_events": evidence, "issues": case_issues})
    if summary:
        if summary.get("cases") != expected:
            issues.append("final summary case count does not match expected matrix")
        if summary.get("completed") != sum(bool(c["api_result"] and c["api_result"].get("completed")) for c in cases):
            issues.append("final summary completed count disagrees with case records")
        if "api_successes" in summary and summary["api_successes"] != sum(bool(c["api_result"] and c["api_result"].get("api_success")) for c in cases):
            issues.append("final summary API success count disagrees with case records")
        if summaries[0]["_line"] < max((r["_line"] for r in app if r.get("event") == "request"), default=0):
            issues.append("summary precedes final request")
    complete = not issues and all(c["diagnostic_complete"] for c in cases)
    failures = sum(c["classification"] == "http_failure" for c in cases)
    http_ok = complete and all(c["http_success"] for c in cases)
    return {"schema": "download_smoke_report_v1", "run": selected, "selection": "explicit run" if run else "newest observed app run",
            "status": "incomplete" if not complete else "complete_with_http_failures" if failures else "complete",
            "diagnostic_complete": complete, "http_success": http_ok, "http_failures": failures,
            "recovered_http_failures": sum(c["recovered_http_failures"] for c in cases),
            "retried_http_failures": sum(c["retried_http_failures"] for c in cases),
            "http_failure_responses": sum(c["http_failure_responses"] for c in cases),
            "expected_cases": expected, "app_start": start, "app_summary": summary, "cases": cases, "issues": issues}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--log", required=True, type=Path)
    parser.add_argument("--run", help="select an explicit archived run; default is newest observed app run")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args(argv)
    output = args.output or args.log.with_suffix(".download-smoke.json")
    try:
        data = args.log.read_bytes()
        records, issues = parse_records(data.decode(errors="replace"))
        report = summarize_run(records, args.run, issues)
        report.update(created_at=datetime.now(timezone.utc).isoformat(), evidence={"log": str(args.log.resolve()), "log_sha256": hashlib.sha256(data).hexdigest()})
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2) + "\n")
    except (OSError, ValueError, TypeError) as error:
        print(f"download smoke report failed: {error}", file=sys.stderr)
        return 1
    print(f"run {report['run']}: {report['status']}; terminal HTTP failures={report['http_failures']}; recovered HTTP failures={report['recovered_http_failures']}; report: {output}")
    for case in report["cases"]:
        print(f"{case['case']:2} {str(case['route']):13} HTTP {str(case['http_status']):4} {case['classification']:31} {case['url']}")
        for issue in case["issues"]:
            print(f"   {issue}")
    for issue in report["issues"]:
        print(f"  {issue}")
    return 0 if report["diagnostic_complete"] and report["http_success"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
