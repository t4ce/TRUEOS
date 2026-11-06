#!/usr/bin/env python3
"""Observe/repair TRUEOS xHCI through its storage-independent LAN HTTP service.

Examples:
  python3 tools/usb-heal.py --host 192.168.178.90 observe --save /tmp/usb.json
  python3 tools/usb-heal.py --host 192.168.178.90 snapshot --controller 1
  python3 tools/usb-heal.py --host 192.168.178.90 port-reset --controller 1 --port 3

Only port-reset mutates hardware. It refuses any root port that has ever been
admitted to addressing, and prints the complete proof/rejection response.
"""
import argparse
import json
from pathlib import Path
import sys
from urllib.error import HTTPError, URLError
from urllib.request import ProxyHandler, Request, build_opener


# LAN diagnostics must not follow workstation HTTP(S)_PROXY settings.
LAN_OPENER = build_opener(ProxyHandler({}))


class HealError(Exception):
    pass


def request_json(base, path, body=None):
    data = None if body is None else json.dumps(body).encode()
    request = Request(base + path, data=data, headers={"Accept": "application/json"})
    if data is not None:
        request.add_header("Content-Type", "application/json")
    try:
        with LAN_OPENER.open(request, timeout=12) as response:
            status, raw = response.status, response.read(1024 * 1024)
    except HTTPError as error:
        status, raw = error.code, error.read(1024 * 1024)
    except (URLError, TimeoutError) as error:
        raise HealError(str(error)) from error
    try:
        payload = json.loads(raw)
    except (ValueError, UnicodeError):
        payload = {"detail": raw.decode("utf-8", errors="replace")}
    return status, payload


def invoke(base, command, controller=0, port=None, timeout_ms=500):
    if command == "describe":
        return request_json(base, "/usb/heal/describe")
    if command == "observe":
        return request_json(base, "/usb/heal/observe")
    status, observation = request_json(base, "/usb/heal", {"op": "observe", "api": 1, "controller": controller})
    if status != 200:
        return status, observation
    if not isinstance(observation, dict) or observation.get("status") != "ok":
        raise HealError(f"fresh observation failed: {observation}")
    if command == "snapshot":
        return status, observation
    revision = observation["revision"]
    return request_json(base, "/usb/heal", {
        "op": "run", "api": 1, "controller": controller, "expected_revision": revision,
        "pattern": {"name": "usb2_port_reset_v1", "port": {"kind": "port", "port_id": port}, "timeout_ms": timeout_ms},
    })


def parser():
    result = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    result.add_argument("--host", required=True, help="TRUEOS LAN hostname or IPv4 address")
    result.add_argument("--http-port", type=int, default=80)
    commands = result.add_subparsers(dest="command", required=True)
    for command in ("describe", "observe", "snapshot", "port-reset"):
        sub = commands.add_parser(command)
        sub.add_argument("--save", type=Path, help="save the exact returned JSON plus HTTP status")
        if command in ("snapshot", "port-reset"):
            sub.add_argument("--controller", type=int, required=True)
        if command == "port-reset":
            sub.add_argument("--port", type=int, required=True)
            sub.add_argument("--timeout-ms", type=int, choices=range(10, 1001), default=500, metavar="10..1000")
    return result


def main():
    args = parser().parse_args()
    if not 1 <= args.http_port <= 65535:
        parser().error("HTTP port must be 1..65535")
    if getattr(args, "controller", 0) < 0 or not 1 <= getattr(args, "port", 1) <= 255:
        parser().error("controller must be nonnegative and USB port must be 1..255")
    base = f"http://{args.host}:{args.http_port}"
    try:
        status, payload = invoke(base, args.command, getattr(args, "controller", 0), getattr(args, "port", None), getattr(args, "timeout_ms", 500))
    except HealError as error:
        print(f"Heal connection failed: {error}", file=sys.stderr)
        return 1
    rendered = json.dumps({"http_status": status, "response": payload}, indent=2) + "\n"
    print(rendered, end="")
    if args.save:
        args.save.parent.mkdir(parents=True, exist_ok=True)
        args.save.write_text(rendered)
    rejected = isinstance(payload, dict) and payload.get("status") == "rejected"
    return 0 if 200 <= status < 300 and not rejected else 1


if __name__ == "__main__":
    raise SystemExit(main())
