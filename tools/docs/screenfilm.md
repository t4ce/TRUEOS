# Shell2 screen recording

In cmd mode, `film 1` records for one minute; the only argument is an integer
from 1 through 10. It selects the exact Matrix slot `film`, marks it running
for the slot-strip heartbeat, and reports elapsed time, frames, measured fps,
saved/buffered bytes, and capture/encode timings every five seconds.

`§film§` stops capture and saves the tail before finalizing. Navigating to
another slot or closing the invoking frontend does not stop recording. The
slot lifetime also prevents a deleted/recreated `film` page from inheriting
the old recording or receiving its late output. An unrelated occupied `film`
slot is rejected, rather than choosing a different slot name.

The command requires the resident encoder to have passed its hardware proof,
a writable TRUEOSFS root, and an idle WD capture source. RDP View-Mode owns
the same hardware and excludes recording; `--no-view` input controls never
claim it. A two-second grace period protects the gap between RDP's bounded
view sessions. New view subscriptions wait until recording/finalization ends.
The `shot` command can borrow a completed recording frame as it does for RDP.

Capture reuses the serialized Pipe-C/WD packed XYUV8888 → Gen12 VDEnc H.264
path, with no CPU pixel conversion or software encoder. Dimensions and cadence
come from the existing encoder configuration (currently 2560×1440, 33 fps).
The output is video-only H.264 Annex-B, with the encoder's fixed-rate SPS
timing. The capture duration is bounded by elapsed wall time; if the system
falls behind, fewer frames are saved and fixed-rate playback is shorter.

Recordings are saved on the preferred writable TRUEOSFS root as:

`screenfilms/wd-postblend-<unix-seconds>-<monotonic-ms>-film<sequence>.h264`

TRUEOSFS needs the final length before opening a streamed Put. The recorder
therefore commits `.h264.part000000`, `.part000001`, etc. every five seconds
or 16 MiB, always between complete access units. Only one bounded payload
chunk is buffered. On stop, pinned read handles copy the chunks in order into
one final file. Temporary chunks are removed only after the final commit and
length verification. Finalization needs space for another copy of the encoded
bytes; TRUEOSFS's append-only log does not reclaim the deleted part records.

On a disk/encode failure, successfully committed chunks remain recoverable.
Concatenate the parts belonging to that exact recording in numeric order;
later parts can depend on pictures in earlier parts. A missing part must not
be replaced by a part from another recording. An in-flight disk error can
prevent saving the current buffered tail. If hardware retirement cannot be
proved, preserve the recording but retain the WD target and reject subsequent
capture admission.

Root write leases serialize streamed Put lifetimes and other log-head
mutations, including checkpoints. This lets the final copy yield during disk
I/O while preventing a screenshot, upload, or app write from reusing the same
log space. Reads through pinned record handles remain available.

Host validation: `python3 tools/test_screenfilm.py` and
`python3 tools/test_rdp_pipeline.py`. These exercise production orchestration,
Matrix lifetime behavior, admission, and teardown using simulated hardware;
actual WD/encoder throughput and saved-video playback still need a rig run.
