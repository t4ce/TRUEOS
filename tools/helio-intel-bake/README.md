# Helio Intel bake

This is the shortest existing build-time compiler lane from a genuine Helio
capture to native Intel graphics shader bytes:

```text
HELIOA captured WGSL
  -> Helio's vendored wgpu/Naga (per-entry SPIR-V)
  -> Mesa ANV on an Intel gfx125 GPU
  -> VK_KHR_pipeline_executable_properties + ANV pipeline cache
  -> native VS/PS ISA sections in a new HELIOA file
```

Run it after `bake_simple_cube`:

```sh
cd /home/t4ce/REPOS/TRUEOS
python3 tools/helio-intel-bake/bake.py \
  /home/t4ce/REPOS/Helio/target/helio-artifacts/simple-cube.trueos.helio
```

The output defaults to `simple-cube.trueos.intel.helio` and adds:

- `intel-xe-lp/vs.simd8.bin`
- `intel-xe-lp/ps.simd8.bin`
- `intel-xe-lp/ps.simd16.bin` when ANV emits it
- `compiler/intel-xe-lp.json`
- `scene/retained-transform-template-v1.bin`

The tool validates the input/output HELIOA tables and CRCs, requires the
expected captured SimpleCube entry points and layouts, rejects empty or
unaligned ISA, and verifies every packaged SHA-256.

Both the SimpleCube and Churn-only bake lanes emit the same canonical
`SectionKind::Other` retained-transform template. It contains no pointers or
GPU addresses: two authored constant identity operations are multiplied at
build time into one row-major 3x4 identity root, followed at runtime by one
dynamic child per render row (up to 4096 rows and a traversal depth of two).
The 128-byte little-endian payload is an 80-byte header followed by the one
48-byte affine; the exact header offsets are documented in
`tools/helio-build/README.md`.

This reaches genuine native gfx125 ISA, but it is not yet directly launchable
by TRUEOS. The EU assembly makes the resource ABI concrete: VS reads the
camera matrix at byte 128 through BTI 1 and PS writes RT0 through BTI 0. The
remaining runtime step is matching Mesa's vertex-fetch, URB, SBE and pixel
payload state before programming `3DSTATE_VS` and `3DSTATE_PS`.
