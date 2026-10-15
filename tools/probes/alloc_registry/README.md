# C allocation registry saturation

Keeps 70,000 C allocations live, crossing the unchanged 65,536-record tracker
capacity. It verifies requested bytes across grow/shrink reallocations,
including allocations made after tracker saturation, frees alternate blocks,
refills them, reallocates again, then checks and releases every allocation.
The existing shared allocator tags must support untracked allocations safely.
Phase timings report numeric counts only. No latency threshold substitutes for
content/ownership checks.

Build on Linux with `cargo run --manifest-path tools/probes/alloc_registry/Cargo.toml`.
The sibling SDK's probe registry builds this standalone directory for TRUEOS.
Publish using the requested Apps share and launch `§§alloc_registry` on the
isolated test VM. Keep it registered as a probe; an Apps manifest entry is not
needed for runtime discovery of its published package.

The identical package `35ac4304` passes on both isolated release kernels. Total
time changes from 1,030 ms to 307 ms; reallocations at capacity change from
147 ms to 1 ms. The alternating-free phase takes 178 ms before and 249 ms after,
so this is not a claim of a uniform speedup. These single-run native timings
have millisecond resolution and do not measure a complete physical Codex turn.
See the [before](validation/native-before.json),
[after](validation/native-after.json) and
[publication readback](validation/published-probe.json) receipts.
