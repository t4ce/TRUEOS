# Gridpaper static-base copy on BCS0

Gridpaper's `render_compute_page_frame` now tries the synchronous vcpy consumer
before its existing `copy_rect_rgba8_complete_mode` call. It copies the full
static base into the frame destination at (0,0). The original compute copy
handles requests BCS rejects before submission, including a busy demo lane.
The renderer presentation copy remains unchanged; `primary.rs` already selects
direct destinations for its direct-output branches.

## Contract

- Linear, unscaled, same storage order, 32 bits per pixel; the source fits the
  destination. Physical allocations and GPU address ranges must be disjoint.
- The existing BCS validator enforces page alignment, mapped extent, 16-bit
  pitch/coordinates and its 1 GiB PPGTT limit. Surface addresses cannot overlap
  BCS control addresses.
- Font batches already release HDC/L3 writes before their completion marker.
  After rebuilding or patching a base, Gridpaper also submits an explicit
  system-compute release, without changing the source cache policy. Cached
  bases need no extra RCS submission.
- Before accessing the surfaces, BCS invalidates translations with
  `MI_FLUSH_DW` bit 18 and a scratch post-sync write. Pre-parser disable/enable
  brackets this boundary. This allows page allocations to recycle VAs between
  completed jobs. The command and result buffers retain their fixed GGTT maps.
- The copy returns success only after the ordered BCS completion marker,
  context-save proof, and software timeline retirement. Subsequent compute
  prologues already invalidate caches before their walkers.
- Ambiguous submission or completion never triggers a second writer. The live
  destination lease stays pinned; the owned static base is quarantined against
  destruction. Print rendering also quarantines its locally owned destination.

This consumer runs on demand, independently of `vcpy start` and the MicroFont
250 ms cadence. It currently blocks while polling, matching Gridpaper's prior
synchronous copy contract; it does not overlap compute and copy execution.

## Evidence and checks

`vcpy status` includes `consumer=gridpaper copies=… bytes=… fallbacks=… failures=…`.
Completed-copy counters exclude the demo. These counters establish use of the
new path; they do not measure a speedup or verify every copied byte.

- `cargo check --bin TRUEOS` and `cargo build --bin TRUEOS`.
- `python3 tools/testpy/test_tgl_bcs_packets.py`: exact packet words, invalidation
  ordering, ring wrap, pitches, padded rows, origins, and alias rejection.
- `python3 tools/testpy/test_vcpy_consumer.py`: scripted admission, pending,
  completion and ambiguous failure outcomes using the production wrapper.

## Hardware result

Deployed runtime SHA-256:
`c304232cee4991fe713b663a9dd7b8c825eb13b3ac2b4983b271220ce477ccd5`.
The reset receipt verifies the staged ELF and a fresh PXE read.

The native startup Gridpaper greeting exercised 32 successful BCS copies,
36,479,232 bytes, zero fallbacks and zero failures. The MicroFont worker remained
stopped. This exercises repeated static-base updates and subsequent font compute
on the real Alder Lake-S rig. It establishes execution and retirement; there is
no speedup claim or byte-for-byte reference comparison yet.

Artifacts: `bld/artifacts/gridpaper-bcs/{boot.log,final-status.txt,reset-receipt.json}`.
A second boot of the same ELF completed 35 copies / 39,899,160 bytes with zero
fallbacks and zero failures. The timed capture
`bld/artifacts/gridpaper-bcs-visual/greeting-1.png` visibly shows the grid and
complete multicolored “Hello from TrueOS §” greeting. The adjacent `status.txt`,
`boot.log`, and `reset-receipt.json` record this run. The screenshot checks the
visible consumer result, not exact pixel equality with the compute reference.

A separate `grid` Blueprint launch was unavailable because the online app
catalog was empty. The native startup workload supplied the consumer evidence.

## References

Local TGL PRM Vol 2a pp. 955 and 990–992 specify pre-parser control and the BCS
TLB invalidate/post-sync requirements. The same sequence is used by Linux's
[`gen12_emit_flush_xcs`](https://github.com/torvalds/linux/blob/v6.12/drivers/gpu/drm/i915/gt/gen8_engine_cs.c#L339).
