# depthchecker_factchecker

One Blueprint experiment exercises seven real entry paths from the migrated
Codex crate tree: fragmented text, path resolution, protocol permissions,
transport JSON/Zstd, TRUEOS time, a native thread and file write/read/sync.
The [probe source and expectations](../../../TRUEOS-Blueprints/probes/depthchecker_factchecker/README.md)
document the fixtures. This directory supplies raw Cargo evidence, first-hit
captures and a plain stack report; there is no generated SVG.

Cargo dependency depth, physical call depth and async causal depth are distinct.
`collect.py` preserves the exact staged unit graph and authenticated native
archive inventory. `plan.py` chooses actual emitted symbols, and refuses absent
or ambiguous targets. In particular, text parsing inlines `next_match`: the
observable terminal call is `str::find` → `StrSearcher::new`.

`stack-capture = true` in package metadata selects a separate diagnostic cache,
full DWARF, Rust/C frame pointers and coalesced ELF sections. The packer retains
the full and packed ELF plus a hash binding to the Blueprint. Allocated section
bytes must agree before the debugger uses the packed loader layout. Diagnostic
code generation is deliberately different from an ordinary Codex release.

## Run

From `TRUEOS-Blueprints`, build without publishing:

```sh
TRUEOS_BLUEPRINT_SKIP_APPS_PUBLISH=1 CARGO_BUILD_JOBS=2 cargo bp --probes depthchecker_factchecker
```

From `TRUEOS`, use fresh output directories:

```sh
python3 tools/depthchecker_factchecker/prepare_iso.py \
  --bp ../TRUEOS-Blueprints/dist/depthchecker_factchecker.bp \
  --base-iso bld/trueos.iso --output bld/depthchecker_factchecker/image

python3 tools/depthchecker_factchecker/collect.py \
  --work ../TRUEOS-Blueprints/probes/depthchecker_factchecker/target/trueos-blueprint/work/depthchecker_factchecker \
  --output bld/depthchecker_factchecker/raw

python3 tools/depthchecker_factchecker/plan.py \
  --debug-elf ../TRUEOS-Blueprints/dist/depthchecker_factchecker.debug.o \
  --packed-elf ../TRUEOS-Blueprints/dist/depthchecker_factchecker.packed.o \
  --unit-graph bld/depthchecker_factchecker/raw/native-unit-graph.json \
  --output bld/depthchecker_factchecker/plan.json

python3 tools/depthchecker_factchecker/runner.py \
  --iso bld/depthchecker_factchecker/image/trueos.iso \
  --kernel bld/depthchecker_factchecker/image/TRUEOS.full.elf \
  --debug-elf ../TRUEOS-Blueprints/dist/depthchecker_factchecker.debug.o \
  --packed-elf ../TRUEOS-Blueprints/dist/depthchecker_factchecker.packed.o \
  --bp ../TRUEOS-Blueprints/dist/depthchecker_factchecker.bp \
  --plan bld/depthchecker_factchecker/plan.json \
  --output bld/depthchecker_factchecker/run

python3 tools/depthchecker_factchecker/report.py \
  --run bld/depthchecker_factchecker/run --raw bld/depthchecker_factchecker/raw \
  --output bld/depthchecker_factchecker/report
```

The ISO builder uses an independent kernel cache and embeds only this local
probe. The runner creates its own snapshot QEMU, private loopback ports and RAM
filesystem, pins the development SSH host key, disables guest outbound access,
and stops its own VM. It does not attach to an existing emulator or physical rig.

## What is captured

The debugger arms targets at each scenario begin, records the first real target
entry, and disarms at its end. A missing target cannot pass. Each capture saves
register evidence, raw `stack.bin`, readable ranges, SHA-256, physical frames,
source/inline labels and an explicit unwind termination. At an entry before its
prologue, the immediate caller is `[RSP]`; inherited RBP belongs to that caller.
The native worker has its own stack and cannot be inferred from its parent.

QEMU software breakpoints inside nested VMX are intercepted by TRUEOS as `#BP`.
The probe-only, default-disabled hypervisor handoff gives GDB the latched guest
registers and CR3/EPT mapping at an L1 stop. The original opcode is restored and
execution resumes at the same RIP. Begin/end breakpoints alternate after the
previous marker returned, so no instruction is skipped or emulated. Ordinary
native continuation stops use the direct GDB register view. A guest's raw XSAVE
image and a host debugger register display must remain separately labeled.

The physical chain reaches a known root only when captured evidence supports
that root and zero ancestry. Frame limits, unreadable words and broken chains
remain incomplete. Inlining and tail calls can remove frames; a captured stack
does not contain suspended async parents, nor prove every branch of a crate ran.
`capture_ok` means all selected first-hit checkpoints were captured, independently
of unwind completeness and any broader claim about unexpected execution.
