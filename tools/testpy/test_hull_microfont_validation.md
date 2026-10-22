Validation record, 22 October 2026

The integrated kernel, Hull buildin, and ISO built successfully with Cargo's
development profile (`opt-level=1`). The packaged-image verifier confirmed
the ISO's kernel matches the staged runtime ELF and verified the linked GPU
copy artifacts. The final instrumented build transcript is
`/tmp/hull-tsc-iso-build.log`; the initial build transcript is
`/tmp/hull-final-iso-build.log`.

| Artifact | SHA-256 |
| --- | --- |
| `bld/TRUEOS.elf` | `2b2ed4c5504994efa2c977c2eb726dbc0df3a70fe8a82ce28407aca1a7670e7d` |
| `bld/trueos.iso` | `1ebfd4dac8466c02adff7195dbd4c8d775fd70dbe2f6b8c69e1bad55743b5dd4` |
| `../TRUEOS-Blueprints/dist/hull.bp` | `1c164afe8039f3edc1b9ca2720210b52fd779b6f8e399d3d3945de03f1e41945` |

Ten host cache tests pass, including deterministic reuse of a request's
retained masks while another request evicts their shared-cache entries.

The physical rig answered HTTP and authenticated Shell3 SSH on port 22 with
the existing identity. Its mounted TRUEOSFS roots were discovered as 1 and 2.
The current Shell3 `pic` menu successfully generated a new 2560 × 1440 UI4
postblend screenshot, downloaded to `/tmp/hull-before.png`. This baseline
shows the autostarted font demo; it does not show an older Hull instance.

After the user explicitly approved deployment and reset, the ESP32 reset
target verified fresh PXE reads and the exact hashes above. The new boot's
capture is `bld/baremetal-logs/trueos-baremetal.2.log`, with a copied reset
receipt and capture header in each result directory.

The final three-Hull test passed. Launch requests spanned 200.16 ms, and each
independent SSH session reported its layout frame. The first page generated
94 glyph masks; the next two instances reused the shared masks. The repeated
page registered 157 cache hits and zero new rasterizations. Resident R8 masks
occupied 18,236 bytes of a 58,982,400-byte budget; no eviction or duplicate
concurrent rasterization occurred.

| Final TSC measurements (µs) | Prepare | Raster | Compose | Cache sync | Full overlay |
| --- | ---: | ---: | ---: | ---: | ---: |
| Cold example.com | 1,578 | 1,199 | 816 | 7,156 | 11,415 |
| Second Hull, shared cache | 124 | 0 | 664 | 7,157 | 9,694 |
| Third Hull, shared cache | 215 | 0 | 662 | 6,974 | 9,645 |
| Identical warm reload | 129 | 0 | 813 | 7,161 | 9,898 |
| docs.rs in retained first Hull | 3,592 | 2,732 | 5,137 | 7,920 | 26,416 |

These elapsed measurements use the calibrated fenced TSC. Every complete
overlay record returned status zero. The initial experiment exposed 1 ms
clock quantization; that first image's artifacts remain preserved in the
directories without the `-tsc` suffix. The final measurement waits for the
full overlay record, including both frame cache synchronization steps.

Final default evidence is in `bld/hull-microfont-20261022-default-tsc/`: `summary.json`,
`font.log`, three SSH transcripts, the saved `layout.svg`, and a fresh
2560 × 1440 `postblend-1.png`. Visual inspection confirms dark microfont text
at the SVG positions over the existing pink boxes.

The retained first Hull then loaded `https://docs.rs/`, after stopping only
the other two test Hulls so its frame was visible. The final fetched page
produced 235 text runs, 1,285 visible glyphs, and 282 new masks, with text
sizes 11, 12.8, 16, and 48 px. Resident masks reached 116,466 bytes with no
eviction. Its fresh SVG, screenshot, complete log, and summary are in
`bld/hull-microfont-20261022-docs-tsc/`. Both captures retain the existing
pink fills and text follows the saved SVG geometry. The earlier docs.rs
fetch exercised two missing glyphs; its screenshot contains red fallback
pixels within the Hull bounds and is preserved in the unsuffixed directory.

The small page's roughly 7 ms cache synchronization cost exceeds its cold
rasterization cost and persists on warm frames. Full-frame synchronization
currently touches a 3,686,400-byte RGBA frame. These results support measuring
smaller synchronization regions or a composition path before adding raster
workers. No AP-pool scaling or BCS0 text acceleration is claimed.

The runtime test is `test_hull_microfont_baremetal.py`; workflow and metric
interpretation are in `test_hull_microfont_baremetal.md`. It verifies the
kernel/ISO hashes against both the reset receipt and selected capture, starts
three Hulls close together, checks each instance's layout frame, measures an
identical warm reload, and requires a newly saved screenshot.
