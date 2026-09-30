# Tiger Lake BCS marker bring-up

## Hardware result: 2026-10-01

Verified on the physical Alder Lake-S GT1 rig (`8086:4680`, revision 0C).
Both classical BLT and GuC Fast Copy now execute:

- A direct-execlist cold-boot `XY_SRC_COPY_BLT` copied one scratch pixel from
  `0xB17C0000` to a poisoned destination. The destination matched, the completion
  cookie was `0xC0DEBC50`, and ring head/tail reached `0x50`.
- After the GGTT correction below, ordinary `vcpy start` retired the marker,
  completed all 20 rows (450,560 bytes), and continued into subsequent cycles.
  Status showed zero failures and no pinned allocation at the 250 ms cadence.

The decisive fault was a **GGTT ownership collision**. BCS reserved controls at
`0x00B00000..0x00B71000`, inside Render0's 2560×1440 RGBA streamout mapping
`0x00880000..0x01690000`. Later render boot initialization replaced the BCS PTEs.
GuC then read the wrong backing for the registered context. Its scheduling-enable
acknowledgement did not prove that it had loaded the intended context.

BCS now reserves `0x01A80000..0x01B00000`, between Picasso Render1 controls and
system RCS. Compile-time assertions constrain the neighboring reservations;
submission verifies every BCS control PTE against its original physical page.
It rejects a changed mapping rather than rewriting shared GGTT entries.

The missing boot setup was also repaired: initialize BCS's status page, set
Gen12 `GFX_MODE` bit 3 following the required BCS reset, and clear `STOP_RING`.
That change alone did not fix submission; the old mappings still failed.

The direct classical probe is retained behind `BOOT_BCS_LEGACY_PROBE = false`
in `src/intel/copy/blt.rs`. It is a cold-boot diagnostic before GuC startup, with
private scratch operands and a BCS reset before scheduler handoff. The normal
demo remains GuC-owned and uses Fast Copy.

Local hardware evidence (ignored build artifacts):

- `bld/artifacts/bcs-direct-legacy/boot.log`: classical one-pixel success.
- `bld/artifacts/bcs-mapping-fix/boot.log`: repeated full Fast Copy cycles.
- `bld/artifacts/bcs-mapping-fix/vcpy-display.png`: fresh post-blend capture,
  visually checked for the green MicroFont rows in the demo window.
- The matching directories' `reset-receipt.json` files verify the runtime ELF
  hashes and fresh PXE reads for each experiment.

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

These packet corrections are independent of the GGTT collision that prevented
the first marker from executing.

## First submission

Every submission starts with a ring-level store cookie and batch start. The
first batch contains:

```
MI_STORE_DATA_IMM (batch-entry cookie)
MI_ARB_CHECK (disable pre-parser)
MI_FLUSH_DW (invalidate BCS TLB, post-sync scratch write to GGTT)
MI_ARB_CHECK (enable pre-parser)
MI_NOOP
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
into a host harness and checks marker packets, linear RGBA copy packets,
pitch overflow rejection, ring wraparound, and the classical one-pixel packet.
All five tests and `cargo build --bin TRUEOS` passed. Hardware execution evidence
is recorded separately above.
