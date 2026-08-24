#!/usr/bin/env python3
"""Bit-exact host semantics for the TRUEOS parallel-u32 GPU incubator."""

from __future__ import annotations

from dataclasses import dataclass
from typing import Iterable, Sequence

U32_MASK = 0xFFFF_FFFF
SUBGROUP_WIDTH = 16
TILE_ROWS = 16
TILE_ITEMS = SUBGROUP_WIDTH * TILE_ROWS
RADIX_BITS = 4
RADIX_BINS = 1 << RADIX_BITS
RADIX_MASK = RADIX_BINS - 1


def u32(value: int) -> int:
    return value & U32_MASK


def normalized_u32(values: Iterable[int]) -> list[int]:
    return [u32(value) for value in values]


@dataclass(frozen=True)
class ScanLevel:
    input_count: int
    tile_count: int


@dataclass(frozen=True)
class ScanPlan:
    count: int
    levels: tuple[ScanLevel, ...]
    temporary_words: int


def scan_plan(count: int) -> ScanPlan:
    if count < 0 or count > U32_MASK:
        raise ValueError("count must fit u32")
    levels: list[ScanLevel] = []
    current = count
    temporary_words = 0
    while current > 1:
        tiles = (current + TILE_ITEMS - 1) // TILE_ITEMS
        levels.append(ScanLevel(input_count=current, tile_count=tiles))
        # Each nonterminal level needs local tile sums and their scanned offsets.
        temporary_words += tiles * 2
        current = tiles
    return ScanPlan(count=count, levels=tuple(levels), temporary_words=temporary_words)


def exclusive_scan_u32(values: Sequence[int]) -> list[int]:
    output: list[int] = []
    carry = 0
    for value in values:
        output.append(carry)
        carry = u32(carry + value)
    return output


def tiled_exclusive_scan_u32(values: Sequence[int]) -> list[int]:
    source = normalized_u32(values)
    if not source:
        return []

    output = [0] * len(source)
    tile_sums: list[int] = []
    for tile_base in range(0, len(source), TILE_ITEMS):
        carry = 0
        tile_end = min(tile_base + TILE_ITEMS, len(source))
        for index in range(tile_base, tile_end):
            output[index] = carry
            carry = u32(carry + source[index])
        tile_sums.append(carry)

    if len(tile_sums) == 1:
        tile_offsets = [0]
    else:
        tile_offsets = tiled_exclusive_scan_u32(tile_sums)
    for tile, offset in enumerate(tile_offsets):
        start = tile * TILE_ITEMS
        end = min(start + TILE_ITEMS, len(output))
        for index in range(start, end):
            output[index] = u32(output[index] + offset)
    return output


def normalize_flags_u32(flags: Sequence[int]) -> list[int]:
    return [1 if flag else 0 for flag in flags]


def reduce_sum_u32(values: Sequence[int]) -> int:
    result = 0
    for value in values:
        result = u32(result + value)
    return result


def tiled_reduce_sum_u32(values: Sequence[int]) -> int:
    source = normalized_u32(values)
    if not source:
        return 0
    while len(source) > 1:
        source = [
            reduce_sum_u32(source[start : start + TILE_ITEMS])
            for start in range(0, len(source), TILE_ITEMS)
        ]
    return source[0]


def select_indices_u32(flags: Sequence[int]) -> list[int]:
    normalized = normalize_flags_u32(flags)
    positions = tiled_exclusive_scan_u32(normalized)
    selected = [0] * (positions[-1] + (1 if flags[-1] else 0) if flags else 0)
    for index, flag in enumerate(flags):
        if flag:
            selected[positions[index]] = index
    return selected


def collective_probe_report(values: Sequence[int]) -> list[int]:
    if len(values) != SUBGROUP_WIDTH:
        raise ValueError("collective probe requires exactly 16 inputs")
    source = normalized_u32(values)
    exclusive = exclusive_scan_u32(source)
    inclusive = [u32(prefix + value) for prefix, value in zip(exclusive, source)]
    reduction = reduce_sum_u32(source)
    minimum = min(source)
    maximum = max(source)
    return (
        source
        + exclusive
        + inclusive
        + [reduction] * SUBGROUP_WIDTH
        + [minimum] * SUBGROUP_WIDTH
        + [maximum] * SUBGROUP_WIDTH
        + [source[0]] * SUBGROUP_WIDTH
        + list(reversed(source))
        + [((SUBGROUP_WIDTH << 16) | lane) for lane in range(SUBGROUP_WIDTH)]
    )


def histogram_16_u32(values: Sequence[int], shift: int) -> list[int]:
    if shift < 0 or shift > 28 or shift % RADIX_BITS:
        raise ValueError("shift must be one of 0,4,...,28")
    result = [0] * RADIX_BINS
    for value in values:
        result[(u32(value) >> shift) & RADIX_MASK] += 1
    return result


def radix_pass_4bit_u32(
    keys: Sequence[int],
    values: Sequence[int] | None,
    shift: int,
) -> tuple[list[int], list[int] | None]:
    source_keys = normalized_u32(keys)
    source_values = normalized_u32(values) if values is not None else None
    if source_values is not None and len(source_values) != len(source_keys):
        raise ValueError("key/value lengths differ")

    tile_count = (len(source_keys) + TILE_ITEMS - 1) // TILE_ITEMS
    histograms: list[list[int]] = []
    for tile in range(tile_count):
        start = tile * TILE_ITEMS
        histograms.append(histogram_16_u32(source_keys[start : start + TILE_ITEMS], shift))

    tile_prefixes = [[0] * RADIX_BINS for _ in range(tile_count)]
    totals = [0] * RADIX_BINS
    for bin_index in range(RADIX_BINS):
        carry = 0
        for tile in range(tile_count):
            tile_prefixes[tile][bin_index] = carry
            carry += histograms[tile][bin_index]
        totals[bin_index] = carry

    bin_bases = exclusive_scan_u32(totals)
    output_keys = [0] * len(source_keys)
    output_values = [0] * len(source_keys) if source_values is not None else None
    for tile in range(tile_count):
        start = tile * TILE_ITEMS
        end = min(start + TILE_ITEMS, len(source_keys))
        local_counts = [0] * RADIX_BINS
        for index in range(start, end):
            digit = (source_keys[index] >> shift) & RADIX_MASK
            destination = bin_bases[digit] + tile_prefixes[tile][digit] + local_counts[digit]
            output_keys[destination] = source_keys[index]
            if output_values is not None and source_values is not None:
                output_values[destination] = source_values[index]
            local_counts[digit] += 1
    return output_keys, output_values


def radix_sort_u32(
    keys: Sequence[int],
    values: Sequence[int] | None = None,
) -> tuple[list[int], list[int] | None]:
    current_keys = normalized_u32(keys)
    current_values = normalized_u32(values) if values is not None else None
    for shift in range(0, 32, RADIX_BITS):
        current_keys, current_values = radix_pass_4bit_u32(
            current_keys, current_values, shift
        )
    return current_keys, current_values


def rle_u32(keys: Sequence[int]) -> tuple[list[int], list[int], list[int]]:
    source = normalized_u32(keys)
    if not source:
        return [], [], []
    run_keys = [source[0]]
    run_starts = [0]
    for index in range(1, len(source)):
        if source[index] != source[index - 1]:
            run_keys.append(source[index])
            run_starts.append(index)
    run_lengths = [
        (run_starts[index + 1] if index + 1 < len(run_starts) else len(source))
        - start
        for index, start in enumerate(run_starts)
    ]
    return run_keys, run_starts, run_lengths


def _normalize_heads(head_flags: Sequence[int], count: int) -> list[int]:
    if len(head_flags) != count:
        raise ValueError("value/head lengths differ")
    if any(flag not in (0, 1) for flag in head_flags):
        raise ValueError("segmented head flags must be exactly 0 or 1")
    heads = list(head_flags)
    if count and heads[0] != 1:
        raise ValueError("non-empty segmented input must start with a head")
    return heads


def segmented_exclusive_scan_u32(
    values: Sequence[int], head_flags: Sequence[int]
) -> list[int]:
    source = normalized_u32(values)
    heads = _normalize_heads(head_flags, len(source))
    output: list[int] = []
    carry = 0
    for value, head in zip(source, heads):
        if head:
            carry = 0
        output.append(carry)
        carry = u32(carry + value)
    return output


def tiled_segmented_exclusive_scan_u32(
    values: Sequence[int], head_flags: Sequence[int]
) -> list[int]:
    source = normalized_u32(values)
    heads = _normalize_heads(head_flags, len(source))
    output = [0] * len(source)
    metadata: list[tuple[bool, int, int, int]] = []

    for tile_base in range(0, len(source), TILE_ITEMS):
        tile_values = source[tile_base : tile_base + TILE_ITEMS]
        tile_heads = heads[tile_base : tile_base + TILE_ITEMS]
        local = _segmented_local_zero_carry(tile_values, tile_heads)
        output[tile_base : tile_base + len(local)] = local
        first_head = next((i for i, head in enumerate(tile_heads) if head), len(tile_heads))
        has_head = first_head != len(tile_heads)
        tail = _segmented_tail_zero_carry(tile_values, tile_heads)
        metadata.append((has_head, tail, first_head, len(tile_values)))

    tile_carries: list[int] = []
    carry = 0
    for has_head, tail, _, _ in metadata:
        tile_carries.append(carry)
        carry = tail if has_head else u32(carry + tail)

    for tile, (_, _, first_head, valid_count) in enumerate(metadata):
        base = tile * TILE_ITEMS
        for offset in range(first_head):
            if offset < valid_count:
                output[base + offset] = u32(output[base + offset] + tile_carries[tile])
    return output


def _segmented_local_zero_carry(values: Sequence[int], heads: Sequence[int]) -> list[int]:
    output: list[int] = []
    carry = 0
    for value, head in zip(values, heads):
        if head:
            carry = 0
        output.append(carry)
        carry = u32(carry + value)
    return output


def _segmented_tail_zero_carry(values: Sequence[int], heads: Sequence[int]) -> int:
    carry = 0
    for value, head in zip(values, heads):
        if head:
            carry = 0
        carry = u32(carry + value)
    return carry


def segmented_reduce_sum_u32(
    values: Sequence[int], head_flags: Sequence[int]
) -> list[int]:
    source = normalized_u32(values)
    heads = _normalize_heads(head_flags, len(source))
    totals: list[int] = []
    carry = 0
    started = False
    for value, head in zip(source, heads):
        if head:
            if started:
                totals.append(carry)
            carry = 0
            started = True
        carry = u32(carry + value)
    if started:
        totals.append(carry)
    return totals
