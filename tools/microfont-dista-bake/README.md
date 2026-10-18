# Dista GPU body stamp

The UI4 triangle preview stamps `ABC123/Yy%` twice: Meso above Lean, each glyph
96 × 176 px. The vertex shader in
[`vendor/microfont/shaders/dista_stamp.vert`](../../vendor/microfont/shaders/dista_stamp.vert)
constructs the body from seed descriptors. Rust prepares indexed task slots;
GLSL reconstructs the links and generates the positions. The ordinary triangle
pipeline rasterizes them. The CPU reconstruction remains available as fallback.

## Why the sampled field can become simple geometry

The CPU field takes the **maximum**, rather than the sum, of Gaussian site and
straight-link influences. Thresholding a maximum is a union. With influence width
`σ = 0.20` and body threshold `T`, each site is a disk of radius
`r = σ sqrt(-2 ln T)`. A link of weight `w > T` is a capsule of radius
`σ sqrt(-2 ln(T/w))`; its endpoint caps are already covered by the site disks.
Therefore the complete body is a union of site disks and link rectangles.

The compact Gaussian taper starts at `3σ`, where its value is approximately
`0.0111`, below both body thresholds (`0.22` and `0.63`). It cannot affect these
outlines. The radii are approximately `0.3480376684` source units for Meso and
`0.1922572151` for Lean. This is the ideal envelope; the CPU's sampled mesh has
its own interpolation error.

All sites lie on an integer lattice before the common half-unit translation.
Reach `1.76` admits squared link lengths `1, 2`; reach `2.76` additionally admits
`4, 5`. Even the longest admitted link leaves more than the `0.30` weight fade
interval, so all current admitted links have weight exactly one. GPU link
pruning uses the same closed diameter disc as the CPU. Boundary ties matter:
a length-two horizontal link is also blocked by the midpoint's neighbors one
row above and below, outside the endpoints' bounding rectangle.

## Quality and triangle counts

A regular inscribed disk fan with `N` segments has maximum radial error
`r_px (1 - cos(π/N))`. Select `N` by solving this bound and round upward to a
multiple of four, with at least eight segments. This controls the curved
boundary; straight rectangle sides need no subdivisions. At the demo's scale
of 16 px per source unit:

| Maximum boundary error | Meso segments | Lean segments | Surviving triangles, both rows | Submitted triangles, both rows |
| --- | ---: | ---: | ---: | ---: |
| 0.25 px | 12 | 8 | 3,428 | 5,496 |
| 0.125 px | 16 | 12 | 4,580 | 6,648 |
| 0.0625 px | 24 | 16 | 6,308 | 8,376 |

The demo contains 144 sites and 137 admitted links per row. Surviving geometry
costs `sites × N + 2 × links` triangles. Submission reserves four forward link
candidates per ordinary site and ten per long-reach site. GLSL collapses absent
or pruned links to degenerate triangles. Submitted count includes that work;
surviving count excludes it. Neither count describes visible pixel area or
vertex shader invocation count. Indexed fan and rectangle vertices are shared.

The one-shot demo selects its preset with `QUALITY` in
[`microfont_triangle_preview.rs`](../../src/r/services/microfont_triangle_preview.rs).
It currently uses Fine. The text is fixed; keyboard text no longer selects a glyph.
The log reports the preset, segment counts, submitted and surviving triangles,
CPU task preparation time, host GPU wait time, and an optional hardware timestamp
duration (`ticks`, `hz`, `ns`). Seed/shape edits made after the last shader bake
select the CPU fallback until a matching shader is baked.

The triangles overlap at joins. Their opaque union gives the body and its
outline without extracting or welding a contour. Applying opacity separately
to every overlapping primitive would accumulate coverage. A future union mask
or stencil resolve can apply color and transparency once; that is outside this
stamp's scope. The generated mesh is not a manifold outline mesh for export.

## Reproduce the checks

```sh
python3 tools/microfont-dista-bake/bake.py --source-only --work-dir /tmp/dista-bake
python3 tools/microfont-dista-bake/audit.py --text 'ABC123/Yy%' --error-px 0.125
python3 tools/microfont-dista-bake/audit.py --all-basic --raster-step 0.25 > /tmp/dista-audit.json
python3 tools/microfont-dista-bake/test_audit.py
python3 tools/microfont-dista-bake/host_validate.py --atlas --work-dir /tmp/dista-atlas-host
```

`audit.py` independently reads the Rust Basic table, `z(bits, shift)` values,
and reconstruction overrides, including `A`, `/`, `%`, and `y`. Its slow graph
oracle checks every pair against every third site. It compares that graph with
a reconstruction of the GLSL candidate offsets and integer pruning search.
Optional raster sampling compares the original compact Gaussian field against
the analytic union in both weights. This is host numerical evidence, not proof
of shader execution on the target GPU; the polygon error is checked separately
through its sagitta bound.

`--source-only` validates/prepares shader source without replacing the native
artifact. A native bake requires the compatible Mesa build described by
`bake.py --help`.

`host_validate.py --atlas` executes the production GLSL on Lavapipe for all
95 printable Basic slots, U+0020 through U+007E. Each weight occupies a
1536 × 1056 RGBA8 atlas, 16 columns and six rows, with the same 96 × 176 px
glyph size as the demo. Space is empty; the final grid cell is unused. Both
the loop and direct blocker variants render, and their complete RGBA buffers
must match byte for byte. An independent envelope oracle checks pixel centers
every four pixels by default and permits only the prescribed disk chord error.
Use `--oracle-step 1` for the full pixel oracle; `--quality draft|fine|high`
selects the preset. PNG previews, raw RGBA buffers, logs and `validation.json`
are written beneath the selected work directory. This is software Vulkan
correctness evidence, not a measurement of Intel iGPU execution.

Each weight's tightly packed RGBA8 atlas contains 6,488,064 bytes (6.1875 MiB).
The printable set has 1,302 occupied sites and 1,243 accepted links per weight:

| Quality | Meso submitted / active triangles | Lean submitted / active triangles |
| --- | ---: | ---: |
| Draft | 26,196 / 18,110 | 20,988 / 12,902 |
| Fine | 31,404 / 23,318 | 26,196 / 18,110 |
| High | 41,820 / 33,734 | 31,404 / 23,318 |

The atlas descriptor uses the existing Float3 stream. Packed stamp bits 0..6
hold the codepoint, 7..10 the column, 11 the weight, 12..14 the atlas grid row,
15 selects atlas coordinates, and 16 selects direct blocker tests. Ordinary
demo descriptors leave the new bits clear. No CPU-generated positions or
precomputed triangle outlines are uploaded.

## Timing

CPU task construction, upload/submission, and waiting for retirement measure
different work. A wall-clock interval around synchronous rendering includes
queueing and CPU waits, and must not be labeled GPU construction time. GPU
timestamps around the draw measure the combined procedural vertex work and
triangle rasterization. Separating those costs requires additional timestamped
passes or a dedicated generation buffer; the single vertex draw does not
provide an isolated generation duration.

The physical rig's existing Fine demo measured **714.323 µs** for the combined
procedural vertex and triangle raster draw: 13,715 hardware ticks at 19.2 MHz.
Its 960 × 352 target held the two specimen rows, with 6,648 submitted triangles
and 4,580 surviving union triangles. The timestamp excludes clear and final
render-target writeback. It is a single observation, not an atlas benchmark.
The same log reports 8 ms render/wait time and 12 ms complete stamp time;
the legacy host clock resolves only 1 ms, so a reported CPU preparation time
of zero means less than one tick. Evidence is at
`bld/baremetal-logs/trueos-baremetal.0.log`, capture run
`90e02377e58f33311b60872997f6f85d`, started 2026-10-18 21:08:51 CEST,
shader SHA-256 `a0b873492831bdc20c2b48ca257ff60b9fdd80fbec1a2beb8a0631d877ab95a7`.
The shader artifact changes when new experiments are baked.
The extracted values and capture identity are retained in
[`baseline-demo.json`](baseline-demo.json) because the normal rig logs rotate.

## Background atlas experiment

The ordinary `make iso` build includes the benchmark. No Cargo command or extra
build flags are needed. Nothing runs at startup. In Shell3, open the GPU
diagnostics menu and choose `dista-bench 7`, or explicitly request it:

```text
gpu dista-bench 7
gpu dista-bench 15
```

The optional argument selects 1..15 warm samples per configuration; the default
is seven. One first submission precedes them. The command queues background
work, creates no window, and rejects concurrent benchmark requests. Twenty-four
configurations cover both weights, three qualities, the two blocker variants,
and RGBA8 versus R8 output. Each produces the complete Basic atlas through the
same resident triangle renderer, with the same procedural vertex shader,
white ink, zero clear, four GPU timestamps and PAT3/UC target aliases. R8 stores
coverage directly, without rendering RGBA and converting it. There is no new
scanout format or font-consumer integration in this experiment.

Each configuration reuses its uploaded descriptor mesh and destination across
warm samples, then releases both. Every warm sample rebuilds every glyph;
`stage: atlas-rebuild` deliberately distinguishes this from copying a retained
atlas. Format order reverses for every configuration in successive matrices.
There is no pixel readback, screenshot, display publication, storage write, or
CPU triangulation in the timed bursts. After each complete burst, a cold CPU
snapshot is taken before releasing the retired target. For each format pair,
the diagnostic compares every R8 byte with RGBA red coverage and checks all
RGBA channels agree, every coverage value is zero or 255, all 94 nonblank glyphs
contain ink, and the space and final unused cell remain blank. This comparison
is logged separately as `kind: validation`; its allocation, cache flushes and
pixel reads are excluded from every timing interval.

An atlas is 1536×1056 pixels. Its R8 target has a 1536-byte row pitch and occupies
1,622,016 bytes, versus a 6144-byte pitch and 6,488,064 bytes for RGBA8: 75% less
intermediate storage. R8 bytes represent coverage; color and opacity can be
applied by an existing later composition operation. No copy, color application,
composition or CPU/BCS0 sink operation is included in this matrix.
Setup records also report cleanup results. Uncertain GPU completion retains
the resident mesh and destination lease; missing hardware timestamps abort the
matrix after safely releasing a completed target and logging the unavailable
sample. The command never reports an untimed atlas as a completed timing run.

The two blocker implementations are a controlled geometry-preserving experiment:
`loop` searches the small endpoint neighborhood; `direct` tests the exact few
integer sites in each candidate's closed diameter disk. Direct tests include all
boundary ties and leave triangle counts and the ideal body unchanged. Their
starting order alternates across quality presets to expose order effects;
results still share the live GPU with other work and are not isolated GPU time.

The JSON log records distinguish these timing scopes:

| Field | Scope |
| --- | --- |
| `first_target_ready_ns` | First case start through completed output in its recorded format, including task preparation, upload, allocation and render; only sample zero. |
| `wall_ns` | One synchronous render call through completed output, including host preparation, queueing and waiting; setup excluded. |
| `gpu_draw_ns` | GPU procedural vertices plus fixed-function triangle rasterization after clear; the two stages overlap. |
| `gpu_target_ready_ns` | GPU interval from before clear through draw and ordered render-target/tile writeback, in the recorded output format. |
| `gpu_clear_setup_ns` | Before-clear through draw-begin: full-surface clear and its secondary state setup. |
| `gpu_writeback_ns` | Draw-end through target-ready: ordered target writeback and marker overhead. |
| `task_prepare_ns`, `mesh_upload_ns`, `allocation_ns` | Setup stages, reported in the configuration's `setup` record. |
| `output_format`, `destination_bytes`, `destination_pitch_bytes` | Actual target format, backing size and byte pitch. |
| `cpu_prepare_us`, `gpu_wait_us` | Existing renderer counters, still quantized to the legacy 1 ms host clock. |

The new host `*_ns` intervals use ordered TSC reads and the calibrated TSC
frequency recorded in `begin`; their units do not imply nanosecond accuracy.
GPU intervals use the hardware timestamp frequency. Missing timestamp values
remain null. GPU target-ready stops before the release-cookie retirement that
the host waits for; wall/end-to-end timings include that retirement. None of
these timings include scanout or display composition. Instrumentation adds
ordered timestamp writes and flush boundaries, so compare like captures.
The clear/setup and writeback breakdown uses the same four existing markers;
it does not add another GPU pass or timestamp. Together with the draw interval
they partition the complete GPU interval, subject to timestamp rounding.

Capture the normal rig log and summarize it locally:

```sh
python3 tools/microfont-dista-bake/analyze_bench.py bld/baremetal-logs/LatestOfThree.logs
python3 tools/microfont-dista-bake/analyze_bench.py --json /tmp/dista-bench.log > /tmp/dista-summary.json
```

The analyzer extracts records after `microfont-dista-bench: ` and groups samples
by stage, output format, variant, quality and weight. It keeps sample zero separate and reports warm
median and nearest-rank p95, with counts for each available metric. With only
seven warm samples, p95 is the maximum observed sample. The JSON report retains
`begin`, `setup`, `done` and error records for provenance. Keep captures from
different builds separate. A first submission for a configuration does not
imply a cold GPU or empty caches. The benchmark must be deployed and
run on the rig before making performance claims for the atlas or direct tests.
The analyzer rejects captures containing different recorded shader hashes.
Older captures without `output_format` or `stage` are treated as RGBA8 atlas
rebuilds; their `first_rgba_ready_ns` and `gpu_rgba_ready_ns` names are accepted
as aliases for the new format-independent target-ready metrics. Their original
evidence remains valid for the older colored-on-white configuration; compare
R8 against the matched white-on-zero RGBA cases from the same new build.

The physical rig results from three complete matrices (288 renders) are in
[`results/2026-10-18-atlas.md`](results/2026-10-18-atlas.md), including the
clear/setup versus final writeback breakdown and verified boot identities.

Further useful matched experiments are indexed vertex reuse, removal of empty
candidate slots, smaller glyph scale, and one larger atlas draw versus grouped
draws. Measure upload bytes, submitted/active triangles and complete-buffer GPU
time together: fewer triangles need not reduce time if descriptor/shader cost,
render-target clearing, or host submission dominates. Welding a contour or
splitting generation into its own reusable GPU buffer changes the pipeline and
needs a separate correctness and timing comparison.
