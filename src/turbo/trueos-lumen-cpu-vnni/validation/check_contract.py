#!/usr/bin/env python3
"""Independent, dependency-free arithmetic check for the LFM Q8 VNNI mapping."""

from __future__ import annotations

import ctypes
import ctypes.util
import math
import random
import struct
from dataclasses import dataclass

BLOCK_VALUES = 32
LANES = 8

try:
    _fmaf = ctypes.CDLL(None).fmaf
except AttributeError:
    library = ctypes.util.find_library("m")
    if library is None:
        raise RuntimeError("validation requires an IEEE-754 fmaf implementation")
    _fmaf = ctypes.CDLL(library).fmaf
_fmaf.argtypes = [ctypes.c_float, ctypes.c_float, ctypes.c_float]
_fmaf.restype = ctypes.c_float


def f32(value: float | int) -> float:
    return struct.unpack("<f", struct.pack("<f", float(value)))[0]


def f32_bits(value: float) -> int:
    return struct.unpack("<I", struct.pack("<f", f32(value)))[0]


def fmaf(left: float, right: float, add: float) -> float:
    return f32(_fmaf(f32(left), f32(right), f32(add)))


def f16_bits_to_f32(bits: int) -> float:
    half = struct.unpack("<e", struct.pack("<H", bits))[0]
    return f32(half)


def f32_to_f16(value: float) -> float:
    return struct.unpack("<e", struct.pack("<e", f32(value)))[0]


def scalar_dot4(weight: list[int], activation: list[int], lane: int) -> int:
    start = lane * 4
    pair0 = weight[start] * activation[start] + weight[start + 1] * activation[start + 1]
    pair1 = weight[start + 2] * activation[start + 2] + weight[start + 3] * activation[start + 3]
    pair0 = min(32767, max(-32768, pair0))
    pair1 = min(32767, max(-32768, pair1))
    return pair0 + pair1


def vnni_dot4(weight: list[int], activation: list[int], lane: int) -> int:
    start = lane * 4
    total = 0
    for index in range(start, start + 4):
        q = activation[index]
        magnitude = abs(q)
        signed_weight = -weight[index] if q < 0 else (0 if q == 0 else weight[index])
        total += magnitude * signed_weight
    return total


def reduce_lanes(lanes: list[float]) -> float:
    a0 = f32(lanes[0] + lanes[4])
    a1 = f32(lanes[1] + lanes[5])
    a2 = f32(lanes[2] + lanes[6])
    a3 = f32(lanes[3] + lanes[7])
    b0 = f32(a0 + a2)
    b1 = f32(a1 + a3)
    return f32(b0 + b1)


@dataclass
class Totals:
    exhaustive_pairs: int = 0
    random_dot4_lanes: int = 0
    random_rows: int = 0
    quantized_values: int = 0


def main() -> None:
    totals = Totals()

    for q in range(-127, 128):
        for weight in range(-127, 128):
            magnitude = abs(q)
            signed_weight = -weight if q < 0 else (0 if q == 0 else weight)
            assert magnitude * signed_weight == q * weight
            totals.exhaustive_pairs += 1

    max_pair_magnitude = 2 * 127 * 127
    assert max_pair_magnitude == 32_258
    assert max_pair_magnitude <= 32_767

    rng = random.Random(0x4C464D3235)
    scale_bits = [
        0x0000,
        0x0001,
        0x03FF,
        0x0400,
        0x1000,
        0x2C00,
        0x3400,
        0x3C00,
        0x4000,
        0x57FF,
    ]

    for columns in (1024, 4608):
        blocks = columns // BLOCK_VALUES
        for _row in range(64):
            oracle_lanes = [f32(0.0) for _ in range(LANES)]
            vnni_lanes = [f32(0.0) for _ in range(LANES)]
            for _block in range(blocks):
                weight = [rng.randrange(-127, 128) for _ in range(BLOCK_VALUES)]
                activation = [rng.randrange(-127, 128) for _ in range(BLOCK_VALUES)]
                weight_scale = f16_bits_to_f32(rng.choice(scale_bits))
                activation_scale = f16_bits_to_f32(rng.choice(scale_bits))
                scale = f32(weight_scale * activation_scale)
                for lane in range(LANES):
                    scalar = scalar_dot4(weight, activation, lane)
                    transformed = vnni_dot4(weight, activation, lane)
                    assert scalar == transformed
                    oracle_lanes[lane] = fmaf(scale, f32(scalar), oracle_lanes[lane])
                    vnni_lanes[lane] = fmaf(scale, f32(transformed), vnni_lanes[lane])
                    totals.random_dot4_lanes += 1
            oracle = reduce_lanes(oracle_lanes)
            observed = reduce_lanes(vnni_lanes)
            assert f32_bits(oracle) == f32_bits(observed)
            totals.random_rows += 1

    for _ in range(4096):
        values = [f32(rng.gauss(0.0, 3.0)) for _ in range(BLOCK_VALUES)]
        maximum = f32(max(abs(value) for value in values))
        scale = f32(maximum / f32(127.0))
        stored_scale = f32_to_f16(scale)
        assert math.isfinite(stored_scale)
        inverse = f32(0.0) if maximum == f32(0.0) else f32(f32(127.0) / maximum)
        quantized = [int(round(f32(value * inverse))) for value in values]
        assert min(quantized) >= -127
        assert max(quantized) <= 127
        assert -128 not in quantized
        totals.quantized_values += BLOCK_VALUES

    # Fixed model arithmetic from the integration contract.
    q8_values = 354_418_688
    q8_blocks = q8_values // 32
    q8_bytes = 240_648_192 + 44_564_480 + 20_054_016 + 71_303_168
    assert q8_blocks == 11_075_584
    assert 48 + 20 + 24 + 1 == 93
    assert q8_bytes == 376_569_856

    print("signed-magnitude exhaustive pairs:", totals.exhaustive_pairs)
    print("random dot4 lane checks:", totals.random_dot4_lanes)
    print("bit-identical random rows:", totals.random_rows)
    print("quantizer values checked:", totals.quantized_values)
    print("model Q8 native bytes:", q8_bytes)
    print("model Q8 blocks / VPDPBUSD count:", q8_blocks)
    print("contract validation: PASS")


if __name__ == "__main__":
    main()
