#!/usr/bin/env python3
"""Deterministic model check for the TRUEOS Q64 TSC-to-tick experiment."""

from __future__ import annotations

import argparse
import random

TICK_HZ = 1_000
U64_MAX = (1 << 64) - 1


def old_ticks(delta_tsc: int, tsc_hz: int) -> int:
    return delta_tsc * TICK_HZ // tsc_hz


def scale_q64(tsc_hz: int) -> int:
    if tsc_hz <= TICK_HZ:
        raise ValueError("the experiment requires tsc_hz > TICK_HZ")
    return (TICK_HZ << 64) // tsc_hz


def new_ticks(delta_tsc: int, tsc_hz: int) -> int:
    scale = scale_q64(tsc_hz)
    estimate = delta_tsc * scale >> 64
    next_tick = estimate + 1
    return estimate + int(next_tick * tsc_hz <= delta_tsc * TICK_HZ)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--random-cases", type=int, default=2_000_000)
    args = parser.parse_args()

    frequencies = [
        1_000_000,
        3_000_000,
        24_000_000,
        100_000_000,
        1_000_000_000,
        2_400_000_000,
        3_579_545_000,
        5_800_000_000,
        U64_MAX,
    ]
    deltas = [
        0,
        1,
        2,
        999,
        1_000,
        1_001,
        (1 << 32) - 1,
        1 << 32,
        (1 << 63) - 1,
        1 << 63,
        U64_MAX - 1,
        U64_MAX,
    ]

    checked = 0
    for hz in frequencies:
        for delta in deltas:
            assert new_ticks(delta, hz) == old_ticks(delta, hz), (delta, hz)
            checked += 1

    rng = random.Random(0x545255454F53)
    for index in range(args.random_cases):
        if index & 1:
            hz = rng.randrange(1_000_000, 10_000_000_001)
        else:
            hz = rng.randrange(1_000_000, 1 << 64)
        delta = rng.randrange(0, 1 << 64)
        old = old_ticks(delta, hz)
        new = new_ticks(delta, hz)
        assert new == old, (index, delta, hz, old, new)
        checked += 1

    print(f"ok: {checked:,} exact comparisons")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
