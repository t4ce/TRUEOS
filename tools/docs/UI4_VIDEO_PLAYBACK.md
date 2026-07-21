# UI4 framed video playback

## Status

`vid` starts the hardware-validated embedded path. `vid online` downloads the fixed AVC1 MP4 asset, demuxes it to Annex-B, and then enters the same UI4 playback tail. Add `loop` to either mode to repeat it while keeping the same UI4 window lifetime. There is no boot delay, autostart playback, ten-cut harness, browser-media playback route, or configurable legacy Shell2 route.

The recorded 200-frame run showed recognizable moving video, 200 GuC conversion submissions and retirements, normal SURFLIVE ownership, and the final 200 ms shrink/fade close. This validates the fixed asset and hardware path; it does not yet validate arbitrary codecs, resolutions, color metadata, or sources.

## Command and service routing

| Stage | Code/API | Ownership |
|---|---|---|
| Command | `shell2/cmds/vid.rs::try_parse` accepts only `vid [embedded\|online] [loop]` and spawns one Embassy `vid_task` | Shell2 task owns the command and UI lifetime |
| Embedded source | `UI4_FRAMED_VIDEO_ANNEXB` is the fixed embedded `x31_head_movie.annexb.h264` asset | Kernel read-only bytes; no TRUEOSFS/open/demux dependency |
| Online source | `run_online_ui4_framed_video_playback` downloads one fixed MP4 and converts its AVC1 samples to Annex-B | Network acquisition and MP4 demux end at the same decoder input contract; they do not own presentation |
| Playback | The selected `hw_vid` entry parses/paces one 60 Hz Annex-B lap | The calling Shell2 task awaits it directly; this is not an RPC to a player daemon |
| Decode service | `hw_pic_submit_h264` queues an encoded access unit and returns an ID; resident `hw_pic_service` drives VDBOX; `wait_output_for_id` awaits the matching output | Request/output queue, not a callback |
| UI producer | `present_decoded_nv12_stream_frame` converts the retired decoder surface into an acquired UI4 RGBA buffer | GuC/RCS owns the conversion interval |
| Window/display | UI4 Frame pool, window broker, compositor direct import, plane flip, and SURFLIVE | UI4 owns publication and display lifetime |

Loop mode retains the UI4 session and double-buffered Frame between laps. Each lap independently acquires/releases the media playback guard and restarts its selected source. A non-looping command closes after one lap.

Surf browser video references are deliberately dropped at the asset boundary with `no-forward-no-present`. The abandoned browser candidate queue, SABR probes, Innertube resolver, and browser decoder ingress were removed; browser playback will be designed separately later.

The unused TRUEOSFS range-reader ingress, reverse/cache presentation experiment, decoded-frame CPU cache, and stripe-study path were also removed. `hw_vid` now exposes only the embedded UI4 entry, the fixed-online UI4 entry, and their playback report type.

## What the “player” is

There is no separate player process or 3D scene. The player is the combination of:

- one shell-owned Embassy `vid_task` for command lifetime and repetition;
- the resident `hw_pic_service` for queued VDBOX decode work;
- singleton UI4 video state for its Frame, window, crop/pan state, and teardown;
- the permanent UI4 compositor/display services.

It is genuinely UI4-enabled. The decoder’s NV12 allocation is intentionally **not** a UI4 Frame: it is a transient producer input. The converted RGBA ring, Frame handle, window/session, placement, damage publication, direct-import lease, and teardown are all UI4-owned. Direct slot-1 presentation is a compositor optimization, not a producer bypass; the decoder path performs no display MMIO programming.

The broker API is a trusted typed in-kernel API, not TCP or userspace IPC: `create_frame`, `begin_window_session`, `create_window`, `acquire_frame_buffer`, `publish_gpgpu_video_frame_buffer`, `publish_window_frame`, and `finish_window_session_with_request`.

## Per-frame format and buffer path

```text
embedded H.264 Annex-B access unit
  -> hw_pic request queue
  -> VDBOX long-format decode
  -> decoder-owned media-Y-tiled NV12 DPB surface
     tested: 1920x1088 coded, 1920x1080 visible, pitch 2048
  -> compositor-private PPGTT source alias (PAT0)
  -> GuC/RCS SIMD16 YUV-to-RGBA dispatch
     tested crop: 768x512 at source 576,284
     BT.601 limited-range conversion, opaque alpha
  -> exact acquired UI4 linear premultiplied-RGBA8 backbuffer (PAT3)
     768x512, pitch 3072, two-buffer Frame ring
  -> Frame publication + broker window serial/damage
  -> compositor display-GGTT direct import
  -> slot-1 plane flip at the display boundary
  -> SURFLIVE confirms scanout and releases the previous buffer
```

There is no CPU pixel conversion or full-frame CPU copy after decode. The CPU still parses the compressed stream, builds requests, and observes completion. The decoder DPB source ring and the two UI4 RGBA presentation buffers are different lifetimes: GuC completion releases the NV12 input; SURFLIVE releases the prior RGBA display buffer.

Older Rust/kernel artifact symbols still contain `Tile64` for baked ABI compatibility. The proven VDBOX byte layout is the legacy 128x32 media Y-tile swizzle; human-facing logs now say `media-ytile-nv12` to avoid repeating that bring-up mistake.

## Lifecycle

1. Shell2 reserves the singleton UI4 video lifetime. No Frame/window is allocated yet.
2. The first successful decoded picture creates a `Video + Streaming + Double + Rgba8888Premultiplied` Frame and its broker window.
3. Each picture waits for the non-live RGBA buffer, submits one SIMD16 GuC conversion, then publishes only after GuC retirement proves producer release.
4. UI4 retains the exact published allocation; replacement SURFLIVE supplies double-buffer backpressure and releases the older buffer.
5. End-of-stream, error, or Embassy task drop calls `stop_decoded_nv12_stream`.
6. Normal completion uses the broker’s direct-plane shrink/fade and retires the final Frame only after display ownership ends.

The RAII `VidUi4Session` is the close guarantee: even an early return from the shell task cannot strand the UI4 owner/window.

## Bring-up findings worth preserving

- GuC completion alone was not the bug: the decisive corruption came from reading the VDBOX allocation with display Tile64 addressing instead of its proven media Y-tile layout.
- The SIMD16 cross-thread payload must leave bytes `0..12` as zero global-ID offsets; writing width/height there makes valid-looking submissions execute no useful pixels.
- PPGTT source aliasing and cache policy are part of the ABI. The decoder address, compositor source alias, exact destination mapping, GuC release, and SURFLIVE release must be logged as one ownership chain.
- CPU `CLFLUSH`/`MFENCE` is not the final render-to-display handoff and must not return as a per-frame pixel walk.
- Double buffering is sufficient here: one RGBA buffer can be live while the other is the sole producer target. Decoder reference/DPB slots remain separate.
- A successful close animation is a useful architectural test: it proves the window went through the broker/UI4 lifecycle rather than an old direct-present side path.

## Natural next features

- Accept other sources only by feeding the same framed Annex-B/decode contract; do not create a second present path.
- Add PTS/audio-clock pacing and late-frame dropping instead of treating requested FPS as the only clock.
- Carry BT.709/BT.2020, full/limited range, aspect ratio, scaling, and HDR metadata into the conversion dispatch.
- Add explicit stop/pause/seek controls above `vid loop`; the removed Space toggle did not actually gate Shell2 decode and was intentionally not preserved as fake functionality.
- Generalize singleton ownership for multiple video windows after broker/plane policy exists.
- Export a fenced decoded or converted allocation as a texture if video inside a resident 3D scene is wanted. The current window path is not a mesh or scene texture API.
- Use the same producer-release/UI4/SURFLIVE contract for camera, screen-share, subtitles, and GPU effects.

## Evidence

- Hardware log: `bld/baremetal-logs/trueos-baremetal.1.log`
- Final recorded montage: `bld/baremetal-logs/film/Rig_Cam_10HH_47MM_26SS.jpg`
- Core implementation: `src/shell2/cmds/vid.rs`, `src/intel/media/hw_vid.rs`, `src/intel/media/hw_pic.rs`, `src/ui4/video_frame.rs`, `src/intel/gpgpu/operations/ui4.rs`
