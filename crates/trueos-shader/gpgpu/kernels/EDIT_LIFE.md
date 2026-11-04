# Edit Conway background

`edit_life_rgba8.clcpp` is the offline GPU counterpart of
`src/ui4/edit_life_model.rs`. It uses the existing C++ for OpenCL architecture.
Its generated Zebin, SPIR-V, manifest and ABI contract live in
`artifacts/adls/cpp/edit_life_rgba8.*`. The native producer is
`src/intel/gpgpu/operations/edit_life.rs`; the bounded CPU implementation is
the reference and unavailable-backend fallback.

The source has one SIMD16 entry point and four modes: seed/additive input,
Conway generation, opaque composition, and input comparison-baseline copy.
The comment at the top of the source specifies every buffer and uniform.
The glyph mask and straight glyph-color page are read-only. A separate
viewport-sized R8 buffer retains the last input baseline. Cells occupy two
u32 words, separating binary occupancy from RGB, trail strength and birth.

For each new immutable page: seed on first input or a changed scroll revision;
otherwise add positive R8 differences only on a changed emission revision.
Then copy the input baseline even when there was no emission. Advance ten
generations per second, with a fixed .905 trail decay (the demo's .75
persistence). Put a GPU cache/release dependency between every writer and the
following reader before flipping cell buffers. Never publish the output before
producer retirement.

The composition mode copies the existing foreground RGB outside the viewport and
uses premultiplied foreground RGB plus the effect times one minus foreground
alpha inside. Output everywhere has alpha 255. The effect's fixed
50% scrim never scales text, menu, status, popup, selection or caret opacity.

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
cargo check --locked
python3 tools/testpy/test_edit_life.py
python3 tools/edit_life_shader_bake.py --verify
```

The host tests exercise terminal control identity and publication geometry,
finite Conway boundaries, still lifes, an oscillator and a moving glider,
independent RGB wraparound including black live cells, positive input deltas,
unchanged/whitespace comparison baselines, deletion, scroll reseeding, and
premultiplied foreground composition without additional glyph transparency.

An optional standalone reference timing command is
`python3 tools/testpy/test_edit_life_model.py --benchmark`. On the current
host a 1280×720 page (427×240 world) took approximately 6–7 ms per snapshot,
0.8–1.2 ms per generation and 12–13 ms for a full CPU display pass. These are
CPU fallback measurements, not measurements of the native GPU implementation.
The baked shader and producer path have compile/manifest validation; visual
and performance validation on the physical target remain outstanding.
