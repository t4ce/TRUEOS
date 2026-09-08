# Shared PNG path and viewer baseline

Validated on the physical TRUEOS rig on 2026-09-08 with the default debug
kernel build and release Blueprints.

The shared PNG decoder reuses its header reader and writes RGB, grayscale,
and grayscale-alpha expansion into fixed RGBA output slots. This removes
per-pixel Vec growth checks. The existing SSE2 Up unfilter retains its first-hit
log but avoids a locked atomic exchange on every subsequent row. The four
independent kernel media workers remain unchanged.

`img` routes filesystem and kernel-provided PNG/JPEG bytes through vmedia.
It recognizes file signatures, reports read/decode/publish timings, composites
PNG transparency onto black, and avoids an extra viewport copy for opaque
images already matching the viewport. `img apps/common/images` opens a sorted,
typed folder gallery: Left/Up goes backward, Right/Down forward, and both ends wrap.
A gallery reuses one window, fits each complete image, and retains only the
current decoded image. The gallery list/index survive Blueprint pause/replay.
`show FILE` retains the existing native pan/resize behavior.

`img` does not claim a terminal TUI. Select its Matrix slot and enter `list`,
`show`, `close all`, or `exit` directly on the persistent Shell2 prompt row.
Launching without a source uses the first inferred PNG/JPEG in
`apps/common/images`, or a neutral-gray frame when none exists.

Solara admits `.png` alongside `.jpg` and `.jpeg` through both resource
selection and native image painting. URL queries/fragments do not affect
selection, and encoded signatures select the kernel decoder. The existing
asynchronous fetch/decode, retained upload, layout and cropping path is reused.
Large browser textures are box-filtered into at most 2 MiB per image and 8 MiB
per page, preserving original CPU pixels and intrinsic layout dimensions.
Filtering averages premultiplied colors to avoid transparent-edge halos. This
leaves room for page rendering within the existing 32 MiB guest GPU quota;
the quota is unchanged. The diagnostic 4K image initially exhausted that quota
when uploaded at full size alongside another PNG. Images beyond the page
texture budget are skipped with a diagnostic.

## Measurements

`decode_ms` measures the complete Blueprint vmedia call: upload, worker wait,
decode, and RGBA readback. It is not an isolated inflate/unfilter timer.
`load_to_publish_ms` ends at UI4 submission, not physical scanout. Separate
post-blend screenshots establish that the images reached display composition.

| Fixture | Baseline vmedia call | Updated vmedia call |
| --- | ---: | ---: |
| 1920×1080 RGB, Average | 194 ms | 67–72 ms |
| 3840×2160 RGB, Paeth | 784 ms | 155, 208, 213 ms |
| 3840×2160 RGBA, Up | 218 ms | 219–221 ms |

These are diagnostic samples, not a statistically controlled benchmark.
Cubes continued running during measurement. The substantial RGB improvement
is consistent with removing the costly per-pixel expansion loop; RGBA input
already needs no channel expansion.

File reads still take about 1.2 seconds on this rig. Opening a file also
performs path metadata lookup; first load through publish takes roughly
2.6–3.1 seconds. A 4K RGBA gallery selection measured about 1.62 seconds.
Concurrent uploads substantially worsen storage timings, so comparison samples
were collected after all fixture uploads finished. Filesystem lookup/read and
VM-boundary pixel transfer remain useful next profiling targets.

Host tests at opt-level=1 measured 4K channel expansion at about 9.3 ms before
and 4.4–5.0 ms after. A separate byte-verified comparison of the vendored
nightly Paeth SIMD path was slower on these inputs; `png/unstable` remains off.
No compiler profile change was retained.

## Reproduction

Generate the deterministic corpus with Pillow and NumPy installed:

```
python3 tools/png_path_fixtures.py bld/png-path/assets
python3 tools/benchmark_png_path.py bld/png-path/assets
python3 tools/test_png_row_expand.py
python3 tools/test_png_decoder.py
python3 tools/test_vmedia_image_capacity.py
```

The corpus contains all five PNG filters at Full HD and 4K, in RGB and RGBA,
with SHA-256 expectations for decoded pixels. These are labeled synthetic
screens, not a photographic benchmark. The mixed-format Solara page is
`tools/fixtures/png-path.html`; generation copies it to the output parent as
`index.html` and adds the existing JPEG logo to the assets directory.

Use `trueos-doc topic trueosfs-http` to discover current filesystem roots and
upload the generated PNGs into physical `apps/common/images`. Verify all
expected filenames after upload.
The validated rig has all 20 PNGs there. The helper does not contact the rig.

Serve the output parent for the Solara test page, then use `surf` with that
HTTP URL. Build `img` and `solara` through `cargo bp` in TRUEOS-Blueprints;
`make iso` embeds the builtin archives in app.db. Uploading a root `.bp` file
does not replace an already seeded app.db entry.

## Validation and artifacts

- Shared decoder tests compare pixels with Pillow for RGB/RGBA, grayscale,
  grayscale-alpha, packed 2-bit palettes, palette and color-key transparency,
  16-bit grayscale, Adam7 RGB/RGBA, and malformed/truncated data.
- Row expansion tests exercise partial rows, odd widths, empty ranges and
  insufficient source lengths; gallery tests cover wrapping, singleton and
  empty inputs.
- All 43 Solara host tests pass, including a mixed PNG/JPEG image layout and
  cropping regression and transparent-edge/odd-size/budget upload tests. Both Blueprint target builds pass.
- Kernel packaging, physical reset, fresh PXE artifact hashes and the new boot
  marker were verified. `trueos-doc`'s 17 tests pass after the img help update.

Local run evidence is under `bld/png-path/`: host benchmark logs, decoder and
capacity test logs, before/after kernel captures, terminal transcripts, deploy
receipts, fixture manifests, and screenshots. Screenshots include the rig's
existing foreground Cubes and Lilly overlays.

The final mixed page was verified twice on the rig: three PNGs (including the
4K RGBA source) and one JPEG all reached `image-ready`, with no allocation or
native-window errors. `bld/png-path/solara-png-jpeg.png` shows all four in the
same browser window, including purple visible through the transparent PNG
regions. `img-fullhd.png` and `img-4k.png` show the viewer's physical post-blend
output; the latter follows backward wrapping from item 1 to item 20.

The final diagnostic page is served from the development host on port 8091
for this session. Restart it with:

```
python3 -m http.server 8091 --bind 0.0.0.0 --directory bld/png-path
```
