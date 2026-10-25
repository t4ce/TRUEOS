# WD mirror placement during window animations

The remote-only jump to the top-left during window opening and closing is
fixed and the updated kernel is deployed on the physical rig.

## Cause and change

For a scaled plane, the display engine requires `PLANE_POS = 0,0`. Its actual
destination is carried by `PS_WIN_POS` and `PS_WIN_SZ`. UI4 correctly programmed
these registers on Pipe A, but the Pipe C mirror copied only the plane state.
It therefore presented the unscaled source at the origin during animations.

`src/intel/display/mirror_map_dp_engine.rs` now collects the application-plane
registers and both scaler contracts under the compositor's existing submission
mutex, then releases it before programming Pipe C. Plane geometry, opacity,
input CSC, scaler bindings, phases and destination windows are staged before
the plane surface addresses and WD frame trigger. Scaler bindings follow the
same slot permutation as the mirrored surfaces. Disabled source scalers are
explicitly disabled and armed on Pipe C, including when an animation ends and
when capture stops.

The TGL/ADL hardware has two scalers on every pipe. The shared register helper
previously omitted Pipe C's second scaler at `0x69280`; this is now exposed.
The plane DBUF partition and boot-safe watermarks are initialized when capture
starts rather than reprogrammed for every captured frame. The existing cursor,
plane input color/alpha, WD output CSC and DMA ownership paths are preserved.

This reflects the current UI4 hardware-ROM scaler filter contract. It does
not add support for programmable scaler coefficient banks or guarantee an
exact snapshot of every physical scanout latch. The mutex excludes partially
written application transactions; independently owned slot 4 and cursor updates
retain their existing behavior.

Hardware basis: checked-in local Intel TGL PRM, Vol. 12, Pipe Scaler programming
sequence (pp. 255–258), and Vol. 2c, `PS_CTRL` (pp. 992–997), `PS_WIN_POS`, and
`PS_WIN_SZ`. These document the two Pipe C instances, zero plane position during
scaling, binding fields, and the final window-size write that arms updates.

## Physical validation

A dedicated SSH-launched `img` instance opened the built-in Intel graphics
logo at its centered destination and closed its own frames three times while
the ordinary 2560x1440 TME1 stream was recorded. No user image was needed.
The test instance closed and exited afterward.

| Measurement | Before | After |
| --- | ---: | ---: |
| Complete decoded frames | 330 | 330 |
| Open/close transitions exercised | 6 | 6 |
| Frames showing the logo at the origin | 30 | 0 |
| Receiver/decode validation | Passed | Passed |

Before, the origin excursions appeared at frames 34–38, 65–68, 102–107,
132–136, 172–176 and 203–207. After, the logo appears and animates around its
intended centered destination with no origin excursion. Analysis compared each
run with its own initial background at 640x360 resolution, counting pixels
with a channel change above 25 in the origin region; a count above 1,000
identified the baseline excursions. Contact sheets were also inspected.

The desktop background changed across reboot, so encoded byte totals are not
a controlled bandwidth comparison. The verified improvement is removal of the
incorrect displaced window content and its unnecessary pixel changes.

`make iso` and packaged-artifact checks passed. Fresh PXE reads and the boot
marker verified runtime ELF SHA-256
`3fce78021a3d213740d1ca60c9592059a98bc7eda9adc663940de34fddf74d63`.

## Regression coverage and artifacts

All 26 focused host checks passed:

```sh
python3 tools/testpy/test_wd_mirror_scalers.py
python3 tools/testpy/test_ui4_close_scalers.py
python3 tools/testpy/test_wd_bcs_snapshot.py
python3 tools/testpy/test_rdp_pipeline.py
```

The five new tests exercise production register collection/programming with
a mock MMIO bank: simultaneous scalers including Pipe C's second instance,
all slot permutations, opacity/CSC and surface contract preservation, source-lock
boundaries, commit ordering, animation-end cleanup, teardown, and initialization
without per-frame DBUF/watermark updates.

Raw H.264 streams, strict receiver logs, frame analysis, contact sheets,
test-owned SSH transcripts, reproduction scripts, build output and deployment
receipts are under `bld/wd-mirror-20261025/`.

## Follow-up: first selection after a full deselect

The user subsequently reported a remaining transition with three Shell3
windows: deselect everything, let the scene settle, then select a window.
The original three `img` open/close cycles did not cover that selection path.

A second consistency bug was demonstrated by a production-MMIO regression:
the source geometry, opacity and scaler registers describe the latest
programmed update, but `SURFLIVE` can still name the previous window until
the physical pipe latches. During plane reassignment, that can combine a new
composed plane's geometry at 0,0 with the preceding direct window's pixels.

The mirror now copies the corresponding programmed `SURF` address. These
surfaces are ready for display: the compositor waits for composition work to
complete before submission, retains prepared source leases in the pending
frame, and transfers its direct display leases after the physical latch.
The independent slot-4 producer also flushes its rendered damage before
publishing `SURF`. This keeps surface and geometry in the same update without
adding a wait for physical vblank or starving capture on continuous input.
It remains a snapshot of the programmed scene, rather than an exact physical
scanout latch. The cursor path retains its existing contract.

The new regression fails against the previous code with the old surface
address and passes with the corrected one. The focused suite now has 27
passing checks. The follow-up kernel build, ISO/package checks and physical
deployment verified runtime ELF SHA-256
`e044a949fd06b0499220c753cb7bc6c70e20a22ba58455270b3c3fd353c9cce1`.

Two selection captures on the previous image passed strict decoding without
reproducing the exact reported flash. They exercised exposed Shell3 windows,
background deselection and focus changes, but could not prove that all other
cursor associations were absent. This limits any claim of a live before/after
reproduction of this second report; the failing MMIO test proves the corrected
inconsistency independently.

After the update, a fresh boot established three test-owned physical Shell3
windows at 130,100; 730,220; and 430,320, each 1700x800. With only the test
cursor route active, three strict capture sessions exercised 12 full
deselect–settle–select cycles: nine alternating focus changes and three
already-top-window controls. Settling intervals varied between 0.6 and 1.1
seconds. All 990 frames decoded with zero receiver anomalies, and the native
origin region was pixel-identical to its initial background throughout all
three recordings. The actual window bounds and hardware lease changes were
checked; this was not just repeated selection of an unchanged top plane.
The three windows remain arranged on the rig, with a final background
deselect and released buttons, for immediate manual retesting. These clean
after-tests still do not establish a reproduced before/after result for the
user's exact intermittent trigger.

The existing RDP viewer's exact FFmpeg flags were also replayed against the
recorded selection stream. Its 290 decoded frames were pixel-identical to
frames 40–329 from ordinary offline decoding; the difference was startup GOP
discard, not a selection-dependent placement difference. No viewer rebuild
is required for the kernel snapshot change.

Follow-up build logs, before/after MMIO regression output and deployment
receipts are in `bld/wd-selection-20261025/`. The initial live selection
captures, scripts and decoder comparison remain alongside the original
capture artifacts in `bld/wd-mirror-20261025/`.
