# Shell2 video sessions

`vid fs [path] [loop]` and `vid on [loop]` admit up to three videos. Filesystem
playback accepts the existing AVC MP4 and H.264 Annex-B paths. Each admission
prints its slot number and uses the corresponding Matrix transcript: `vid`,
`vid2`, or `vid3`.

Escape closes the selected video window. `vid stop <1|2|3>` uses the same broker
close path from Shell2. `vid status` reports each slot's occupancy, cancellation,
conversion counts, and active work. Its UI4 resource totals include other apps.
A fourth video is rejected while all three slots are occupied or draining.

## Ownership

- Each playback has a generation, cancellation flag, independent UI4 window
  session, conversion queue, presentation ordering, and accounting.
- The media engine admits three logical decode reservations. The existing
  `hw_pic` service serializes VDBOX submissions and their GuC context teardown.
  Each reservation has its own DPB, presentation pins, output surfaces, DMV
  scratch, and stable PPGTT root. Submission controls remain shared by the
  serialized worker. RCS maps the selected physical picture into its own alias.
- Two RCS workers service the video queues in round-robin order. Each video
  allows at most two outstanding conversions; publication remains ordered
  within that video.
- Closing stops further loading, decoding, deadline waits, and conversion
  admission. Accepted GPU work must retire before its memory can be reused.
  Deferred B-frame pictures return their pins, and the task drains conversions
  before releasing its media reservation and playback slot.
- An already-closed broker window transfers no frame on session teardown, so
  the video owner explicitly retires that RGBA frame after its display readers
  release it. Normal animated closure transfers retirement to the broker.
- Cold filesystem index replay belongs to the filesystem service. A video waits
  cooperatively for readiness and can cancel that wait. Online cancellation
  requests the existing TLS cancellation/fence path rather than abandoning an
  active socket owner.

Decoder backing allocations form a bounded warm cache for the three slots;
closing releases their session ownership and pins, rather than repeatedly
allocating DMA memory. Playback rate shares VDBOX, RCS, compositor, and CPU
capacity with other videos and applications. Three admissions do not guarantee
three simultaneous 60 FPS streams. Audio behavior is unchanged.

## Verification

Run from the repository root:

```sh
cargo check
python3 tools/test_video_sessions.py
python3 tools/test_video_loading_cancel.py
python3 tools/test_media_backing.py
python3 tools/test_avc_context_teardown.py
python3 tools/test_video_colour.py
python3 tools/test_https_fetch.py
python3 tools/probe_vid_fs.py tools/vid/DSC_1879.MP4 tools/vid/screenrec.mp4 tools/vid/x31_head_movie.annexb.h264
```

The host probe interleaves staggered reference histories in all three DPBs and
checks the production parser/commands against ffprobe metadata. It does not
prove GPU pixels or pacing. On an authorized rig, also test three overlapping
playbacks, fourth-slot rejection, selected-window close/replacement, cold-root
early cancellation, and repeated stops. Once playback and display retirement
settle, slot activity must be zero and UI4 resource totals must return to their
pre-playback baseline, allowing for unrelated applications.
