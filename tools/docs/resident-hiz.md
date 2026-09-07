# Resident-scene HiZ: Xe-LP first integration

Implemented for single-sample D32 on device IDs `0xA780` and `0x4680`.
gfx12.5, MSAA and null-depth consumers retain HiZ-disabled state. No shaders,
GLB data, material behavior, camera settings or app ABI changed.

Depth owns a 2 MiB auxiliary tail after its maximum D32 allocation. Both
regions share the existing allocation, mapping transaction, carrier isolation,
quarantine and teardown. HiZ's live footprint is 200,704 bytes at 784×441 and
1,884,160 bytes at 2560×1440. The fixed reservation avoids reallocating on resize.

Each frame's clear secondary enables HiZ, binds the real auxiliary address,
sets valid clear depth 1.0, and runs a full-surface `3DSTATE_WM_HZ_OP` fast clear
with PS/PS_EXTRA/WM disabled. It commits with an immediate-write-only
PIPE_CONTROL to owner-local scratch, resets HZ_OP and drains depth before
normal rendering. Normal shader state is re-emitted after the operation.
The fullscreen color clear remains, but no longer raster-writes D32.
All later depth draws use the same auxiliary layout and clear value. The
driver never samples/exports this depth allocation; no external depth resolve
contract is implied. A HiZ depth-load without an initialization pass is rejected.

HiZ uses the ISL `HIZ` layout, not an invented CPU metadata pattern. The CPU
does not initialize compression bits. Layout and clear rectangles handle odd
extents (8×4 clear alignment inside the padded D32 surface). No CCS or
write-through mode is enabled.

## Host validation

```sh
cargo check --quiet
rustc --edition=2024 --test src/intel/render/hiz.rs -o /tmp/trueos-hiz-tests
/tmp/trueos-hiz-tests
python3 tools/test_hiz_isl.py
```

The last check links the pinned Mesa ISL libraries without opening DRM or
submitting GPU work. It compares pitch, QPitch and byte size at small, odd,
tile-boundary and maximum extents. This is not proof of hardware rendering.

## Picasso-Example helmet validation

Rebuild/boot the kernel and run the existing Picasso-Example helmet unchanged.
Look for `hiz=on` in the depth contract and `hiz=true` in retired Picasso
pipeline statistics. Render-area Info logging must be accepted to see these.
The first diagnostic frame also reports auxiliary address, pitch and byte size.

Check the helmet against the previous rendering while orbiting, moving close,
resizing to odd dimensions, switching apps and reopening the scene. In
particular, look for missing foreground parts, stale tiles or holes after
resize. Compare matching camera/extent/material runs before claiming a speedup;
the existing frame timing and PS counters are not direct HiZ hit counters.
Bare-metal correctness and performance remain unverified by this host change.

## Sources

- [Intel Tiger Lake PRM index](https://www.intel.com/content/www/us/en/docs/graphics-for-linux/developer-reference/1-0/tiger-lake.html)
- [Intel command structures, WM_HZ_OP](https://cdrdv2-public.intel.com/703050/intel-gfx-prm-osrc-tgl-vol-02-d-command-reference-structures.pdf)
- Pinned Mesa source under `.codex_tmp/trueos-adj-instrumented-rpls/mesa-src`:
  `src/intel/isl/isl.c`, `isl_format_layout.csv`, `isl_emit_depth_stencil.c`,
  `src/intel/genxml/gen120.xml`, `src/intel/blorp/blorp.c`,
  `blorp_genX_exec_brw.h`.
