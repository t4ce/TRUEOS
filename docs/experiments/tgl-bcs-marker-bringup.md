# Tiger Lake BCS marker bring-up

## Backend selection

TGL PRM Vol 10, pp. 35–36 describes one BCS command streamer dispatching to
two backends by instruction opcode. `XY_BLOCK_COPY_BLT`, `XY_FAST_COPY_BLT`
and `XY_FAST_COLOR_BLT` all select Fast Copy. Classical BLT instructions select
the legacy backend. Neither needs a different GuC queue just to select it.

Fast Copy understands resources/subresources and internally splits work into
sub-blits. Nested batch buffers are a command-streamer feature, distinct from
that internal division of copy work.

The current demo expands MicroFont on the CPU into an RGBA source, then copies
pixels with Fast Copy. The monochrome font-expansion example in Vol 10 uses
classical BLT capabilities and is a different packet sequence.

## Packet findings

Compared with the local `G12TL_intel_prm` Vol 2a instruction reference:

- Pages 1372–1373: `XY_FAST_COPY_BLT` DW1 bits 29:28 must be zero.
  The old encoder set both as system-memory flags. For a 2048-byte RGBA pitch,
  DW1 must be `0x03000800`, not `0x33000800`.
- Pages 1373–1374: pitches occupy 16 bits. Reject pitches that spill into
  reserved fields.
- Page 955: `MI_ARB_CHECK` opcode is 05h in bits 28:23, hence `0x02800000`.
  The old `0x05000005` had the batch-end opcode with additional low bits set.
- Pages 971–972: batch-start bit 8 clear means privileged GGTT. This was
  already backed by the boot mapping; rename the misleading `MI_BATCH_PPGTT`
  constant while preserving the actual address-space selection.
- Pages 990–992: explicitly select the GGTT result address for the flush's
  post-sync write, matching the privileged batch and boot-mapped result page.

These are source-level findings. They do not establish which instruction caused
the reported hardware timeout without a new hardware run.

## First submission

`vcpy` first submits only:

```
MI_FLUSH_DW (write completion cookie to GGTT)
MI_ARB_CHECK
MI_BATCH_BUFFER_END
MI_NOOP
```

BCS has no shader thread to terminate with an EU EOT message. Batch end returns
control to the ring; the flush writes the observable completion cookie.
The existing saved-context check must also pass before storage is reused.
After this submission retires, the demo proceeds with the first RGBA row copy
on its 250 ms submission cadence.

`vcpy status` exposes `marker_retired`:

- `0` with a pinned failure: inspect the timeout's `marker_observed` and
  `context_saved`. No Fast Copy packet has been submitted by this demo yet.
- `1` with a later pinned failure: the marker-only submission retired; inspect
  the first copy's surface mappings, packet and hardware error registers.

The existing `pinned=1` quarantine is permanent for that boot. Loading the new
kernel in a fresh boot is required to exercise these changes.

## Local verification

`python3 tools/testpy/test_tgl_bcs_packets.py` compiles the production encoder
into a host harness and checks marker-only packets, linear RGBA copy packets,
and pitch overflow rejection. `cargo check --bin TRUEOS` checks kernel integration.
Neither test proves hardware execution.
