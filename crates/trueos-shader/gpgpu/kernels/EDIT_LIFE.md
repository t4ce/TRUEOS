# Edit Conway background

`edit_life_rgba8.clcpp` is the offline GPU counterpart of
`src/ui4/edit_life_model.rs`. It uses the existing C++ for OpenCL architecture.
Its generated Zebin, SPIR-V, manifest and ABI contract live in
`artifacts/adls/cpp/edit_life_rgba8.*`. The native producer is
`src/intel/gpgpu/operations/edit_life.rs`; the bounded CPU implementation is
the reference for outputs without a compatible native backend. A native
initialization failure on supported hardware stops the effect rather than
silently selecting the CPU implementation.

The source has one SIMD16 entry point and six modes: direct bitmap seed/additive
input (0), Conway generation (1), display expansion (2), input comparison-baseline
copy (3), canonical text/background resolution (4), and opaque chrome copy (5).
The comment at the top of the source specifies every buffer and uniform.
Life cells occupy two u32 words, separating binary occupancy from RGB, trail
strength and birth. Animation stays at ten generations per second, with fixed
.905 trail decay (the demo's .75 persistence).

Edit's Shader preference also selects MicroFont's bitmap face inside the page
viewport. Turning it off restores the line/quad raster face. Menus, status,
gutters and overlay exclusions keep their existing raster face, including when
the page animation is paused. Font changes invalidate both terminal buffers,
even for metadata-only updates; raster-only pan/strip caches cannot overwrite
bitmap page pixels. Layout, cursor geometry and the physical cell size do not
change. The font service warms a retained R8 bitmap atlas alongside the existing
line/quad atlas at that same cell size before publishing font readiness.
MicroFont's runtime `hex` feature remains disabled.

Edit's right-aligned Font Size picker selects 50% through 400% in eight steps,
with 100% as the default. Its trailing header space is reserved before the
ordinary menus; narrow headers show only the percentage. The picker scrolls
in short windows and returns focus to the page after selection or Escape.
Opening it uses the ordinary popup pause boundary.

Font size is local to the selected terminal lease. A bounded
`OSC 777;font-scale;1;N` request selects native scale 1 through 8, with scale 2
equal to the existing default. Requests commit with synchronized terminal
frames, survive resize and clear on reset or alternate-screen exit. Shell3
uses its existing cell-size, grid-resize and two-buffer invalidation path;
the line/quad and canonical bitmap sources are reused at every size. Menu and
status text retain the raster face while the enabled editor page uses hex.
Pointer and file-drop coordinates follow the size successfully presented to
that window, including while a newer requested size is pending.

`tools/edit_hex_font.py` generates the 724 shared full 66-bit masks from
`vendor/microfont/data/line-v1`, including ten lowercase compositions assembled
from validated canonical bitmap fragments. The Rust table and OpenCL sampler
share glyph IDs and nearest-neighbour mapping of each 6x11 mask to the physical
cell. Unknown glyphs use the same red question-mark replacement in both paths.
The complete MIT notice is retained in the generated files. Regenerate with
`python3 tools/edit_hex_font.py`, or use `--check` to verify freshness.

Capture now publishes only viewport-local records `[glyph, fg_rgba, bg_rgba,
flags]`: 16 bytes per terminal cell. Flags preserve opaque selection/caret
backgrounds and underlines. No per-pixel R8 coverage, color image, full RGBA
snapshot, glyph-atlas traversal or source-frame CPU read is needed. The GPU
uploads the shared 11,584-byte mask table once per effect and keeps an 8-byte
comparison record per terminal cell. Both seeding and final text composition
sample that exact same font data. The CPU reference samples the shared masks
and retains compact comparison records too.

The simulation and retained RGBA image are exactly `columns*6` by `rows*11`:
one font bit, one Conway cell, one resolved pixel. Seeding reads that bit
directly, without bilinear taps or sampling a rasterized page. Resolution does
the glyph composition, color, trail and neighboring-cell glow once per canonical
pixel. Its RGBA image stays cached until input, viewport or Life state changes.
Several generations require only one final resolution pass.

The display pass launches one lane per canonical texel, loads its RGBA once,
and broadcasts it to its complete screen rectangle. On each axis native pixel
`k` owns `[ceil(k*cell_size/native_size), ceil((k+1)*cell_size/native_size))`
within its character cell, where `native_size` is 6 or 11. This is the exact
inverse of the CPU's floor-based nearest-neighbor lookup, including clipped
final cells. There are no physical-pixel workgroups inside the editor, global
geometry-buffer reads, font lookups or lighting calculations in this pass.
Five by-value vectors carry the display geometry in the thread payload.
Four disjoint chrome strips are copied separately, two pixels per lane, forcing
alpha 255. Physical cells must be at least 6x11. Canonical RGBA accesses use the
allocated pitch, including row padding.
Invisible pixels in a clipped final cell are zeroed during seeding and every
generation, so hidden glyph bits cannot influence the visible world.

The old 512-cell side cap and physical-page/3 simulation grid do not apply to
the bitmap path. The canonical allocation is bounded to 2,097,152 pixels rather
than silently resampling larger inputs. Two eight-byte worlds plus the RGBA
image use about 20 bytes per canonical pixel, before padding and compact input.
For example, a full 1700x800 viewport with 17x32 physical cells has a 600x275
canonical image: 165,000 heavy shading invocations instead of 1,360,000. The
display copy still writes every physical pixel. Actual editor viewports exclude
the menu, status and gutter, and are smaller than this full-page example.

For each immutable page: seed on first input or a changed scroll revision;
otherwise add positive coverage differences only on a changed emission
revision. Advance the compact input baseline even without emission. Put a GPU
cache/release dependency between every writer and the following reader before
flipping world buffers. The source terminal frame is borrowed through a read
lease only during each synchronous render, for unchanged chrome outside the
viewport. It is not copied or uploaded. On uncertain submission, keep that
exact source pinned alongside quarantined GPU resources. Never publish effect
output before producer retirement.

The editor background has a pure-black base and static alpha 153 (60% in
RGBA8), including its shader. Shader RGB is premultiplied by that alpha and
capped to it, so brighter cells never make the background more opaque.
Foreground ink is composited over this background with its original coverage:
output alpha is
`ink_alpha + round(153 * (255 - ink_alpha) / 255)`. Uncovered background remains
at 153; opaque text, menus, dialogs, selection and caret remain at 255.
Bitmap text keeps its exact coverage without added transparency. Outside the
editor viewport alpha stays 255. Window and hardware-plane opacity remain at 100%;
the effect frame is published with per-pixel alpha, without a fully-opaque hint.
Ordinary edits keep the previous completed effect visible until the next page
is ready, avoiding a temporary opaque background on each keystroke.
While Shader is enabled, Edit suppresses its ordinary current-line background
highlight so typed text does not sit on an opaque row. Selection rendering is
unchanged. Unstyled Edit text uses pure-white foreground on TrueOS; explicit
syntax colors continue to apply.

Animation is allowed only while the main editor textarea owns keyboard focus
and the UI has no additional floating roots. Menus, dropdowns, modal dialogs,
file/project pickers and status/search/line inputs therefore pause the effect,
including overlays outside the page or overlays that leave its focus intact.
The terminal's version-2 `edit-life` report separates the Shader `enabled`
preference from this `active` state. Version-1 reports remain accepted, with
any popup exclusion conservatively treated as inactive.

Pausing immediately restores the ordinary opaque terminal frame and bypasses
glyph capture, input upload, Life stepping and effect rendering. The last clean
page, world and buffers are retained; menu text never becomes shader input.
When every effect is paused, the service waits for a state change without an
animation timer. Returning to the page captures its clean current glyph cells once
and resumes the retained world with a fresh generation deadline, without
replaying the paused time. Real document, viewport and scroll changes still
follow the existing reset/emission rules. A serial check prevents an older
in-flight render from replacing an open menu.

The native producer uses its own normal-priority `GpgpuEditLife` context,
principal, submission lock, ring, batch, result storage, PPGTT and quarantine.
Spirit remains a separate normal-priority execution client. Display and font
service priorities are unchanged. Edit's private GGTT control window is warmed
once at boot; failure is optional for core display boot and is never repaired
by rewriting global mappings while peers are running. Retained input teardown
unmaps only the exact Edit PPGTT range after its pending work has retired.

The producer initializes its private PPGTT once for the lane's lifetime, then
prepares changed resource mappings once per render. It refuses initialization
retry or replacement of that physical root. Identical retained mappings incur
no PTE writes; changed mappings are preflighted, published and invalidated
before use. Borrowed source/destination imports are removed after retirement.
Each ordered pass is split into independently retired row chunks, bounded by
64K launched lanes for seed/generation/comparison/resolution and 128K for
display expansion. Expansion chunks also bound the actual expanded pixel writes
to 128K, using the exact physical span of their canonical rows. Chrome copy is
bounded to 64K lanes and at most two writes per lane. Bounds include padded
SIMD16 lanes. The generated descriptor
disables thread preemption, so a single whole-page walker would bypass this
scheduling boundary. Every chunk requires both its completion marker and the
saved context HEAD reaching the published TAIL before batch/payload reuse.
World buffers flip only after the complete pass succeeds. Chunking does not
rebuild the whole page table. The encoder uses the shared supported Gen12
compute prologue, including HDC flush and excluding reserved DW0 bit 11.
Gutter-origin and viewport-size changes reuse allocations
when their existing capacities fit. Scroll resets the world in those buffers.
Unchanged Edit frames emit no terminal metadata or synchronization fences.
Capture logs expose terminal-cell count, compact input bytes and zero source
read bytes, alongside existing native submission timings.

Runtime `edit-life: capture`, `edit-life: enabled`, and `edit-life timing` logs
separate compact text capture, initial allocation/upload, page-table setup, seeding,
baseline copying, generation, canonical resolution (`resolve_ms`), physical
expansion (`upscale_ms`) and chrome (`chrome_ms`). These timings use the `ui4/edit-life` log
target so they remain visible with the Blueprint/HV diagnostic profile.

Native initialization, input, render or frame-publication failure latches the
window's effect off and attempts to restore its newest ordinary opaque frame.
Typing, scrolling, menu round trips and timer ticks cannot restart it. Explicit
removal clears the window registration; GPU context quarantine remains sticky
until reboot. A failed restore retains the displayed effect until detachment
is proven; cleanup must not retire a still-attached frame. An uncertain GPU
submission also retains input allocations and the exact source-read and
destination-write leases. That source pin can exhaust the terminal's double
buffer on subsequent edits; this is containment, not a promise of continued
editor redraw after an ambiguous GPU hang. Clean pre-submit failures release
their leases normally. Peer contexts are never reset or automatically revived.

## Recorded Spirit freeze and verification boundary

The compact-font run recorded in
`bld/baremetal-logs/trueos-baremetal.1.log:270279` captures the actual failure:
compact input capture took 0 ms, initial GPU rendering took 791 ms (223 ms seed,
556 ms composition), then another generation took 615 ms with five catch-up
steps. In between, Spirit reported `spirit-vfx-marker-timeout`, marker zero,
saved HEAD 3516 and published TAIL 3520. Its execution context was quarantined
and the previous sprite remained displayed. No corresponding physical-context
fault was logged in that sequence.

The earlier producer used full-page, nonpreemptible walkers on the high-priority
SystemService context. This is a concrete scheduling defect consistent with
Spirit's timeout; the log alone does not prove the cause of every stalled
command. The separate normal-priority context, bounded walkers and corrected
prologue address those defects without changing Spirit's timeout or weakening
its retirement proof. Physical-target verification must still show Spirit
sprite/VFX progress during Edit startup, typing, scroll and menu pause/resume,
with no execution quarantine. Animation stays at ten generations per second.

The scheduling fix kept strict math. The visual bake now selects the existing
pinned relaxed-math profile: IGC receives `-options -cl-fast-relaxed-math`.
The frontend, source and SPIR-V are identical between compared math variants;
the reviewed toolchain locks differ only in profile hash, with identical
compiler binaries/libraries. This remains FP32, with no FP16 conversion.
Conway evolution, glyph coverage, text tint and alpha use integer operations.
The canonical pipeline uses integer indexing throughout; floating point is
confined to visual color/glow resolution, never the display expansion.

## Historical full-resolution math comparison, 5 November 2026

The following measurements predate the canonical pipeline described above.

The fresh strict-math boot starts at
`bld/baremetal-logs/trueos-baremetal.1.log:275456`. All eleven captured native
timings succeeded, with no Spirit/Edit timeout or quarantine in that boot.
The boot's diagnostic policy filters detailed GPGPU admission logs, so these
are complete render/retirement timings rather than individual admission costs.

| Strict-math phase, 1700x800 page | Total | Composition |
| --- | ---: | ---: |
| Startup before the first logged F12 | 2127 ms | 1695 ms |
| Early generation before F12, five steps | 1883 ms | 1740 ms |
| Page updates before F12 | 2487–2515 ms | 1694–1710 ms |
| Page update after turbo request | 501 ms | 255 ms |
| Six generations after turbo request | 258–290 ms | 237–265 ms |

Post-F12 medians are 266.5 ms total and 243.5 ms composition. The F12 event
requests 1550 MHz and samples 200 MHz at the switch; sustained actual GT clock
is not recorded. The log also contains 468 destination-busy warnings in this
boot. Its captured baseline is `/tmp/trueos-edit-math-baseline.json`.
This deployed baseline predates the explicit grid-index clamp; the following
code comparison rebuilds both math variants with that clamp present.

Reproduce the comparison using the same source and pinned tools:

```sh
bash tools/build-safe.sh python3 tools/edit_life_shader_bake.py --bake --verify \
  --math-mode strict --build-root /tmp/trueos-edit-math/bake-strict \
  --publish-dir /tmp/trueos-edit-math/strict
bash tools/build-safe.sh python3 tools/edit_life_shader_bake.py --bake --verify \
  --math-mode relaxed --build-root /tmp/trueos-edit-math/bake-relaxed \
  --publish-dir /tmp/trueos-edit-math/relaxed
python3 tools/edit_life_shader_compare.py --root /tmp/trueos-edit-math
python3 tools/testpy/test_edit_life_math.py
```

Both bakes pass independent clean-root byte reproducibility and `ocloc
validate`. Disassembly of the matching guarded variants gives:

| Native code metric | Strict | Relaxed | Change |
| --- | ---: | ---: | ---: |
| Entry bytes | 22,880 | 21,800 | -4.72% |
| Static instructions through EOT | 1709 | 1632 | -4.51% |
| Validated composition block instructions | 580 | 503 | -13.28% |
| Seed/generation block instructions | 1050 | 1050 | unchanged |

SIMD16, 128 GRFs, zero scratch, zero SLM and the dispatch ABI stay the same.
The 47 memory sends and 22 integer quotient instructions are unchanged.
The host sensitivity test checks 134 four-mode snapshots, including a complete
4096x2160 composition. Integer simulation/glyph baselines, alpha, opaque
text/selection/caret pixels and chrome remain exact. Of 81,871,232 output bytes,
73,675 RGB bytes differ; the largest measured background difference is 9/255.
ASan and UBSan pass. This uses host strict/fast math, rather than native IGC
execution. Static instruction counts describe emitted code, not GPU time; the next
target boot must establish the runtime gain against the strict baseline.
The one-time `edit-life: artifact` log exposes the admitted binary hash even
with the diagnostic log filter. The relaxed-math-only binary was
`e3cd3873360211b80c8173c86c2ecfc7fc0241203506fccbe94a13b180d65809`.
Animation interval, lane priority, chunk bounds and pause policy are unchanged.
These code counts describe the math-only revision, before the next section's
source optimization; the reproduction commands compare whichever source is
currently checked out.

## Historical full-resolution division and submission changes

The deployed relaxed-math boot starts at log line 278730 and confirms that
compiler relaxation alone did not fix runtime performance. Comparable turbo
1700x800 generation samples have median 266.0 ms total / 245.0 ms composition,
versus strict math's 266.5 / 243.5 ms. At 2560x1440 composition rises to
695.5 ms, approximately following output pixel count. There are no Spirit or
Edit timeout/quarantine records in that boot. Exact samples and instance/power
classification are in `/tmp/trueos-edit-perf-latest.json`.

The source had four variable integer divisions per output pixel to rediscover
the terminal cell and the glyph pixel. Seed sampling repeated the same work
at every bilinear tap, plus 64-bit geometry divisions for every Life cell.
Physical font dimensions and page geometry are invariant between resizes.
That revision cached a 4,176-byte uniform table with width/height reciprocal
constants and the exact fixed-point seed centers/reach. The shader replaces
division with `mul_hi(n, ceil(2^32/d))`, exact for the enforced page bound
`n <= 4096*2160` and physical font sizes `1 <= d <= 256`; size one is explicit.
Plain spaces bypass glyph sampling, and opaque text/selection/caret pixels
return their ink before evaluating background lighting. Font size, glyph masks,
page layout, Conway sampling and visual composition remain the same.

Each chunk previously cleared/flushed the entire 262,144-byte batch and
4,096-byte result allocation, although almost all of it was unused. The encoder
touched 7,296 batch bytes and 256 result bytes, covering commands, surfaces,
the exact payload, geometry table, markers and timestamps. Retaining the PPGTT
also removes its repeated 2 MiB initialization/flush. All reuse still requires
the exact lane's marker and saved HEAD retirement proof.
The owned world/font/kernel mappings persist. Borrowed source and destination
leaves are exact-unmapped and flushed after successful retirement, while both
frame leases are held. This lets the shared UI allocator recycle a GPU address
between source and destination cache-policy roles without stale PTE rejection.
Clean pre-submit failures remove partial imports; uncertain submitted failures
retain imports/backing and quarantine only Edit. The cache-policy guard remains
strict and no peer mapping is changed.

Actual IGA disassembly, deployed `e3cd...` versus candidate `9ec0...`:

| Native code metric | Deployed | Candidate |
| --- | ---: | ---: |
| Integer quotient instructions | 22 | 0 |
| Reciprocal instructions | 6 | 2 |
| Static instructions through EOT | 1632 | 1196 |
| Native entry bytes | 21,800 | 15,904 |
| Memory sends | 47 | 50 |

Composition's static count grows from 503 to 558 while removing all eight of
its integer-quotient instructions and adding early returns. Instruction counts
do not predict the removed operations' latency or GPU scheduling cost. SIMD16,
128 GRFs, zero scratch, zero SLM and the eight-buffer ABI remain unchanged.
That intermediate binary hash was
`9ec0aa04ff39d6e0cd22b4eeb9e7779f212e7c19400138abdd2040ccbf47b9a3`.

Sampled `edit-life: GPU probe` records split each pass into chunk count,
CPU encoding/admission, submit-to-batch queue delay, prologue, actual walker,
release drain, marker observation and saved-context retirement, in microseconds.
Four ordered GPU timestamps use the existing Gen12 helpers and 36-bit clock
handling. Samples cover first startup/input/reset kinds and every 120 renders;
ordinary renders retain their release packets. `samples` must match `chunks`
before treating the aggregated GPU phases as a complete measurement. These
markers distinguish shader execution from submission overhead on the next boot.
They do not weaken timeout, quarantine or resource ownership rules.

The historical reciprocal helper passed 1,164,664 divisor/boundary/random cases.
In that revision the native geometry builder matched the original wide arithmetic,
and all 134 full-shader strict snapshots matched deployed revision
`17270fef458e72deaf06bf23cb1b0cf28de3dea8` byte for byte, including complete
4096x2160 composition, input/reset/generation state, text and alpha. The optional
reference snapshot was `/tmp/trueos-edit-fast/deployed.clcpp`. That comparison
predates the intentional canonical-grid change; the current six-mode fixture
validates the new geometry and pure-copy behavior instead.

## Canonical bitmap pipeline

The current source removes full-resolution font lookup and lighting altogether.
Its 32-word parameter block contains canonical input geometry, image pitch and
clipped visible extents; the seed-axis table is gone. Display expansion and
chrome instead take their geometry directly from the cross-thread payload.
The encoder touches 3,264 batch bytes and 256 result bytes per chunk. World,
font, baseline and canonical-image mappings remain retained, with the same
borrowed-frame retirement and quarantine rules described above.

Mode 4 binds the retained canonical image as its output; mode 2 reads that
image through the font binding, which it no longer needs for glyph lookup.
The same eight explicit pointer arguments are retained, followed by five
16-byte by-value vectors (page, viewport/cell, canonical image/mode, chrome
rectangle, and physical pitches). Cross-thread data is 192 bytes; local IDs
remain 96 bytes. The generated contract contains three implicit payload
arguments; compiler metadata, rather than an
assumed count, defines their exact offsets. Each pass fully retires before its
buffers are consumed or rebound by the next pass.

The first canonical executable hash was
`6da8e0cd10e8ee9dc6c6881c6aa94727d7bf90f91a1b01aeb8c3b2dcfb781a8d`.
The expanded-texel executable is
`5e637cb126174dfb43b571713a174a96b997b908084ab76b7f13ecba5017187a`.
Two clean bake roots produce identical artifacts and `ocloc validate` accepts
the executable. It uses SIMD16, 128 GRFs, no scratch and no SLM. Its native entry
size is recorded in the generated contract. These checks establish the dispatch contract;
they do not establish physical GPU performance. Compare `resolve_ms`,
`upscale_ms`, `chrome_ms` and the mode-4/mode-2/mode-5 GPU probes after deployment.
Sampled probes include the actual admitted `launched_workitems` for each mode.

The first canonical boot confirmed a display-expansion bottleneck: at 1700x800
after F12, `upscale_ms` was 212–255 ms while `resolve_ms` was 5–26 ms. Sampled
mode-2 walker time alone was 212,515–229,912 microseconds, versus about 12 ms
of queue time. Before F12, the same copy walker took about 1.6 seconds. The
canonical shading reduction therefore did not remove the dominant cost.

Matched post-F12 generation samples have median 290 ms total, versus 266 ms
for the earlier relaxed-math build: about 9% slower. The display pass fell
from 245 to 227 ms, but the extra resolve pass and larger-world stepping erased
the gain. Startup fell from about 2180 to 1834 ms; a single page-update sample
also improved, so these results do not establish a uniform regression across
all actions. Parsed evidence is in `/tmp/trueos-edit-canonical-regression.json`.

IGA showed ten global geometry-buffer reads in the old copy branch and a
synchronized global fence before every hardware thread's EOT. The 1700x800
copy launched 85,600 SIMD16 hardware threads. Expanding canonical texels and
copying only chrome strips reduces those threads and fences, without changing
the output pixels or synchronization proof. The fence's precise latency is
not isolated by the existing measurements; this is a measured bottleneck with
a structural correction, not a demonstrated runtime speedup yet.

The expanded-texel boot admits `5e637cb1...` and confirms the copy improvement:
1700x800 mode-2 walker time is about 1.3–3.3 ms after F12, versus 212–230 ms
in the previous canonical build. Sampled later generation totals are 17–46 ms;
one 600-render sample is 22 ms. Queue time remains variable and these are
observed samples, rather than a controlled sustained-clock benchmark.

The same boot exposed a separate publication race during cursor-only clicks.
The effect published its completed buffer into FramePool before checking that
its input serial was still current. If a click changed that serial during GPU
work, the broker never learned about the new front. The old displayed buffer
could remain reader-pinned as the back buffer, while the new front was excluded
from writable-buffer acquisition. This stranded animation until another input
caused a broker publication; it was not an inactive page or GPU quarantine.

The producer now checks the input after GPU retirement and before changing the
front buffer. It holds the page-registration lock through FramePool publication
and broker publication, without holding it during GPU work. Superseded completed
writes are cancelled while still unpublished. A broker failure after front
publication stops on the latest ordinary source instead of silently retrying
with an unreachable back buffer. This race fix still needs target verification.
The per-pixel alpha contract is unchanged: uncovered background remains 153,
the display expansion preserves its stored alpha, and only chrome is forced to
255. FramePool keeps the effect's fully-opaque hint clear.

The inherited VFE configuration is unchanged: Intel's own Gen12LP preamble also uses one large
URB entry, so its size alone does not prove restricted EU occupancy. See the
[official Intel preamble](https://raw.githubusercontent.com/intel/compute-runtime/master/shared/source/gen12lp/preamble_gen12lp.cpp).

## Reproduce the local pinned setup

On 4 November 2026, the compiler tools were recovered into
`/tmp/trueos-edit-life-toolchain/root`, with downloaded `.deb` files next to
that directory. Build intermediates are in `/tmp/trueos-edit-life-bake`.
Nothing was installed into the system package database, no process-wide shell
environment was changed, and no toolchain lock was regenerated. This restores
the repository's existing pinned setup; it is not a migration to the latest
Clang, LLVM-SPIRV or Intel compiler.

The recovery downloaded nine packages, checking the exact SHA-512 or SHA-256
pins already recorded in `tools/intel-gpu-bakery/install_locked_clang21.sh`
and `tools/shadertoy-cpp-offline/bootstrap_toolchain.sh`. The source archives
come from the Ubuntu archive's `pool/` paths; all exact URLs and digests are
printed by the command below. The reusable procedure is saved as
`TRUEOS/tools/edit_life_shader_bake.py`.

| Component | Pinned version |
| --- | --- |
| Clang, common headers, runtime resources, Clang tools | `21.1.8-6ubuntu1` |
| LLVM-SPIRV and its library | package `21.1.5-1`, reports LLVM `21.1.8` |
| ocloc | package `26.05.37020.3-1`, reports `26.05.037020` |
| IGC and FCL | package `2.28.4-4`, libraries `2.28.4+0` |

The recovery reused the host's already installed `libLLVM21` and
`libclang-cpp21`, both `21.1.8-6ubuntu1`. Their exact library bytes, as well as
every recovered executable, compiler library and Clang resource file, matched
`adls-cpp-proof.lock.json`. A host without those two libraries needs the full
existing `install_locked_clang21.sh` package set in its local toolchain root;
the bakery will reject missing or mismatched dependencies.

From the TRUEOS checkout, inspect the exact package pins, recover the same
isolated setup if needed, and compile with the existing lock:

```sh
python3 tools/edit_life_shader_bake.py --print-package-pins
bash tools/build-safe.sh python3 tools/edit_life_shader_bake.py --recover --bake --verify
```

Use `--bake --verify` alone to reuse the recovered tools. `--verify` alone
requires no compiler. `--bakery` selects the directory containing the existing
`bake.py`; its default is the sibling workspace `tools/intel-gpu-bakery`.
The helper defaults to `--math-mode relaxed`, selecting
`adls-4680-r0c-shadertoy.json` with `adls-shadertoy-relaxed.lock.json` for this
Edit artifact only. Use `--math-mode strict` to reproduce the numerical profile,
or `--publish-dir` to keep either comparison outside the kernel artifact tree.

The original strict-math bake on 4 November was equivalent to the script's
printed command with `--math-mode strict` and these absolute arguments:

```sh
python3 /home/t4ce/Repositories/tools/intel-gpu-bakery/bake.py \
  --source /home/t4ce/Repositories/TRUEOS/crates/trueos-shader/gpgpu/kernels/edit_life_rgba8.clcpp \
  --artifact-name edit_life_rgba8 \
  --expect-kernel edit_life_rgba8 \
  --rust-symbol edit_life_rgba8=EDIT_LIFE_RGBA8_ADLS_CPP_ABI_CONTRACT \
  --profile /home/t4ce/Repositories/tools/intel-gpu-bakery/profiles/adls-4680-r0c-cpp.json \
  --toolchain-lock /home/t4ce/Repositories/tools/intel-gpu-bakery/toolchains/adls-cpp-proof.lock.json \
  --repro-check \
  --build-root /tmp/trueos-edit-life-bake \
  --clang /tmp/trueos-edit-life-toolchain/root/usr/lib/llvm-21/bin/clang \
  --llvm-spirv /tmp/trueos-edit-life-toolchain/root/usr/bin/llvm-spirv-21 \
  --ocloc /tmp/trueos-edit-life-toolchain/root/usr/bin/ocloc-26.05.1 \
  --publish-dir /home/t4ce/Repositories/TRUEOS/crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp
```

The bakery creates a sanitized child environment with `LANG=C`, `LC_ALL=C`,
`PATH=os.defpath`, `SOURCE_DATE_EPOCH=0`, `TZ=UTC`, and
`LD_LIBRARY_PATH=/tmp/trueos-edit-life-toolchain/root/usr/lib/x86_64-linux-gnu`.
It does not inherit ambient compiler flags or library overrides.

Both clean build roots produced byte-identical bitcode, SPIR-V and Zebin;
`ocloc validate` accepted the executable. The generated ABI is SIMD16,
128-byte cross-thread data, 96-byte local IDs, eight bindings, zero scratch
and zero SLM. The manifest and generated contract are the executable
provenance record.

Publication retains the generated manifest, SPIR-V, Zebin and Rust ABI
contract. The native operation checks its generated pointer offsets,
binding-table entries and entry range. The existing profile requires ADL-S `8086:4680` revision
`0c`, SIMD16, and zero scratch/SLM. Hardware verification remains necessary.
WGSL and GLSL frontends are absent from this maintained compute path; the
kernel never compiles sources at runtime. A language compile-time comparison
requires separate frontend tools and equivalent generated native code.

## Validation

```sh
bash tools/build-safe.sh cargo check --release --bin TRUEOS --locked --offline
python3 tools/testpy/test_edit_life.py
python3 tools/testpy/test_edit_life_capture.py
python3 tools/testpy/test_edit_hex_font.py
python3 tools/testpy/test_edit_hex_paint.py
python3 tools/testpy/test_edit_hex_shader.py
python3 tools/testpy/test_edit_life_native.py
python3 tools/testpy/test_edit_life_encoder.py
python3 tools/testpy/test_edit_life_imports.py
python3 tools/testpy/test_edit_life_lane.py
python3 tools/testpy/test_edit_life_failure.py
python3 tools/testpy/test_edit_life_publication.py
python3 tools/testpy/test_terminal_font_scale.py
python3 tools/testpy/test_edit_life_math.py
python3 tools/testpy/test_edit_life_division.py
python3 tools/edit_life_shader_bake.py --verify
```

The host tests exercise terminal control identity and publication geometry,
finite Conway boundaries, still lifes, an oscillator and a moving glider,
independent RGB wraparound including black live cells, positive input deltas,
unchanged/whitespace comparison baselines, deletion, scroll reseeding, and
premultiplied foreground composition without additional glyph transparency.
Edit's UI tests cover focused pages, keyboard menus, status/search controls,
modal and nonfocused overlays, silent paused frames and preserved revisions.
Capture regressions verify compact viewport records, clipped cells, shared
fallbacks, zero source reads and paused-page retention. Font tests compare
Rust, canonical source bits and the actual compiled C++ sampler, including
noninteger physical cell sizes, underlining and all 66 mask bits. Paint tests
exercise metadata-only font toggles and both-buffer invalidation.
Native dispatch tests compile the production row partitioner and dispatch
method across 432 padded dimensions/modes, checking coverage, retirement before
reuse and stopping after a failed chunk. Packet tests compile the actual shared
prologue. Expansion tests independently invert the physical nearest mapping for
29 row-partition cases and check the exact write/lane bounds; 108 chrome/body
partitions cover every physical pixel exactly once. Encoder tests execute the
actual six-mode encoder against bounded storage, inspecting all eight bindings,
the five direct vectors, padded canonical pitch, uniform/local-ID payloads and
the exact batch/result clear ranges. Import-retirement tests check exact physical ownership, missing and
partial imports, cache-policy role reuse, pending work and failure retention.
Lane tests exercise priority, identity, nonoverlapping control storage,
optional boot admission, exact quarantine and private-resource retirement.
Registration tests exercise failure latching, source freshness, pause/resume,
frame detachment and cleanup wakeups using the production lifecycle functions.
Six publication tests execute the production render method with cursor/source,
pause and failure changes during CPU/native authoring. Superseded outputs must
leave the front unchanged and their write lease reusable; a fresh render must
publish without another input event. Geometry preflight, post-publication broker
failure, transient busy buffers and uncertain GPU lease retention are covered.
The old render method fails the stale-front assertion in the same fixture.
Font-size tests cover strict OSC parsing, synchronized updates, resize/reset,
per-terminal isolation, applied-size coordinate conversion and scene drop
coordinates. Edit's host binary tests exercise all eight single-click choices,
narrow headers, five/six-row scrolling, keyboard selection and focus return.
The full-shader host math test compiles strict and fast variants with memory
and undefined-behavior sanitizers. Its 599 snapshots cover all six modes,
including padded launches, clipped cells and viewport edges. It checks body
expansion and chrome strips against the prior physical-pixel reference copy,
and runs both display branches with all unused input pointers null.
The interval test compiles the production expansion calculations and checks
257,451 axis cases, 13,629,465 rectangles and 80,608,265 output pixels across
all supported cell sizes, clipped edges and padded launches with ASan/UBSan.
Integer state, glyph baseline,
alpha, opaque ink and chrome must remain exact; it reports measured background
RGB differences. This host sensitivity check does not reproduce IGC's native
instruction semantics or measure Intel GPU performance.
The canonical fixture compares 113,104,724 RGBA bytes between strict and fast
host math; the current run has zero differing RGB bytes and exact alpha.

An optional standalone reference timing command is
`python3 tools/testpy/test_edit_life_model.py --benchmark`. It reports compact
input bytes and CPU fallback timings; these are not native GPU measurements.
The baked shader and producer have compile/manifest validation. Visual and
performance validation on the physical target remain outstanding.
