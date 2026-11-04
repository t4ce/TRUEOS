# Edit Conway background

`edit_life_rgba8.clcpp` is the offline GPU counterpart of
`src/ui4/edit_life_model.rs`. It uses the existing C++ for OpenCL architecture.
Its generated Zebin, SPIR-V, manifest and ABI contract live in
`artifacts/adls/cpp/edit_life_rgba8.*`. The native producer is
`src/intel/gpgpu/operations/edit_life.rs`; the bounded CPU implementation is
the reference and unavailable-backend fallback.

The source has one SIMD16 entry point and four modes: seed/additive input,
Conway generation, background composition, and input comparison-baseline copy.
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

The native producer prepares its PPGTT and retained resource mappings once per
render. Each ordered pass uses one full-domain walker: initial/scroll seeding
needs three submissions (seed, comparison baseline, composite), and a normal
generation needs two. It does not rebuild the whole page table for every
128K-pixel chunk. Gutter-origin and viewport-size changes reuse allocations
when their existing capacities fit. Scroll resets the world in those buffers.
Unchanged Edit frames emit no terminal metadata or synchronization fences.
Capture logs expose terminal-cell count, compact input bytes and zero source
read bytes, alongside existing native submission timings.

Runtime `edit-life: capture`, `edit-life: enabled`, and `edit-life timing` logs
separate compact text capture, initial allocation/upload, page-table setup, seeding,
baseline copying, and composition. These timings use the `ui4/edit-life` log
target so they remain visible with the Blueprint/HV diagnostic profile.

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
python3 tools/edit_life_shader_bake.py --recover --bake --verify
```

Use `--bake --verify` alone to reuse the recovered tools. `--verify` alone
requires no compiler. `--bakery` selects the directory containing the existing
`bake.py`; its default is the sibling workspace `tools/intel-gpu-bakery`.

The exact original bakery command was equivalent to the script's printed
command, with these absolute arguments:

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

An optional standalone reference timing command is
`python3 tools/testpy/test_edit_life_model.py --benchmark`. It reports compact
input bytes and CPU fallback timings; these are not native GPU measurements.
The baked shader and producer have compile/manifest validation. Visual and
performance validation on the physical target remain outstanding.
