# T9 group-ID accum16-hi live32 artifact contract

Candidate source:

- `crates/trueos-shader/t9_groupid_accum16_hi_live32_trueos_arena_bf16_unpack.comp`

Purpose:

- This is the direct follow-up to the 2026-05-26 `step=46` negative control.
- The existing `gfx12-t6-3-accum16-hi-live32` artifact retires under a 32-group
  launch, but only writes rows `0..7`: `compare_mask=0x000000FF` against
  `expected_mask=0xFFFFFFFF`.
- T9 keeps the proven accum16-hi live32 math, but changes the row/output selector
  from `gl_LocalInvocationID.x` to `gl_WorkGroupID.x`, matching the T8 live16
  row-addressing shape.

Contract:

- One row per workgroup: `layout(local_size_x = 1)`.
- `gl_WorkGroupID.x` selects the row and output slot.
- The tile-record layout is unchanged:
  - `x[0..2047]` begins at dword `0`.
  - packed BF16 row data begins at dword `2048`.
  - each row stride is `1024` packed dwords.
  - output begins at dword `264192`.
- T8 live16 must run first, writing the live16 partial into `output[row]`.
- T9 reads `output[row]`, accumulates packed BF16 lanes `16..31`, and stores the
  live32 partial back into `output[row]`.

Boot acceptance target:

- Use the same 32-row compare contract as `step=46`.
- Required proof:
  - `groups=32`
  - `row_count=32`
  - `live_k_dim=32`
  - `expected_lane_dispatch=256`
  - `observed_lane_dispatch=256`
  - `compare_mask=0xFFFFFFFF`
  - `expected_mask=0xFFFFFFFF`
  - `finish_marker=0xC0DE7732`
- If this proves, the separate live32 projection can move from accounting to a
  trusted-path candidate: `3000 -> 2928` submits.

Do not promote runtime ownership from this source alone.  Promotion requires the
native EU bytes to be extracted into `crates/trueos-eu`, wired as a distinct T9
artifact, then proven in baremetal with the acceptance target above.
