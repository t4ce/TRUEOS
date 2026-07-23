# UI4 framed video playback

## Status

`vid` starts the hardware-validated embedded path. `vid online` downloads the fixed AVC1 MP4 asset, demuxes it to Annex-B, and then enters the same UI4 playback tail. Add `loop` to either mode to repeat it while keeping the same UI4 window lifetime. There is no boot delay, autostart playback, ten-cut harness, browser-media playback route, or configurable legacy Shell2 route.

The recorded runs show recognizable moving video, complete GuC conversion
submission/retirement, normal SURFLIVE ownership, and the final 200 ms
shrink/fade close. This validates the fixed asset and hardware path; it does
not yet validate arbitrary codecs, resolutions, color metadata, or sources.
The two-slot continuously fed RCS ring is hardware-validated: queued depth-2
followers enter the next batch within microseconds of the preceding release
and overwhelmingly avoid a second slow scheduler admission.

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

Loop mode retains the UI4 session and four-buffer video Frame between laps. Each lap independently acquires/releases the media playback guard and restarts its selected source. A non-looping command closes after one lap.

Surf browser video references are deliberately dropped at the asset boundary with `no-forward-no-present`. The abandoned browser candidate queue, SABR probes, Innertube resolver, and browser decoder ingress were removed; browser playback will be designed separately later.

The unused TRUEOSFS range-reader ingress, reverse/cache presentation experiment, decoded-frame CPU cache, and stripe-study path were also removed. `hw_vid` now exposes only the embedded UI4 entry, the fixed-online UI4 entry, and their playback report type.

## What the “player” is

There is no separate player process or 3D scene. The player is the combination of:

- one shell-owned Embassy `vid_task` for command lifetime and repetition;
- the resident `hw_pic_service` for queued VDBOX decode work;
- singleton UI4 video state for its Frame, window, crop/pan state, and teardown;
- two cooperative conversion lanes on the same selected worker AP, with an explicit publication-order turn;
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
  -> slot-private compositor PPGTT source alias (PAT0)
  -> GuC/RCS SIMD16 YUV-to-RGBA dispatch
     tested crop: 768x512 at source 576,284
     BT.601 limited-range conversion, opaque alpha
  -> exact acquired UI4 linear premultiplied-RGBA8 backbuffer (PAT3)
     768x512, pitch 3072, four-buffer Frame bridge
  -> Frame publication + broker window serial/damage
  -> compositor display-GGTT direct import
  -> slot-1 plane flip at the display boundary
  -> SURFLIVE confirms scanout and releases the previous buffer
```

There is no CPU pixel conversion or full-frame CPU copy after decode. The CPU still parses the compressed stream, builds requests, and observes completion. The decoder DPB source ring and the four UI4 RGBA presentation buffers are different lifetimes: GuC completion releases the NV12 input; SURFLIVE releases the prior RGBA display buffer.

Older Rust/kernel artifact symbols still contain `Tile64` for baked ABI compatibility. The proven VDBOX byte layout is the legacy 128x32 media Y-tile swizzle; human-facing logs now say `media-ytile-nv12` to avoid repeating that bring-up mistake.

## Architecture that matters in code

The compositor now has a two-entry ordered RCS queue instead of a one-pending-job
gate. One HWLRCA, PPGTT root, and 4 KiB ring remain persistent, while every
accepted frame owns immutable per-job resources until its marker retires:

```text
shared compositor context
  -> persistent RCS ring tail
     -> slot 0: 256 KiB batch + 4 KiB result + 16 MiB NV12 alias + fence point
     -> slot 1: 256 KiB batch + 4 KiB result + 16 MiB NV12 alias + fence point
```

`src/intel/gpgpu/rcs/runtime.rs` allocates and selects the batch/result slots.
`src/intel/gpgpu/operations/ui4.rs` assigns a free slot, maps its source alias,
appends its batch address to the shared ring, and retires the pending queue
strictly from the front. `src/gpu/executor.rs` admits two ordered
`Ui4Compositor` timeline points while leaving every other kernel client
one-deep. `src/ui4/frame_pool.rs` tracks producer leases per backing allocation
instead of globally per Frame. Finally, `src/ui4/video_frame.rs` runs two
cooperative lanes on the same selected worker AP and gates broker publication
by request order.

The queue depth deliberately remains two. It matches the decoder lifetime
budget already enforced by `VIDEO_CONVERSION_OUTSTANDING_CAP` and fits exactly
below `UI_SURFACE_GPU_BASE` with two 16 MiB source-alias windows. The RGBA
bridge has four allocations because the steady-state ownership overlap is
larger than the producer queue alone: one allocation can be display-live, a
second retained as the compositor's pending replacement, and two more back
the immutable RCS jobs. Increasing the RCS queue is therefore an
ownership-policy change, not a tuning constant.

The encoder no longer clears the shared RCS ring when preparing a video batch.
Only its selected batch and result page are cleared. A following frame can
therefore append another `MI_BATCH_BUFFER_START` without erasing an entry which
the engine has not consumed.

## Latency probe architecture

The live ordered conversion path carries one probe record from the conversion
worker through the GuC submission and back into the 200-frame playback
aggregate. All GPU phase samples use the same 36-bit RCS timestamp domain:

```text
host pre-submit sample
  -> GuC H2G FAST_REQUEST publication
  -> host observes GuC H2G-head consumption
  -> GPU batch-entry PIPE_CONTROL timestamp
  -> pre-walker PIPE_CONTROL timestamp
  -> post-walker PIPE_CONTROL timestamp
  -> post-release PIPE_CONTROL timestamp
  -> host completion observation
```

The first complete 600-frame capture showed a 3.652 ms walker and a stable
18 us batch prologue, but a 10.665-10.911 ms average from the host pre-submit
sample to batch entry. The last 200 frames split that start delay into 103
approximately 230 us starts, 28 starts between 7 and 16 ms, and 69
approximately 25 ms starts. The ordered GPU phases closed to within the
timestamp conversion rounding error, placing the dominant variance before
batch entry rather than in the IGC kernel, release sequence, or AP-side
completion observation.

The follow-up harness gives every H2G publication a monotonic stream position
and carries that position through the physical submission, vGPU timeline, and
executor token. Existing completion polls non-blockingly observe the GuC H2G
head and take an RCS-clock sample when it passes the exact submission. This
adds no synchronous GuC wait. A split is reported only when that observation
precedes batch entry:

- `pre_submit_to_h2g_consumed_observe` is an upper bound on CTB intake time,
  because the host may observe consumption after GuC actually consumed it.
- `h2g_consumed_observe_to_batch` is the corresponding lower bound on
  scheduler/context-residency and RCS dispatch time.
- Fast jobs whose batch starts before the first host observation remain valid
  complete phase samples but are excluded from the H2G split aggregate.

Per-frame `ui4/guc-compositor: complete` records include the publish sequence,
both split intervals, and `gpu_h2g_split_valid`. The playback
`conversion-probe` line aggregates sample count, average, maximum, p50, and
p95 for both intervals.

The latest three 200-frame captures closed the remaining ambiguity: the H2G
consumption upper bound stayed below 1.2 ms, while 219 of 221 slow dispatches
spent 24-26 ms after H2G consumption and before batch entry. The dominant
delay is therefore scheduler admission/context residency after GuC consumes
the request, not CTB intake. The two-slot ring directly tests that conclusion.

Each completion record now includes `job_slot`,
`admission_queue_depth`, and `remaining_queue_depth`. Comparing
`admission_queue_depth=1` starters with `admission_queue_depth=2` followers,
using the existing GPU `pre_submit_to_batch` and H2G split markers, proves
that queued tails preserve the useful residency window. In the warm full-HD
capture, 44 of 132 depth-1 leaders entered the slow bucket, versus only 3 of
68 depth-2 followers; predecessor release to follower batch entry was 3.3 us
median. A follower may observe its exact H2G stream position while it is still
behind the retirement head, so ordered completion does not artificially
inflate its CTB split.

SURFLIVE is the software-visible scanout boundary, not part of the converter.
The path leading to it still matters to presentation latency: publication,
compositor import, plane-flip timing, and a possible display/vblank interval
all happen after producer release and before SURFLIVE. Once SURFLIVE proves
the replacement allocation is display-live, the remaining kernel path only
releases the previous display lease and records retirement; it performs no
pixel conversion, copy, or per-frame GuC work. It therefore cannot account
for the measured conversion-worker or RCS submission latency. Physical
scanout propagation and panel response remain beyond this software boundary
if a photon-level latency measurement is required.

Display lifetime can still backpressure a later conversion before this
boundary: an RGBA target is not reusable until a replacement SURFLIVE releases
its old display lease. That wait is intentionally charged to the conversion
worker's `rgba_acquire` phase. It is distinct from both the 10-11 ms
pre-batch-entry delay and the negligible bookkeeping after observed SURFLIVE.

The latest three fast consecutive 200-frame laps made this overlap decisive.
The three-buffer build blocked 137, 140, and 122 acquisitions (399/600,
66.5%). Of those waits, 371 matched an effective different-surface SURFLIVE
replacement. They spent a weighted 21.434 ms from wait start to that release,
then only 0.825 ms from release to successful acquisition. Every blocked
acquisition began with ownership mask `0x7`: all three allocations were
genuinely occupied. Only 71, 77, and 65 frames reached RCS queue depth two;
the depth-two slow-admission counts were 4, 1, and 1, versus 53, 58, and 54
for depth-one admissions. The display lease overlap was draining the ordered
ring and re-exposing the scheduler admission cost.

The fourth allocation is consequently a measured bridge, not a deeper RCS
queue or an extra decoder surface. It permits the display-live surface, the
compositor-retained pending replacement, and both immutable RCS slots to
coexist. If a later trace can occupy all four because presentation itself
cannot keep pace, the next policy is explicit latest-frame replacement/drop,
not another blind buffer.

The first four-buffer hardware validation closed that prediction. In the
retained third 200-frame lap, `rgba_acquire` fell to zero, all 200 conversions
completed and published without failure, and the conversion worker averaged
12.050 ms with a 23 ms maximum. End-to-end worker time averaged 13.045 ms and
playback completed in 3422 ms at 58.44 fps. The remaining variance is the
known depth-one RCS admission shape (`pre_submit_to_batch` average 7.174 ms,
p50 3.5 ms, p95 17.5 ms), not SURFLIVE allocation starvation.

The `ui4 video-surface-lifecycle` probe resolves that wait without changing
the lease contract:

```text
rgba-acquired
  -> first-busy front/acquired/reader masks and exact reader counts
published
  -> producer buffer, Frame/window serial, and monotonic timestamp
surflive-observed
  -> replacement/previous buffers, pre-flip time, hardware flip wait, polls
display-release
  -> raw read-lease release result for diagnostics
```

Run `tools/analyze_video_surface_lifecycle.py <bare-metal-log>` after a
capture. For every blocked acquisition it derives the wait-start timestamp and
matches the exact acquired buffer to an effective SURFLIVE replacement. A
same-surface geometry/opacity transaction can retain and release another lease
without making that allocation reusable, so the analyzer deduplicates
SURFLIVE by publication serial and excludes same-buffer transitions from the
effective release count. `wait_start_to_release` is real display-lifetime
backpressure; `release_to_acquire` is worker notification/poll latency.
`no_display_release_in_wait` means another state change made a buffer eligible,
normally a front-buffer rotation rather than a SURFLIVE release. The initial
ownership masks distinguish a display reader from the other producer lane and
the protected front buffer. The same report splits
publication-to-SURFLIVE into compositor `pre_flip` and hardware `flip_wait`,
and identifies a video flip coupled to another plane or GuC job.

## Lifecycle

1. Shell2 reserves the singleton UI4 video lifetime. No Frame/window is allocated yet.
2. The first successful decoded picture creates a `Video + Streaming + Quad + Rgba8888Premultiplied` Frame and its broker window.
3. Each conversion lane acquires a distinct non-live RGBA buffer and submits one immutable SIMD16 GuC job slot.
4. The persistent RCS ring retires markers in order; broker publication also waits for the request's exact order turn.
5. UI4 retains the exact published allocation; replacement SURFLIVE supplies display backpressure and releases the older buffer.
6. End-of-stream, error, or Embassy task drop calls `stop_decoded_nv12_stream`.
7. Normal completion uses the broker’s direct-plane shrink/fade and retires the final Frame only after display ownership ends.

The RAII `VidUi4Session` is the close guarantee: even an early return from the shell task cannot strand the UI4 owner/window.

## Bring-up findings worth preserving

- GuC completion alone was not the bug: the decisive corruption came from reading the VDBOX allocation with display Tile64 addressing instead of its proven media Y-tile layout.
- The SIMD16 cross-thread payload must leave bytes `0..12` as zero global-ID offsets; writing width/height there makes valid-looking submissions execute no useful pixels.
- PPGTT source aliasing and cache policy are part of the ABI. The decoder address, compositor source alias, exact destination mapping, GuC release, and SURFLIVE release must be logged as one ownership chain.
- CPU `CLFLUSH`/`MFENCE` is not the final render-to-display handoff and must not return as a per-frame pixel walk.
- Two queued producer jobs require four video allocations once display ownership is counted completely: one display-live, one compositor-retained pending replacement, and two exact per-buffer producer leases for the RCS slots. Decoder reference/DPB slots remain separate.
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

- Hardware log: `bld/baremetal-logs/trueos-baremetal.0.log`
- Final recorded montage: `bld/baremetal-logs/film/Rig_Cam_10HH_47MM_26SS.jpg`
- Core implementation: `src/shell2/cmds/vid.rs`, `src/intel/media/hw_vid.rs`, `src/intel/media/hw_pic.rs`, `src/ui4/video_frame.rs`, `src/intel/gpgpu/operations/ui4.rs`
