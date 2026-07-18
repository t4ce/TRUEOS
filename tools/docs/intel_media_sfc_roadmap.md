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

1. `sfc_cmd` defines the fixed packet sizes and headers, exact non-submitting
   `SFC_LOCK`, `SFC_STATE`, `SFC_AVS_STATE`, CPU-oracle `SFC_IEF_STATE`, and
   `SFC_FRAME_START` encoders, the narrow progressive AVC-to-UI4 plan, and
   Intel's single-pipe scratch sizing. State encoding rejects missing, short,
   overlapping, unaligned, or out-of-range GPU bindings.
2. UI4 video output is split into acquire, publish, and cancel operations. An
   acquired target carries one non-copyable write lease plus its virtual,
   physical, and GPU addresses. The current CPU converter uses this seam.
3. The media context now owns a stable sparse PPGTT root. External UI4 targets
   can be mapped before a submit and unmapped after retirement without replacing
   the VDBOX context root.
4. A valid boot-video target logs `sfc-target planned ...
   mode=shadow-disabled`. This proves allocation, pitch, alignment, command
   budget, and scratch budget without enabling SFC.

The 2026-07-16 boot test retired and published frames through all three UI4
buffers after the sparse media PPGTT change. It also exposed a useful boundary:
the clip is coded as `1920x1088` and displayed as `1920x1080`. Intel's VD mode
requires source-region state to use the coded extent, so those eight padding
rows make this a scaling command stream, not the provisional 93-DWORD
same-size stream. The corrected plan is 287 DWORDs and requires both AVS
coefficient-table packets. An exact coded-size output remains 93 DWORDs.

The automatic dummy-UI4 boot demo now uses an `ui4-probe-required`
presentation policy. Decode frames must pass through the UI4 RGBA target seam;
if acquire, conversion, or publish fails, the frame is reported and dropped
instead of silently falling back to linked NV12 planes or direct-primary CPU
presentation. Manual playback retains those legacy fallbacks while the SFC
producer remains non-submitting.

## Next implementation gates

### Gate A: complete offline packet encoding

The complete 61-DWORD `SFC_STATE` packet is now encoded for one VDBOX pipe,
progressive NV12 input, linear uncompressed UI4 `A8B8G8R8` output, opaque
alpha, CSC, no histogram/MMC, and checked AVS/SFD scratch bindings. UI4's
little-endian `[R,G,B,A]` bytes match this hardware format without channel
swap.

`SFC_IEF_STATE` is also encoded with IEF sharpening disabled and the same
limited-range integer BT.601 coefficients used by the current CPU oracle.
Compile-time fixtures check the command headers, channel format, CSC matrix,
offsets, scaling flags, pitch, and representative addresses.

The remaining Gate A work is the 5x5 polyphase luma and chroma coefficient
generation/packing required by the real boot clip's `1088 -> 1080` vertical
conversion. The scaled AVS state already selects NV12 left/center chroma
siting, but no scaling stream is eligible for submission until both tables are
encoded and fixture-checked.

An exact coded-size stream is 93 DWORDs. A stream with scaling is 287 DWORDs:
the same five fixed commands plus 129-DWORD luma and 65-DWORD chroma tables.

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
