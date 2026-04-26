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

Use this doc shape above every bring-up/proof function:

```text
Contract:
  Block: <PCI | MMIO/forcewake | GGTT/memory | RCS/execlist | VF | VS | clip/raster | PS/RT | display>
  Requires: <the named proof(s) that must already be true>
  Inputs: <GPU address, pitch, stride, format, count, register offsets, shader hash, etc.>
  Success: <one observable log/register/memory/counter change>
  Does not prove: <the next pipeline boundary>
```

Example proof shape:

```text
intel/render: vertex-upload-proof ok vertices=3 stride=32 gpu=... readback=1
```

That line proves CPU-visible upload and layout only.  It does not prove VF
fetch, VS execution, rasterization, pixel shader dispatch, or display output.

## HDA Trace Shape To Imitate

The HDA boot log is admirable because it is a readable proof transcript:

```text
[HDA] PCI 00:1F.3 8086:7A50
[HDA] BAR0 phys = ...
[HDA] MMIO mapped at virt ...
[HDA] STATESTS = ... (codec presence)
[HDA] Codec 0 present
[HDA] CORB: 256 entries
[HDA] RIRB: 256 entries
[HDA] Vendor=10EC, Device=0897
[HDA] Path found: HP Out -> ["20", "12", "2"]
[HDA] DAC NID 2 -> stream_tag=1, fmt=0x0011
[HDA] NID 20 conn_sel=0 (-> NID 12)
[HDA] Stream configured: 48kHz 16-bit stereo
[HDA] Initialization complete!
```

Each line is small and named:

- `PCI ...` proves the controller was found. It does not prove BAR access.
- `BAR0 phys ...` proves the address was discovered. It does not prove the map
  works.
- `MMIO mapped ...` proves a CPU mapping exists. It does not prove controller
  reset.
- `STATESTS ...` proves codec presence bits. It does not prove verb transport.
- `CORB/RIRB ...` proves command/response rings are allocated and sized. It
  does not prove any codec widget route.
- `Vendor=...` and widget walk prove codec discovery. They do not prove
  playback.
- `Path found ...` proves a route through codec nodes. It does not prove the
  selected connection registers accepted it.
- `stream_tag/fmt` and `conn_sel` prove stream/path programming. They do not
  prove DMA is moving.
- `Stream configured ...` proves the intended format and DMA buffer state. It
  still should be followed by a playback-position or audible-sample proof when
  debugging audio output.

Intel GPU logs should read the same way.  Avoid:

```text
intel/render: triangle failed
```

Prefer:

```text
intel/gpu: pci-claim ok bdf=00:02.0 id=8086:4680 rev=0x0C bar0=... mmio_len=0x1000000
intel/gpu: mmio-proof ok forcewake=render ack=1 reg=... read0=... read1=...
intel/gpu: memory-proof ok name=vertex gpu=0x00870000 bytes=36 flush=clflush readback=1
intel/render: batch-submit-proof ok engine=rcs start=0xC0DE7721 end=0xC0DE7701 retired=1
intel/render: vertex-upload-proof ok vertices=3 stride=12 layout=R32G32B32_FLOAT gpu=0x00870000
intel/render: vf-proof ok ia_vtx_delta=3 ia_prim_delta=1 post_vf=0xC0DE7723
intel/render: vs-proof ok vs_delta=3 shader_hash=... post_vs=0xC0DE7724
intel/render: clip-raster-proof ok cl_delta=1 raster_marker=0xC0DE7727
intel/render: ps-rt-proof ok ps_delta=1 rt_gpu=... pixel_before=... pixel_after=...
intel/display: display-proof ok pipe=pipe-a plane=0 surf_live=... frame_delta=1 sample=...
```

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

## Bring-Up Proof Ladder

Treat "render works" as the last sentence in a report, not the task.  Each
function in the Intel GPU path should move exactly one boundary when possible.

### `pci-claim`

Contract:

- Block: PCI config space and OS bus ownership.
- Requires: PCI enumeration is available and the device has not been claimed by
  another TrueOS driver.
- Inputs: bus/device/function, vendor ID, device ID, revision, BAR registers,
  command register.
- Success: log includes the expected Intel GPU ID, enabled memory access, BAR
  base/length, and no conflicting owner.
- Does not prove: MMIO registers are mapped, forcewake works, the display is
  live, or any GPU engine can execute commands.

### `mmio-proof`

Contract:

- Block: GPU MMIO aperture and forcewake/uncore register access.
- Requires: `pci-claim`.
- Inputs: mapped BAR virtual address, MMIO length, known read-only/stable
  register offsets, forcewake request and ack offsets.
- Success: stable register reads are sane, forcewake request observes its ack,
  and posted writes are read back through the intended register path.
- Does not prove: memory ownership, GGTT mappings, command submission, or that a
  3D pipeline register write has any effect.

### `memory-proof`

Contract:

- Block: CPU-owned pages that will be referenced by the GPU, plus cache flush
  and address translation policy.
- Requires: `pci-claim`; `mmio-proof` when the proof uses GPU-visible mappings
  or register readback.
- Inputs: CPU virtual address, physical address, GPU address/GGTT slot, byte
  length, cacheability/flush operation.
- Success: CPU writes a marker pattern, flushes it through the named path, and
  reads the same bytes back from the address that later code will hand to the
  GPU.
- Does not prove: the GPU can see the bytes, command streamer address decoding,
  vertex fetch, or render target writes.

### `batch-submit-proof`

Contract:

- Block: RCS ring/execlist command submission and MI command execution.
- Requires: `mmio-proof`; `memory-proof` for ring, context, batch, and result
  storage.
- Inputs: context descriptor, ring/head/tail, batch GPU address, result GPU
  address, start/end marker values.
- Success: a start marker is written, an end marker retires, head/tail or
  execlist status changes are logged, and no engine error register names the
  submitted command as faulting.
- Does not prove: any 3D state packet was accepted, vertices were consumed, or a
  pixel could be produced.

### `vertex-upload-proof`

Contract:

- Block: vertex buffer allocation/layout only.
- Requires: `memory-proof` for the vertex storage.
- Inputs: vertex GPU address, CPU pointer, vertex count, stride, element
  formats, byte size, expected per-vertex values.
- Success: three vertices exist at the GPU address with the declared
  stride/layout, and CPU readback after flush matches the encoded values.
- Does not prove: VF consumed those vertices, VS ran, clip/raster accepted a
  primitive, or any render target changed.

### `vf-proof`

Contract:

- Block: vertex fetch/input assembler.
- Requires: `batch-submit-proof`; `vertex-upload-proof`; valid
  `3DSTATE_VERTEX_BUFFERS`, `3DSTATE_VERTEX_ELEMENTS`, topology, and draw count.
- Inputs: vertex buffer address, pitch/stride, vertex element declarations,
  topology, vertex count, VF statistics enable.
- Success: input assembler/VF counters advance by the expected vertex/primitive
  count, or a post-VF marker retires after the draw packet.
- Does not prove: VS execution, correct attribute values inside VS, streamout,
  clipping, rasterization, or pixels.

### `vs-proof`

Contract:

- Block: vertex shader stage, shader upload, URB allocation, and VS binding
  state.
- Requires: `vf-proof`; shader bytes have a known signature/hash and live at the
  GPU address programmed into `3DSTATE_VS`.
- Inputs: VS kernel GPU address, shader hash, scratch/bindless state if used,
  URB allocation, SBE/VUE layout expectation.
- Success: VS invocation counters advance for the submitted vertices, or a
  streamout/VUE proof captures values attributable to the VS.
- Does not prove: clipper acceptance, raster setup, PS dispatch, render target
  writes, or display visibility.

### `clip-raster-proof`

Contract:

- Block: clipper, SF/raster setup, viewport, scissor, sample state, and
  primitive setup.
- Requires: `vs-proof`; valid viewport/scissor/drawing rectangle state.
- Inputs: clip mode, viewport bounds, scissor rectangle, primitive topology,
  cull/raster state, sample mask.
- Success: clip/raster/SF counters or named post-clip/post-raster markers move
  after the draw, even if no PS thread is launched yet.
- Does not prove: pixel shader dispatch, color blend/backend acceptance, render
  target memory writes, or scanout.

### `ps-rt-proof`

Contract:

- Block: pixel shader dispatch, binding table, surface state, blend/color
  backend, and render target memory.
- Requires: `clip-raster-proof`; render target storage has a `memory-proof`.
- Inputs: PS kernel GPU address/hash, binding table pointer, surface state,
  format, pitch, dimensions, clear/baseline samples, expected changed sample.
- Success: PS or color backend counters advance and at least one sampled pixel
  in the render target changes from the pre-submit baseline.
- Does not prove: the display plane points at this render target or that a human
  can see the pixel on screen.

### `display-proof`

Contract:

- Block: display pipe, primary/overlay plane, cursor, framebuffer surface, and
  live surface registers.
- Requires: framebuffer/plane memory has a `memory-proof`; `ps-rt-proof` only if
  the display handoff is meant to show GPU-rendered pixels.
- Inputs: pipe, plane slot, surface address, stride, format, size, live-surface
  register, before/after pixel samples.
- Success: programmed plane/cursor registers and live registers point at the
  expected memory, frame counter advances, and sampled framebuffer bytes match
  the image or render target being handed off.
- Does not prove: the 3D pipeline rendered those bytes unless paired with the
  matching `ps-rt-proof`.

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
