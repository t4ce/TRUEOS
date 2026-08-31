#!/usr/bin/env python3
"""TRUEOS entropy research oracle.

No external dependencies. This is intentionally a slow, exact/reference lane:
- exact enumerative rank/unrank for fixed-weight binary strings;
- finite-depth binary CTW block probability using KT estimators;
- 32-state byte-rANS round trips and redundancy accounting;
- chunk reports that compare raw size against model-aware lower bounds.

It is not a production container format. Model-description overhead is shown
separately so research numbers cannot silently claim a decoder already knows a
model that was learned from the block being encoded.
"""

from __future__ import annotations

import argparse
import math
import os
import random
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

RANS_SCALE_BITS = 12
RANS_TOTAL = 1 << RANS_SCALE_BITS
RANS_L = 1 << 23
RANS_LANES = 32


def ceil_log2_int(value: int) -> int:
    if value <= 1:
        return 0
    return (value - 1).bit_length()


def bytes_to_bits(data: bytes) -> list[int]:
    return [((byte >> shift) & 1) for byte in data for shift in range(7, -1, -1)]


def enumerative_rank(bits: list[int]) -> int:
    """Lexicographic rank among n-bit strings with the same Hamming weight."""
    n = len(bits)
    ones = sum(bits)
    rank = 0
    for index, bit in enumerate(bits):
        remaining = n - index - 1
        if bit:
            rank += math.comb(remaining, ones)
            ones -= 1
    return rank


def enumerative_unrank(n: int, k: int, rank: int) -> list[int]:
    population = math.comb(n, k)
    if rank < 0 or rank >= population:
        raise ValueError("enumerative rank outside fixed-weight class")
    bits: list[int] = []
    ones = k
    for index in range(n):
        remaining = n - index - 1
        if ones == 0:
            bits.extend([0] * (n - index))
            break
        if ones == remaining + 1:
            bits.extend([1] * (n - index))
            break
        zero_population = math.comb(remaining, ones)
        if rank < zero_population:
            bits.append(0)
        else:
            bits.append(1)
            rank -= zero_population
            ones -= 1
    return bits


def enumerative_payload_bits(n: int, k: int) -> int:
    return ceil_log2_int(math.comb(n, k))


def bitplane_enumerative_bound(data: bytes) -> tuple[int, int]:
    """Exact payload bound for eight independently fixed-weight bitplanes.

    Returns (payload_bits, simple_metadata_bits). The payload bound is exact
    *given* the eight Hamming weights. The metadata term is a deliberately
    simple upper bound that stores each k in ceil(log2(n+1)) bits.
    """
    if not data:
        return 0, 0
    n = len(data)
    payload = 0
    for shift in range(8):
        k = sum((byte >> shift) & 1 for byte in data)
        payload += enumerative_payload_bits(n, k)
    metadata = 8 * ceil_log2_int(n + 1)
    return payload, metadata


def log2_kt_probability(zeroes: int, ones: int) -> float:
    """log2 of the Krichevsky-Trofimov block probability."""
    total = zeroes + ones
    value = (
        math.lgamma(zeroes + 0.5)
        + math.lgamma(ones + 0.5)
        - math.lgamma(total + 1.0)
        - 2.0 * math.lgamma(0.5)
    )
    return value / math.log(2.0)


def log2_add(a: float, b: float) -> float:
    if a == -math.inf:
        return b
    if b == -math.inf:
        return a
    hi = max(a, b)
    lo = min(a, b)
    return hi + math.log2(1.0 + 2.0 ** (lo - hi))


def ctw_code_bits(data: bytes, depth: int = 8) -> float:
    """Finite-depth binary CTW block length in bits.

    The first `depth` bits are charged raw so every tree node sees the same
    suffix-complete sample set. Counts use recent-bit-first contexts. At an
    internal node Pw = 1/2 * P_KT + 1/2 * Pw(left) * Pw(right).
    """
    bits = bytes_to_bits(data)
    if not bits:
        return 0.0
    depth = max(0, min(depth, len(bits)))
    if depth == 0:
        z = bits.count(0)
        o = len(bits) - z
        return -log2_kt_probability(z, o)
    if len(bits) <= depth:
        return float(len(bits))

    counts: dict[tuple[int, ...], list[int]] = {}
    for index in range(depth, len(bits)):
        symbol = bits[index]
        history = tuple(bits[index - 1 - offset] for offset in range(depth))
        for level in range(depth + 1):
            context = history[:level]
            pair = counts.setdefault(context, [0, 0])
            pair[symbol] += 1

    memo: dict[tuple[int, ...], float] = {}

    def weighted(context: tuple[int, ...]) -> float:
        cached = memo.get(context)
        if cached is not None:
            return cached
        zeroes, ones = counts.get(context, [0, 0])
        kt = log2_kt_probability(zeroes, ones)
        if len(context) == depth:
            result = kt
        else:
            split = weighted(context + (0,)) + weighted(context + (1,))
            result = log2_add(kt - 1.0, split - 1.0)
        memo[context] = result
        return result

    return float(depth) - weighted(())


def empirical_entropy_bits(data: bytes) -> float:
    if not data:
        return 0.0
    n = len(data)
    return sum(-count * math.log2(count / n) for count in Counter(data).values())


def normalize_frequencies(data: bytes, total: int = RANS_TOTAL) -> list[int]:
    if not data:
        result = [0] * 256
        result[0] = total
        return result
    counts = Counter(data)
    active = sorted(counts)
    raw = {symbol: counts[symbol] * total / len(data) for symbol in active}
    freq = {symbol: max(1, int(math.floor(raw[symbol]))) for symbol in active}
    current = sum(freq.values())

    if current < total:
        order = sorted(
            active,
            key=lambda symbol: (raw[symbol] - math.floor(raw[symbol]), counts[symbol], -symbol),
            reverse=True,
        )
        index = 0
        while current < total:
            symbol = order[index % len(order)]
            freq[symbol] += 1
            current += 1
            index += 1
    elif current > total:
        while current > total:
            candidates = [symbol for symbol in active if freq[symbol] > 1]
            if not candidates:
                raise ValueError("frequency normalization exhausted")
            symbol = max(candidates, key=lambda s: (freq[s] - raw[s], freq[s], -s))
            freq[symbol] -= 1
            current -= 1

    result = [0] * 256
    for symbol, value in freq.items():
        result[symbol] = value
    if sum(result) != total:
        raise AssertionError("normalized frequencies do not sum to rANS total")
    return result


def cumulative_from_frequencies(freq: list[int]) -> list[int]:
    cumulative = [0] * 256
    running = 0
    for symbol, value in enumerate(freq):
        cumulative[symbol] = running
        running += value
    if running != RANS_TOTAL:
        raise ValueError("rANS frequency total mismatch")
    return cumulative


def rans32_encode(data: bytes, freq: list[int]) -> bytes:
    """Reference 32-state byte-rANS stream.

    Format for the oracle is simply 32 little-endian initial decoder states
    followed by the interleaved renormalization byte stream. It deliberately
    omits the model table and original length; callers account for those.
    """
    cumulative = cumulative_from_frequencies(freq)
    states = [RANS_L] * RANS_LANES
    emitted: list[int] = []

    for index in range(len(data) - 1, -1, -1):
        symbol = data[index]
        f = freq[symbol]
        if f == 0:
            raise ValueError("cannot rANS-encode zero-frequency symbol")
        state_index = index % RANS_LANES
        x = states[state_index]
        x_max = ((RANS_L >> RANS_SCALE_BITS) << 8) * f
        while x >= x_max:
            emitted.append(x & 0xFF)
            x >>= 8
        states[state_index] = ((x // f) << RANS_SCALE_BITS) + (x % f) + cumulative[symbol]

    header = b"".join(state.to_bytes(4, "little") for state in states)
    return header + bytes(reversed(emitted))


def rans32_decode(stream: bytes, length: int, freq: list[int]) -> bytes:
    header_bytes = 4 * RANS_LANES
    if len(stream) < header_bytes:
        raise ValueError("truncated rANS32 state header")
    cumulative = cumulative_from_frequencies(freq)
    table = [0] * RANS_TOTAL
    for symbol, f in enumerate(freq):
        start = cumulative[symbol]
        for slot in range(start, start + f):
            table[slot] = symbol

    states = [
        int.from_bytes(stream[i * 4 : i * 4 + 4], "little")
        for i in range(RANS_LANES)
    ]
    cursor = header_bytes
    output = bytearray(length)
    mask = RANS_TOTAL - 1

    for index in range(length):
        state_index = index % RANS_LANES
        x = states[state_index]
        slot = x & mask
        symbol = table[slot]
        output[index] = symbol
        x = freq[symbol] * (x >> RANS_SCALE_BITS) + slot - cumulative[symbol]
        while x < RANS_L:
            if cursor >= len(stream):
                raise ValueError("truncated rANS32 renormalization stream")
            x = (x << 8) | stream[cursor]
            cursor += 1
        states[state_index] = x

    if cursor != len(stream):
        raise ValueError(f"rANS32 left {len(stream) - cursor} unread bytes")
    return bytes(output)


def quantized_model_bits(data: bytes, freq: list[int]) -> float:
    if not data:
        return 0.0
    return sum(-math.log2(freq[byte] / RANS_TOTAL) for byte in data)


@dataclass
class ChunkReport:
    index: int
    bytes: int
    raw_bits: int
    h0_bits: float
    ctw_bits: float
    enum_payload_bits: int
    enum_metadata_bits: int
    rans_payload_bits: int
    rans_model_bits: float


def analyze_chunk(index: int, data: bytes, depth: int) -> ChunkReport:
    freq = normalize_frequencies(data)
    stream = rans32_encode(data, freq)
    decoded = rans32_decode(stream, len(data), freq)
    if decoded != data:
        raise AssertionError("rANS32 round trip failed")
    enum_payload, enum_metadata = bitplane_enumerative_bound(data)
    return ChunkReport(
        index=index,
        bytes=len(data),
        raw_bits=len(data) * 8,
        h0_bits=empirical_entropy_bits(data),
        ctw_bits=ctw_code_bits(data, depth),
        enum_payload_bits=enum_payload,
        enum_metadata_bits=enum_metadata,
        rans_payload_bits=len(stream) * 8,
        rans_model_bits=quantized_model_bits(data, freq),
    )


def print_report(report: ChunkReport) -> None:
    def ratio(bits: float) -> float:
        return report.raw_bits / bits if bits > 0 else float("inf")

    print(f"chunk {report.index}: {report.bytes} bytes")
    print(f"  raw                    {report.raw_bits:12d} bits  1.0000x")
    print(f"  empirical H0           {report.h0_bits:12.2f} bits  {ratio(report.h0_bits):.4f}x")
    print(f"  finite-depth CTW       {report.ctw_bits:12.2f} bits  {ratio(report.ctw_bits):.4f}x")
    enum_total = report.enum_payload_bits + report.enum_metadata_bits
    print(
        f"  enum bitplanes         {enum_total:12d} bits  {ratio(enum_total):.4f}x"
        f"  (payload={report.enum_payload_bits}, k-metadata={report.enum_metadata_bits})"
    )
    print(
        f"  rANS32 oracle payload  {report.rans_payload_bits:12d} bits  {ratio(report.rans_payload_bits):.4f}x"
        f"  (known-model ideal={report.rans_model_bits:.2f}; model table/original length NOT serialized)"
    )


def self_test() -> None:
    # Exhaustively check the executable reference rank/unrank inverse over a
    # small domain; the formal proof target generalizes this to arbitrary n/k.
    for n in range(0, 13):
        for value in range(1 << n):
            bits = [((value >> (n - bit - 1)) & 1) for bit in range(n)]
            k = sum(bits)
            rank = enumerative_rank(bits)
            if not 0 <= rank < math.comb(n, k):
                raise AssertionError("enumerative rank bound failed")
            if enumerative_unrank(n, k, rank) != bits:
                raise AssertionError("enumerative inverse failed")

    rng = random.Random(0x545255454F53)
    samples = [
        b"",
        b"a",
        b"TRUEOS" * 37,
        bytes(range(256)) * 3,
        bytes(rng.randrange(0, 7) for _ in range(4096)),
        os.urandom(4096),
    ]
    for data in samples:
        freq = normalize_frequencies(data)
        stream = rans32_encode(data, freq)
        if rans32_decode(stream, len(data), freq) != data:
            raise AssertionError("rANS32 self-test failed")
        ctw = ctw_code_bits(data, depth=8)
        if not math.isfinite(ctw) or ctw < 0:
            raise AssertionError("CTW code length is invalid")
    print("entropy-reference: self-test passed")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("path", nargs="?", type=Path)
    parser.add_argument("--chunk-bytes", type=int, default=256 * 1024)
    parser.add_argument("--depth", type=int, default=8)
    parser.add_argument("--max-chunks", type=int, default=4)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test or args.path is None:
        self_test()
        if args.path is None:
            demo = (b"TRUEOS entropy walker research\x00" * 1024) + bytes(range(256)) * 8
            print_report(analyze_chunk(0, demo, args.depth))
            return

    if args.chunk_bytes <= 0 or args.max_chunks <= 0:
        raise SystemExit("chunk size and max chunk count must be positive")
    data = args.path.read_bytes()
    for index, start in enumerate(range(0, len(data), args.chunk_bytes)):
        if index >= args.max_chunks:
            break
        print_report(analyze_chunk(index, data[start : start + args.chunk_bytes], args.depth))


if __name__ == "__main__":
    main()
