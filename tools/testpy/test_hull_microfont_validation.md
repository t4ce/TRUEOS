Validation record, 22 October 2026

The integrated kernel, Hull buildin, and ISO built successfully with Cargo's
development profile (`opt-level=1`). The packaged-image verifier confirmed
the ISO's kernel matches the staged runtime ELF and verified the linked GPU
copy artifacts. The final instrumented build transcript is
`/tmp/hull-bounds-iso-build.log`.

| Artifact | SHA-256 |
| --- | --- |
| `bld/TRUEOS.elf` | `7ac81d99759a8124945bea1187388e69b64c2183212515d2d3ba8aff3e8d09d2` |
| `bld/trueos.iso` | `62e087ba469c698923230339bbddf990bb3730cb0090ef49c9f10de697f5605c` |
| `../TRUEOS-Blueprints/dist/hull.bp` | `1c164afe8039f3edc1b9ca2720210b52fd779b6f8e399d3d3945de03f1e41945` |

Twelve host cache tests and eight bridge tests pass, including deterministic
reuse of a request's retained masks during competing eviction and clipped
paint bounds for synchronization.

The physical rig answered HTTP and authenticated Shell3 SSH on port 22 with
the existing identity. Its mounted TRUEOSFS roots were discovered as 1 and 2.
The current Shell3 `pic` menu successfully generated a new 2560 × 1440 UI4
postblend screenshot, downloaded to `/tmp/hull-before.png`. This baseline
shows the autostarted font demo; it does not show an older Hull instance.

After the user explicitly approved deployment and reset, the ESP32 reset
target verified fresh PXE reads and the exact hashes above. The new boot's
capture is `bld/baremetal-logs/trueos-baremetal.0.log`, with a copied reset
receipt and capture header in each result directory.

The final three-Hull test passed. Launch requests spanned 200.39 ms, and each
independent SSH session reported its layout frame. The first page generated
94 glyph masks; the next two instances reused the shared masks. The repeated
page registered 157 cache hits and zero new rasterizations. Resident R8 masks
occupied 18,236 bytes of a 58,982,400-byte budget; no eviction or duplicate
concurrent rasterization occurred.

| Final TSC measurements (µs) | Prepare | Raster | Compose | Cache sync | Full overlay |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cold example.com | 978 | 709 | 786 | 446 | 3,977 |
| Second Hull, shared cache | 124 | 0 | 603 | 447 | 2,939 |
| Third Hull, shared cache | 123 | 0 | 603 | 434 | 2,909 |
| Identical warm reload | 181 | 0 | 690 | 448 | 3,108 |
| docs.rs in retained first Hull | 3,547 | 2,626 | 6,621 | 7,807 | 27,751 |

These elapsed measurements use the calibrated fenced TSC. Every complete
overlay record returned status zero. The final measurement waits for the
full overlay record, including both frame cache synchronization steps.

Final default evidence is in `bld/hull-microfont-20261022-default-bounds/`: `summary.json`,
`font.log`, three SSH transcripts, the saved `layout.svg`, and a fresh
2560 × 1440 `postblend-1.png`. Visual inspection confirms dark microfont text
at the SVG positions over the existing pink boxes. A pixel comparison of
the static text crop `(1040, 530, 1520, 660)` against the previous full-frame
synchronization image found zero differing pixels.

The retained first Hull then loaded `https://docs.rs/`, after stopping only
the other two test Hulls so its frame was visible. The final fetched page
produced 231 text runs, 1,706 visible glyphs, and 282 new masks, with text
sizes 11, 12.8, 16, and 48 px. Resident masks reached 116,466 bytes with no
eviction. Its fresh SVG, screenshot, complete log, and summary are in
`bld/hull-microfont-20261022-docs-bounds/`. Both captures retain the existing
pink fills and text follows the saved SVG geometry. The earlier docs.rs
fetch exercised two missing glyphs; its screenshot contains red fallback
pixels within the Hull bounds and is preserved in the unsuffixed directory.

The measured small-page optimization reduced synchronization from the full
3,686,400-byte RGBA frame to the clipped union of glyph paint bounds. The
two passes requested 373,920 logical bytes across 228 rows, compared with
7,372,800 bytes for two full-frame passes. Cache-line rounding may touch a
few additional edge bytes.

| Identical warm example.com | Full-frame sync | Paint-bounds sync |
| --- | ---: | ---: |
| Cache synchronization | 7,161 µs | 448 µs |
| Complete text overlay | 9,898 µs | 3,108 µs |

The docs.rs text union covers most of the frame, so its synchronization
still requests 7,088,064 logical bytes and takes about 7.8 ms. Its live
release list changes between fetches, so the docs.rs passes are correctness
and size coverage rather than a controlled performance comparison. The
peak resident R8 cache is 116,466 bytes (0.111 MiB), well below the 56.25 MiB
display-derived budget. No AP-pool scaling or BCS0 text acceleration is
claimed. Earlier results remain in the unsuffixed and `-tsc` directories.

The runtime test is `test_hull_microfont_baremetal.py`; workflow and metric
interpretation are in `test_hull_microfont_baremetal.md`. It verifies the
kernel/ISO hashes against both the reset receipt and selected capture, starts
three Hulls close together, checks each instance's layout frame, measures an
identical warm reload, and requires a newly saved screenshot.
