# TrueOS retrospective commit messages

This is the subsystem-coherent five-agent pass over the 173-commit Blueprint timeline. Each entry is rewritten in the context of the related API, refactor, userspace, and kernel-service commits that belong to the same subsystem, even when those commits are separated in time.

The individual subsystem drafts remain under tools/comitmsg/subsystem-pass/ for audit. This archive is descriptive only; it does not rewrite Git history or imply a commit or push.

## 001 — `1e0ac40f` — 2026-03-19

Original: `first commit`

Establish the first Blueprint package and TRBP ABI

The root commit establishes `hello_world_bp` as a nightly `#![no_std]`/`#![no_main]` consumer whose `main(args)` calls `trueos::vsys::log_info_with_args` through `trueos::portal!(main)`, targets `x86_64-unknown-none`, and is packaged by `trueos-blueprint/src/main.rs` into a `TRBP` header plus relocatable payload. The adjacent `trueos-sys/src/vcabi.rs` declares the initial allocation, filesystem, network, input, shell, NTP, UI2, and graphics symbols; this is the Blueprint precursor to the kernel’s `9cefefeb` TRUEOS V ABI and `1421d1c6` graphics-path work.

## 002 — `13803d0a` — 2026-03-19

Original: `Add README instructions for app setup and requirements`

Document the first pack-and-run handoff

`README.md` records the concrete `cd`, `cargo bp`, copy-to-primary-filesystem, and `run`/`run <id>` workflow, along with nightly Rust and `7z`/`ld`/`objcopy`/`readelf` prerequisites. It is the documentation consumer of the packer and `hello_world_bp` contract from 001, naming the shell-facing artifact handoff without changing a Blueprint API or kernel path.

## 003 — `c35d82a6` — 2026-03-19

Original: `Improve README formatting and headings`

Make the initial packaging instructions scannable

This documentation-only cleanup reshapes `README.md` headings around the existing `cargo bp`, generated artifact, and `run` instructions. It stabilizes the 002 authoring path for readers while leaving `trueos-blueprint/src/main.rs`, `trueos::portal!`, and the kernel-facing artifact contract unchanged.

## 004 — `a5100df0` — 2026-03-19

Original: `Revise README for clearer build and run instructions`

Clarify how a TRBP artifact reaches the kernel

`README.md` replaces ambiguous pseudo-headings with a fenced build-result explanation and explicitly separates copying the `.BP` into the mounted primary filesystem from invoking `run` or `run <id>`. This is a consumer-facing refinement of 001–003: it clarifies the boundary between `trueos-blueprint` output and the kernel shell without altering either side.

## 005 — `db017a76` — 2026-03-19

Original: `Revise README instructions for clarity`

Correct the filesystem installation wording

The README changes the destination to the root folder of the primary filesystem and emphasizes the `run` and `run <id>` commands and build prerequisites. It is a narrow stabilization of the 004 documentation path; no `TRBP` fields, `trueos_cabi_*` declaration, or kernel implementation changes are present.

## 006 — `f90fd9ba` — 2026-03-19

Original: `Revise README instructions for app setup`

Compress the first-run authoring guide

`README.md` folds the app-directory and `cargo bp` steps into compact headings and removes redundant whitespace while preserving the build, artifact, and kernel-shell instructions. This cleanup closes the initial 002–005 documentation chain without changing `hello_world_bp/src/main.rs`, the `x86_64-unknown-none` target, or the packer.

## 007 — `12de839f` — 2026-03-21

Original: `amble`

Import Amble as an isolated authoring experiment

The commit adds an independent `amble` Cargo workspace with `amble_data` definitions/validation, `amble_engine` loaders, commands, goals, entities, RON worlds, and authoring documentation. It is exploratory data and narrative tooling rather than a consumer of `trueos-blueprint` or the kernel C ABI; the later 008 deletion is its explicit cleanup, and kernel commit `fd9c8080` is only a parallel experiment reference.

## 008 — `dbd2a09d` — 2026-03-25

Original: `ok`

Remove the temporary Amble workspace

This inverse change deletes the `amble` workspace, including its structured data, validator, engine, RON worlds, docs, editor configuration, and assets. It returns the repository to the active Blueprint packaging line and mirrors the removal of the corresponding kernel experiment in `d795bc0d`; no Blueprint ABI is introduced.

## 009 — `9886a4be` — 2026-03-25

Original: `api export`

Add a hosted API export alongside the freestanding app

`hello_world_bp/Cargo.toml` gains `host-std`; `hello_world_bp/src/main.rs` moves its entry to `app_main` behind `cfg_attr`; and `trueos/src/lib.rs` selects host-backed `vclock`, `vfetch`, `vfs`, `vgfx`, `vinput`, `runtime`, `vshell`, `vsys`, and `ui2` modules while retaining the CABI path. This creates a host consumer for the wrappers first introduced in 001 and provides the Blueprint-side exercise path for kernel `9cefefeb`’s exported V ABI.

## 010 — `37a9f88f` — 2026-03-25

Original: `api export`

Refresh generated outputs after the API export

The diff is generated `hello_world_bp/target` state and rebuilt objects/rlibs/binaries after 009, with no source, manifest, or CABI declaration change. It records a consumer rebuild of the host/freestanding API split rather than a second interface, so the semantic boundary remains the one in `9886a4be`.

## 011 — `02dd9bc7` — 2026-04-20

Original: `Rework towards VM`

Start the VM-oriented localcoder Blueprint path

The new `localcoder_bp` is a `no_std + alloc` portal crate importing `trueos_v` from `TRUEOS/crates/trueos-v`, exporting `main(argc, argv)`, and exercising environment, shell history, filesystem, and asynchronous fetch CABI operations; `build_bp.py` emits a raw `TRBP` artifact. It is the Blueprint precursor to kernel `d088bfd1` VM guest execution and `a5799825`’s localcoder shim, moving beyond the original hello-world packer while preserving the same guest-module handoff.

## 012 — `e6169b0f` — 2026-04-20

Original: `authoring`

Put VM authoring behind reusable virtual-environment wrappers

`localcoder_bp` stops importing `trueos_v::vcabi` directly and consumes `venv`, `vfetch_job`, `vfs`, `vshell`, and `ui2`; `trueos/src/venv.rs` plus `venv_host.rs` provide `venv::var_bytes`/`venv::var` through CABI or `std::env`. This refactors 011’s direct guest probe into an app-facing authoring layer, aligned with the kernel localcoder service work in `a5799825`, while leaving `trueos-sys` as plumbing.

## 013 — `c4f8f1e3` — 2026-04-20

Original: `vm hull execution`

Exercise VM-owned windows and fetch-job lifetimes

`hello_world_bp` and `localcoder_bp` now create/leak UI2 `OwnedWindow`s, while `trueos/src/vfetch_job.rs` introduces `BytesJob` to own fetch start, wait, read, and discard around the new `trueos_cabi_env_var` declaration. The commit is the consumer/stabilization step after 012’s wrappers and 011’s VM probe, matching kernel `d088bfd1` guest execution and `a5799825` localcoder integration.

## 014 — `1d25e121` — 2026-04-21

Original: `blueprint rework`

Replace app-specific packaging with one Blueprint package

The root gains canonical `Cargo.toml`, `src/lib.rs`, and `src/main.rs`; `hello_world/hello_world.rs` becomes a Tokio current-thread example, `localcoder/hello_world.rs` remains a second source, and the old `hello_world_bp`, `localcoder_bp`, README, and generated targets are removed. This is a boundary reset after the VM probes, preparing the shared runtime/UI2 layers that kernel commits `78ee7f53` and `e307a05f` were validating.

## 015 — `77dbadbc` — 2026-04-21

Original: `tokio runtime unification`

Establish shared Tokio, system, and UI2 Blueprint layers

The root package adds the `triangle` example and `trueos` dependency; `trueos/src/ui2.rs`, `vgfx.rs`, and `vsys.rs` add `OwnedWindow`, `SurfaceWindow`, `WindowId`, RGB triangle rendering, texture/window wrappers, logging, and polling, with matching declarations in `trueos-sys/src/vcabi.rs`. The triangle is the first concrete consumer of this shared surface, following 014’s package reset and probing kernel `126c7167`’s Tokio integration and VMX/UI lane.

## 016 — `43890fbd` — 2026-04-21

Original: `UI2 api`

Make UI2 repaint an explicit graphics contract

`trueos-sys/src/vcabi.rs` renames window creation to `trueos_cabi_app_window_create`/`trueos_cabi_app_surface_window_create`, adds `trueos_cabi_ui2_window_request_repaint` and queued RGB-triangle rendering, and `trueos/src/ui2.rs` exposes `WindowId::request_repaint` and queues repaint from `SurfaceWindow::render_rgb_triangles`. This refines 015’s consumer API around kernel UI2 plumbing in `fc1ec0b1`, while `src/main.rs` switches object emission to `-Zno-link` for the VM artifact path.

## 017 — `9d5f9ff8` — 2026-04-22

Original: `vmx execution ui2 success`

Split Tokio-std and thin-no-std UI2 execution

The package adds optional `tokio-runtime`, converts `src/lib.rs` to `no_std`, selects Tokio-std versus thin builds in `src/main.rs`, and adds `TrueosAllocator`, `panic_abort`, async texture upload, and hosted synchronous upload helpers; `triangle` becomes an exported no-std entrypoint. This is the buildable runtime consumer of 015–016 and the Blueprint counterpart to kernel VMX execution `e307a05f` plus UI2 refinement `fc1ec0b1`.

## 018 — `e12f9f2b` — 2026-04-22

Original: `last std link gone`

Make the hello-world probe freestanding

`hello_world/hello_world.rs` becomes `#![no_std]`/`#![no_main]` with an exported `extern "C" fn main()` calling `trueos::vsys::log_info`, and supplies a `TrueosAllocator` and `panic_abort`. It is the smallest consumer proving that the UI2/VMX path from 017 can run without `std`, in the same kernel stabilization window as `3ced0b39`’s VMX console bypass.

## 019 — `42308ab0` — 2026-04-22

Original: `no allocator no panic`

Centralize thin-runtime allocator and panic fallbacks

The packer’s `BuildSettings` detects `#[global_allocator]` and `#[panic_handler]` and injects `thin-default-global-allocator`/`thin-default-panic-handler` only when a no-std app lacks them; `trueos/Cargo.toml` and `trueos/src/lib.rs` provide those conditional defaults. `hello_world/hello_world.rs` consequently drops its local handlers, making this a cleanup/stabilization of 018 and the precursor to 024’s app-demo simplification.

## 020 — `61228289` — 2026-04-22

Original: `need rework`

Remove the superseded localcoder example

The commit deletes `localcoder/hello_world.rs`, the duplicate Tokio example left by the 014 workspace rework. It removes a stale consumer after the shared root examples and thin runtime path had superseded it, without changing the VM wrappers or kernel-facing API.

## 021 — `4392200b` — 2026-04-22

Original: `time demo`

Add a Blueprint virtual-clock/NTP probe

`trueos/src/vclock.rs` wraps `trueos_cabi_ntp_current_unix_seconds` and `trueos_cabi_ntp_kernel_date_day_month_year`; `vsys.rs` adds formatted logging and `ntp_once/ntp_once.rs` reports Unix time and kernel date availability through `bp_info!`/`bp_error!`. This adds a focused consumer after the VM/UI2 stabilization, corresponding to kernel SNTP support in `e40bdecc`, and is later retired with the compatibility probes in 027–029.

## 022 — `7410e2a4` — 2026-04-22

Original: `Ok`

Build the comprehensive hosted API demonstration

`apidemo/main.rs` probes `vsys`, `vclock`, app-owned and primary UI2 windows, texture dimensions, sync/async uploads, queued triangles, screenshots, window state, and `poll_once`, using the allocator and panic path from 019; `src/main.rs` also broadens `ld`/objcopy/readelf discovery and entry-hint fallback. It is the broad consumer that exposes the 015–021 API surface before 023 narrows ownership and 024 changes the payload format.

## 023 — `4cc3c304` — 2026-04-22

Original: `stable`

Narrow UI2 validation to app-owned windows

`apidemo/main.rs` stops probing the kernel-owned primary browser window, while `trueos/src/ui2.rs` and `trueos-sys/src/vcabi.rs` remove `primary_browser_window()` and `trueos_cabi_ui2_primary_browser_window_id`. This stabilization trims the contract exercised by 022 to `OwnedWindow` and `SurfaceWindow`, avoiding a consumer of a kernel-owned window path that was still evolving.

## 024 — `dc8508bb` — 2026-04-23

Original: `cleanup pass`

Compress TRBP payloads and remove demo startup boilerplate

`src/main.rs::write_blueprint` changes the `TRBP` flag from 1 to 2, compresses the stripped object with LZMA2/7z, and records compressed and raw lengths; `apidemo/main.rs` removes its duplicate allocator and panic handler in favor of 019’s runtime defaults. This is both a packaging-format consumer change for the kernel loader and the cleanup endpoint of the 022–023 demo sequence; the archive does not show a kernel loader diff here.

## 025 — `b7f26403` — 2026-04-24

Original: `ok`

Establish filesystem/network probes around the first byte-fetch lifecycle

`trueos/src/vfetch.rs` wrapped prewarm, file/byte fetch, wait, result-length, read, and discard CABI operations, while `clone_probe/main.rs` used the wrapper for a Gitoxide archive and `socket2_probe/main.rs` exercised socket creation; the manifest also added `tokio-net-probe`, `clone-probe`, and `socket2-probe` feature paths. This is the Blueprint consumer layer for kernel `9458d3b34080ed2106e7ba57c9d1224bea8eb732` and `a6b9cad833724da8ce280f66ce7a07058ba2ecc3` (Mio/socket2 and zkvm compatibility), and it precedes the later move of these probes behind the shared API.

## 026 — `ed950fb7` — 2026-04-25

Original: `ok`

Move C ABI ownership into the kernel workspace

The Blueprint removes its local `trueos-sys/src/vcabi.rs` implementation, points `trueos` at `TRUEOS/crates/trueos-sys`, documents the retirement, and adds the `apps` alias plus a multi-example launcher and `tokio_net` Mio/socket2/TCP/UDP probes. This is the API-ownership correction that makes 001’s copied declarations a kernel-owned consumer boundary; the committed `TRUEOS` gitlink is `eefda2aa235dafb441631f83eea9d0186267fae3`, which is unavailable in the checked-out kernel repository, so exact symbol contents at this snapshot cannot be independently verified.

## 027 — `79160c58` — 2026-04-25

Original: `ok`

Isolate legacy probes behind compatibility modules

`trueos/src/compat/mod.rs` receives `vclock.rs` and `vfetch.rs`, while `trueos/src/lib.rs` stops presenting them as primary modules and `apidemo`, `clone_probe`, and `ntp_once` update imports; dependency paths also move to `../TRUEOS` and per-app target directories. This refactor follows 026’s kernel `trueos-sys` ownership move by separating exploratory consumers from the public wrapper, immediately before their deletion in 028–029.

## 028 — `f05c247d` — 2026-04-25

Original: `refactor`

Delete superseded API and compatibility probes

The commit removes `apidemo`, `clone_probe`, `ntp_once`, the retired `trueos-sys/README.md`, and compatibility `vclock`/`vfetch` implementations, leaving the newer Tokio-first examples as validation. It is the deletion phase of 027’s isolation and narrows the Blueprint surface after exploratory CABI consumers rather than adding another kernel API.

## 029 — `a9affbc1` — 2026-04-25

Original: `ok`

Remove the final stale compatibility export

The one-line change deletes `pub mod compat;` from `trueos/src/lib.rs` after 028 removed its contents. It completes the 027–028 cleanup so the public Blueprint API no longer advertises retired vclock/vfetch wrappers at the kernel boundary.

## 030 — `92634f4e` — 2026-04-25

Original: `ok`

Turn Tokio filesystem and socket services into repeatable Blueprint probes

`trueos/src/lib.rs` added `runtime::current_thread`/`current_thread_net`, feature-gated `net` re-exports for Tokio, Mio, and socket2, `fs::try_exists`, and variadic logging; `tokio_fs/main.rs` then covered write/read/open/seek/flush/create-dir/remove while `tokio_net/main.rs` covered sockets, UDP, TCP, and HTTP fallback. The shared facade converts kernel `7228be7a7922503f3248dc3ca429dda15811d84e`’s runtime-owned networking into concrete consumer checks and is the precursor to the dedicated runtime matrix in 031.

## 031 — `95f8e26a` — 2026-04-26

Original: `ok`

Isolate Tokio executor behavior in a conformance workload

The new feature-gated `tokio_rt/main.rs` tested runtime construction and drop, `block_on`, yield/spawn/join and local sets, `JoinSet`, `join!`/`try_join!`, abort/select, I/O, synchronization, and timeout/sleep behavior with staged diagnostics. It separated executor validation from the filesystem/socket probes in 030 and provided a Blueprint-level consumer for kernel `126c7167640a81459369d87e5be590d6510cbe0c` and `78ee7f53b8ad504081996f09f6d2819d91e75358`.

## 032 — `600915af` — 2026-04-27

Original: `ok`

Prove common no-std crate graphs survive Blueprint linking

`test_some_crates/test_some_crates.rs` linked `serde`, `serde_json`, `regex`, and `anyhow` in a no-std/alloc entrypoint, round-tripped `CrateProbe { name, count }`, constructed a regex, and logged through `trueos::vsys`; `src/main.rs` added `--gc-sections` and `--undefined=main` so the portal entry survived the larger graph. This is a linker-stability consumer alongside, rather than an extension of, the Tokio service probes in 030–031.

## 033 — `d43800f1` — 2026-04-27

Original: `ok`

Expand the graphical Blueprint catalog with native raster probes

`Cargo.toml` registers `chart`, `petersen`, and `svg_grid`; their entrypoints use `TrueosAllocator`, `panic_abort`, `SurfaceWindow`, `vgfx::upload_*_to_texture`, repaint, and `vsys::poll_once` to turn a sine chart, Petersen graph, and embedded SVG into runnable Blueprint consumers. The shared bridge is `trueos/src/vgfx.rs`, while kernel graphics exposure is evidenced by `crates/trueos-v/src/ffi.rs` and `src/gfx/backends/mod.rs` in `1421d1c64aef2092f0b35e6cd53176a42ff30dd5`; these examples are the catalog precursor to the later `api/src/ui2/gfx.rs` consolidation.

## 034 — `ddbf5016` — 2026-04-30

Original: `ok`

Register external localcoder packages with the Blueprint launcher

`.gitmodules` adds the `localcoder` gitlink, and `src/main.rs` expands dispatch from named built-in examples to a sibling package with its own `Cargo.toml`, placing its target in the parent `dist` directory and accepting `target.json`, `trueos.json`, or `trueos-app.json`. This is the packaging precursor to the later registry and external-app work; kernel commit `86e71eaf4ca16faaad1d6b034b787907180b3ef6` wires the localcoder service through `src/shell2/cmds/c4.rs` and `src/shell2/cmds/run.rs`, but the gitlink leaves the exact localcoder revision and internal API usage unavailable here.

## 035 — `1f00dab5` — 2026-04-30

Original: `im not certain if i have to vendor libc but aswell i was uncertain about fork rust language`

Stage a vendored libc source for the TrueOS target

The commit imports the complete `vendor/libc-0.2.185` tree, including newlib and x86_64 platform modules, but does not yet add a Cargo patch or dependency selection. It is preparatory evidence for the later 036 routing and 084/085 hosted-std work; the diff alone cannot prove that libc was linked, and the kernel-side relationship is therefore limited to the concurrent std ABI work around `d657983e`.

## 036 — `173f7e19` — 2026-05-01

Original: `ok`

Route every app build through vendored libc

`src/main.rs` adds ancestor-based `find_vendor_dir` discovery and injects a TOML-escaped `[patch.crates-io] libc.path` entry for `vendor/libc-0.2.185`. This turns 035’s source staging into an active build overlay and provides the userspace dependency route needed alongside kernel `5b9364a5`’s std ABI shim and `2262f578`’s runtime ABI state.

## 037 — `4e18b6a9` — 2026-05-06

Original: `ok`

Add a reusable weather API and profile-aware application packer

The commit adds `crates/trueos-weather` with `WetterMinute`, `WetterAktuell`, `WetterTag`, `OpenWeatherResponse`, localized parsing, and `demo.json`, plus `trueos/src/net_fetch.rs::fetch_bytes` over the `trueos_cabi_net_fetch_bytes_*` lifecycle; `weather/main.rs` and `framework_stack/main.rs` make those services concrete app consumers. `src/main.rs` adds `CargoProfile`, `--release`, source-overlay staging, and kernel-vendored dependency discovery, making the weather catalog entry reproducible alongside kernel `1edbef779c4afeb90472da1fb3192d3380420b4`'s `crates/trueos-weather` and localized UI2 demo support.

## 038 — `693c21e2` — 2026-05-07

Original: `ok`

Turn input, shell, and game demos into launcher examples

`Cargo.toml` registers `cli_tetris` and `full_tetris`, while `trueos/src/input.rs` adds keyboard/cursor readers, `trueos/src/vshell.rs` adds attached shell I/O, and `trueos/src/vsys.rs` exposes polling, sleep, and logging used by the games; `full_tetris/main.rs` becomes the large consumer and weather is moved onto the Tokio runtime. The target changes toward PIC/small code and therefore follows the kernel-facing CABI direction in `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`, whose `crates/trueos-sys/src/vcabi.rs`, `src/r/gfx_cabi.rs`, and zkvm Mio/socket2 shims provide the corresponding runtime boundary.

## 039 — `04ea3700` — 2026-05-07

Original: `ok`

Register the flags example in the root manifest

The five-line `Cargo.toml` change adds `[[example]] name = "flags" path = "flags/main.rs"` and the `trueos-flags` dependency, making the existing flags source discoverable by the launcher. It is the registration precursor to commit 040’s first flags UI and commit 041’s extracted crate/package metadata, consuming the input and graphics CABI direction recorded by kernel `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`.

## 040 — `26243c20` — 2026-05-07

Original: `ok`

Factor reusable games and add the first flags UI

`crates/trueos-tetris` becomes a no-std library containing the Tetris engine plus `bejewled`, `chess`, `minesweeper`, `shell`, and `snake`, while `flags/main.rs` builds a UI2 quiz with `input::pop_keyboard_output`, cursor events, `vgfx::upload_svg_to_texture`, and `trueos_flags::getFlagSVG`. Its manifest points at kernel `trueos-v`, so this is both a reusable-game precursor and a concrete consumer of the virtual-system/CABI types that the following flags extraction and async-fetch commits refine.

## 041 — `901ff722` — 2026-05-07

Original: `ok`

Extract flags into a crate and make weather app-aware

The new `crates/trueos-flags/Cargo.toml` and `flags.rs` move flag data and SVG lookup out of the binary, while `src/main.rs` introduces `PackageAppSpec`, `package_app_specs`, and automatic package builds; `weather/main.rs` adds icons, richer forecast presentation, and a UI2 resize policy. This refactors the flags registration from 039/040 into a package-level catalog and makes weather a first-class consumer of the localized-weather/kernel direction in `1edbef779c4afeb90472da1fb3192d3380420b4b`.

## 042 — `780b44fc` — 2026-05-07

Original: `ok`

Expand the graphics and data-demo catalog

`currency/main.rs`, `mandelbrot/main.rs`, and `particle/main.rs` add network-backed text, Mandelbrot rendering, and textured particle examples, with `particle/parapath.svg` as a concrete asset; `trueos/src/ui2.rs` and `trueos/src/vgfx.rs` add the window/render helpers and weather is reorganized around the expanded UI. These examples consume the same kernel graphics service boundary and anticipate shader-backed rendering in `05a329f47c0d598ed8effe29105a0f43a6ce9c4e`, especially `src/tst/mandelbrot_gpu_sidequest.rs` and `crates/trueos-eu/src/gfx12.rs`.

## 043 — `68ad5f4a` — 2026-05-08

Original: `ok`

Make flags fetching asynchronous and add shared randomness

`trueos-flags` replaces synchronous code-list validation with `getCachedFlagSVG`, `startFlagSVGFetch`, `pollFlagSVGFetch`, and `discardFlagSVGFetch`, while `flags/main.rs` tracks per-slot SVG state and redraws as byte fetches complete; the root `trueos` crate adds `tyche::SoftRng` and `rand`. The fetch helpers model the kernel `trueos_cabi_net_fetch_*` operation lifetime and the flags UI becomes a consumer of the runtime/network lane advanced in kernel `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`.

## 044 — `26c46def` — 2026-05-08

Original: `ok`

Convert flags to byte-fetch consumption and relax the timing probe

`trueos-flags` abandons the file-cache path for `trueos_cabi_net_fetch_bytes_start`, `wait`, `result_len`, `read`, and `discard`, exposing `readFlagSVGFetch` while retaining embedded fallback SVGs; `flags/main.rs` stores the fetched SVGs and `tokio_rt/main.rs` widens its nested sleep/timeout probe to 100/5 ms. This is a stabilization consumer of 043’s async design and aligns with the runtime worker/lane changes in kernel `8326f6636ba87d4864c7d49f456d41ce9c39c4f0` (`src/trueos_tokio_worker.rs`, `src/r/lane.rs`, and `src/r/gfx_cabi.rs`).

## 045 — `4efd7f28` — 2026-05-08

Original: `ok`

Add Retrosun to the application catalog

`retrosun/main.rs` introduces a no-std procedural retro-scene demo using `ui2`, `vgfx`, and `vsys`, and `Cargo.toml` plus `src/main.rs` register and discover it with the other examples. It is a new graphics consumer following the chart/graph/particle lineage and provides a Blueprint-side counterpart to kernel `8326f6636ba87d4864c7d49f456d41ce9c39c4f0`’s graphics-CABI/runtime lane; no new kernel symbol is changed here.

## 046 — `cbf35f86` — 2026-05-08

Original: `ok`

Finish Retrosun’s launcher wiring

The two-file patch completes `retrosun/main.rs` integration and adds the launcher logic in `src/main.rs` needed to recognize and build the registered example through the existing allocator, panic, UI2, and graphics path. It is a consumer-facing cleanup after 045 rather than an ABI change; the kernel history contains no corresponding new symbol in this Blueprint diff, so the exact runtime effect is limited to the already-established graphics/CABI route.

## 047 — `9d924c59` — 2026-05-08

Original: `ok`

Define the Blueprint filesystem metadata boundary

`trueos/src/vfs.rs` introduced `FsNodeKind`, `FsStat`, and `stat` over `trueos_cabi_fs_stat`; `tokio_fs/main.rs` consumed it to distinguish file and directory kinds and check canonicalization, while `STD_REROUTE_MEMO.md` recorded the intended std/Tokio routing. This was the metadata precursor to the later `v::vfs` and `api::fs` layers; kernel `4288c8d9e3585bcaad80f319e46e6862f4236329` confirms the contemporaneous direct-TRUEOSFS read path, but not a new stat implementation in this Blueprint commit.

## 048 — `d43af295` — 2026-05-09

Original: `ok`

Remove obsolete dependency overlays while binding randomness to the kernel

The commit deleted unused HTTP/Tokio overlay entries from `Cargo.toml` and `src/main.rs`, moved those dependencies into lockfile notes in `KNOWN_GOOD.md`, and removed the stale `readme`; `trueos/src/tyche.rs` instead shifted `SoftRng` toward the kernel `sys_rand` ABI with `fill_bytes` and `random_u64`. This is a dependency cleanup and service-binding stabilization around kernel `22bb5e4ecf85a9054aa08579ccb744ddac061ef1`’s Tokio/Tungstenite compatibility, not a new network protocol.

## 049 — `07c3f8b1` — 2026-05-13

Original: `ok`

Make vendored kernel/runtime dependencies resolvable from the app workspace

The root and app manifests rewrote `trueos-gfx-core`, `trueos-tetris`, `trueos`, Tokio, Mio, socket2, Hyper, and `trueos-sys` paths from sibling `../TRUEOS` locations to repository-relative paths. Although the diff is layout-only, it is the enabling cleanup for subsequent platform-facade and service-app builds to consume the kernel’s vendored runtime without an accidental checkout-path dependency.

## 050 — `955264b0` — 2026-05-13

Original: `ok`

Prepare the root API crate for a shared platform facade

The small `src/lib.rs` cleanup removes the redundant `extern crate alloc` placement and adjusts the std/no-std cfg boundary. It is a direct preparatory refactor for 051, where applications stop importing service-specific details and begin using one platform vocabulary over kernel-backed time, polling, filesystem, and networking.

## 051 — `d4676c85` — 2026-05-13

Original: `ok`

Introduce the platform facade and package-aware service build planning

`src/lib.rs` added `platform::{String, Vec, Path, thread, io, future}`, while the filesystem/network/runtime probes and `currency` migrated to those imports; `.cargo/config.toml`, `src/build_plan.rs`, and `src/main.rs` began staging kernel overlays and materializing package metadata instead of making each app know the vendor layout. This is the architectural bridge from the individual Tokio/fs/net probes in 030–032 to the web services in 052, and it packages the kernel’s poll/time/fs/socket services behind stable names rather than adding a kernel symbol.

## 052 — `da031b4d` — 2026-05-13

Original: `ok`

Bring chat, file, device, and mail servers into the service-facing workspace

The commit imported `servers/chatserver`, `servers/fileexplorer`, `servers/webdevices`, and `servers/webmail`, including Hyper/Axum handlers, `servers/common/tailwind.css`, and the framework probe; it also synchronized the `localcoder` pointer. These are first concrete web consumers of the platform/Tokio/network/filesystem direction, with kernel-side context in `8326f6636ba87d4864c7d49f456d41ce9c39c4f0`, `ec4980123082ea08fc5b2da9bacc8c97ecf11060`, and the localcoder ports `b0763055b86289001d6a002e5cd8a36f2b51416`/`b108852e15c71661cd44d6de3574cb7b763eee73`.

## 053 — `88e8e262` — 2026-05-13

Original: `ok`

Make framework and weather services first-class launcher targets

`framework_stack/main.rs` gained a serde frontmatter probe, `weather/main.rs` was aligned with the shared weather path, and `src/main.rs` changed the no-argument flow to build declared examples plus package apps discovered from manifests. This closes the consumer loop opened in 052: Hyper/Tokio and weather workloads are now repeatable Blueprint targets without another low-level ABI layer.

## 054 — `b1c21660` — 2026-05-13

Original: `ok`

Separate build-plan analysis from packer execution

`src/build_plan.rs` extracts `BuildTarget`, `BuildFlavor`, `BuildSettings`, source inspection, feature directives, and Tokio-network detection from the launcher, while `src/main.rs` retains staging and linking and the `localcoder` pointer advances. This refactor makes the target overlay and no-std/runtime decisions inspectable before Cargo runs, preparing the SDK manifest work in 055–057 against kernel Tokio changes `8326f663` and `c3a28e79`.

## 055 — `30c5fa2f` — 2026-05-13

Original: `ok`

Establish an explicit Blueprint SDK manifest

`BLUEPRINT_SDK_MANIFEST.md`, `sdk/manifests/sdk.toml`, and `sdk/targets/trueos-blueprint.json` define target/cfg, libc, std, and Tokio compatibility phases; `src/main.rs` begins SDK-aware packaging and vendored newlib gains `Ioctl` declarations. This turns 054’s extracted planning into a documented dependency order—libc/std before Tokio—for apps consuming kernel ABI and runtime overlays.

## 056 — `b1030c2e` — 2026-05-14

Original: `ok`

Record SDK readiness and app resource requirements

The SDK manifest and documentation add explicit compatibility/readiness language and list app-facing requirements such as `fs`, `net.socket`, and `ui2`, synchronized with the `localcoder` state. This stabilization makes the service contract from 055 visible before launch instead of reducing readiness to whether individual `trueos_cabi_*` symbols happen to resolve.

## 057 — `e352ca47` — 2026-05-14

Original: `ok`

Move the SDK target toward PIC and newlib compatibility

`sdk/manifests/sdk.toml` records `relocation_model = "pic"` and `code_model = "small"`, while `sdk/targets/trueos-blueprint.json` switches from newlib to musl and `src/main.rs` improves libc overlay selection, lock matching, symlink replacement, Rust-main detection, and source shims; vendored libc also adjusts pthread/unistd routing and `Ioctl`. It is the build-compatibility refinement after 055–056, not a new app API, and bridges the SDK’s artifact assumptions to the kernel std/libc work.

## 058 — `9257ab49` — 2026-05-14

Original: `ok`

Move the launcher into a standalone host/app workspace

The former root examples, crates, servers, weather code, probes, and `localcoder` pointer move under `apps/`, while `host-cli/Cargo.toml` and `host-cli/src/{main.rs,build_plan.rs}` take over host orchestration and the root becomes a workspace shell. This structural split follows 054–057: SDK/API target code can now be built for the kernel without compiling the host launcher as target code.

## 059 — `6342e33f` — 2026-05-14

Original: `ok`

Remove unrelated vendored configuration dependencies

The cleanup deletes `vendor/gix-config-value-0.17.2`, `vendor/gix-sec-0.13.3`, and `KNOWN_GOOD.md`, while adding `host-cli/.cargo/config.toml` for host build-std settings. It narrows 058’s new workspace to actual Blueprint app/API/kernel-overlay dependencies and keeps Git configuration ABI out of the initial catalog.

## 060 — `1cf17b19` — 2026-05-14

Original: `ok`

Restore a root build plan over the apps workspace

`hello_world`, `petersen`, and `svg_grid` move to `apps/*`; `src/main.rs` and `src/build_plan.rs` return to the root; and `sdk.toml`/`trueos-blueprint.json` become root metadata while the temporary `host-cli` wrapper and old paths disappear. The three no-std consumers retain `TrueosAllocator`, UI2 surface creation, and `vgfx` uploads, so this is a layout correction that preserves the 055–057 SDK contract independently of host CLI packaging.

## 061 — `ace9c139` — 2026-05-14

Original: `ok`

Normalize app manifests and packaged entrypoints

Each of `apps/hello_world`, `apps/petersen`, and `apps/svg_grid` gets a `Cargo.toml` and `src/main.rs`; root `.cargo/config.toml` gains host target/build-std settings; and `src/main.rs` discovers package metadata while generating `hello_world.bp`. This makes the relocated consumers from 060 uniform for the kernel-facing artifact pipeline and sets up the SDK rename in 062.

## 062 — `2e27b3a3` — 2026-05-14

Original: `ok`

Replace the legacy wrapper with the SDK package

The `trueos` wrapper moves to `sdk`, the three apps depend on `../../sdk`, `hello_world.bp` is removed, and the former `trueos/src/{input,net_fetch,tyche,ui2,vfs,vgfx,vgfx_hosted,vshell,vsys}.rs` tree is recreated under `sdk/src`. This is a direct ownership refactor after 061: the SDK becomes the canonical Rust layer over kernel `trueos-sys` CABI services instead of maintaining a duplicate app wrapper.

## 063 — `f31c8734` — 2026-05-14

Original: `ok`

Publish the SDK as the shared TrueOS API crate

The complete SDK tree moves to `api/`, `api.toml` becomes its manifest, apps switch to `../../api`, and `apps/target.json` becomes the target source; `api/src/{input,net_fetch,tyche,ui2,vfs,vgfx,vgfx_hosted,vshell,vsys}.rs` now define the public wrapper boundary. This finalizes the 062 migration: HID, fetch, time/randomness, UI2, filesystem stat, graphics, shell, and polling consumers have a stable API package over the kernel CABI.

## 064 — `6544b30d` — 2026-05-15

Original: `ok`

Populate the public API with the first broad service-consumer catalog

The API renamed `input.rs` to `hid.rs` while retaining `pub use hid as input`, removed `api/src/net_fetch.rs`, and added chart, currency, flags, Tetris, weather, Mandelbrot, particle, Retrosun, framework, Tokio probes, and server apps under `apps/`. This is the catalog-scale consumer pass for the kernel `trueos-v`/`trueos_sys` CABI surface, establishing the workload set that 065–070 then consolidate behind shared transport, platform, and rendering modules.

## 065 — `e2fc9e3d` — 2026-05-15

Original: `ok`

Centralize asynchronous byte fetching for currency and weather

`api/src/vnet.rs` and `apps/crates/trueos-weather/src/transport.rs` added `fetch_bytes`/`fetch_text` over the `trueos_cabi_net_fetch_bytes_start`, `wait`, `result_len`, `read`, and `discard` lifecycle, and the currency/weather apps deleted their hand-rolled polling loops. This is a focused refactor of the broad catalog in 064: parsing remains app-owned while operation ownership moves to one kernel-facing API boundary.

## 066 — `774cd567` — 2026-05-15

Original: `ok`

Add one platform/Tokio facade for the catalog’s service dependencies

`api/src/lib.rs` added `platform`, `diag`, `runtime`, `task`, `sync`, `time`, `io`, `fs`, and `net` re-exports, while `api/Cargo.toml` gained runtime/net features and the apps migrated to those names; `src/build_plan.rs` and `src/main.rs` made source overlays and package metadata drive the isolated builds. This consolidates the transport work in 065 and gives filesystem, Mio/socket2, Tokio, and poll consumers stable imports over the kernel ABI.

## 067 — `12215fc3` — 2026-05-15

Original: `ok`

Split artifact construction from remote Blueprint publication

`src/artifact.rs` took over tool discovery, Cargo object/rlib collection, entry-symbol detection, and compressed Blueprint generation, while `src/publish.rs` owned `.bp` enumeration, remote cleanup, `gio mount`/mkdir/copy/remove, and the `TRUEOS_BLUEPRINT_*PUBLISH*` controls; `src/main.rs` became the coordinator. This refactor turns the kernel-facing relocatable object into a named distributable artifact and follows the catalog validation/release work in kernel `0fd9132c4f3e2fb5c25e9ffec5d3c131a09b9368`.

## 068 — `ee3e061a` — 2026-05-15

Original: `ok`

Consolidate UI2 graphics behind one rendering module

`api/src/ui2/gfx.rs` becomes the home for `RgbVertex`, texture upload/status, SVG/PNG/JPEG helpers, triangle queues, and Mandelbrot rendering; `ui2::SurfaceWindow` calls it, `vgfx` re-exports it, and `vgfx_hosted` reuses its immediate upload helpers while the chart, currency, flags, Tetris, particle, Petersen, Retrosun, SVG, and weather consumers migrate. This is a refactor/stabilization of the many graphics examples from 033–046 into one API contract over kernel UI2/gfx CABI, with the kernel graphics implementation evidenced by `src/r/gfx_cabi.rs` in `8326f6636ba87d4864c7d49f456d41ce9c39c4f0`.

## 069 — `0b87d224` — 2026-05-15

Original: `ok`

Rename the virtual-system module to the application-facing platform contract

`api/src/vsys.rs` became `api/src/platform.rs`, `api/src/lib.rs` exposed the new module, and the first app group migrated `vsys::log_*`, `poll_once`, and `sleep_ms` calls to `platform::*`. This is an API naming stabilization over unchanged CABI behavior, clarifying the consumer contract after 066 and before the final import cleanup in 070.

## 070 — `95e3c981` — 2026-05-15

Original: `ok`

Finish the platform import migration across service and graphics consumers

The commit settled `api::platform` exports including `String`, reordered the prelude and Tokio/Mio re-exports, and migrated chart, Tetris, flags, Mandelbrot, particle, Petersen, Retrosun, SVG, triangle, and `tokio_net` to the new names. It is the cleanup/stabilization endpoint of 069: no kernel symbol changes are evidenced, but every listed consumer now speaks the shared HID/platform/network/graphics API.

## 071 — `e67710fa` — 2026-05-15

Original: `ok`

Normalize alloc imports across weather and graphics consumers

The commit only groups `alloc::format`, `String`, `Vec`, and `vec` imports across `apps/chart`, `apps/flags`, `apps/particle`, `apps/petersen`, and `trueos-weather` (`frog.rs`, `lib.rs`, `oc3.rs`, and `transport.rs`). It is a compatibility-preserving cleanup after the API/rendering consolidation in 068, keeping no-std/alloc consumers consistent with the CABI-backed app model exposed through the kernel `crates/trueos-v` wrappers; no kernel path changes are evidenced.

## 072 — `b82a51c5` — 2026-05-15

Original: `ok`

Centralize allocator and panic defaults for app binaries

The catalog binaries stop importing `TrueosAllocator` and declaring per-file global allocators while retaining `panic_abort`, and `hello_world`, `petersen`, and `svg_grid` switch to `trueos.workspace = true`. This makes the build planner select `trueos/default-global-allocator` and `trueos/default-panic-handler` centrally, a packaging-side stabilization of the kernel allocator/ABI boundary rather than a new app API.

## 073 — `0b186e6d` — 2026-05-15

Original: `ok`

Let the shared API own panic handling for catalog examples

`apps/chart`, `cli_tetris`, `flags`, `full_tetris`, `mandelbrot`, `particle`, `petersen`, `retrosun`, `svg_grid`, and `triangle` remove local `PanicInfo`, `panic_abort`, and `#[panic_handler]` boilerplate. This completes 072’s allocator centralization: thin apps use the feature-selected shared handler and its kernel-facing logging route, with no new kernel symbol or runtime behavior introduced in the cleanup.

## 074 — `b63a3a88` — 2026-05-15

Original: `ok`

Make build planning understand workspace applications

`src/build_plan.rs` stops using `trueos::platform` as the platform-support signal, while `src/main.rs` detects a real `trueos` dependency before injecting `trueos/default-global-allocator` and `trueos/default-panic-handler`; staged manifests materialize `trueos.workspace = true` as `../../api`. This is the dispatch/build refactor required by 072–073, preserving the API’s kernel-backed services while allowing isolated target builds to link them uniformly.

## 075 — `b6af7c5b` — 2026-05-15

Original: `ok`

Add a shared global logging API and migrate consumers

`api/src/globalog.rs` introduces `Level`, `LogRange`, `LogAmount`, `LogMessage`, `log`, `log_with_level`, `log_with_concept_level`, and excerpt helpers; `diag` routes through it and `platform.rs` drops the older `log_info*`/`log_error*` helpers. Graphics, HID, platform, panic, and `test_some_crates` call sites now use the stream-aware formatter, matching the kernel `trueos_cabi_write`/Blueprint log plumbing in `src/hv/blueprint/blueprint.rs` and `src/r/mod.rs` from `5bc8e2135d793385790533736255974e24bf7e65`.

## 076 — `b1a64a69` — 2026-05-15

Original: `ok`

Simplify shared logging imports in every example

The functional change is limited to `apps/test_some_crates/test_some_crates.rs`: fully qualified `trueos::globalog::log_with_level` calls become `use trueos::globalog::{self, level}` plus `globalog::log_with_level` for serde/JSON/regex success and failure messages. It is a call-site cleanup immediately after 075, preserving the `globalog` contract and the kernel-backed write path rather than changing logging semantics.

## 077 — `28f9dcd4` — 2026-05-15

Original: `ok`

Add `logl` as the short application logging facade

`api/src/logl.rs` re-exports the global logging types/constants and supplies compact `log`, `plain`, `concept`, and `excerpt` wrappers; `api::prelude` exports it and `panic_abort` now uses `logl`. The catalog examples, including flags, Tetris, Mandelbrot, particle, and `test_some_crates`, migrate from `globalog::log_with_level` to `logl::log`, making 075’s stream logger easier to consume while retaining the kernel `trueos_cabi_write` boundary.

## 078 — `2c76f5a7` — 2026-05-15

Original: `ok`

Move catalog binaries to ordinary Rust `main`

The example binaries remove `#![no_main]` and `#[unsafe(no_mangle)] pub extern "C" fn main()`, retain `#![no_std]`, and use private Rust `fn main()`. This shifts entry-symbol retention and packaging responsibility to the Blueprint packer, preparing the registry/dispatch work in 079–083 and matching the kernel’s relocatable Blueprint launch/import path without changing UI2 or graphics APIs.

## 079 — `436e6a5d` — 2026-05-15

Original: `ok`

Introduce an explicit application registry

`apps.json` becomes authoritative for `hello_world`, `petersen`, and `svg_grid`; `src/main.rs` adds `AppRegistry`, reads registered names, defaults builds to Release, caches Cargo under the detected Blueprint root, and dispatches through `package_app_spec`. This replaces inferred directory membership with an explicit catalog contract for the app object that will be launched by the kernel Blueprint VM, and is the precursor to 080’s validation and 083’s full catalog expansion.

## 080 — `bd449b6c` — 2026-05-15

Original: `ok`

Validate registered applications before dispatch

`src/main.rs` replaces filesystem scanning with `registered_app_names`, `package_app_spec_required`, and `package_app_spec`, rejecting empty names, duplicates, missing `apps/<name>/Cargo.toml`, and package-name mismatches. This is the registry stabilization step after 079: `apps.json` becomes a checked dispatch contract before an unexpected module can reach the kernel Blueprint loader.

## 081 — `22ea3936` — 2026-05-15

Original: `ok`

Reformat registry cache and validation code

This commit only reformats the `blueprint_root` cache-path closure and the registered-app missing-manifest error in `src/main.rs`; computed paths and validation behavior are unchanged. It is a compatibility-preserving cleanup in the 079–080 dispatch chain, with no new Blueprint API or kernel-facing symbol.

## 082 — `4b35c6d4` — 2026-05-15

Original: `ok`

Link artifacts from each object’s `.rlink` metadata

`src/artifact.rs` adds `collect_rlibs_for_object`, `printable_tokens`, and object-specific `.rlink` parsing, while `src/main.rs` stops collecting every dependency `.rlib` and fails clearly on missing or unreadable metadata. The packer now links only the libraries recorded for the selected registry app object, an artifact-layer stabilization between 079–081 dispatch and 083’s workspace-wide catalog, reducing accidental cross-app linkage before kernel loading.

## 083 — `990c6b56` — 2026-05-15

Original: `ok`

Expand the workspace and registry to the full application catalog

`Cargo.toml` and `apps.json` add the chart, currency, flags, Tetris, framework, Mandelbrot, particle, Retrosun, crate probe, Tokio probes, triangle, and weather entries, with per-app manifests and no-default-feature dependencies; `build_plan.rs` learns `[[bin]]` `path` entries and `src/main.rs` materializes workspace dependencies such as `trueos-flags`, `trueos-weather`, `trueos-tetris`, and `trueos-gfx-core`. `src/artifact.rs` now requires `.rlink` dependencies and accepts absolute or `deps`-relative rlib tokens, completing the registry/dispatch/artifact chain begun in 079–082 for the catalog consumers consolidated in 068.

## 084 — `4081aa48` — 2026-05-15

Original: `ok`

Vendor libc for the hosted TrueOS target

The commit adds the complete `vendor/libc-0.2.185` source tree and changes overlay diagnostics in `src/main.rs` to print patched crates as `name=path`. This is the concrete dependency consumer of the libc routing staged in 035–036 and complements, rather than replaces, the kernel `src/std_abi_shim.rs` ABI implementation.

## 085 — `ec773d42` — 2026-05-15

Original: `ok`

Add the hosted std compatibility ABI

`api/src/std_abi.rs` exports errno, `pthread_self`, pthread attribute/name/create/join/detach stubs, `sched_yield`, and `getenv` under `tokio-runtime`; `pthread_self` delegates to `trueos_cabi_thread_current_id` and `pthread_create` reports `EINVAL`, while `src/main.rs` locks the TokioStd overlay and hides the libc pin. This extends 084’s vendored declarations into callable symbols for hosted Tokio consumers and bridges kernel `5bc8e213`’s `src/std_abi_shim.rs` and `119736a2`’s Tokio trueos-std bindings.

## 086 — `15952974` — 2026-05-15

Original: `ok`

Make localcoder and weather fit the hosted TrueOS build contract

`apps.json` registered localcoder, its manifest moved reqwest/Tokio/serde and shell/editor dependencies to workspace declarations, and `apps/localcoder/trueos.json` selected musl, PIC, and the small code model; weather centralized `DEMO_JSON` and consumed it from `apps/weather/main.rs`. The corresponding `src/main.rs` dependency/version materialization and diagnostic filtering prepare a std/network application for the kernel’s Tokio, DNS, and std-ABI overlays, extending the package planning introduced in 066.

## 087 — `a02f5375` — 2026-05-15

Original: `ok`

Adapt localcoder’s LSP and shell tools to freestanding filesystem and process paths

`apps/localcoder` began depending on `trueos`, converted LSP working directories with `to_string_lossy`, moved `Stdio`/`Output` to Tokio process imports, and replaced reqwest URL parsing with explicit `file://` percent encoding/decoding in `services/lsp/types.rs`; `bash_tool.rs` retained `tokio::process::Command`. These are consumer-side corrections to the hosted std/Tokio boundary from 086, with no new kernel API evidenced in the diff.

## 088 — `213429b9` — 2026-05-15

Original: `ok`

Extract a reusable currency UI over the TrueOS network and UI2 services

`apps/crates/trueos-currency` introduced `CurrencyAppConfig`, `run_currency_app`, FXFeed parsing for EUR/GBP/JPY, loading/error snapshots, and RGBA composition through `ui2::SurfaceWindow` and `gfx::upload_texture_rgba_image_now`; `apps/currency_reqwest/main.rs` supplied the async fetcher and `tokio-net-probe` was registered. This makes one concrete UI/network consumer from the service facade in 066, with the kernel Tokio/UI2 direction represented by the existing CABI-backed wrappers rather than a new Blueprint ABI.

## 089 — `9d5e1d69` — 2026-05-15

Original: `ok`

Co-locate reusable service crates with their owning applications

The reusable `trueos-currency`, `trueos-flags`, and `trueos-weather` crates moved from `apps/crates/*` beside `currency_reqwest`, `flags`, and `weather`, preserving their source APIs while `src/main.rs` gained the path mappings needed to build them. This is a catalog/build-layout refactor after 088: the network helpers still target the existing `trueos_cabi_net_fetch_bytes_*` contract, and no kernel behavior can be attributed to the move.

## 090 — `23ecad38` — 2026-05-15

Original: `ok`

Exercise asynchronous reqwest networking from flags and weather

`apps/flags/main.rs` adopted `runtime::current_thread_net`, a timed reqwest client, sequential `fetch_round_svgs`, and `flag_url`, while `apps/weather/main.rs` used the same net runtime for geocoding and forecast fallback; manifests enabled reqwest JSON/rustls. This temporarily moved the apps above the byte-fetch helper from 065 and made the Blueprint workloads consumers of the kernel Tokio/Mio/DNS/TLS path, before the later worker-ABI rollback in 098.

## 091 — `906e3b68` — 2026-05-15

Original: `ok`

Make artifact linking and the target std ABI resilient

`apps/target.json` changes `env` from musl to newlib; `api/src/lib.rs` exports `abort`, `_Unwind_Backtrace`, and `_Unwind_GetIP`; vendored newlib gains pthread, signal, directory, vector-I/O, clock/sleep, and `stat64` declarations; and `src/artifact.rs`/`src/main.rs` parse Cargo JSON, locate rlibs, identify/globalize `main`, and link with GC plus an explicit undefined entry. This stabilizes 063’s public API and 085’s hosted ABI at the actual Blueprint object boundary, corresponding to the kernel std shim rather than adding a high-level app service.

## 092 — `0bd97c5f` — 2026-05-15

Original: `ok`

Give Tokio consumers a compact, explicit facade

`api/src/lib.rs` added feature-gated `t::{fs, io, net, runtime, sync, task, time, tokio}` and exported `t` through the prelude; `tokio_fs`, `tokio_net`, and `tokio_rt` migrated their probes to it. This is a naming/conformance refactor over the same file, Mio/UDP/TCP, task/sync/select, and timing checks from 030–031, making the supported kernel-backed surface easier to audit.

## 093 — `9f57718b` — 2026-05-15

Original: `ok`

Add stage-level observability to service and runtime probes

Currency, weather, and `framework_stack` replaced `bp_info!`/`bp_error!` with `logl::log` stages, while the Tokio filesystem/network/runtime probes were rewritten against `t` with explicit checkpoints for builders, sockets, filesystem operations, tasks, synchronization, and timeouts. The change is a stabilization consumer for the readiness-sensitive kernel runtime, making failures at the shared platform boundary diagnosable without changing the network or filesystem APIs.

## 094 — `9e0659b1` — 2026-05-15

Original: `ok`

Diagnose allocation failures at the kernel CABI allocator boundary

`TrueosAllocator` added bounded `AllocDiagLine`/`log_alloc_null` records for operation, reason, size, alignment, and realloc size when `trueos_cabi_alloc`/`calloc`/`realloc` fail or alignment is too large; the prelude also restored `t` and allocator exports. This is runtime hardening for the hosted std/Tokio consumers, complementing the service-stage logging in 093 rather than changing the allocator ABI itself.

## 095 — `122d7da1` — 2026-05-15

Original: `ok`

Add aligned allocation support and capture abort-entry diagnostics

`api/src/lib.rs` imported `posix_memalign`/`free`, used aligned allocation for layouts above the base word alignment, zeroed and copied aligned reallocations, and logged stack/frame pointer words through `log_abort_entry` from `abort` and the panic handler; currency also diagnosed a failed default reqwest builder before probing insecure TLS. This extends 094’s allocator visibility for hosted code and records the low-level entry state needed when a kernel-backed service consumer aborts.

## 096 — `de752f2e` — 2026-05-15

Original: `ok`

Preserve reqwest error chains during network diagnosis

`apps/currency_reqwest/main.rs` added debug-formatted request/body failure records and `log_error_sources`, walking up to eight `core::error::Error::source()` causes without changing the fetch path. It is a diagnostic stabilization of the reqwest/TLS consumer introduced in 088–090 and complements, rather than replaces, the kernel-facing transport boundary.

## 097 — `5e03fb14` — 2026-05-15

Original: `ok`

Make the insecure TLS fallback an explicit service state

Currency removed the attempted default reqwest builder and labeled its fallback `reqwest.client.build.insecure_tls`; flags gained the same timeout/debug/source-chain helper and `tls_danger_accept_invalid_certs(true)`. This documents a transport compromise in the consumers from 090 instead of hiding it behind a generic connection failure, setting up the later choice between direct reqwest and worker fetch in 098–103.

## 098 — `88e71c65` — 2026-05-15

Original: `ok`

Route flags and currency back through the worker byte-fetch ABI

Currency and flags replaced their reqwest client/request plumbing with `vnet::fetch_text`, while weather deleted `transport.rs`; the builder also selected the vendored `hyper-rustls` overlay needed by that path. This is a deliberate transport refactor after the direct reqwest experiments in 090 and 097, returning the apps to the `trueos_cabi_net_fetch_bytes_*` lifecycle before 099 moves its implementation behind `crates/trueos-v`.

## 099 — `ede93e09` — 2026-05-15

Original: `ok`

Implement the public vnet facade through the kernel-adjacent virtual runtime

`api/src/vnet.rs` stopped calling raw CABI declarations and delegated start/wait/result/read/discard to `v::vnetfs`, handling zero-length bodies and centralizing `fetch_error_string`; the flags crate received the same `vnetfs` dependency. This makes the consumer path match `crates/trueos-v/src/vnetfs.rs` and preserves the existing operation IDs/error codes, completing the lower-level refactor begun in 098.

## 100 — `23e5bd33` — 2026-05-15

Original: `ok`

Bound virtual-network reads and remove the obsolete flags adapter

`api/src/vnet.rs` capped its returned `Vec<u8>` to the reported result length, while the standalone `apps/flags/trueos-flags` manifest and source were deleted after the app moved to direct worker networking. The cap is a concrete contract-safety fix for 099’s `v::vnetfs` path, and the deletion is cleanup of the older CABI adapter rather than a new kernel change.

## 101 — `afc104ad` — 2026-05-16

Original: `ok`

Sanitize remote SVG roots before composing the flags UI

`apps/flags/main.rs` added `root_svg_attr` and `push_filtered_svg_attrs`, extracted the SVG head/body, synthesized a missing `viewBox`, dropped incoming `x`, `y`, `width`, and `height`, and placed each flag into the four-cell board. This hardens the UI2 consumer while retaining the worker/runtime networking stabilized in 098–100; it changes application composition, not the graphics CABI.

## 102 — `0f2693fe` — 2026-05-16

Original: `ok`

Add kernel cursor coordinates while reopening direct network diagnostics

`api/src/hid.rs` exposed `cursor_pos` and `cursor_buttons` over `trueos_cabi_input_cursor_pos`/`trueos_cabi_input_cursor_buttons`, and flags mapped screen coordinates into its content rectangle and 2×2 hit grid; currency moved back from `vnet` to direct reqwest with status/body logging. The commit is therefore both a HID consumer correction and a diagnostic reversal of 098–100, using the corresponding `crates/trueos-v/src/vinput.rs`/`vnetfs.rs` wrappers without establishing a new kernel commit in the Blueprint diff.

## 103 — `55b5fd17` — 2026-05-16

Original: `ok`

Mark insecure certificate acceptance at every live network consumer

Currency and weather set `tls_danger_accept_invalid_certs(true)` and emitted WARN records naming insecure TLS; weather also recorded detailed client-build errors. This is a stabilization/operational honesty pass over the direct reqwest path in 102, preserving the apps’ FXFeed/OpenWeather behavior while making the transport limitation visible to users and operators.

## 104 — `7a301072` — 2026-05-16

Original: `ok`

Expose clock and VFS primitives while moving chat toward an app-owned service

`api/src/clock.rs` exported `ntp_current_unix_seconds` through `v::vclock`, `api/src/vfs.rs` added read/read-UTF8/write/create-dir wrappers over `v::vfs`, and the chat Tailwind asset moved beside `apps/chatserver` as the old `apps/servers/chatserver/server.rs` was removed. The flags/currency diagnostics were adjusted in the same pass, preparing 105’s userspace chat server to consume the kernel CABI clock/filesystem path associated with kernel `2d07235d9d82dcb014ef7bb2cd2cc4056731eef8`.

## 105 — `eff38b8a` — 2026-05-16

Original: `ok`

Make chat a packaged Tokio/axum service over TrueOS clock and filesystem APIs

`apps/chatserver/server.rs` became a workspace binary using axum, `trueos`, and `trueos-chat`: it served `/`, `/tailwind.css`, `/api`, and `/api/rooms/{room}/messages` on port 3, loaded/saved `chat/rooms.json` through `vfs`, timestamped messages with `clock::ntp_current_unix_seconds`, and batched persistence. The same commit moved file explorer, webdevices, and webmail assets into app packages, making the web consumers from 052 concrete clients of the kernel-side chat/runtime work represented by `896a469cadfa8a484333c3b9e92543c94bd76556`.

## 106 — `9d6b6d20` — 2026-05-16

Original: `ok`

Move the web service set onto the shared TrueOS API facade

The commit added package manifests for `apps/fileexplorer`, `apps/webdevices`, and `apps/webmail`, moved file explorer off std/Embassy and old `crate::r` filesystem calls onto `trueos::{clock, logl, platform, runtime, time, tokio, vfs}`, and gave webmail its own server entrypoint; `apps/common/tailwind.css` became the shared asset. This is the consumer migration after chat in 105 and aligns the web services with kernel webmail/static routes in `b12942170da8442abc1c5b148f9d988538d51b2b` and the Mio/Tokio substrate later identified by `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`.

## 107 — `bf998b8b` — 2026-05-16

Original: `ok`

Make chat persistence asynchronous and standardize packaged HTTP entrypoints

`apps.json` registered the four web services, `apps/http_rust_server_template` added a reusable `Cargo.toml`/`server.rs`, and `apps/chatserver/server.rs` changed its persistence from `std::sync::Mutex` to `trueos::tokio::sync::Mutex`, making load/snapshot/save helpers awaitable while retaining `chat/rooms.json`. The root launcher also learned package-bin selection and staged metadata, completing the async service migration begun in 105 and matching kernel Tokio worker integration `c3a28e797e1dde3a0961b465a91211813dd1506a`.

## 108 — `06465e28` — 2026-05-16

Original: `ok`

Give independently packaged web services the same visual asset surface

The common 920-line Tailwind stylesheet was copied into `apps/fileexplorer/tailwind.css`, `apps/webdevices/tailwind.css`, and `apps/webmail/tailwind.css`, and each server’s embedded asset/HTML handling was updated accordingly. This is a packaging and presentation cleanup after 106–107: it preserves one TRUEOS visual vocabulary without altering the web, Tokio, or filesystem service APIs.

## 109 — `0f4c85aa` — 2026-05-16

Original: `ok`

Register Matrix and harden external network-service packaging

`.gitmodules` and `apps.json` added the `matrix-construct/tuwunel` app pointer, while `src/build_plan.rs`/`src/main.rs` added bindgen clang arguments, package-bin naming, generated-lock discovery, nested manifests, `getrandom_backend="unsupported"`, and virtual package-app handling. The superproject proves the Matrix registration and build-planner adaptations, but the submodule’s internal service code and kernel calls are unavailable here; the visible packaging boundary follows the TRUEOS V userspace ABI established by `9cefefeb65377ecab9b8bd066f5caa6ccdcb263b`.

## 110 — `22994236` — 2026-05-16

Original: `ok`

Teach the packer to stage Helix and Matrix virtual workspaces

The commit registered Helix at `8c41b11607924f7584b77c8a6e6b16439a2f559f`, advanced the Matrix pointer to `ed3c583e8e08119c315565a2adeb3b7d20016751`, and added `virtual_package_app_manifest_path`, `virtual_package_app_alias`, `is_helix_app_dir`, temporary Helix work directories, no-default-features builds, host include paths, and RocksDB flags to `src/main.rs`. Only the pointer identities are available for Helix/Matrix internals, so the evidenced change is the build-system consumer path for large no-std/ABI-sensitive apps, not their hidden kernel behavior.

## 111 — `979ec0e9` — 2026-05-19

Original: `ok`

Extract a reusable no-std chat engine and reconnect filesystem application packaging

`apps/chatserver/trueos-chat/src/lib.rs` introduced bounded `ChatConfig`, `ChatRequest`/`ChatResponse`, room/message sanitization, `/api/rooms` routing, JSON snapshot import/export, and `statement`, letting the server consume a no-std model instead of owning it; the commit also registered the `apps/file-system` submodule and adjusted workspace/API manifests. The local chat library is concrete evidence of a consumer refactor following 105–107, while the external filesystem pointer’s internal APIs and kernel interaction remain unavailable without its submodule history.

## 112 — `529097e8` — 2026-05-19

Original: `ok`

Align the SDK with refreshed libc and Tokio snapshots

The commit removes `vendor/libc-0.2.185`, points `src/main.rs::source_overlay_patches` at libc 0.2.186, and advances workspace/materialized Tokio pins from 1.52.1 to 1.52.3. It is a dependency stabilization after 084–091, keeping the API/packer overlay aligned with the kernel’s vendored Tokio/Mio environment from `7d957a74`; no new Blueprint symbol is introduced.

## 113 — `385ab91f` — 2026-05-19

Original: `ok`

Preserve the reproducible TrueOS rust-src overlay

`toolchain_diff/README.md`, `reapply_trueos_toolchain.sh`, and `trueos-nightly-rust-src.patch` document patching `library/std`, copying `TRUEOS/vendor/libc-0.2.186` into rust-src, regenerating its checksum, pinning rust-src’s `Cargo.lock`, and carrying that overlay from `src/main.rs`. This operationalizes the libc/std assumptions established in 085, 091, and 112 at the synthetic `x86_64-unknown-trueos` toolchain boundary, alongside kernel ABI commit `9cefefeb`.

## 114 — `86a8116e` — 2026-05-19

Original: `ok`

Make artifact discovery match the TrueOS toolchain overlay

`src/artifact.rs` adds sysroot/PATH tool discovery, `latest_cargo_object`, `.rlink` parsing through `collect_rlibs_for_object`, and artifact-stem normalization; `src/main.rs` reports the selected object/rlib and applies the additional rust-src patch. This is the consumer/stabilization step after 113: the packer no longer assumes a host linker layout when assembling the no-std ABI objects and dependencies.

## 115 — `c14912e0` — 2026-05-19

Original: `ok`

Consolidate application consumers onto one `trueos` API

The application set is rewritten around the shared `api/src/{lib,platform,tyche,ui2,gfx,vnet}.rs` facade: chatserver, fileexplorer, webdevices, webmail, Tokio probes, weather, currency, and games leave the removed `v`/host paths, while Helix and Matrix are removed from the active submodule list. The large `src/main.rs` refactor adds alias-aware package discovery, source overlays, target/build-std fixes, and feature propagation, making 083’s catalog consume one userspace boundary over the kernel ABI introduced by `9cefefeb65377ecab9b8bd066f5caa6ccdcb263b` (`crates/trueos-v/src/{lib.rs,iter.rs}` and `src/main.rs`).

## 116 — `b10fbfea` — 2026-05-19

Original: `ok`

Move Tetris and fd onto the consolidated UI2 surface

`api/src/ui2.rs` adds the missing window/graphics exports, `apps/crates/trueos-tetris` switches from `v::vled::Rgb8` to `trueos::ui2::Rgb8`, and `src/main.rs` gains staging/package handling for the external fd and Tetris inputs; the `apps/fd` pointer also advances. This is a concrete consumer migration after 115 from the old V path to the consolidated ABI-backed API, with kernel `9cefefeb65377ecab9b8bd066f5caa6ccdcb263b` as the evidenced V userspace ABI provider.

## 117 — `a2511ba4` — 2026-05-19

Original: `ok`

Import Kibi as the first full terminal-editor workload

`apps/kibi` adds a complete Rust editor with `src/editor.rs`’s editor loop, `row.rs` text rows, `syntax.rs` and `syntax.d/*.ini` highlighting, configuration, terminal support, licenses, and `kibi.desktop`. It is an application-level stress consumer of the consolidated filesystem and console surface rather than a new kernel service; the relevant kernel lineage is TRUEOSFS support in `7ca96227a6c7e694c6115a4a3a42717284f9f25c` and the userspace ABI in `9cefefeb65377ecab9b8bd066f5caa6ccdcb263b`.

## 118 — `e2cdb396` — 2026-05-20

Original: `ok`

Remove stale distribution binaries after catalog consolidation

The commit deletes the generated `dist_bak/*.bp` snapshot containing chart, chatserver, currency, fd, file-system, fileexplorer, flags, framework, Tetris, localcoder, graphics, Tokio, weather, and web apps, while leaving source manifests and build logic intact. This is distribution cleanup after 115–117’s source/API consolidation, with no direct kernel-side API change and no runtime evidence beyond the removed artifacts.

## 119 — `137fc034` — 2026-05-21

Original: `ok`

Advance the filesystem app and make package binaries explicit

The `apps/file-system` gitlink advances to `08220eb322fd03da1d3661ced5e4b21192ce6870`, while `src/main.rs::build_one_target_to` passes `--bin <output_name>` for `main.rs` package apps, updates the Rust-std `set_name` overlay to the newer `core::ffi::CStr` signature, and stages a Tokio `trueos-io` compatibility overlay. The explicit binary selection is a packaging precursor to the later external-app cleanup; the submodule’s internal changes are unavailable, but kernel `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5` later exposes `crates/trueos-v/src/vfs.rs` and `src/r/io/fs_cabi.rs` for the filesystem bridge.

## 120 — `48926f80` — 2026-05-23

Original: `ok`

Stop patching Tokio in place and clean generated output

The `apps/file-system` pointer advances to `68baf58bb6d977714b9b4c8a192ad7c7b094c6a9`, `src/main.rs` removes `stage_tokio_trueos_overlay` and `patch_tokio_trueos_overlay`, and the remaining `dist_bak/*.bp` artifacts are deleted. This is cleanup/stabilization after 119 that narrows the Blueprint overlay now that the shared kernel/runtime dependency is vendored; the submodule’s internal revision and behavior cannot be verified from this superproject diff.

## 121 — `2cc10550` — 2026-05-28

Original: `ok`

Support path-aware external apps and optional service dependencies

`apps.json` changed from string names to `{name,path}` entries and added `tactics`; `api::env` re-exported `v::env`, while `src/main.rs` gained `RegisteredAppSpec`, `current_blueprint_root`, optional workspace dependency injection, `trueos-blueprint` dependency materialization, and root-relative targets; framework and Tokio net manifests declared Mio/socket2 probes. This packaging layer feeds kernel-facing Tokio/Mio consumers consistently, extending the facade/build-plan work from 066 and matching kernel compatibility work `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`.

## 122 — `a7104ec4` — 2026-05-28

Original: `ok`

Extend the low-level UI2 frame and draw wrapper before feature gating

`api/src/ui2/gfx.rs` adds `begin_frame`, `begin_frame_preserve`, `begin_frame_no_present`, render-target, blend, sampler, scissor, `draw_rgb_triangles_no_present`, `draw_tex_triangles_no_present`, and `end_frame` wrappers over the `trueos_cabi_gfx_*` ABI, giving later UI consumers a complete frame lifecycle. This is the precursor to commit 123: the graphics surface was first expanded, then made optional so non-UI Blueprint applications would not inherit it; the kernel-side graphics direction is evidenced by `1421d1c64aef2092f0b35e6cd53176a42ff30dd5`. Evidence: Blueprint file `api/src/ui2/gfx.rs`; no kernel source changed in this Blueprint commit, so the exact implementation correspondence beyond the cited kernel graphics-path commit is not established here.

## 123 — `56725e67` — 2026-05-30

Original: `ok`

Make the UI2 graphics dependency opt-in for hosted and service workloads

`api/src/platform.rs`, `api/src/ui2.rs`, and `api/src/ui2/gfx.rs` add the feature-gated UI2/graphics surface, re-export `vgfx` and `vgfx_hosted` only under `ui2`, and expose `spawn_blocking`, cursor events, title-bar controls, and the frame helpers needed by UI applications. It stabilizes the broad contract introduced by commit 122 by keeping filesystem/web services from linking graphics symbols while preserving the kernel-facing UI2 ABI associated with `1421d1c64aef2092f0b35e6cd53176a42ff30dd5`. Evidence: the change is entirely in the Blueprint API and feature surface; no same-commit kernel diff is present, so the kernel hash identifies the surrounding graphics contract rather than a one-to-one implementation change.

## 124 — `c35ac5ab` — 2026-06-17

Original: `ok`

Trim the catalog and reconnect filesystem builds to the reduced service surface

The workspace removed obsolete members/dependencies, feature-gated API UI2 modules, advanced the `apps/fd` and `apps/file-system` pointers, and reworked `src/main.rs`/`src/build_plan.rs` for reduced-catalog source overlays and lock handling. The visible Blueprint changes prepare consumers for kernel filesystem CABI work in `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5` while making UI2 optional; the two submodule revisions are pointers only, so their internal filesystem behavior is not evidenced.

## 125 — `3da790c3` — 2026-06-21

Original: `ok`

Strengthen root catalog transitions and kernel-compatible source overlays

`src/main.rs` gained the large catalog/build transition layer for discovering root apps, staging overlays, materializing workspace dependencies, propagating `trueos-blueprint` features, patching Tokio/Hyper/Tower HTTP, and validating lockfile versions. This is the build-planner counterpart to kernel executor/service-lane work in `0ef84b713e7eae0f25650295c0474e5e0e59e1eb4c` and filesystem CABI registration in `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`, not a new application API.

## 126 — `ca6e2a38` — 2026-06-21

Original: `ok`

Prune the superseded application workspace before rebuilding the facade

The commit deleted the stale catalog and implementations for games, currency/weather, Kibi, web templates, old Tetris code, and assorted demos, leaving the smaller set expected by the new build planner. It is a cleanup boundary after 124–125: the diff contains no new kernel path or API symbol, so its service significance is the removal of consumers that no longer matched the packaging contract.

## 127 — `0acfbe2f` — 2026-06-21

Original: `ok`

Remove the superseded API tree before the virtual-service rebuild

The commit deleted the old `api/src/{clock,globalog,hid,lib,logl,platform,std_abi,tyche,ui2,vfs,vgfx,vgfx_hosted,vnet,vshell}.rs` surface and reduced `apps.json`, `apps/hello_world/src/main.rs`, and `src/main.rs` to the post-consolidation path. This is deliberately destructive cleanup following catalog pruning in 126; no kernel counterpart or new Blueprint service is evidenced, but it clears the boundary that 128 reconstructs.

## 128 — `36e6cfb9` — 2026-06-21

Original: `ok`

Rebuild the public API around virtual system services

The replacement no-std `api/src/lib.rs` re-exported Tokio and `v::{env,vclock,vinput,vshell,vsys}`, then defined `platform`, `runtime`, `task`, `sync`, `time`, `io`, async `fs`, optional `net`, `TrueosAllocator`, `panic_abort`, and the prelude; chat, file explorer, and Tokio FS migrated from `vfs` to `fs::read`, `write`, `metadata`, and `create_dir_all`. This is the direct consumer-facing rebuild after 127 and mirrors kernel filesystem/poll CABI additions in `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`, including `trueos_cabi_fs_*` and `trueos_cabi_poll_once`.

## 129 — `15bf3b80` — 2026-06-22

Original: `ok`

Expose filesystem listing/stat and external-target metadata to packaged apps

`api/src/lib.rs` expanded the filesystem/net/runtime exports, `apps/hello_world/src/main.rs` declared `trueos_cabi_fs_list_dir` and `trueos_cabi_fs_stat`, and the file explorer backend, `apps/target.json`, `src/build_plan.rs`, and `src/main.rs` adopted the new target/package shape. This turns 128’s facade into a buildable application contract over kernel `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`’s `src/r/io/fs_cabi.rs` functions rather than leaving listing/stat as a hand-written probe.

## 130 — `7f81740d` — 2026-06-22

Original: `ok`

Finalize workspace dependency materialization for Tokio networking

The reduced API, framework, and `tokio_net` manifests were aligned while `src/main.rs` gained workspace-manifest/package metadata handling, source overlays, target profiles, and dependency-path materialization that maps hosted Tokio/Hyper/Mio/socket2 onto vendored TRUEOS-compatible sources. This is the packaging stabilization endpoint of 121 and 125, directly consuming the kernel Mio/socket2 compatibility work in `7d957a74de0685245dd5a61fd9274b9eaaf7aafa` rather than introducing another network ABI.

## 131 — `88a69208` — 2026-06-23

Original: `ok`

Establish the `trueos-v`/`trueos-io` vocabulary at the Blueprint boundary

`crates/trueos-io/src/lib.rs` introduces the shared `core3::io`-based `Read`/`Write`/`Seek` vocabulary, `IoSlice`/`IoSliceMut`, errno/status mapping, and constructors such as `would_block` and `not_connected`; `crates/trueos-v` then supplies the first `bp_abi`, `vcabi`, `vfs`, `vnet`, `vhttp_srv`, `vshell`, `vinput`, `vio`, `vclock`, and `vsys` facade. The concrete `vfs::{read_file,write_begin,write_chunk,write_finish,stat,create_dir_all}` calls and `vsys::thread_current_id` mirror kernel `src/r/io/fs_cabi.rs` from `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5` and the executor/runtime service lane in `0ef84b713e7eae0f25650295c0475e0e59e1eb4c`; this is the precursor vocabulary consumed by the following MRT, mail, filesystem, and concurrency applications.

## 132 — `bbf0722b` — 2026-06-23

Original: `ok`

Turn the new virtual filesystem vocabulary into a Tokio smoke test

`apps/tokio_mrt/src/main.rs` is a small no-std consumer of commit 131: it selects `clock::utc_date_time` or `clock::monotonic_millis`, builds `t::runtime::current_thread()`, and writes `/hello_world.txt` with `t::fs::write`. The app therefore exercises the `trueos-v` clock/runtime/filesystem facade over kernel `src/r/io/fs_cabi.rs` and its `trueos_cabi_fs_*` path from `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`; the `apps/file-system` pointer advance is external evidence only.

## 133 — `9dd32ae9` — 2026-06-25

Original: `ok`

Connect Webmail to runtime SMTP configuration and sending

`crates/trueos-v/src/vmail.rs` and `bp_abi.rs` expose account configuration, password state, asynchronous `MailOp` wait/result/discard, and blocking text send; `apps/webmail/server.rs` consumes them through `MailConfigRequest`, `MailSendRequest`, `/api/webmail/config`, and `/api/webmail/send`. This Blueprint wrapper is synchronized with kernel `129339bcb44d492941a17226275a4f7970eb669d`, specifically `src/r/net/mail_config.rs`, `src/r/net/smtp_cabi.rs`, and `src/r/net/srv/mail.rs`; commit 134 is the UI consumer that completes the same service path.

## 134 — `5053dc61` — 2026-06-25

Original: `ok`

Finish the Webmail frontend for the SMTP service contract

`apps/webmail/app.js` now loads configuration and password state, posts account settings, and sends recipient/subject/body JSON to the server routes added in commit 133; `server.rs` adjusts the route response shape to match. It is a consumer/UI stabilization of the SMTP ABI from kernel `129339bcb44d492941a17226275a4f7970eb669d` (`src/r/net/mail_config.rs` and `src/r/net/smtp_cabi.rs`), not a new mail protocol or kernel symbol.

## 135 — `04a1a91f` — 2026-06-25

Original: `ok`

Make File Explorer a real filesystem client

`apps/fileexplorer/axum_server.rs` grows normalized and percent-decoded paths, host fallback, directory listing/stat, folder creation, upload/download, and content reads, including direct `trueos_cabi_fs_list_dir` and `trueos_cabi_fs_stat` bindings. This turns the earlier `trueos-v` filesystem facade into a browser-facing consumer of kernel `src/r/io/fs_cabi.rs` from `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`, with path validation serving as the application-side stabilization boundary.

## 136 — `38724f88` — 2026-06-25

Original: `ok`

Add Crossbeam and Tokio worker-pressure coverage

`apps/cross/src/main.rs` exercises Crossbeam `AtomicCell`, `CachePadded`, `Backoff`, `ArrayQueue`, `SegQueue`, epoch pinning, Fibonacci ordering, Tokio `JoinSet`/`spawn_blocking`, WLS-visible thread IDs, pressure blockers/sentinels, shell echo, and a `t::fs::write("/cross_smoke.txt", ...)` completion marker; `trueos-v/src/vsys.rs` supplies `thread_current_id`. This is a concurrency consumer of the Tokio/Mio substrate in kernel commits `c3a28e797e1dde3a0961b465a91211813dd1506a` and `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`, with the thread-ID ABI completed by `39c5504a70d38759dd768ffe78db03416b937d91`; the vendor patches in `src/main.rs` make those dependencies fit the Blueprint target.

## 137 — `1d64aa93` — 2026-06-25

Original: `ok`

Build a filesystem-backed BlockNote editor service

The new `apps/texteditor` package combines an Axum server with a React/BlockNote frontend; `DocumentEnvelope` persists BlockNote JSON plus Markdown/HTML exports, and `/api/texteditor/document`, `/api/texteditor/export`, `/healthz`, and static routes run on `runtime::current_thread_net`. Its server calls `trueos::fs::{read,create_dir_all,write}`, making this a concrete browser consumer of kernel `trueos_cabi_fs_*` handling in `src/r/io/fs_cabi.rs` from `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`, before commit 138 adds browsing and store-copy operations.

## 138 — `d081ac35` — 2026-06-25

Original: `ok`

Probe condition variables and extend the text editor’s filesystem browser

`apps/condvar/main.rs` covers std `Mutex`/`Condvar` signal and broadcast waits plus a Tokio `spawn_blocking` waiter, while the text editor adds `FsEntry`, path normalization, host/TRUEOS listing and stat, folder creation, and store-copy/export handlers. The two halves consume the scheduler/blocking lane associated with kernel `0ef84b713e7eae0f25650295c0475e0e59e1eb4c` and the filesystem C ABI in `src/r/io/fs_cabi.rs` from `084d4bd48a467a6ec49c1ad781fb7830fbdcfbb5`, extending commit 137 rather than introducing a separate editor API.

## 139 — `2b6734cf` — 2026-06-26

Original: `ok`

Validate WLS isolation across std threads and Tokio blocking workers

`apps/wls/main.rs` uses a `thread_local!` `WLS_MARKER` to test runtime stability, fresh std-thread slots, per-thread isolation, and `spawn_blocking` boundaries; the build planner patches Rust std no-thread TLS to lazy storage, expands it to 4096 slots, and resolves each slot through `trueos_cabi_wls_current_slot`. This directly consumes kernel WLS support in `643f3d8909b90dff6f4912b424c32e16dc05104d` and the thread/runtime groundwork in `39c5504a70d38759dd768ffe78db03416b937d91`, turning the Cross pressure and WLS-visible IDs from commit 136 into a dedicated ABI conformance probe.

## 140 — `488abf51` — 2026-06-26

Original: `ok`

Make Crossbeam and Rayon dependencies self-contained

The commit vendors the Crossbeam family under `vendor/crossbeam-*` and changes the workspace patches from `../TRUEOS/vendor/...` to local paths, while adding local `rayon` and `rayon-core` paths. This is cleanup and build stabilization after the `apps/cross` demand in commit 136 and the WLS/thread contract validated by commit 139; no new Blueprint or kernel symbol is present, although the environment remains the one supplied by kernel commits `39c5504a70d38759dd768ffe78db03416b937d91` and `643f3d8909b90dff6f4912b424c32e16dc05104d`.

## 141 — `3c97ffab` — 2026-06-26

Original: `ok`

Add Reticulum and Tantivy as first-class Blueprint dependencies

The workspace adds `apps/Reticulum-rs` at submodule revision `d7ce77a76a986d697a2a10637ba2d30f5c791025` and `vendor/tantivy` at `02e34508e2c66485764bb07ff9b3ec50a383de88`, registers the Tantivy URL in `.gitmodules`, and exposes `crossbeam-skiplist` from `vendor/crossbeam-skiplist-0.1.3`. This expands application packaging above the existing runtime without adding a kernel ABI or VM import; the superproject records only pointers/manifest wiring, so Reticulum and Tantivy internals and kernel interaction remain uncertain.

## 142 — `d2950fcd` — 2026-06-26

Original: `Register Reticulum app submodule`

Register and advance the Reticulum Blueprint application

`.gitmodules` declares `apps/Reticulum-rs` at `https://github.com/t4ce/Reticulum-rs`, and the Blueprint pointer advances from `d7ce77a76a986d697a2a10637ba2d30f5c791025` to `7231345f85bde2058d96ecdb88c4d62f5f4f76a7`. This is a catalog registration/update after 141, but the submodule-only diff exposes no Reticulum symbols, build behavior, or kernel-facing path beyond the source pointer.

## 143 — `7faf20c9` — 2026-06-27

Original: `ok`

Add the Monaco editor service and an initial SQLite probe

The workspace and `apps.json` gain `monaco` and `rusqlite_probe`; Monaco’s `server.rs` serves embedded assets through `handle_document_get`/`handle_document_save`, `normalize_fs_path` rejects absolute paths, `..`, and NULs, and `build.rs` emits `monaco_assets::STATIC_ASSETS`, while the browser editor detects Rust, TypeScript, JavaScript, JSON, Markdown, HTML, and CSS with a textarea fallback. `apps/rusqlite_probe/main.rs` exercises an in-memory `rusqlite` schema, inserts, update, query, and logging through the Blueprint `trueos`/Tokio surface, so this is an external-app/editor consumer rather than a kernel ABI addition.

## 144 — `9d22399c` — 2026-06-27

Original: `ok`

Move Tantivy from a vendor pointer into the app catalog

The commit replaces `vendor/tantivy` with `apps/tantivy`, changes its `.gitmodules` registration, and adds `apps/tantivy` to `apps.json`, making the external search engine discoverable by the application registry. It is a repository-layout/dispatch correction following 141’s dependency import; the source remains a submodule, so no Tantivy API or kernel interaction is evidenced.

## 145 — `f4e26d02` — 2026-06-27

Original: `ok`

Disable stack-protector assumptions in native Blueprint dependency builds

`src/main.rs::push_trueos_cc_flags` adds `-fno-stack-protector` to both `CFLAGS` and `CXXFLAGS`, alongside the RocksDB POSIX defines `ROCKSDB_PLATFORM_POSIX`, `ROCKSDB_LIB_IO_POSIX`, and `OS_LINUX`. This is packaging/toolchain stabilization for C/C++ dependencies such as the newly cataloged search/database workloads, avoiding an unresolved compiler-runtime dependency in the freestanding guest path; no Rust API or kernel source changes are present.

## 146 — `857c5fcd` — 2026-06-28

Original: `ok`

Expose the first owned UI3 frame and batch-rendering ABI

`api/src/lib.rs` adds `trueos::ui3` wrappers for frame bounds, `Frame`/`SurfaceWindow`, cursor events, texture upload/status/dimensions, frame begin/end, render-target selection, and RGB/texture triangle batches; `crates/trueos-v/src/bp_abi.rs` declares the matching `trueos_cabi_gfx_*` and `trueos_cabi_ui3_frame_*` imports. `WindowId::take_cursor_events` drains `trueos_cabi_input_read_cursor_events_since`, while `SurfaceWindow` closes on drop unless leaked, establishing ownership semantics over kernel UI3 commit `7616b14f432b79df8d017c405f4aa4976874452e`; commit 147 then refactors these calls into solid/sprite batches.

## 147 — `dadad1de` — 2026-06-28

Original: `ok`

Collapse UI2/UI3 feature selection around solid and sprite batches

The API removes the separate `ui2` feature, makes `ui3` self-contained behind `ui3_core`, replaces `RgbVertex` with `SolidRect`, and adds `SpriteCorner`/`SpriteQuad`; `crates/trueos-v/src/bp_abi.rs` correspondingly renames the raw calls to `trueos_cabi_ui3_frame_draw_solid_batch` and `...draw_sprite_batch`, with no-present Rust callers. This is a consumer-facing ABI-shape refactor of commit 146’s frame contract and remains tied to kernel UI3 frame support in `7616b14f432b79df8d017c405f4aa4976874452e`, while `src/build_plan.rs` and `src/main.rs` stabilize feature detection for generated guests.

## 148 — `d63d1c5a` — 2026-07-01

Original: `ok`

Introduce PrismQ as a bounded quantum-simulation workload and oracle

The workspace registers `apps/prismq` and `apps/prism_q_probe`; the CLI uses `CircuitBuilder`, `simulate`, `openqasm`, `TextOptions`, and `SvgOptions` for `run`, `probs`, `shots`, `counts`, `draw`, `inspect`, and `validate`, while the probe verifies Bell probabilities, an eight-qubit GHZ state, and rejection above `MAX_QUBITS = 26`. This establishes the consumer and test oracle that commit 149 turns into a JSON/web workload and later commits adapt for freestanding math and memory constraints; the diff adds no kernel import or ABI symbol. Evidence: concrete files are `apps/prismq/main.rs`, `apps/prism_q_probe/main.rs`, `Cargo.toml`, and `apps.json`; kernel interplay is not evidenced for this initial host-side workload.

## 149 — `972274af` — 2026-07-01

Original: `ok`

Turn PrismQ into a JSON designer and expose RAPL history to Webdevices

`apps/prismq/main.rs` adds `JsonCircuit`/`JsonGate` conversion and a current-thread `trueos`/`zkvm` runtime, while `designer.html` makes the circuit editor usable; in parallel, `crates/trueos-v/src/vrapl.rs` exposes `snapshot_text`, bounded `history_bytes`, and `history_len` through `trueos_vlayer_rapl_snapshot_read`, and `apps/webdevices` serves `/api/rapl/snapshot` and `/api/rapl/history`. This is the first consumer expansion after commit 148 and the telemetry precursor to commit 150’s PCI dashboard parsing; the RAPL vlayer is evidenced by kernel commit `abf8967e87bb10ffc487aa039cb5d72e5fd92994` and the Blueprint build planner’s PrismQ/Rayon overlays. Evidence: Blueprint files include `apps/prismq/{main.rs,designer.html}`, `crates/trueos-v/src/vrapl.rs`, and `apps/webdevices/axum_server.rs`; the cited kernel hash is the ABI/provider evidence, not a same-commit kernel change.

## 150 — `e9f07660` — 2026-07-01

Original: `ok`

Add structured PCI inventory beside the RAPL dashboard

`crates/trueos-v/src/vpci.rs` reads length-prefixed `trueos_vlayer_pci_snapshot_read` data into text/bytes, and `apps/webdevices::parse_pci_devices` turns `dev`/`bar` rows into BDF, vendor/device, class, command/status, role, BAR, and projected USB-controller JSON while extending RAPL history charts. It is the direct Webdevices consumer of commit 149’s telemetry wrapper and the stabilization point before commit 153 adds thermal availability; the provider is kernel commit `4e515258ae05edd684d612a7ed5805356df24c61`, with resolver wiring in `src/hv/blueprint/blueprint.rs` and the implementation in `src/r/net/vlayer.rs`. Evidence: Blueprint files are `crates/trueos-v/src/vpci.rs`, `crates/trueos-v/src/vrapl.rs`, and `apps/webdevices/{axum_server.rs,index.html,tailwind.css}`; no kernel source was changed in this commit.

## 151 — `76ce1f27` — 2026-07-01

Original: `ok`

Adapt PrismQ’s stress ladder and math for freestanding Blueprint guests

`apps/prismq/main.rs` expands the examples from Bell to generated `ghz-8/16/24/26` and `mesh-16/20/22/24/26` circuits, while the vendored `prism-q-0.20.0` patch routes `sin`, `exp`, `ln`, and `floor` through `libm`, replaces label/depth digit sizing with integer arithmetic, and adds `panic=abort` in `src/main.rs`. This is the refactor that makes commit 148’s 26-qubit probe plausible on the freestanding target, with the kernel’s related math/import coverage recorded as `a90b580ee5ac3a3587c664b69dbcbd6f95c1c44b`. Evidence: concrete changes are in `apps/prismq/main.rs`, `vendor/prism-q-0.20.0/{Cargo.toml,src/circuit/draw.rs,src/circuit/expr.rs}`, and `src/main.rs`; the kernel hash is contextual evidence rather than a same-commit diff.

## 152 — `22bc89c7` — 2026-07-01

Original: `ok`

Add networked speech-ingress and no-std flight-control probes

`apps/esp32_stt/main.rs` adds a current-thread network workload that tracks RTP-like L16 loss, reordering, levels, mono folding, and deterministic transcript output, while `apps/flight_controller_sim/main.rs` supplies no-std `Pid`, `KalmanAxis`, `FlightController`, `Plant`, and motor-mixing scenarios at 15 ms and 1.5 ms. These are adjacent consumers of the existing `trueos` runtime rather than new ABI work, and they keep the subsystem’s probe emphasis between PrismQ stabilization in commit 151 and the thermal/dashboard work in commit 153. Evidence: Blueprint files are `apps/esp32_stt/main.rs`, `apps/flight_controller_sim/main.rs`, their manifests, `apps.json`, and `src/main.rs`; no new kernel symbol or direct kernel path is evidenced.

## 153 — `b4339b7b` — 2026-07-03

Original: `ok`

Add thermal snapshots and per-core health telemetry to Webdevices

`crates/trueos-v/src/vthermal.rs` wraps length/read/text access to `trueos_vlayer_thermal_snapshot_read`, and `apps/webdevices` parses package/core rows into `ThermalPackageRow` and `ThermalCoreRow` with TJ-max, temperature, effective-permille, HLT, limit, PROCHOT, critical, and stale-state rendering. It completes the PCI/RAPL dashboard assembled in commits 149–150 by making hardware availability explicit, consuming kernel commit `4e0d71040fd46f4c9ef286601e9253c70aca1832` (`src/power/thermal.rs`, `src/r/net/vlayer.rs`, `OP_BP_THERMAL_SNAPSHOT_READ`, and the Blueprint resolver). Evidence: Blueprint files are `crates/trueos-v/src/{lib.rs,vthermal.rs}`, `api/src/lib.rs`, and `apps/webdevices/{axum_server.rs,index.html,tailwind.css}`; the kernel provider is referenced by hash and path, not modified here.

## 154 — `05cbb064` — 2026-07-03

Original: `ok`

Prototype a Ratatui terminal consumer and CPU/GPU Skybox window

The commit registers `vendor/ratatui`, `apps/ratatui_demo`, `apps/skybox`, and the `apps/superseedr` pointer: `render_shell_model` validates a buffer-only terminal model, while `skybox/build.rs` turns `assets/skybox8k.png` into RGB565 bytes and `skybox/src/main.rs` renders/resizes/moves/fullscreens a CPU ray-like scene through UI3 texture presentation. This is the precursor to commit 155’s Konsole backend and dedicated RGB565 GPU call, so the initial Skybox path intentionally establishes a working texture/presentation fallback first. Evidence limits: the Ratatui and Superseedr repositories are submodule/pointer content; their internals are unavailable in this superproject diff, while the visible Blueprint evidence is `apps/ratatui_demo`, `apps/skybox`, `.gitmodules`, `Cargo.toml`, and `apps.json`.

## 155 — `f645afc1` — 2026-07-04

Original: `ok`

Connect Ratatui to Konsole and promote Skybox to the RGB565 GPU path

`apps/ratatui_demo/main.rs` introduces `TrueOsKonsoleBackend` over `ratatui_core::{Backend,Terminal,Frame}` and translates styled buffers/cursors through `vshell::{konsole_begin_frame,konsole_write_row,konsole_set_cursor,konsole_end_frame}`, while Skybox uploads RGB565, builds `SkyboxRenderParams`, tries `render_skybox_rgb565_no_present`, and retains CPU RGBA presentation as fallback. The commit is the consumer/refactor bridge from 154’s buffer-only/texture prototype to the kernel ABI added in `f57fb9dcf06b751da6ed25f6b6ae06ea536cb561`, whose `src/r/io/ui3_cabi.rs`, `src/ui3/{ui3_frame.rs,ui3_img.rs}`, `src/hv/vmcall.rs`, and `src/intel/gpgpu.rs` implement the skybox dispatch. Evidence: Blueprint files are `apps/ratatui_demo/main.rs`, `apps/skybox/src/main.rs`, `crates/trueos-v/src/{bp_abi.rs,vshell.rs}`, `api/src/lib.rs`, and `Cargo.toml`; commit 156 then polishes the same Konsole consumer.

## 156 — `22095225` — 2026-07-04

Original: `ok`

Preserve Ratatui widget styling across the Konsole frame ABI

`apps/ratatui_demo/main.rs` extends `TrueOsKonsoleBackend::styled_row` with SGR reset/modifier/foreground/background emission and the `CellStyle`, `emit_sgr`, `emit_modifier_codes`, and color-conversion helpers needed to retain widget appearance at the terminal boundary. It is a stabilization pass directly on commit 155’s `vshell::konsole_*` consumer, while the visible `apps/superseedr` change only advances an external revision. Evidence limits: `apps/superseedr` is a submodule pointer, so its internal application changes and any kernel calls cannot be assessed; the visible renderer evidence is confined to `apps/ratatui_demo/main.rs`.

## 157 — `205c5a6a` — 2026-07-04

Original: `ok`

Make Skybox GPU failure recovery stateful and launchable

`apps/skybox/src/main.rs` adds `gpu_ready`, calls `present_skybox_gpu` on motion, and permanently falls back to CPU `present_skybox_texture` after upload/render failure while retaining `full`, `1080p`, `720p`, `window`, `size`, `pos`, `status`, and `quit` commands; `src/cli.rs` adds the launcher path. This is the stabilization/consumer endpoint of commits 154–155’s GPU experiment, using the Skybox vmcall/C ABI from kernel `f57fb9dcf06b751da6ed25f6b6ae06ea536cb561` rather than retrying a known-bad GPU path indefinitely. Evidence limits: the `apps/superseedr` pointer advance is visible but its internal revision is unavailable, so no claim about its behavior is made; visible Blueprint evidence is `apps/skybox/src/main.rs` and `src/cli.rs`.

## 158 — `92a0dc85` — 2026-07-05

Original: `ok`

Add POSIX descriptor operations and durable SQLite serialization

`crates/trueos-v/src/bp_abi.rs` declares `trueos_cabi_fs_fd_open/close/read/write/lseek/pread/pwrite/fstat/ftruncate/fsync/fdatasync/fcntl`, while `vfs_fd.rs` supplies POSIX flags, seeks, lock constants, `TrueosCabiFdStat`, and `TrueosCabiFdLock`; `apps/posix_fd_probe` exercises sequential/offset I/O, truncation, sync, stat, and `F_GETLK`/`F_SETLK`. `apps/rusqlite_probe` loads `/common/usersettings.db` into `MAIN_DB` with `deserialize_read_exact`, migrates `user`/`settings`, and persists `Connection::serialize` bytes through `trueos::vfs`; the Blueprint evidence points at kernel `src/r/io/fs_cabi.rs`, but no unambiguous kernel commit hash for these descriptor additions was found, so the exact implementation pairing remains uncertain.

## 159 — `3cc0fb91` — 2026-07-05

Original: `ok`

Register the flight-control, Rebels, and Superseedr application revisions

The superproject adds `apps/KG-Flight-Controller` at `2c1ad1c07af420f768e552920d2c9bf7787f4c8a`, adds `apps/rebels-in-the-sky` at `c83e161b04031087ff2d5111e3c09667681d4611`, and advances `apps/superseedr` from `5ad6397f55ba69c3712a6798a8d55ad824d89216` to `ac9c1985540a0faad90fcb0cf43bc2472c4f9a7c`. This records external application identity after the visible Skybox/flight probes but supplies no local consumer or kernel integration to connect beyond those pointers. Evidence limits: all three changes are submodule pointers; their internal modules, APIs, runtime behavior, and kernel calls are unavailable from the Blueprint superproject diff.

## 160 — `b8cd6a12` — 2026-07-05

Original: `ok`

Adapt Scope’s collections and clock types to freestanding builds

The catalog adds `apps/scope-tui`, re-exports `v::collections`, and supplies `BTreeMap`, `BTreeSet`, `HashMap`, and `HashSet`; `vclock::Duration` gains seconds/microseconds/floating conversions and `core::time::Duration` interoperability, while `Instant` gains checked/saturating arithmetic. `src/app_catalog.rs` rewrites `std::collections::*` to `trueos::collections::*` and `std::time::Instant` to `trueos::clock::Instant` for target-specific dependencies, so this is build/API adaptation for the Scope consumer rather than a new kernel symbol; the external Scope pointer’s internals are not visible here.

## 161 — `b5f9799e` — 2026-07-06

Original: `ok`

Define the Blueprint audio playback and monitor facade

`crates/trueos-v/src/vaudio.rs` adds `PlaybackParams`, `Stream`, `State`, `Monitor`, pause/resume, volume, queue/buffer counts, drain, and S16 stereo 48-kHz helpers; `bp_abi.rs` and `api` expose the corresponding `trueos_cabi_audio_*` contract as `trueos::audio`. The Scope pointer advances and Symphonia AAC/core sources are added as the intended decoder consumer, while kernel `60d5149ffd7619e97e9302fa5b084ed8f83a9589` provides `src/aud/cabi.rs` and the initial audio C ABI and `0ccf4ec20499fdebb91cc64406fb11f322951e82` adjusts the vlayer/`vaudio` side; commit 167 later consumes this facade for actual playback.

## 162 — `4076f2f6` — 2026-07-07

Original: `ok`

Add portable TUI input and terminal-handoff metadata

The workspace vendors `tui-input-0.15.3`, renames the catalog alias to `aud_player_scope_tui` while accepting the old alias, advances the external Scope pointer, and adds `KONSOLE_FRAME_TERMINAL_HANDOFF = 1 << 31` in `crates/trueos-v/src/vshell.rs`. This flag is a preparatory consumer-side convention for the terminal-aware Scope/player path that commit 163 formalizes; no kernel source changed here, so its exact shell consumer semantics are not independently evidenced.

## 163 — `5df7aecb` — 2026-07-08

Original: `ok`

Give terminal Blueprints an explicit exit and handoff protocol

`crates/trueos-v/src/bp_abi.rs` adds `trueos_cabi_blueprint_exit_reason`, `trueos_cabi_blueprint_shutdown`, and `trueos_cabi_konsole_size`, while `vshell.rs` wraps them as `report_exit_reason`, `shutdown_current_blueprint`, `leave_terminal_handoff`, and `konsole_size`. This stabilizes the handoff flag introduced in commit 162 with a deliberate return/shutdown path backed by kernel `97582d2e55021a0522dd72929a8c283a5a16f8b7`; the catalog transition toward `apps/aud-player-scope-tui` is only partial in this diff, making the replacement app boundary uncertain until commit 164.

## 164 — `cf7b402f` — 2026-07-08

Original: `ok`

Register the external audio-player Scope application boundary

The commit adds only the `apps/aud-player-scope-tui` submodule pointer. It therefore connects the catalog transition from commit 163 to an external player revision, but the superproject contains no command handlers, decoder code, audio calls, or kernel-facing symbols to describe; those details remain unavailable without the submodule history.

## 165 — `85b530dd` — 2026-07-08

Original: `ok`

Replace the external Scope pointer with a local Player/Scope TUI

The new `apps/Player` package exposes `control::{parse_command,dispatch,ControlEventHandler,COMMAND_SPECS}` and `ui::{run,UiConfig,TrackData,PlaylistEntryData}`, includes a 500-entry demo dataset, and replaces the `apps/aud-player-scope-tui` pointer while registering `unix_api_probe`. This local Ratatui/Crossterm application is the visible consumer boundary after commit 164, but its initial play/record/edit flows are visual/logged behavior and add no new kernel ABI; the actual `trueos::audio` consumer arrives in commit 167.

## 166 — `dd11e8e7` — 2026-07-08

Original: `ok`

Move Player toward the TrueOS terminal backend and UDS event path

The old `apps/ratatui_demo` is removed after its Konsole direction is folded into Player, `apps/Player/src/cmd_boxes.rs` fixes Unicode width/color layout, and `src/main.rs` stages TrueOS overlays for versioned `rustix`, `signal-hook-mio`, and `crossterm`. The vendored Mio changes add `uds_trueos.rs`, TrueOS selector handling, and error conversion so Crossterm polling, registration, and wakers can run in the guest; this is integration with the existing Mio/socket/UDS compatibility lineage of kernel `7d957a74de0685245dd5a61fd9274b9eaaf7aafa`, not a new Player-specific ABI.

## 167 — `f8ba5890` — 2026-07-08

Original: `ok`

Connect Player decoding to real TrueOS audio playback

Player adds `audio::{m4a,m4a_demux}`, `playback::PlaybackEngine`, and UI actions that load a path, decode MP4/WAV data, report codec/rate/size/duration, feed frames, pause, finish, and close streams; WAV validation requires stereo 48-kHz S16 and reads go through `trueos::vfs`. On TrueOS/zkVM, `PlaybackEngine` opens `trueos::audio::Stream::open_playback`, applies volume, writes interleaved i16 frames, starts/resumes, monitors queue capacity, and drops/closes the stream, making this the direct consumer of commit 161 and kernel `src/aud/cabi.rs` from `60d5149ffd7619e97e9302fa5b084ed8f83a9589`; the unrelated Rebels pointer bump carries no additional evidence.

## 168 — `e937e589` — 2026-07-08

Original: `ok`

Bring Frog weather into the guest TUI surface and finish Player runtime wiring

The new `apps/Frog` package and `trueos-weather` library load demo/OpenWeather-shaped data and render conditions, an eight-day table, selected-day detail, gauges, refresh/error state, and keyboard navigation, with `main.rs` selecting `trueos::runtime::current_thread_net` on `trueos`/`zkvm`; the same commit makes Player clear/reload its `PlaybackEngine` and stages dependency overlays in `src/build_plan.rs` and `src/main.rs`. Frog is a consumer of the existing Tokio/network/terminal boundary and Player is a continuation of the audio/Konsole line, not a new kernel export; its checked lockfile packaging is stabilized in the following commit outside this assignment. Evidence: visible Blueprint files include `apps/Frog/{src/main.rs,src/ui.rs,src/weather.rs,trueos-weather/src/*}`, `apps/Player/{src/playback.rs,src/ui.rs}`, `src/build_plan.rs`, and `src/main.rs`; no same-commit kernel change is present.

## 169 — `b60779ff` — 2026-07-10

Original: `ok`

Extend guest networking with TUN packets and safety probes

`crates/trueos-v/src/vnet.rs` adds `Tun`, `OpenTun`, `SendIpPacket`, and `IpPacket` around `trueos_cabi_tun_open/close/send/recv`, while `vhttp_srv` accepts packet events; the build planner stages Hickory/libp2p/Quinn and a TrueOS `futures-timer` backend, and `apps/panick` probes safe controls plus deliberately invalid C/read pointers. The kernel counterpart is visible in `src/r/net/socket_cabi.rs` and `src/hv/blueprint/blueprint_net_wire.rs` from `7ce3274d0f03e3487bfca6fb539ca0e08cb537c8`, where TUN ownership, open/close, IP send/receive, and packet events are implemented; publishing’s `§§<sha256>` naming and cleanup are Blueprint artifact hygiene rather than another VM operation.

## 170 — `1f1129c0` — 2026-07-10

Original: `ok`

Establish the shared calculator protocol and system-services dashboard

`crates/trueos-math/src/calculator_base.rs` defines the generic arithmetic/scientific/integer/statistical operations, `CalculatorOperation`, `CalculatorFunctionSpec`, `CALCULATOR_PROTOCOL_VERSION`, an eight-argument limit, and `evaluate_operation_id`; `crates/trueos-v/src/calculator_base.rs` maps the checked protocol to `trueos_cabi_calculator_evaluate` and explicit pointer/arity/unknown-operation/invalid-integer errors. `apps/calculator` supplies the Ratatui/Crossterm custom-function UI, while `crates/trueos-v/src/vsystem_services.rs` and `apps/system-services/server.rs` read `trueos_vlayer_system_services_snapshot_read` into a web dashboard; these consumers pair with kernel `6debd8250026d93b4013cfa0baaef473df8405da` (`src/r/io/calculator_cabi.rs`, `src/hv/vmcall.rs`, and the system-service snapshot wiring), and commit 171 hardens dispatch and terminal transport.

## 171 — `b61108b5` — 2026-07-10

Original: `ok`

Stabilize calculator dispatch and buffer console frames

`CalculatorOperation::from_raw` now indexes `CALCULATOR_FUNCTIONS` instead of transmuting a presumed contiguous enum range, so unknown IDs fail through the shared protocol introduced in commit 170; `apps/calculator` adds a 128-KiB `BufWriter`, 16-ms input polling, event draining before one redraw, and one buffered Ratatui frame crossing the console ABI. This is a focused safety/performance stabilization of the calculator consumer and still targets kernel `6debd8250026d93b4013cfa0baaef473df8405da`’s `src/r/io/calculator_cabi.rs`; the Rebels pointer update is unrelated.

## 172 — `ab300be5` — 2026-07-11

Original: `ok`

Ship the PrismQ designer persistence service and a Solara handoff model

`apps/prismq/main.rs` and `designer.html` turn PrismQ into a port-8338 service with `/designer.html`, `/api/healthz`, `/api/simulate`, and circuit list/load/save/delete routes, `prismq.circuit.v1` validation, `_revN` revision archiving, and host/`trueos::fs` persistence; `crates/trueos-v/src/{bp_abi.rs,vsys.rs}` adds `trueos_cabi_log`/`log_record`, and the vendored memory backend uses `trueos_cabi_heap_stats` on `trueos`/`zkvm`. The adjacent `tools/solara-handoff/{README.md,app.js,index.html,styles.css}` is a dependency-free design lab for coherent epochs, stale rejection, deferred retirement, and damage promotion, while kernel commit `581ce19e74cfbc19806a1534c5522713f3af12ef` supplies the structured-log/heap-ABI counterpart; commit 173 then only advances Rebels’ external revision. Evidence limits: Solara is a packaged design artifact, not a kernel implementation, and the kernel hash is contextual provider evidence; concrete Blueprint code is in `apps/prismq`, `crates/trueos-v`, `vendor/prism-q-0.20.0/src/backend/memory.rs`, and `src/main.rs`.

## 173 — `d7b580cb` — 2026-07-11

Original: `ok`

Advance Rebels in the Sky after the PrismQ designer handoff

The only Blueprint change advances `apps/rebels-in-the-sky` from submodule revision `3900d809ea190696ab5db383af6d5683547152bf` to `45f59d4686b950cb5db4466523fe2c315ae18d66`, making it an external-application revision handoff immediately after commit 172’s integrated PrismQ/Solara work. No local PrismQ, UI, graphics, or kernel ABI code changes accompany it, so the commit cannot be connected to a specific Rebels feature beyond the pointer update. Evidence limits: the Rebels repository is not expanded in this superproject diff; its internal modules, APIs, behavior, and kernel interaction require the external submodule history.
