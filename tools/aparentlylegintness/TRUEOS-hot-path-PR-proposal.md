# TRUEOS hot-path review and narrow PR proposal

Repository snapshot: branch `true`, commit `ff78bdc7d9c456c13252bb362c67250611698498` (2026-08-29).

## Conclusion

The first experiment I would submit is:

> **time: remove 128-bit division from the clock hot path**

It is one file (`src/r/time.rs`), 22 insertions and 3 deletions. It precomputes a Q64 reciprocal after TSC calibration, uses a multiply-high estimate in `TimeDriver::now()`, and applies one multiplication-based correction. The correction makes the result exactly equal to the current integer division for every `u64` TSC delta under TRUEOS's current clock contract (`TICK_HZ = 1_000`, detected `tsc_hz >= 1_000_000`).

The ready-to-apply patch is `trueos-time-q64-experiment.patch`.

## What “deep and hot” should mean

Directory nesting and dependency depth are weak hotness proxies. Track three independent quantities:

1. **Dynamic cost**: sampled inclusive cycles or `calls × cycles/call`.
2. **Foundational reach**: number of transitive reverse dependencies and number of runtime subsystems that use the symbol.
3. **Static dependency depth**: longest package path from the kernel binary to the crate.

Sort primarily by dynamic cost. Use foundational reach as the tie-breaker. Use raw dependency depth only as context. A low-level leaf can be cold, while a root-module scheduler helper can execute on every CPU turn.

A useful triage-only score is:

```text
leverage = measured_total_cycles × log2(2 + transitive_reverse_dependents)
```

Keep the raw columns beside the score so a broad but cold helper is not mistaken for a hot one.

### Suggested report schema

| Rank | Symbol | Crate/module | Cycle share | Calls/s | Cycles/call | Dependency depth | Reverse dependents | Confidence |
|---:|---|---|---:|---:|---:|---:|---:|---|

### How to collect it in TRUEOS

- Generate package depth and reverse reach from `cargo metadata --format-version=1`.
- Retain an unstripped kernel ELF or linker map and map each symbol to its crate/module.
- Add a low-rate, allocation-free, per-CPU instruction-pointer sampler. Record RIP and a short RBP call chain into bounded per-CPU rings. The build already forces frame pointers.
- Use PMU counter overflow sampling when available. The current PMU code can read fixed counters but does not yet attribute samples to RIPs.
- Join symbol samples with the Cargo graph offline.
- For one suspect helper, sample only 1 in 1,024 or 4,096 calls. Do not place two timestamp reads around every invocation; that can cost more than the helper and distort the result.
- For micro-timing, use an ordered counter-read sequence. Plain `RDTSC` is not serializing.

The existing BSP task profiler remains useful for task-level inclusive cost, but it cannot identify the crate/function beneath a task and its current always-on hooks contribute their own cost.

## Static hot-path map

| Layer | Code | Static execution trigger | Reach |
|---|---|---|---|
| Kernel timing substrate | `src/r/time.rs` | At least twice per AP runtime turn (`poll`, then `ticks_until_next_wake`), plus timer scheduling and direct clock reads | System-wide |
| Executor substrate | `crates/trueos-executor/src/raw/mod.rs` and `src/executor_task_profile.rs` | Every queued task poll while the profile feature is enabled | System-wide async work |
| Per-CPU identity | `src/percpu.rs` | Profiler checks, allocator routing, runtime helpers, and other CPU-local operations | System-wide |
| Allocation substrate | `src/allocators.rs` | Every global allocation/deallocation | Broad, workload-sensitive |
| Model arithmetic leaves | `crates/trueos-kokoro/f32` and `gemm` | Inner loops during Kokoro inference | Deep but workload-specific |
| Text/pattern helper | `src/r/pat.rs` | String searches through the local pattern abstraction | Broad API, unknown dynamic share |

## Top seven experiments

This is a static ranking, not a measured profile. “Impact” means plausible leverage from the observed call path; it still needs on-hardware A/B data.

| Rank | Idea | Tiny work currently repeated | Narrowest experiment | Plausible impact | Risk |
|---:|---|---|---|---|---|
| 1 | **Make task profiling cheap to decline and sampled when active** | Every task poll enters begin/end hooks. Each hook checks the BSP through `current_slot`; that path validates GS with an `RDMSR`. BSP polls also take two `RDTSC` reads and update a probed table. The root enables the feature and the policy switch is currently `true`. | Cache `profile_enabled` in per-CPU/executor state and test it with a GS-relative load; sample 1/N polls before timestamping. Keep a dedicated full-profile build. | High and guaranteed wherever task polls dominate | Medium: measurement semantics |
| 2 | **Add an atomic not-due/empty fast path to the timer queue** | Every AP loop locks the same global queue in `poll`, then locks it again through `ticks_until_next_wake`. Due entries use `remove(0)`, shifting the remaining vector; the local wake vector has capacity 9,000. | Publish the earliest deadline in an atomic sentinel. If `now < earliest`, skip the mutex. Drain due wakers in bounded chunks and store the queue in an order that permits `pop()`. | High on SMP; removes cache-line bouncing in idle/busy loops | Medium-high: wakeup race proof required |
| 3 | **Cache allocation-routing identity once per call** | The ordinary host route can invoke `cpuid_slot()` three times, then later call `current_slot()`, whose validated path reads `IA32_GS_BASE`. This happens before the allocator mutex and first-fit scan. | Compute `cpuid_slot` once and reuse it for all three force-depth checks. In a second change, add a proven GS-only host fast path. | High when allocation rate is nontrivial | Medium: Hull/host realm contracts must remain explicit |
| 4 | **Replace clock-path 128-bit division with an exact reciprocal** | Every `now()` computes `delta × 1,000 / tsc_hz` as `u128`; common x86-64 code generation calls a wide-division helper. | The attached one-file Q64 reciprocal plus one-candidate correction. | Medium per call, very broad reach, easiest clean A/B | Low: result is mathematically exact |
| 5 | **Add per-CPU magazines for small host allocations** | The host allocator is one global spin mutex over a first-fit free list; allocation and deallocation scan/link/coalesce under that lock. | Add a few fixed small-size classes per CPU with bounded batch refill/flush; leave large and guest allocations unchanged. | High for allocation-heavy services | High: memory accounting, migration, and reclaim |
| 6 | **Split checked and trusted Kokoro arithmetic paths** | GEMM scans all LHS/RHS values before computing. Several AVX2 elementwise operations compute a complete validation pass and then recompute a complete store pass so output remains unchanged on error. | Preserve checked public APIs, but let a sealed graph executor call a trusted finite/non-aliasing lane after validation at the graph boundary. | High during inference; cuts full memory/arithmetic passes | Medium: proof boundary must be airtight |
| 7 | **Benchmark and simplify the short-needle SSE4.2 search path** | For 2–8 byte needles it copies prefixes/tails into temporary XMM blocks, runs `PCMPESTRI`, then verifies the candidate again. This may lose to the existing `memmem` implementation for many lengths/distributions. | Criterion-style host corpus: hit/miss, length, alignment, ASCII/binary. Keep the custom lane only where it wins; otherwise use `memchr(first)` plus scalar compare or `memmem`. | Low-to-medium; broad only if profiles show it | Low, but expected win is uncertain |

A small eighth cleanup is visible in `src/runtime.rs`: `poll_local_executor()` obtains `local_cpu()`, then `local_executor()` obtains it again. Passing the already-loaded `PerCpu` reference avoids one redundant GS load per executor pass, but it is probably too small to outrank the items above.

## Chosen PR: exact Q64 TSC conversion

### Current hot expression

```rust
((delta_tsc as u128) * (TICK_HZ as u128) / (tsc_hz as u128)) as u64
```

### Proposed conversion

At initialization:

```rust
scale = floor(TICK_HZ * 2^64 / tsc_hz)
```

At every clock read:

```rust
estimate = high64(delta_tsc * scale)
result = estimate + (((estimate + 1) * tsc_hz <= delta_tsc * TICK_HZ) as u64)
```

### Exactness argument

Let `m = floor(TICK_HZ × 2^64 / tsc_hz)` and `d` be a `u64` TSC delta. Then:

```text
m / 2^64 <= TICK_HZ / tsc_hz < m / 2^64 + 1 / 2^64
```

Multiplying by `d`, where `d < 2^64`, shows the reciprocal estimate is less than one real tick below the exact real quotient. Therefore the integer result is either `estimate` or `estimate + 1`. The multiplication comparison tests exactly which one it is; no division is needed in the hot path.

TRUEOS currently configures `TICK_HZ = 1,000`, and every TSC detection branch returns at least 1 MHz, so the required `tsc_hz > TICK_HZ` invariant holds.

### Local validation already performed

- Patch applies cleanly to blob `453b67c0c711c1256605b4bd1a7fb3bc404d1554` from the stated base commit.
- `git diff --check` passes.
- Deterministic Python model: 2,000,108 edge/random comparisons, all exactly equal to the current formula.
- Clang 17 x86-64 proxy:
  - current expression emitted a call to `__udivti3`;
  - approximate reciprocal emitted one `mul` and returned the high half;
  - exact reciprocal/correction emitted three `mul` operations plus compares, with no divide/helper call.
- On the available AMD EPYC 9V74 host, a synthetic no-inline C proxy ran about 1.5–1.8× faster per conversion across local runs. This is directional evidence only, not a TRUEOS result and not proof of Rust code generation.

### Required before merge

1. Run formatter and the normal TRUEOS build in the repository environment.
2. Disassemble the actual release kernel and verify `TimeDriver::now()` has no `__udivti3` path.
3. Boot old/new images on the same rig and compare conversion cycles using low-rate ordered sampling.
4. Verify timer wake traces, monotonic time, long uptime, and TSC wraparound behavior.
5. Compare whole-system fixed counters and task throughput. Merge only if the system-level result is positive; a faster helper can still be lost in an extra load or surrounding queue contention.

I could not run the TRUEOS Rust build in the analysis environment because no Rust toolchain or the repository's sibling path dependencies were available.

## Proposed PR text

### Title

```text
time: remove 128-bit division from the clock hot path
```

### Body

```markdown
## Summary

Precompute the TSC-to-Embassy-tick ratio as a Q64 reciprocal after TSC calibration and use it in `TimeDriver::now()`.

The hot path uses a multiply-high estimate and one multiplication-based correction. The correction keeps the result exactly equal to the existing integer expression:

`delta_tsc * TICK_HZ / tsc_hz`

## Why this path

Each AP runtime turn calls the clock through `time::poll()` and again through `ticks_until_next_wake()`. Timer scheduling and direct Embassy clock reads add more calls. The current conversion performs variable-width `u128` division on every read.

## What changes

- store one Q64 TSC-to-tick reciprocal after clock initialization
- replace hot-path wide division with multiply-high
- test the sole possible one-tick underestimate using multiplication
- preserve TSC detection, epoch selection, timer queue behavior, and exact tick values

## Exactness

For `m = floor(TICK_HZ * 2^64 / tsc_hz)` and any `u64` delta, the reciprocal estimate is at most one tick below the exact integer quotient. Testing whether `(estimate + 1) * tsc_hz <= delta * TICK_HZ` selects the exact current result.

The current build uses `TICK_HZ = 1_000`; TSC detection accepts or falls back to frequencies of at least 1 MHz, satisfying `tsc_hz > TICK_HZ`.

## Scope

- one file: `src/r/time.rs`
- no timer queue changes
- no inline assembly or new ISA requirement
- no intended behavior change

## Validation

- patch applies cleanly to `true` at `ff78bdc7d9c456c13252bb362c67250611698498`
- deterministic model matched the old expression for 2,000,108 edge/random cases
- local Clang x86-64 proxy removed the `__udivti3` call and was directionally faster

Still required on the TRUEOS rig:

- normal release build and formatting
- release-kernel disassembly
- ordered low-rate cycle A/B
- timer/monotonicity soak test
```
