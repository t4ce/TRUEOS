# Helio Intel bake

This is the shortest existing build-time compiler lane from a genuine Helio
capture to native Intel graphics shader bytes:

```text
HELIOA captured WGSL
  -> Helio's vendored wgpu/Naga (per-entry SPIR-V)
  -> Mesa ANV on the selected Intel GPU
  -> VK_KHR_pipeline_executable_properties + ANV pipeline cache
  -> native VS/PS ISA sections in a new HELIOA file
```

This archived conversion utility is not part of the TRUEOS build or release
graph. It requires an explicit Helio capture and is retained only for
historical artifact inspection:

```sh
cd /home/t4ce/REPOS/TRUEOS
python3 tools/helio-intel-bake/bake.py \
  /path/to/historical-capture.helio
```

The output derives its name from the supplied capture and adds:

- `intel-xe-lp/vs.simd8.bin`
- `intel-xe-lp/ps.simd8.bin`
- `intel-xe-lp/ps.simd16.bin` when ANV emits it
- `compiler/intel-xe-lp.json`
- `scene/retained-transform-template-v1.bin`

The tool validates the input/output HELIOA tables and CRCs, requires the
expected captured SimpleCube entry points and layouts, rejects empty or
unaligned ISA, and verifies every packaged SHA-256.

Historical captures used a pointer-free retained-transform template: two
authored constant identity operations folded at build time into one row-major
3x4 root, followed at runtime by a dynamic child per render row. This format
is not a maintained TRUEOS renderer contract.

This reaches genuine target-specific Intel ISA, but it is not yet directly launchable
by TRUEOS. The EU assembly makes the resource ABI concrete: VS reads the
camera matrix at byte 128 through BTI 1 and PS writes RT0 through BTI 0. The
remaining runtime step is matching Mesa's vertex-fetch, URB, SBE and pixel
payload state before programming `3DSTATE_VS` and `3DSTATE_PS`.
