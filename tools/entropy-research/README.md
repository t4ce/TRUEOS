# TRUEOS entropy research tools

This directory is the slow oracle lane for `docs/INTEL_ENTROPY_WALKERS.md`.
Nothing here is part of the boot/runtime compression path.

Run the executable invariants and a synthetic report:

```sh
python3 tools/entropy-research/reference.py --self-test
```

Inspect real data in independent 256 KiB chunks:

```sh
python3 tools/entropy-research/reference.py path/to/input \
  --chunk-bytes $((256 * 1024)) \
  --depth 8 \
  --max-chunks 16
```

The report separates model-aware bounds from actual payload sizes. In
particular, the rANS32 row says when the frequency model is known; it does not
silently pretend the decoder got that table for free.

## WGSL probe

Validate and lower the research probe with the existing Naga tool:

```sh
cargo run --manifest-path tools/wgsl-spv/Cargo.toml -- \
  entropy_probe \
  crates/trueos-shader/gpgpu/research/entropy_probe.wgsl \
  /tmp/trueos-entropy-probe.spv
```

That SPIR-V is for inspection/differential experiments. It is not an admitted
TRUEOS direct-RCS artifact.

## Native Intel promotion

When a kernel is worth running on the physical GT, port/freeze its ABI and use
the repository's pinned Intel artifact bakery. The production source plus
`.bin`, `.spv`, manifest, and generated Rust contract must land together. The
runtime should then reuse caller PPGTT and the existing GuC/direct-RCS walker
machinery rather than grow a generic OpenCL runtime.
