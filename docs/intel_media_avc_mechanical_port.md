# Intel Media AVC Decode Mechanical Port

This is the rule for bringing H.264 back into TRUEOS: no guessed VDBOX packet
streams. The Rust path should be a mechanical port of the upstream
`intel/media-driver` AVC decode packet model.

Reference checkout:

- Repo: `https://github.com/intel/media-driver`
- Local path: `/home/t4ce/REPOS/reference/intel-media-driver`
- Commit inspected: `a203cfc`
- Platform family to follow first: `Xe_LPM_plus_base`

Primary upstream files:

- `media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_packet_xe_lpm_plus_base.cpp`
- `media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_picture_packet_xe_lpm_plus_base.cpp`
- `media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/codec/hal/dec/avc/packet/decode_avc_slice_packet_xe_lpm_plus_base.cpp`
- `media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_picture_packet.cpp`
- `media_softlet/agnostic/common/codec/hal/dec/avc/packet/decode_avc_slice_packet.cpp`
- `media_softlet/agnostic/common/hw/vdbox/mhw_vdbox_mfx_impl.h`
- `media_softlet/agnostic/common/hw/vdbox/mhw_vdbox_mfx_cmdpar.h`
- `media_softlet/agnostic/Xe_M_plus/Xe_LPM_plus_base/hw/vdbox/mhw_vdbox_mfx_impl_xe_lpm_plus_base.h`

Mechanical sequence:

1. Prolog: forcewake, frame tracking/status markers.
2. Picture: `MFX_WAIT`, `MFX_PIPE_MODE_SELECT`, `MFX_WAIT`.
3. Picture: `MFX_SURFACE_STATE`.
4. Picture: `MFX_PIPE_BUF_ADDR_STATE`.
5. Picture: `MFX_IND_OBJ_BASE_ADDR_STATE`.
6. Picture: `MFX_BSP_BUF_BASE_ADDR_STATE`.
7. Picture: `MFD_AVC_PICID_STATE`.
8. Picture: `MFX_AVC_IMG_STATE`.
9. Picture: four `MFX_QM_STATE` commands.
10. Picture: `MFX_AVC_DIRECTMODE_STATE`.
11. Per slice: optional `MFX_AVC_REF_IDX_STATE`.
12. Per slice: optional `MFX_AVC_WEIGHTOFFSET_STATE`.
13. Per slice: `MFX_AVC_SLICE_STATE`.
14. Per slice: `MFD_AVC_BSD_OBJECT`.
15. Epilog: `MI_FLUSH_DW`, status report, batch end.

First TRUEOS milestone:

- Long-format, single IDR/I-slice only.
- No B/P refs, no weighted prediction, no MVC, no encrypted content.
- Exact SPS/PPS/slice parse into typed Rust structs.
- NV12 destination surface with explicit pitch/UV offsets.
- Typed Rust parameter builders before raw command dwords:
  - `MFX_PIPE_MODE_SELECT`
  - `MFX_SURFACE_STATE`
  - `MFX_PIPE_BUF_ADDR_STATE`
  - `MFX_IND_OBJ_BASE_ADDR_STATE`
  - `MFX_BSP_BUF_BASE_ADDR_STATE`
  - `MFD_AVC_PICID_STATE`
  - `MFX_AVC_IMG_STATE`
  - `MFX_QM_STATE` x4
  - `MFX_AVC_DIRECTMODE_STATE`
  - `MFX_AVC_REF_IDX_STATE` dummy L0 for the I-slice
  - `MFX_AVC_SLICE_STATE`
  - `MFD_AVC_BSD_OBJECT`
- Row-store scratch buffers sized from upstream formulas:
  - deblocking: `pic_width_in_mbs * 4 * 64`
  - BSD/MPC: `pic_width_in_mbs * 2 * 64`
  - intra: `pic_width_in_mbs * 64`
  - MPR: `pic_width_in_mbs * 2 * 64`
- DMV buffers allocated before `MFX_AVC_DIRECTMODE_STATE`.
- TRUEOS runtime staging now binds rowstore and DMV ranges inside a real mapped
  AVC scratch window, not synthetic GPU addresses.
- The limited H.264 path programs the destination as Tile64 NV12. For the
  current `1920x1088` sample that means pitch `2048`, UV row `1280`, and a
  `0x400000` byte surface. Pre-clear, output probing, and presentation share
  the same Tile64 NV12 layout and swizzle helpers. JPEG/IMC3 keeps its separate
  planar tiled path.
- On Xe LPM-plus `MFX_SURFACE_STATE.DW3.Tilemode` is the generated 2-bit enum,
  not the older Gen12 `TileWalk/TiledSurface` split. The Tile64 value is
  `TILEMODE_TILEYS_64K = 1`; `3` is `TILEMODE_TILEF`. The Rust packet recipe
  names the constant after that generated enum and validates the surface field
  widths before emitting the command.
- NV12 pre-clear uses video-range black (`Y=16`, `UV=128`) for both the decode
  destination and the dummy reference surface. The output probe uses the same
  baseline so an unchanged surface is not misclassified as decoded detail.
- `MFX_PIPE_BUF_ADDR_STATE` follows upstream AVC decode selection: when
  deblocking is enabled, bind the destination only as post-deblock (`DW4..6`);
  when it is disabled, bind it only as pre-deblock (`DW1..3`). The current
  single-IDR sample enables deblocking, so the probe expects pre=0/post=dest.
- `MFX_IND_OBJ_BASE_ADDR_STATE` uses the mapped 4K-aligned bitstream window as
  its upper bound; the BSD object still carries the exact encoded slice length.
- `MFD_AVC_BSD_OBJECT` byte/bit offsets are derived from an explicit
  payload-relative slice bit offset plus one included NAL header byte.  For the
  current sample this is payload bit `26`, byte offset `4`, low bit offset `2`.
- TRUEOS now models Intel's long-format `sliceRecord.offset/length` split:
  `offset = (slice_data_bit_offset >> 3) + dwNumNalUnitBytesIncluded`, while
  `length` is the slice payload window minus that offset. The BSD object then
  adds the offset back for `IndirectBsdDataLength`, matching upstream's
  non-Intel-entrypoint long-format path.
- `MEMORYADDRESSATTRIBUTES` MOCS fields are encoded as `mocs << 1`, unlike the
  direct 0..6 MOCS fields in `MFX_PIPE_BUF_ADDR_STATE`.
- `MFX_AVC_IMG_STATE` DW5 keeps the Xe3 generated command default
  `0x30000000`; the common AVC decode setter does not overwrite that field.
- `MFX_AVC_IMG_STATE` follows the generated Gen12 field widths: DW1 frame size
  is 16 bits, and DW2 frame width/height are 8-bit `minus1` fields. The
  milestone validator rejects pictures that cannot be represented by those
  fields instead of silently truncating them.
- `MFX_AVC_IMG_STATE` active reference counts are the PPS defaults plus one,
  matching Intel's setter, while the actual active reference-frame count remains
  zero for the single-IDR milestone. The host probe now checks DW3, DW4, DW13,
  DW14, and DW15 from parsed SPS/PPS fields.
- `MFD_AVC_PICID_STATE` disables picture-id remapping for the no-reference
  single-IDR milestone, matching Intel's no-remap branch. The host probe checks
  DW1 plus the zeroed remap-list payload.
- Even for the single-IDR/no-active-reference case, reference address slots must
  be valid: pipe-buffer reference surfaces use a separate black dummy NV12
  surface placed after the real decode destination inside the existing output
  backing. The host probe checks all 16 pipe-buffer reference slots.
  Direct-mode uses separate current/write and dummy/reference DMV scratch
  buffers, matching Intel's current-vs-available DMV layout, and the host probe
  checks all 16 direct-mode reference DMV slots.
- Direct-mode POC slots `32/33` carry the parsed current top/bottom field order
  counts, matching Intel's current-picture POC fill before any reference POC
  list entries. All other POC-list entries stay zero for the no-reference
  milestone.
- Intel's AVC slice packet emits an otherwise empty `MFX_AVC_REF_IDX_STATE` for
  dummy-reference I-frames; TRUEOS mirrors that with a zeroed L0 command before
  `MFX_AVC_SLICE_STATE`, and the host probe checks the whole dummy command.
- `MFX_AVC_SLICE_STATE` uses the generated Gen12 bit widths for slice
  positions: current slice X/Y are 8-bit fields, while next-slice X/Y are
  9-bit fields.
- `MFD_AVC_DPB_STATE` is intentionally not emitted for the first long-format
  single-IDR milestone. Intel's Xe3P AVC picture packet emits it only on the
  short-format branch; references/P/B slices must add it as a separate feature
  gate.
- Current host probe packet shape for `x31_head_movie_first_frame.h264`:
  `coded=1920x1088`, `mb=120x68`, `command_blocks=19`,
  `command_dwords=326`, with Tile64 pitch/UV layout asserted.
- `tools/avc_recipe_trace.rs` emits the full long-format IDR command stream
  grouped by command block, offset, dword count, and upstream symbol. Use this
  as the local side of any future diff against an instrumented
  `intel/media-driver` packet compiler or C ABI shim.

Remaining gate before reporting playback:

- Boot the live VDBOX submit path on hardware and inspect retire/fault/detail
  logs for the AVC batch.
- Trust `HwPicStatus::Ready` only when the batch retires and the NV12 output
  surface probe reports detail.

Guardrail:

Every emitted AVC dword needs a source mapping to an upstream `SETPAR` or
`ADDCMD` symbol. If a field cannot be traced, it does not ship.
