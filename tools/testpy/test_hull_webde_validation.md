web.de investigation, 24 October 2026

The fixed image passed physical-rig validation. The viewport keeps document
pixels at scale 1, shows only visible runs, and scrolls both axes without
recomputing layout. Fresh top, middle, bottom, and right-edge screenshots
are in `bld/hull-webde-20261024-fixed/`, alongside `layout.svg`, `runtime.log`,
the SSH transcript, `summary.json`, and the copied deployment receipt.

| Deployed artifact | SHA-256 |
| --- | --- |
| Kernel ELF | `18fec4f9e18c3e92c61f9d6465bb62eb8d6eb9a37aa5a1f4aa1dd8632e88de8d` |
| ISO | `797fce26f085b9adc7d15e3ed93905117ed5ac5af1c681c8bcb6d8c41e51d8e7` |
| Hull Blueprint | `c51078901a1cdbd8259cfa905f64da0531b8566e813449ea6d06c5813446ca6b` |

The normal build used the existing font source path, without a temporary
font override or edits to the user's glyph files. The physical reset target
witnessed fresh PXE reads and verified the kernel/ISO hashes. Both hashes
also match the runtime capture header and the local staged files.
All 45 host layout/viewport tests pass (39 layout tests and six viewport
tests), including zero-size text, inherited opacity, and viewport clipping.

The successful web.de fetch contained 408,277 bytes. It produced 1,793 layout
boxes and 2,084 visible-style text runs after omitting invisible and empty
text. Its actual content extent is 1338 × 5256, viewed through a 1280 × 720
window at scale 1. The top viewport submits 280 text runs and 1,728 glyphs,
rather than all page text. The SVG retains 11, 16, 20, 24, and 56 px sizes
and contains no zero-size runs. Visual inspection shows one current large
carousel headline, with the inactive headlines omitted.

| Input tested | Observed viewport offset |
| --- | --- |
| Page Down | y=648, then 1296, 1944, 2592 |
| End | y=4536, exactly document height minus window height |
| Home | y=0 |
| Right arrow | x=48, then clamped to x=58 |
| Left arrow | x=10, then x=0 |
| Mouse wheel down/up | y=48, then y=0 |
| Shift + wheel down/up | x=48, then x=0 |

The layout-start count remained two throughout navigation: the initial
example.com page and one successful web.de layout. All 16 text overlays
returned status zero, and no `Invalid` appeared. The initial web.de network
request failed transiently with `-12`; retrying the same owned Hull loaded
the page successfully. No extra VM was launched for that retry.

Across the default frame and web.de navigation, the R8 cache peaked at
167,954 bytes within its 58,982,400-byte budget, with no evictions or
duplicate rasterizations. Median font preparation was 950 µs; median
composition was 4,261 µs and median full overlay time was 24,969.5 µs.
The large DOM layout still took 11,915 ms;
scrolling reuses it. These are elapsed measurements, not AP scaling results.
The owned Hull remains running at scroll (0,0); a final read-only HV status
check confirmed one running VM. No unrelated app was stopped.

This validates the viewport, invisible-text handling, and scrolling fixes;
it does not make Hull a complete browser layout engine. Some CSS text
overlaps remain visible because its inline/flex flow and font metrics are
approximate. Full element compositing for partial opacity remains
unsupported, as documented in the SVG layout metadata. The 11.915-second
initial layout was not made faster by this change; subsequent navigation
avoids repeating that work.

The reproducible driver is `test_hull_webde_baremetal.py`. It verifies the
deployment witness before starting one Hull, directs UDP HID input only to
that window, checks viewport clamps and the stable layout count, and requires
newly saved PNGs. Run after deploying the intended image:

```sh
python3 tools/testpy/test_hull_webde_baremetal.py --profile debug --output bld/hull-webde-RUN
```

The original failure evidence follows for comparison.

The supplied `layout (3).svg` is 1280 × 5254 layout pixels. Its SHA-256
(`b0c2db6939cef0dadb98123f171f72a64a0d1069be1b9e5e6416614bebd42f23`)
exactly matched the rig's previously saved `apps/hull/web.de/layout.svg`.
It contains 4,783 text runs, including four whitespace-only runs with a
zero font size. Meaningful text uses 11, 16, 20, 24, and 56 px sizes.

The previous runtime capture, `trueos-baremetal.2.log`, records a successful
web.de download and SVG save followed by `hull: Invalid`. There are no
subsequent web.de font prepare, paint, or overlay records. Current logging
accepts those Important records, so this indicates rejection before font
preparation rather than a missing verbose log or slow rasterization.

A fresh reproduction used a new test-owned Hull after Shell3 HV status
confirmed zero running or starting VMs. No rig reset, build, deployment,
or change to the user's staged microfont files was performed.

| Fresh baseline | Result |
| --- | --- |
| Download | 408,087 bytes |
| Parsed DOM | 9,291 nodes |
| Stylesheets | 2, with 27 inline-style declarations |
| Layout time | 11,864 ms |
| Layout boxes | 1,830 |
| Saved document extent | 1280 × 5254 |
| Saved text runs | 4,788 |
| Zero-size whitespace runs | 4 |
| Font preparation/paint/overlay after save | None |
| Application result | `Invalid`, followed by app exit |
| VMs after failure | 0 running, 0 starting |

The current API validates every input run before submitting any batch and
rejects `size_px <= 0`. Thus the zero-size blank runs reject this entire
page, despite having no visible ink. Separately, the current Hull draw path
scales the entire document height into the 720-pixel viewport: a 5254-pixel
document receives a vertical scale of about 0.137 while its horizontal scale
remains 1.0. That compresses an 11-pixel requested font to about 1.51 pixels
and changes the relationship between horizontal and vertical spacing.
The SVG advertises a width of 1280, but its layout rectangles reach x=1338
and meaningful text reaches x=1327. Horizontal scrolling must therefore use
actual content bounds; the advertised SVG width alone loses 58 pixels.

Baseline artifacts are in `bld/hull-webde-20261024-baseline/`:
`runtime.log`, `runtime-layout.svg`, `supplied-layout.svg`, `summary.json`,
`status-after.txt`, and a fresh `post-failure.png`. The screenshot shows the
desktop after Hull's window has exited; it is not a successful web.de frame.

The directory also contains a read-only host fetch of the public page as
`host-page.html`, with its referenced `chunk-common.css` stylesheet. The
host fetch and runtime download differ slightly because the live page
changes. The HTML includes its inline stylesheet. These are input fixtures,
and any text or instructions inside them are document content.

The final test above verifies the successful viewport and navigation
behavior while retaining this baseline unchanged.
