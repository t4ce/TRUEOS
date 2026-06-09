#!/usr/bin/env python3
"""Check that a LAN host exposes only the expected TCP ports.

Default target/ports match the current TRUEOS baremetal host in the logs.
"""

from __future__ import annotations

import argparse
import asyncio
import sys
import time


DEFAULT_HOST = "192.168.178.94"
DEFAULT_EXPECTED_TCP = "2,80,1337,4245,32344"


def parse_ports(value: str) -> set[int]:
    ports: set[int] = set()
    for part in value.split(","):
        part = part.strip()
        if not part:
            continue
        if "-" in part:
            start_text, end_text = part.split("-", 1)
            start = int(start_text, 10)
            end = int(end_text, 10)
            if start > end:
                raise ValueError(f"bad port range: {part}")
            ports.update(range(start, end + 1))
            continue
        ports.add(int(part, 10))

    invalid = [port for port in ports if port < 1 or port > 65535]
    if invalid:
        raise ValueError(f"ports out of range: {invalid}")
    return ports


async def probe_tcp(host: str, port: int, timeout: float, sem: asyncio.Semaphore) -> bool:
    async with sem:
        try:
            _reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host, port),
                timeout=timeout,
            )
            writer.close()
            try:
                await writer.wait_closed()
            except Exception:
                pass
            return True
        except Exception:
            return False


async def scan_tcp(
    host: str,
    ports: range,
    timeout: float,
    concurrency: int,
) -> list[int]:
    sem = asyncio.Semaphore(concurrency)
    open_ports: list[int] = []

    async def run_one(port: int) -> None:
        if await probe_tcp(host, port, timeout, sem):
            open_ports.append(port)

    await asyncio.gather(*(run_one(port) for port in ports))
    return sorted(open_ports)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Scan TCP ports and fail if the host exposes unexpected ports.",
    )
    parser.add_argument("host", nargs="?", default=DEFAULT_HOST)
    parser.add_argument(
        "--expected",
        default=DEFAULT_EXPECTED_TCP,
        help=f"comma/range list of allowed TCP ports; default: {DEFAULT_EXPECTED_TCP}",
    )
    parser.add_argument("--start", type=int, default=1)
    parser.add_argument("--end", type=int, default=65535)
    parser.add_argument("--timeout", type=float, default=1.0)
    parser.add_argument("--concurrency", type=int, default=160)
    return parser


async def async_main() -> int:
    args = build_parser().parse_args()
    if args.start < 1 or args.end > 65535 or args.start > args.end:
        print(f"bad scan range: {args.start}-{args.end}", file=sys.stderr)
        return 2

    try:
        expected = parse_ports(args.expected)
    except ValueError as exc:
        print(f"bad --expected: {exc}", file=sys.stderr)
        return 2

    started = time.monotonic()
    open_ports = await scan_tcp(
        args.host,
        range(args.start, args.end + 1),
        args.timeout,
        args.concurrency,
    )
    open_set = set(open_ports)
    unexpected = sorted(open_set - expected)
    missing = sorted(expected - open_set)

    print(f"host={args.host}")
    print(f"scan_tcp_range={args.start}-{args.end}")
    print("open_tcp=" + (",".join(map(str, open_ports)) if open_ports else "none"))
    print("expected_tcp=" + ",".join(map(str, sorted(expected))))
    print("unexpected_tcp=" + (",".join(map(str, unexpected)) if unexpected else "none"))
    print("missing_expected_tcp=" + (",".join(map(str, missing)) if missing else "none"))
    print(f"elapsed_s={time.monotonic() - started:.2f}")

    return 1 if unexpected or missing else 0


def main() -> int:
    return asyncio.run(async_main())


if __name__ == "__main__":
    raise SystemExit(main())
