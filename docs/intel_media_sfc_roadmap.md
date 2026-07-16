# Intel AVC VD-to-SFC road

TRUEOS's target video path is:

`VDBOX AVC decode -> SFC scale/CSC -> opaque linear RGBA UI4 frame -> normal UI4 composition`

The decoded Y-tiled NV12 surface remains the AVC destination and DPB/reference
surface. SFC writes a separate presentation surface. The existing CPU
Y-tiled-NV12-to-RGBA converter remains the visible path and the pixel oracle
until a shadow SFC output has retired and matched it on hardware.

## Mechanical source

- repository: `https://github.com/intel/media-driver`
- pinned TRUEOS port commit: `a203cfc`
- platform family: `Xe_LPM_plus_base`
- AVC insertion point:
  `decode_avc_picture_packet_xe_lpm_plus_base.cpp`, after the second
  `MFX_WAIT` and before `MFX_SURFACE_STATE`
- command order: `SFC_LOCK`, `SFC_STATE`, `SFC_AVS_STATE`, optional AVS luma
  and chroma coefficient tables, optional/CSC `SFC_IEF_STATE`, then
  `SFC_FRAME_START`

No packet may enter the live AVC batch based only on topology's `sfc: true`.
`MediaCapabilities::sfc_programmed` stays false until the complete command,
resource, mapping, retirement, and fallback chain is present.

## Landed foundation

1. `sfc_cmd` defines the fixed packet sizes and headers, the upstream command
   recipe, the narrow progressive same-size AVC-to-UI4 plan, and Intel's
   single-pipe scratch sizing. It does not submit hardware work.
2. UI4 video output is split into acquire, publish, and cancel operations. An
   acquired target carries one non-copyable write lease plus its virtual,
   physical, and GPU addresses. The current CPU converter uses this seam.
3. The media context now owns a stable sparse PPGTT root. External UI4 targets
   can be mapped before a submit and unmapped after retirement without replacing
   the VDBOX context root.
4. A valid boot-video target logs `sfc-target ready ... mode=shadow-disabled`.
   This proves allocation, pitch, alignment, command budget, and scratch budget
   without enabling SFC.

## Next implementation gates

### Gate A: complete offline packet encoding

Mechanically port and validate the remaining dwords for:

- `SFC_STATE` for one VDBOX pipe, progressive NV12 input, linear uncompressed
  UI4 RGBA output, opaque alpha, and no histogram/MMC;
- `SFC_IEF_STATE` with the same limited-range CSC currently used by the CPU
  oracle;

`SFC_LOCK`, one-to-one `SFC_AVS_STATE`, and `SFC_FRAME_START` already have fixed
encoders and compile-time shape checks.

The first packet stream is 93 dwords. It deliberately excludes AVS coefficient
tables because scaling is disabled.

### Gate B: resource backing and mapping

Allocate and bind the checked AVS/SFD line and line-tile buffers. Acquire one
UI4 video target before constructing the AVC batch, map that target into the
stable media PPGTT, and retain the lease through retirement. Add the required
VDBOX address-translation synchronization before first use of a new mapping.
Add an explicit retiring state to video-stream teardown before allowing a
write target to live across an `await`; teardown must block new acquisition
without closing a window underneath an in-flight producer.

Any failure cancels the target and runs the current decode plus CPU conversion.

### Gate C: shadow hardware proof

Emit SFC into an unpublished target while the CPU-converted frame remains
visible. On successful retirement:

- invalidate/read the SFC output safely;
- compare selected pixels and signatures with the CPU oracle;
- record command headers, output address, fault registers, and completion
  markers;
- cancel the shadow target rather than publishing it.

This mode must remain device-gated and explicitly disabled by default until a
boot test proves stable output.

### Gate D: hot-path rewire

After the shadow result matches, publish the retired SFC target directly and
skip CPU conversion. On any SFC-specific failure, cancel it and immediately use
the existing CPU path. Only after same-size CSC is stable should scaling and the
two AVS coefficient-table commands be enabled.

## Invariants

- Never make an RGB surface the AVC decode/DPB destination.
- Never publish or recycle an SFC target before media retirement.
- Never unmap a target while VDBOX can still reference it.
- Never consume linked NV12 display planes when the UI4 RGB path succeeds.
- Never remove the CPU fallback as part of initial SFC bring-up.
