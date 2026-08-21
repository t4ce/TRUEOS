# TRUEOS `update live` — RAM-only FULLFORGET prototype

Base: `t4ce/TRUEOS` branch `true`, commit `ff9773b632fb04b7d54bbed92f92f0f8cc35ad0e`.

This bundle adds a second, deliberately separate update mode:

```text
update <disk-id>   # existing persistent GPT/ESP/TRUEOSFS update
update live        # RAM-only candidate kernel + VM checkpoint + FULLFORGET
```

`update live` never invokes the disk installer, never formats the ESP, and never replaces the installed `TRUEOS.elf`. It may write **VM application checkpoints** to TRUEOSFS (`live-update-vm-XX`) because those are the portable app-state boundary. With no active VM apps, the live path does not intentionally write persistent state.

## Apply

The exact-anchor Python applicator is the authoritative path. It validates every edited source anchor before writing anything.

```bash
git checkout true
git pull --ff-only
test "$(git rev-parse HEAD)" = ff9773b632fb04b7d54bbed92f92f0f8cc35ad0e
git checkout -b feat/update-live-fullforget

python3 /path/to/apply_trueos_live_update.py --check .
python3 /path/to/apply_trueos_live_update.py .

cargo fmt --all
git diff --check
```

The zero-context unified patch is also included:

```bash
git apply --check --unidiff-zero /path/to/trueos-update-live-fullforget.patch
git apply --unidiff-zero /path/to/trueos-update-live-fullforget.patch
```

The applicator and patch were self-tested against displaced exact anchors and the exact base blob of `src/shell2/cmds/update.rs`. This environment did not contain `rustc`, `cargo`, or `rustfmt`, so a real TRUEOS compile is still mandatory.

## What the implementation does

1. Downloads the same release archive over HTTPS, extracts `trueos.iso`, and copies only `/TRUEOS.elf` into the live path.
2. Parses ELF64 without allocating metadata after the final rendezvous.
3. Requires a candidate live-update manifest emitted by `linker.ld`; both generations must agree on the ABI and handoff structure size.
4. Allocates one contiguous physical arena for the candidate PT_LOAD span, an immutable copy of the ELF, transition stacks, handoff/control records, copied trampoline code, and replacement page tables.
5. Copies the current immutable Limine request/response records into the candidate, while overriding the HHDM, executable bases, and kernel-file bytes through the warm handoff.
6. For every active VMX app:
   - requests the existing replicatable PreparePause snapshot;
   - waits for the committed pause boundary;
   - writes the existing portable VM envelope to TRUEOSFS;
   - records whether the VM was running and should resume;
   - records the original pointer-bearing guest-heap physical range.
7. Excludes both the candidate arena and checkpointed VM heap ranges from the replacement kernel's PMM. A VM restore can claim its exact range once, preventing host allocations from consuming it before pointer-bearing heap restoration.
8. Snapshots PCI BDFs while the ordinary runtime is alive.
9. Sends rendezvous vector `0x43` to all APs. Every AP executes `VMXOFF`, enters copied position-independent transition code, and parks without touching the old executor again.
10. Copies and loads the current GDT from permanent HHDM memory, disables MSI/MSI-X/INTx and PCI bus mastering without taking runtime locks, verifies Bus Master Enable cleared, and waits for a short DMA drain.
11. Replaces only the fixed kernel PML4 entry, globally flushes translations, and jumps to the candidate `_start`. There is no return to the old kernel.
12. The candidate BSP performs an ordinary kernel initialization from retained machine facts, then releases APs through `warm_ap_start`, where each AP creates fresh per-CPU, VMX, and Embassy executor state.
13. After the topology and TRUEOSFS services are ready, VM envelopes are restored and previously running VMs are restarted.
14. The first newly established Shell2 TCP connection receives once:

```text
update live: hey that worked, new kernel here :)
```

The old TCP control block, Embassy future, and shell session are intentionally not migrated; reconnecting is the clean generation boundary.

## Failure boundary

Before AP rendezvous succeeds, errors return to the old kernel and VMs paused by this attempt are restarted.

After APs are parked, normal old-generation code is no longer allowed to run. If an AP cannot drain from an aborted rendezvous, or a PCI requester retains Bus Master Enable, the prototype fail-stops in `cli; hlt` rather than freeing transition memory or exposing replacement RAM to stale DMA. The recovery mechanism is the physical reset the operator was already prepared to use.

There is no rollback after the PML4 commit.

## Compatibility gates

A live candidate is rejected unless all of these hold:

- ELF64, little-endian, x86-64 `ET_EXEC`;
- PT_LOAD span is within one PML4 slot and below the configured cap;
- candidate kernel uses the same kernel PML4 slot;
- `.limine_requests` exists, is writable, and has the exact current byte size;
- `.live_update_slot` contains the expected magic and ABI version;
- candidate AP entry is executable;
- candidate handoff section is writable and exactly the expected size;
- CPU topology fits `VM_CPU_SLOT_LIMIT`;
- a free high-half PML4 slot exists for the temporary transition mapping;
- every active VM is checkpoint-replicatable and its persistent envelope commits successfully.

## Important remaining risks

This is a realistic first generation-swap implementation, not a claim that every bare-metal device is already warm-reboot-safe.

- The release payload is protected by the existing HTTPS path and format checks, but this patch does not add a signed release-manifest verification step. Add signature/hash verification before treating `update live` as a trusted production boundary.
- PCI Bus Master Enable and MSI/MSI-X are contained, but not every device receives a full controller reset. GPU, xHCI, NVMe, audio, and NIC drivers must tolerate being initialized against warm hardware state or grow explicit generation-reset hooks.
- The copied Limine section is size-gated, not schema-hashed field by field. Changes to request ordering should bump the live ABI.
- NMI/MCE handling during the tiny IDT transition window remains platform-sensitive.
- VM envelope compatibility is governed by the existing snapshot/heap/hull/Blueprint formats. A new kernel that intentionally breaks those formats must reject or migrate the persisted envelope.
- The transition maps candidate text/data RWX, matching the repository's current single RWX PT_LOAD. Future hardening should preserve ELF page permissions.

## Suggested validation ladder

1. **Build gate**: `cargo fmt --all`, normal TRUEOS build, warnings review, linker manifest inspection with `readelf -lSW TRUEOS.elf`.
2. **Parser gate**: `update live` on a candidate without the manifest must refuse and keep the old kernel alive.
3. **Single CPU VM**: no active apps; verify candidate boots and reconnect notice appears.
4. **SMP VM**: verify all APs rendezvous, leave VMX root, and register fresh executors after boot.
5. **One replicatable VM**: keep a visible counter/document in a VM, run `update live`, reconnect, and verify restored state plus automatic resume.
6. **Abort paths**: interrupt during download and checkpoint; confirm VMs resume and the old kernel remains usable.
7. **Disk invariant**: hash the installed ESP `TRUEOS.elf` before and after. It must remain identical; only `live-update-vm-XX` checkpoint files and normal filesystem metadata may change.
8. **Bare metal**: start with nonessential PCI devices disabled, then add NIC, storage, USB, audio, and GPU one subsystem at a time.

## Expected high-level log sequence

```text
update live: starting RAM-only generation replacement; no kernel disk install will run
update live: candidate TRUEOS.elf=... bytes; disk install path skipped
update live: candidate staged arena=...
update live: vmN checkpointed as live-update-vm-NN (... bytes)
update live: captured ... PCI functions for lock-free DMA containment
update live: final rendezvous next; on success the TCP shell will disconnect

# TCP disconnect / generation commit

live-update: generation=1 ... mode=fullforget-warm
live-update: released ... parked APs into generation 1
live-update: vmN restored and resume scheduled ...
```
