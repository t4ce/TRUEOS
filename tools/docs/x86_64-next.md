# Pinned x86_64 next experiment

## Scope and provenance

This is a source/dependency experiment, not a live-kernel patch and not a VM
reclamation redesign. Do not merge or deploy on the strength of the small
probe alone. The currently running image is unchanged until a separately
built image is explicitly deployed/booted; this PR performs neither action.

Reviewed TRUEOS base: `67631dcecdbbaf813a516aa91962f3790b56374b`.
Upstream pin: `78b81025d65a3e899f48a6fded8b27889a09d70b` (2026-09-16).
Upstream still declares **0.15.5**, despite unreleased breaking changes.
The root `[patch.crates-io]` overrides the implementation with that exact Git
commit. The exact `=0.15.5` dependency prevents a later registry 0.15.x from
silently winning resolution. It does not make this a published stable release.
No floating `branch = "next"`, local fork, or registry publication is involved.

Features remain equivalent to the old defaults plus `memory_encryption`:
`instructions`, `nightly`, `memory_encryption`, now explicit. The existing
`nightly-2026-07-10` toolchain is deliberately unchanged. Upstream now declares
Rust 1.98; the gate prints `rustc -Vv`, checks the reported version, and actually
compiles. A calendar date alone does not establish compiler compatibility.
If that toolchain fails, investigate before independently changing it; do not
hide the failure with `--ignore-rust-version` or a newer unrecorded nightly.

## Impact on the existing kernel

| Area | Assessment and validation needed |
| --- | --- |
| Host MMIO and retained-firmware RAM mapper | `src/pci/mmio.rs::active_mapper` changes `OffsetPageTable::new` to `from_phys_offset`. CR3/HHDM lookup, locking, allocator, page size, flags and local flush behavior are otherwise unchanged. Validate real device access and the firmware bridge on hardware. |
| Memory encryption | Upstream #603 changes address masking **after** `enable_memory_encryption` is called. Merely enabling the Cargo feature does not configure encryption. The default mask remains 52 bits. Any configured C/S-bit path needs a separate audit: configuration is once-only, and addresses at/above that bit are no longer accepted. |
| Page-table API | `unmap` returns `(frame, flags, flush)`; `clear` returns `UnmappedFrame::{Present, NotPresent}`. Neither frees memory. Non-present entry contents must not be treated as an owned physical frame. Page/range representation and non-Copy range changes also require a full build to catch downstream assumptions. |
| Guest page tables and EPT | `src/hv/memory.rs` uses custom `GuestTables` and `EptTables` arena-backed structures. This PR does not replace them, free their interior pages, change EPT permissions, or alter VM destruction. |
| Translation retirement | Existing EPT/VPID and CPU synchronization contracts remain necessary. Software tests never exercise INVLPG, remote shootdowns, INVEPT or INVVPID. Ordinary mapper flush tokens are not VM-retirement acknowledgements. |
| GPU/DMA ownership | PPGTT mappings, retirement fences, shared references, DMA pins and quarantine are unchanged. A successful mapper operation proves nothing about outstanding device access. |

The constructor change is not a fix for existing MMIO error handling: the
current MMIO loop ignores PageAlreadyMapped and ParentEntryHugePage, and has
no transactional rollback on partial allocation failure. The RAM-mapping
path has different conflict handling. Those pre-existing policies merit
separate review, not an untested change hidden inside this dependency PR.

## Checks and evidence

Run the offline validation-gate tests:

```sh
python3 tools/test_check_x86_64_next.py
python3 tools/check_x86_64_next.py --static-only
```

With the pinned Rust toolchain and its `rust-src` component installed:

```sh
python3 tools/check_x86_64_next.py
```

This creates a temporary independent Cargo workspace outside the repository
so the host tests cannot inherit the kernel's target/linker configuration.
It verifies Cargo metadata resolves exactly one x86_64 package from the pin,
executes software-only tests using private inactive page tables, and checks
the no_std probe against `.cargo/x86_64-unknown-trueos.json` using build-std.
The tests cover MMIO flags, display, returned unmap flags, non-present clear,
separate table-frame reclamation and the unconfigured encryption address mask.
No test loads a page table into CR3 or accesses MMIO. Flush tokens are ignored
only because the test tables have never been used by hardware.

In a fully provisioned TRUEOS checkout, including its submodules, generated
inputs and sibling `TRUEOS-Blueprints/crates/log-os` dependency:

```sh
python3 tools/check_x86_64_next.py --kernel-check
```

This also verifies the **real workspace** dependency graph and runs
`cargo check --bin TRUEOS` with the existing custom-target configuration.
It can update the local, already-ignored root Cargo.lock; it does not delete
that lockfile or run a broad `cargo update`. An old lockfile can require a
targeted `cargo update -p x86_64`; inspect its diff and re-run the source gate.
Review any duplicate version instead of bypassing the check.

Evidence is written to `tgt/x86_64-next-validation/`, including compiler
identity, dependency metadata, command output and generated lockfiles.
The x86_64 pin is reproducible; the entire kernel dependency graph is not
fully pinned by this PR. Preserve matching baseline/candidate lockfiles when
comparing kernels. The repository's root lockfile remains untracked by policy.

The GitHub-hosted workflow runs only the isolated probe, not the full kernel,
not a release, and not a self-hosted rig job. Its green result is NOT boot or
memory-safety certification. The probe is not a substitute for compiling the
actual MMIO module and all other kernel call sites in the full build.

## Before merging or deliberately deploying

1. Pass the source gate, host tests, custom-target probe, full kernel check,
   and the repository's normal release build. Record compiler, lockfile and
   image identity for both baseline and candidate.
2. Boot a separate candidate with a known-good recovery image. Validate
   ordinary PCI/MMIO consumers, device interrupts and retained-firmware RAM
   mappings, rather than enabling automatic startup page-table dumps.
3. Exercise VM create/run/destroy, pause/resume, snapshot/restore and reuse on
   multiple lanes. Compare PMM usage and existing VPID/EPT retirement evidence.
   Include an outstanding GPU/DMA consumer and failure/quarantine paths.
4. Keep CPU/EPT/GPU retirement and ownership redesigns in separate changes.
   Follow repository `trueos-doc` guidance before composing rig commands or
   interpreting hardware logs. Documentation does not authorize rig access.

## Rollback and eventual release migration

Revert the dependency change AND the constructor migration together; removing
only the patch leaves code using an API absent from crates.io 0.15.5. Retire
or update the experiment-specific gate/workflow too. No hardware state has
been changed merely by reverting repository files.

Once a release actually contains these changes, compare that release with
the pinned commit, update the version requirement, remove the Git override,
retain explicitly required features, and rerun all gates. Version equality
with today's unreleased manifest is not evidence of implementation equality.

## Primary sources

- [Cargo patch semantics](https://doc.rust-lang.org/cargo/reference/overriding-dependencies.html)
- [Pinned upstream source](https://github.com/rust-osdev/x86_64/tree/78b81025d65a3e899f48a6fded8b27889a09d70b)
- [Display #574](https://github.com/rust-osdev/x86_64/pull/574)
- [OffsetPageTable alias #576](https://github.com/rust-osdev/x86_64/pull/576)
- [Clear/unmap #484](https://github.com/rust-osdev/x86_64/pull/484)
- [Encryption mask #603](https://github.com/rust-osdev/x86_64/pull/603)
- [Release discussion #600](https://github.com/rust-osdev/x86_64/issues/600)
- [TRUEOS VPID contract](vpid_runtime_validation.md)
