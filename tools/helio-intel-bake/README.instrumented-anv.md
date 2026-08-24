# Instrumented ANV HelioC capture

This capture mode copies the pinned Mesa source into a caller-owned temporary
directory, applies `mesa-helioc-capture-6fb2611.patch`, and builds the Intel
Vulkan ICD plus Mesa's no-op DRM shim. It never changes the reference tree.

The patch makes three otherwise-private values observable only when
`TRUEOS_HELIOC_ANV_DUMP_DIR` is set:

- compiler `bind_map` and raw `brw_prog_data` via executable internal representations;
- ANV's complete, commit-pinned shader serialization (code, pointer-zeroed
  program data, relocations, and full bind map);
- raw descriptor image `SURFACE_STATE` and `SAMPLER_STATE` writes;
- resolved binding-table entries, emitted sampler state, and command-buffer bytes.
- address-free V5 indirect-descriptor templates: the six shader-loaded
  payloads with surface/sampler/image fields zeroed and typed source,
  addend/contiguous-mask relocation descriptions;
- address-free V6 image-surface templates: sampled/storage states for both
  cloud volumes and the runtime-addressed, runtime-sized UI4 render target.
  The capture verifies the packed gfx120 base-address field against ANV's
  source address before clearing it;
- address-free V7 buffer/descriptor-set `SURFACE_STATE` templates: sim params
  for both compute ping-pong sets, render params for both graphics sets, and
  all four descriptor-set-buffer surfaces. Each record names its exact
  set/resource role, descriptor-relative layout and range, and verifies then
  clears the packed gfx120 base address with a typed relocation. Tables,
  descriptor payload contents, sampler/program state, SBA, IDD, and command
  packet relocations are still required before HELIOCRS v2 may be assembled.

Each binary record is an individual file (not an append stream) and uses a little-endian seven-u32 header:
`magic=0x48434d56` (`VMCH`), version, kind, stage-or-descriptor-type,
binding, element, byte length. Kinds are 1 surface, 2 sampler, 3 binding
table, 4 emitted sampler state, and 5 command bytes. Raw state contains
driver virtual addresses: it is evidence only, never a relocatable package.

Run from TRUEOS after the required build tools/dependencies are available:

```sh
python3 tools/helio-intel-bake/instrumented_anv_capture.py \
  --work-dir /tmp/helioc-instrumented
```

The helper requires Mesa `6fb261147bbb4cc488ea9f16fb3b6fe02105332e`, makes a
temporary source copy, applies the patch with `git apply --check`, configures a
minimal Intel Vulkan build, points `VK_DRIVER_FILES` at that build's ICD JSON,
and runs the normal `bake.py --helioc` dumper. It invokes the shim with
`INTEL_STUB_GPU_DEVICE_ID=4680`, `TRUEOS_HELIOC_STUB_PCI_REVISION=0x0c`, and
`TRUEOS_HELIOC_STUB_KMD_REVISION=0`. PCI and KMD revisions are intentionally
separate Mesa inputs; resulting metadata is shim evidence, not physical hardware.

The source-level target is ADL GT1: Mesa must report `ver=12` and `verx10=120`.
`gfx125` is DG2/Xe-HP, not ADL-S UHD 770. The bakery rejects source-level trace
metadata that reports a different target. Even a successful shim run does not
prove actual 0x4680/r0c ownership, relocation normalization, allocation layout,
or hardware execution; `bake.py` remains fail-closed and does not write HELIOA.
