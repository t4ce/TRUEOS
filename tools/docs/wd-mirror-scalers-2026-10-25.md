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
all slot permutations, opacity/CSC and live surface preservation, source-lock
boundaries, commit ordering, animation-end cleanup, teardown, and initialization
without per-frame DBUF/watermark updates.

Raw H.264 streams, strict receiver logs, frame analysis, contact sheets,
test-owned SSH transcripts, reproduction scripts, build output and deployment
receipts are under `bld/wd-mirror-20261025/`.
