# Intel May-Help Reference

This folder is a reference archive, not build input.  It keeps older Intel,
i915, Xe/iGPU, and display/render notes close to the current driver so useful
facts do not get lost again.

The HDA driver worked because each step became a small contract: claim the
device, map registers, discover widgets, select a path, program a stream, then
prove playback.  The Intel GPU work needs the same discipline.  A failed
triangle must not erase smaller wins like a valid PCI claim, a working MMIO
read, a submitted batch marker, a verified vertex buffer, or a render counter
moving.

## Main Lesson To Apply

Every Intel function should say:

- Which hardware block it touches.
- Which earlier proof must already be true.
- Which values are meaningful and where they came from.
- Which observable result proves this step.
- Which tempting conclusion it does not prove.

Example proof shape:

```text
intel/render: vertex-upload-proof ok vertices=3 stride=32 gpu=... readback=1
```

That line proves CPU-visible upload and layout only.  It does not prove VF
fetch, VS execution, rasterization, pixel shader dispatch, or display output.

## Proof Boundaries

Use these as the top-level Intel map:

- `pci-claim`: device ID, revision, BARs, MMIO length, bus ownership.
- `mmio-proof`: mapped register window, stable readback, forcewake if needed.
- `memory-proof`: CPU writes GPU-owned memory, CPU reads it back, flush path is
  explicit.
- `batch-submit-proof`: command buffer accepted, start marker executes, end
  marker retires.
- `vertex-upload-proof`: vertex count, stride, format, GPU address, CPU
  readback.
- `vf-proof`: vertex fetch consumes the expected number of vertices.
- `vs-proof`: vertex shader is uploaded, matched by signature/hash, counters
  show activity.
- `clip-proof`: clipper receives primitives; viewport/clip mode/scissor state
  is decoded.
- `raster-proof`: raster/SF state is valid; raster counters or downstream
  markers advance.
- `ps-proof`: pixel shader and backend state are armed; dispatch reason is
  named.
- `rt-proof`: render target state maps to the memory being sampled before/after.
- `display-proof`: plane/cursor/framebuffer points at the expected GPU memory
  and live registers agree.

## Files

- `journal-i915.txt`
  - Full verbose Linux DRM/i915 startup log from host `PCJB`.
  - Device line shows `0000:00:02.0 [8086:a780]`.
  - Linux identifies it as `alderlake_s/raptorlake_s`, display version `12.00`,
    stepping `D0`.
  - Boot args include `i915.enable_guc=0`, `i915.force_probe=none`,
    `drm.debug=0x1e`, and `intel_iommu=on iommu=pt`.
  - Useful for Linux ordering, connector discovery, display init, GuC choice,
    force-probe behavior, BAR layout, and names of i915 stages.
  - Not proof that TrueOS touched the same registers correctly.

- `pci.txt`
  - Small PCI snapshot from the old `intelpain` folder.
  - Use for comparing IDs, BARs, and topology against current TrueOS logs.

- `connectors.txt`
  - Old connector notes.
  - Use when checking display pipe/plane/connector naming and handoff.

- `drm-tree.txt`
  - Old DRM object tree snapshot.
  - Use to map Linux concepts like connector, encoder, CRTC, plane, and FB to
    TrueOS display ownership.

- `i915-adls-tc3-power-map.md`
  - Power/register map notes for Alder Lake-S / related i915 behavior.
  - Use when a register seems power-gated or needs forcewake.

- `gfx_intel.rs`
  - Older gfx Intel implementation.
  - Use for high-level ownership ideas: how the generic gfx layer expected an
    Intel backend to look.

- `gfx_backend_intel.rs`
  - Older gfx backend Intel file.
  - Use for UI/gfx API subset mapping and backend responsibilities.

- `gfx_intel_disp.rs`
  - Older display-side Intel gfx code.
  - Use for framebuffer, plane, and scanout decisions.

- `intelpain_intel.rs`
  - Old `intelpain` Intel source.
  - Use as archaeology for early assumptions and dead ends.

- `intel_old.rs`
  - Old `src/intel/intel.rs`.
  - Use for earlier module boundaries and boot-time init layout.

- `intel_igpu770.rs`
  - Older iGPU 770-specific code.
  - Use for PCI/MMIO/register bring-up, display assumptions, and hardware
    constants.

- `intel_igpu770_rcs.rs`
  - Older render command streamer / RCS attempt.
  - Use when comparing batch submission, rings, execlists, and render markers.

- `intel_guc.rs`
  - Older GuC handling.
  - Use for firmware placement, WOPCM/GGTT assumptions, and bootstrap status
    names.

- `intel_770_registers.rs`
  - Old register constants.
  - Use only after checking names/offsets against the current hardware path and
    current logs.

## Questions To Answer While Reading

- Which old values are observations from Linux, and which are guesses?
- Which values apply to current device `0x4680`, and which only apply to the
  older host log device `0xa780`?
- Which old steps have a matching current TrueOS proof log?
- Which old steps wrote registers without a readback or visible result?
- Which old constants can become named structs/enums instead of magic numbers?
- Which pieces belong in display, render, GuC, memory, or generic gfx backend?
- Which proof boundary is missing between `batch submitted` and `pixel changed`?

## Current TrueOS Anchors

Known current useful log facts to preserve as proof anchors:

- Intel PCI claim works for `00:02.0`, device `0x4680`, revision `0x0C`.
- MMIO length has been observed as `0x1000000`.
- GuC firmware is found and bootstrap can report ready/auth status.
- Display primary boot surface can be identified with pipe, pitch, GPU address,
  physical address, live surface register, and logo result.
- Streamout/VF experiments have produced counters and byte counts even when a
  full triangle did not appear.
- Mesa-compare logs already give a useful reference vocabulary for SBE, clip,
  raster, PS, and backend state.

## Rule For Future Intel Code

Do not create another single giant probe path where success means "triangle on
screen" and failure means "nothing learned."  Keep every durable observation in
a named proof step with one small log line and one small data structure.
