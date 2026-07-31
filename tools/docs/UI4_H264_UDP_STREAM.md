# TRUEOS UI4 hardware H.264 stream

The default `trueos_h264_encode_stream` feature stages a resident,
subscriber-driven kernel service:

1. At boot, TRUEOS procedurally fills one 1920x1088 NV12 surface and submits it
   through Intel Gen12 VDEnc/MFX on VCS0. The service proceeds only after GuC
   submission, command-stream status writeback, Annex-B structure, and coded
   output all validate. No diagnostic media file or software encoder is linked
   into the kernel.
2. A receiver sends `TME1GET1` to UDP port 9650.
3. On subscription, one isolated final-AP worker enters a bounded,
   three-task producer/consumer pipeline. This intentionally remains a weak
   core when the topology's final AP is an E-core, making the path expose CPU
   work that should have stayed behind hardware fences. The producer builds an
   explicit RDP composition manifest: immutable slot-0 base, selected visible
   broker windows in plane/z order, slot-4 service rectangles, and Spirit.
   One C++ for OpenCL layer-compositor dispatch samples the base and selected
   window allocations directly into one of two persistent DMA-backed
   premultiplied-RGBA final mirrors. The full 2560x1440 base is neither cleared
   nor copied by the CPU. Small slot-4 solid rectangles and the BGRA Spirit
   cursor remain exact finishing passes after the composition marker. If the
   GPU request cannot be admitted before submission, the previous scalar
   composer remains the correctness fallback.

   A second C++ for OpenCL dispatch through the same isolated UI4 GuC/RCS
   context applies the fixed 4:3
   center-sampled nearest downscale, converts directly to limited-range BT.601
   NV12, and fills four black rows above and below the centered 1920x1080 image
   in the macroblock-aligned 1920x1088 surface. Ordinary screenshots still
   export straight-alpha RGBA.

   RCS completion is observed before the same persistent NV12 allocation is
   mapped directly as the VDBOX source. The encoder performs no full-frame
   NV12 CPU copy, and bounded change telemetry samples at most 4,096 bytes
   rather than hashing the complete frame. The RDP destination VA is disjoint
   from both decoder source aliases and the complete UI-surface arena, so local
   playback conversion and RDP conversion cannot remap one another's PPGTT
   pages. Exactly two reusable RGBA/NV12 slot pairs let preparation of the next
   frame overlap Gen12 VDEnc/MFX encode and UDP egress of the preceding frame.
   VDBOX completion is awaited cooperatively on that worker, so preparation
   and egress remain runnable while hardware owns the encode interval.
   The consumer emits a fresh IDR access unit on each absolute 40 Hz deadline.
   The first frame is prepared before cadence measurement begins, and the
   producer cannot advance more than one frame ahead of the consumer.

   This fixed test-rig mapping preserves the native 16:9 composition and avoids
   dynamic crop selection. The capture follows UI4 plane/z order but is not a
   bit-exact latch of the physical scanout. The immutable slot-0 base and
   Spirit's dedicated cursor plane are explicit physical-plane inputs; generic
   hardware mouse cursors remain absent.
4. Each access unit is immediately fragmented into CRC-protected TME1
   datagrams and unicast to the subscriber. The media socket has a 64 KiB
   transmit ring, and every datagram carries an internal adapter receipt token.
   Up to eight fragments (9,600 bytes) are submitted as one bounded window
   before their receipts are drained. This exactly matches the socket's eight
   TX packet-metadata entries, allowing one network-service turn to admit each
   window without a metadata-ring retry. The sequence advances only after each
   matching adapter acceptance; a confirmed full ring retries that exact
   packet after one millisecond, while a missing or fatal receipt aborts
   without an uncertain retransmission. Accepted fragments have no artificial
   inter-packet delay. The live high-water mark is one access unit; no
   framebuffer or encoded payload is written to TRUEOSFS.
5. After 400 frames (ten seconds), the socket closes and the resident service
   waits for the next subscriber, which receives a fresh session.

There is no software-codec or filesystem fallback in the kernel path. AVC
playback and encode share VCS0 with frame-level exclusion: decode keeps its
session reservation, while the 40 Hz encoder may take a bounded turn between
decode submissions. A transport-mode change resets and reactivates VCS0 before
the next complete batch. If a decode reservation is already active while the
boot hardware proof is waiting, the encoder sleeps and keeps retrying; it does
not permanently disable the service after a fixed timeout.

`trueos_h264_encode_probe` remains only as a compatibility feature alias for
older build scripts; the former disk-output probe crate, its software encoder,
and its 30-frame embedded input file have been removed.

## Ubuntu receiver

Build and run the standalone receiver from the repository root:

```sh
rustc --edition=2024 -O tools/media_encode_udp_receiver.rs \
  -o media_encode_udp_receiver
./media_encode_udp_receiver \
  --bind 0.0.0.0:9650 \
  --subscribe-target 192.168.178.94:9650 \
  --output trueos-ui4.h264 \
  --ffmpeg-check
```

The receiver sends the eight-byte subscription token, pins the first valid
`(source, session_id)` pair, validates every 32-byte TME1 header and payload
CRC, reorders fragments within bounded memory, writes complete Annex-B access
units, reports loss/reordering/duplicates, and can invoke FFmpeg for an
end-to-end decode check. Strict validation is the default: an empty capture,
an incomplete session, any integrity/loss counter, or a decoder failure makes
the command fail. Use `--allow-loss` only for diagnostics. Omit
`--subscribe-target` when LAN broadcast discovery is desired.

The subscription token is discovery/gating rather than authentication: the
first sender of `TME1GET1` receives that bounded session. Use this service only
on a trusted LAN or behind network policy that restricts UDP port 9650.

Each TME1 UDP datagram is at most 1200 bytes:

| Offset | Bytes | Field |
| ---: | ---: | --- |
| 0 | 4 | ASCII `TME1` |
| 4 | 1 | version (`1`) |
| 5 | 1 | start/end/keyframe/session-end flags |
| 6 | 2 | header bytes (`32`, big-endian) |
| 8 | 4 | session ID |
| 12 | 4 | datagram sequence |
| 16 | 4 | access-unit sequence |
| 20 | 2 | fragment index |
| 22 | 2 | fragment count |
| 24 | 2 | payload bytes |
| 26 | 2 | reserved |
| 28 | 4 | payload CRC32 |
| 32 | <=1168 | Annex-B fragment |
