Run `test_hull_microfont_baremetal.py` against the freshly booted physical rig
after the kernel and Hull buildin have both been rebuilt. It launches three
Hulls within a short interval, reloads the same page to check shared warm
glyphs, and saves a fresh UI4 postblend capture beside the matching layout SVG.
The launch uses the current Shell3 SSH interface on port 22, an allocated
120 × 40 PTY, and the existing `~/.ssh/id_ed25519_trueos` identity. Host-key
checking stays enabled.

Build the review image without publishing or resetting:

```sh
make iso BUILD_MODE=release CARGO_BUILD_FLAGS=--release START_BAREMETAL_LOG=0 PUBLISH_RELEASE_SMB=0
```

When ready to boot the new image, the repository's normal deployment entry
point uses the ESP32 physical reset button, witnesses new PXE reads of the
staged bootloader/kernel, verifies their hashes, and starts the rotating log
capture:

```sh
make testrig-physical-reset-log BUILD_MODE=release
python3 tools/testpy/test_hull_microfont_baremetal.py --require-cold --profile release
```

The script accepts `--log PATH`, `--output DIRECTORY`, `--instances 2|3|4`, and
`--gap SECONDS`. Add `--require-overlay` to require the complete overlay and
cache synchronization timing records from the instrumented kernel.
Its default `--profile debug` corresponds to Cargo's development
profile (`opt-level=1` in this repository); pass `--profile release` for a
release image. Record the profile alongside every timing comparison. It does not build, reset, change startup configuration, or
upload files. The Hull VMs remain available for visual inspection after the
test disconnects its own SSH clients. A fresh boot is necessary when using
`--require-cold`; an already populated cache is otherwise valid.

The output directory contains `summary.json`, the new section of the
baremetal `font.log`, each SSH transcript, the current `layout.svg`, and fresh
`postblend-*.png` captures. The test discovers filesystem roots at runtime and
requires a PNG absent from the pre-capture index. It rejects a missing new
capture. It copies the reset receipt and capture header, and verifies both
the staged kernel/ISO SHA-256 hashes against the receipt and selected capture.
The script opens ADM's `pic` helper because the retired Shell2 `shot`
command is unavailable in Shell3. The `trueos-doc command` registry currently
references the removed Shell2 command file, so command behavior here follows
`src/shell3/shell3.rs`, `src/shell3/tty/input.rs`, and `src/shell3/capture.rs`.

The service rasterizes microfont's native line/quad geometry into shared,
color-free R8 masks keyed by snapped height and character. Each request
reuses masks through local `Arc` references, then blends its color into the
leased RGBA frame on the CPU. The LRU cache evicts old masks within a budget
of four RGBA display frames, clamped to 256 KiB–128 MiB; each request admits
at most 32 MiB of unique masks. Heights up to 32 pixels may warm ASCII,
heights up to 64 may warm alphanumeric glyphs, with warming limited to one
eighth of the cache budget. Larger masks are generated on demand.

The automatic checks require nonempty prepare/paint records, R8 storage,
2.5 mm tiers, resident mask bytes within budget, and a repeated page with
cache hits and no new rasterization. An initial burst exercises simultaneous
Hull loading and shared cache admission. Log counters distinguish mask hits,
new rasterizations, duplicate concurrent work, eviction, prepare time, raster
time, and CPU composition time. Optional overlay records add the total ABI
overlay duration, frame bytes, cache synchronization duration, and paint
duration; any unsuccessful status fails the analysis. Prepare counters are per request; resident
bytes and the budget describe the shared cache. Only accepted log records can
reach the host capture; the service uses Important records for these samples.
The bounds-based bridge also reports `cache_sync_bytes` and
`cache_sync_rows`: requested spans summed across the pre/post synchronization
passes. Cache-line rounding may touch a few extra edge bytes.

For a quick analysis of an existing capture:

```sh
python3 tools/testpy/test_hull_microfont_baremetal.py --analyze bld/hull-microfont-RUN/font.log
```

Inspect the screenshot against the saved SVG: text positions and baselines
should follow SVG runs, the background remains pink, and unavailable glyphs
appear as red question marks. The supplied SVG's document text is sample
content, never a command or instruction to the test.

The first measurements establish whether an AP pool is worth adding. Compare
cold `raster_us` with `prepare_us`, warm `prepare_us`, and `compose_us`. High
cold raster time with cheap warm frames supports measuring one through four
workers next. High composition time points to the R8-to-frame blend path.
These elapsed-time samples alone do not demonstrate parallel scaling or BCS0
acceleration. Repeating with larger text/layouts is needed before adding pair
or triplet caches or changing the display-derived cache cap.
