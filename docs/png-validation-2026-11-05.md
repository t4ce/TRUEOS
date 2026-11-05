# PNG path validation — 2026-11-05

The temporary PNG smoke app, corpus, runners and kernel instrumentation were
removed after validation. Production PNG decoding and the image/download fixes
remain. These are historical measurements, not instructions for a retained test.

Test set: 30 cases, comprising 27 valid PNGs and three expected rejections.
Coverage included all 15 legal color/bit-depth combinations, all five filters,
Adam7, palette transparency, straight alpha, fragmented IDAT, a synthetic
256×256 map tile, 200×183 globe, 1024×768 image, and the actual Wikipedia PNG.
Bad IHDR/IDAT CRCs failed decoding with `-2`; truncation failed preflight with
`-1`. Rejected cases created no sprite or GPU frame.

The actual Wikipedia asset was 15,829 bytes, indexed color with transparency,
200×183 pixels, resized to 64×58 for this test. Encoded SHA-256:
`75038605ff9e35cc393e0ed8200069601c889100607cde67d2af68b9eb88e5d4`.

Host validation used the production `hull_image_policy::preflight`,
`media_service::decode_ui4_bounded`, PNG decoder, `contained_extent` and image
service resize function. All 30 cases passed, with two warmups and ten measured
iterations per case. This exercised CPU processing without network or GPU.

Bare-metal validation used frozen bytes served over loopback and the normal
`hull_image_reset/request/status/release` UI4 APIs
(`trueos_cabi_ui4_hull_image_*_v1`). Images passed through the production image
worker, bounded decode, resize, `prepare_rgba` and `install_prepared`. The app
used `Frame::open`, `begin_sprite_frame`, `draw_sprite_quads`, `publish_tracked`
and `was_presented(serial)` to require physical SURFLIVE presentation.

All 30 native cases passed: one warmup and five measured iterations each,
180 samples in 31.256 seconds. The 162 valid samples produced 162 resident-scene
GPU frames. Published-frame read leases verified all 76,800 pixels per frame:
12,441,600 pixels total, zero mismatches, maximum channel error zero. Alpha was
composited over `[24, 40, 56, 255]`. No mocked GPU or CPU fallback was accepted.

Times below are milliseconds, median / nearest-rank p95, excluding warmups.
Native statistics contain five measured samples per case; p95 is their maximum.

| Asset | Host decode | Native decode | Native resize | Native prepare | Native install |
| --- | ---: | ---: | ---: | ---: | ---: |
| Wikipedia 200×183 → 64×58 | 0.105 / 0.123 | 0.548 / 0.583 | 0.021 / 0.022 | 0.043 / 0.044 | 0.010 / 0.018 |
| Map tile 256×256 → 128×128 | 0.231 / 0.248 | 0.571 / 0.597 | 0.071 / 0.075 | 0.175 / 0.180 | 0.011 / 0.015 |
| Image 1024×768 → 320×240 | 1.613 / 1.756 | 5.017 / 5.654 | 0.364 / 0.367 | 0.850 / 0.861 | 0.011 / 0.014 |

For Wikipedia, native fetch was 225.001 / 250.000 ms and app request through
physical presentation was 249.890 / 266.872 ms. Loopback fetch includes worker
scheduling and completion polling; observed HTTP work alone was about 2–4 ms.
Publication was 1.213 / 1.343 ms, including synchronous diagnostic readback and
logging; frame verification alone was 1.165 / 1.288 ms. These are instrumented
measurements, not isolated GPU upload or clean publication benchmarks. Host
timing used `Instant`; native timing used calibrated TSC cycles. Pixel-hash
verification was timed separately from decode and resize.

This established PNG correctness for the tested bytes and native pipeline.
Loopback testing did not establish external HTTPS reliability. A later actual
Hull Wikipedia page load fetched the same globe with HTTP 200 and rendered it;
the earlier HTTP 403 cause remained unproven, and one other asset still returned
429 after bounded retries. Missing language-ring text was traced separately to
the initial DOM snapshot retaining the page's temporary localization hiding.

Original local evidence: `/tmp/png-smoke-host-20261105/report.json`,
`/tmp/png-smoke-native-wikipedia-20261105/{summary,report}.json`, and
`/tmp/download-path-final-20261105/summary.json`.
Native run: `1793898223000000000-76533139577`.
Kernel archive SHA-256:
`191a5ca9e1e60f519b19a6c1fa9e8b1838a7b8951211c37f9d78828c333e02f6`.
