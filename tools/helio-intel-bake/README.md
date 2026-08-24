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

This reaches genuine target-specific Intel ISA, but it is not yet directly launchable
by TRUEOS. The EU assembly makes the resource ABI concrete: VS reads the
camera matrix at byte 128 through BTI 1 and PS writes RT0 through BTI 0. The
remaining runtime step is matching Mesa's vertex-fetch, URB, SBE and pixel
payload state before programming `3DSTATE_VS` and `3DSTATE_PS`.

## HelioC volume/raymarch preflight

`--helioc` is a separate, sealed lane for the exact authored cloud sources in
`Helio-Examples/cloud-engine-webgpu-linux-aligned/shaders/`. It admits neither
the adjacent C++/OpenCL experiments nor copied/reformatted WGSL. It runs the
pinned Helio Naga frontend for `simulate.wgsl:main`, `render.wgsl:vs_main`, and
`render.wgsl:fs_main`, and fixes the eventual HELIOC descriptor to gfx120,
ADL-S UHD 770 revision 0C, a `4x4x4` local group, `24x12x24` groups, and a
metadata-selected compute SIMD16 or SIMD32.

```sh
python3 tools/helio-intel-bake/bake.py --helioc \
  --work-dir /tmp/helioc-bake-work \
  --out /tmp/helioc-native.adl-s.gfx120.helio
```

The HelioC dumper now creates the real compute pipeline and fullscreen graphics
pipeline, two `96x48x96` RGBA16F 3D images, a repeat/clamp/repeat linear sampler,
both compute ping-pong descriptor sets, and the graphics descriptor set. It
records two `24x12x24` compute dispatches plus the fullscreen draw, then captures
native compute/VS/FS ISA through the pipeline executable/cache cross-check. It
writes valid `VkMemoryRequirements` for the actual optimal-tiled images and, if
the driver supports sampled+storage linear 3D images, separately creates a
linear probe and queries its valid `VkSubresourceLayout`. It never queries a
subresource layout for an optimal-tiled image.

This still emits no HELIOA file. On the current gfx120 RPL-S capture, the actual
optimal images require 6,316,444 bytes with 65,536-byte alignment, rather than
the guest contract's 3,538,944-byte backing. The valid linear probe does match
the guest layout (3,538,944 bytes, 768-byte row, 36,864-byte depth/array pitch),
but it is not treated as proof of the optimal image's tiling or state encoding.
The public executable API additionally does not expose ANV's descriptor-to-BTI/
sampler map or compute program data, and the pipeline cache is opaque. The
preflight reports these facts, the exact device ID, and the absence of a public
PCI revision before refusing packaging. The assembler therefore remains
reachable only after a reviewed capture supplies a relocatable surface/sampler/
bind-map contract compatible with the broker allocation.

When the matched instrumented ANV patches are enabled, `bake.py` also requires
the complete observed gfx120 trace: two compute binding-table records and two
compute sampler-map records (the ping-pong descriptor flushes), one fragment
binding-table record, one fragment sampler-map record, and one completed command
record. The vertex stage is intentionally absent because its bind map has zero
surfaces and samplers. Gfx120 uses indirect descriptor forms, so the older
ANV_DESCRIPTOR_SURFACE/SAMPLER hooks may produce no kind-1/2 records; those
records are not treated as required evidence. Record kinds, Mesa shader stages,
bindings, and counts are checked exactly; a truncated, duplicated, mislabeled,
or partial trace is rejected. These records remain capture evidence only and
never become runtime state or a fallback package.

The next admission format is the binary, versioned
`compiler/helioc-relocatable-state-v2.bin` (`HELIOCRS` v2) section paired with
the 384-byte HELIOC v3 descriptor. It must authenticate gfx120/ADL-S r0c and
contain fixed-window objects plus typed, bounded relocations—never ANV process
addresses or a JSON surrogate. A relocation writes a field as
`((resolved + addend) >> right_shift) << trailing_zeros(mask)`, verifies the
unshifted value fits a contiguous mask, then performs a masked RMW. This is
required for bit-positioned width/height/pitch fields as well as addresses.
The current bakery does not emit this section or a HELIOA. It now accepts and
cross-checks three genuinely address-free source-instrumented slices: the V5
indirect descriptors, V6 sampled/storage/UI4 image-surface templates, and V7
sim/render-parameter plus descriptor-set-buffer `SURFACE_STATE` templates.
V7 requires exactly the two compute and two graphics roles, binds their
descriptor layouts/ranges and typed address relocations, and rejects any
nonzero packed base address. The capture metadata records this as
`partial-v7`; command packets, binding and sampler tables, descriptor payload
contents, and program state still lack a complete typed relocation map, so the
package gate remains fail-closed.
