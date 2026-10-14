# Codex native-code scan — 2026-10-14

## Result

The current TRUEOS Codex module contains code compiled from C and assembly.
The Codex application source is predominantly Rust, but its dependency tree is
not entirely Rust. Nine dependency archives contain 192 non-Rust object members;
715 global native definitions from those members survive in `module.o`.
These counts describe archive inputs and retained symbols, not 192 whole object
files necessarily retained without section garbage collection.

This was a read-only source/build-artifact investigation. No dependencies,
features, code or deployment were changed, and no new build was run.

## Scope and evidence

Scanned `codex-rs` source/build scripts, then used the current TRUEOS build's
unit graph, link archive list, archive members and final module symbol table:

```text
/home/t4ce/Repos/codex/codex-rs/cli/target/trueos-blueprint/work/codex/
  native-unit-graph.json
  native-folds.json
  module.o
```

These are the existing October 14 build artifacts used by the Codex package
whose digest matched the online catalog in the QEMU investigation. This is an
artifact audit, not a claim about every platform/configuration in Cargo.lock.

Machine-readable inventory:
`bld/codex-debugger/20261014-141846/native-code-scan.json`.
Final undefined symbols:
`bld/codex-debugger/20261014-141846/codex-imports.txt`.

For each linked `.rlib`, `ar t` identifies object members ending in `.o` but not
Rust `.rcgu.o`. Build scripts/source establish their language. `nm` then checks
that global definitions from those specific members exist in final `module.o`.
That last check distinguishes native code present in a candidate archive from
native code actually retained in the module. Only global symbols are counted;
local C functions are outside the retained-symbol count.

## Native code in the TRUEOS module

| Dependency | Language / purpose | Native input objects | Retained global native definitions |
| --- | --- | ---: | ---: |
| `onig_sys` 69.9.3 | C, Oniguruma regex engine | 48 | 134 |
| patched `ring` 0.17.14 | C and x86-64 assembly, cryptography beneath TLS | 18 | 58 |
| `tree-sitter` 0.25.10 | C parsing runtime, `src/lib.c` | 1 | 156 |
| `tree-sitter-bash` 0.25.1 | C generated parser and scanner | 2 | 6 |
| `tree-sitter-powershell` 0.26.4 | C generated parser and scanner | 2 | 6 |
| `lzma-sys` 0.1.20 | C, XZ/liblzma compression | 73 | 63 |
| `bzip2-sys` 0.1.13+1.0.8 | C compression/decompression | 7 | 8 |
| `zstd-sys` 2.1.1+zstd.1.5.7 | C and AMD64 assembly, compression | 37 | 262 |
| `blake3` 1.8.2 | x86-64 SIMD assembly implementations | 4 | 22 |

Examples retained in the module include `ts_parser_*`,
`tree_sitter_bash`, `tree_sitter_powershell`, Oniguruma definitions,
`ring_core_0_17_14__CRYPTO_*`, `BZ2_bzDecompress`, liblzma definitions,
Zstd definitions and BLAKE3 SIMD routines. Exact object names, sample retained
symbols and native-member system imports are in the JSON inventory.

The active build scripts compile these through `cc::Build`. The archives are
folded into the relocatable Blueprint module; their packaging inside `.rlib`
does not turn the embedded C/assembly into Rust.

## Present in source, excluded or different on TRUEOS

- `codex-rs/vendor/bubblewrap` contains C sources. `bwrap/build.rs` compiles
  `bubblewrap.c`, `bind-mount.c`, `network.c`, and `utils.c` and links libcap
  on Linux. It returns immediately for non-Linux targets. `codex-bwrap` is
  absent from this TRUEOS target graph/link list.
- `cli/build.rs` adds Objective-C linker flag `-ObjC` only on macOS. That does
  not establish Objective-C code in this TRUEOS module.
- `windows-sandbox-rs/build.rs` handles Windows manifest linker arguments and
  returns on TRUEOS. It does not compile a C++ sandbox in this build.
- `openssl-sys`, `aws-lc-sys`, `libgit2-sys` and `libsqlite3-sys` are absent
  from this target graph/link archive list. Their presence in general manifests
  or another platform's lock/dependency tree is not evidence that they ship
  here. TLS uses the `ring` feature of Rustls.
- The active SQLite integration is the patched
  `TRUEOS-Blueprints/vendor/sqlx-sqlite-0.9.0` Rust crate. This scan concerns
  the Codex module, not the separately launched SQLite service's implementation.

No active target build script compiling C++ was found, and the final module
has no undefined C++ runtime symbols (`__cxa*`, `__gxx*`, `_Z*`). This is
evidence against a C++ runtime in this artifact, not a universal claim about
all optional code or all other Codex targets.

## C ABI, libc and scheduling implications

The active `libc` dependency is patched Rust source under
`TRUEOS-Blueprints/vendor/libc-0.2.186`. The Rust `libc` crate provides ABI
types/constants/declarations; finding it does not establish a bundled glibc
implementation. `module.o` has no dynamic section. It retains undefined C ABI
symbols to be satisfied by TRUEOS's loader/kernel compatibility surface.

The final imports include pthread mutexes, condition variables, thread-local
storage, signal-mask and at-fork APIs. Rust standard-library code can call
these as well as native dependencies. An import alone does not identify its
caller or prove the associated path executes during startup.

Native-member imports show allocation APIs in Oniguruma, Tree-sitter and the
compression libraries. Tree-sitter/Zstd also reference `fwrite`. liblzma's
archive contains POSIX multithreading code and references `pthread_create` and
`pthread_join`; its config selects `MYTHREAD_POSIX`. However no liblzma public
MT/thread function was found among the final module's global definitions, and
`pthread_create`/`pthread_join` are not final undefined imports. Archive-level
threading support must therefore not be reported as proof of a running native
thread pool. The active Zstd features exclude `zstdmt`; compiling objects named
`threading.o` or `zstdmt_compress.o` does not mean multithreading was enabled.

The debugger's observed deadlock remains in the Rust implementation of
TRUEOS's C ABI `open`/regular-file registry, reached from Rust `std::fs`.
There is no debugger evidence assigning that freeze to one of these native
libraries. Both Rust and C callers nevertheless rely on the same compatibility
surface's locking, waiting, errno and identity contracts.
