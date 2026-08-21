# PR title

`kernel: add RAM-only update live FULLFORGET generation swap`

# PR body

## Summary

Add `update live` alongside the existing `update <disk-id>` command.

- `update <disk-id>` remains the persistent GPT/ESP installer.
- `update live` downloads and validates the candidate in RAM and never invokes the disk installer.
- Active replicatable VMX apps use the existing TRUEOSFS persistent-envelope path as the app-state boundary.
- APs execute VMXOFF and rendezvous in copied transition code.
- PCI DMA/interrupt generation is contained before the fixed kernel PML4 entry is replaced.
- The replacement kernel performs a fresh software boot from retained Limine machine facts and restores VM envelopes.
- Shell2 TCP reconnects rather than migrating a socket; the first new connection gets a one-shot success notice.

## Safety model

Before the final AP rendezvous, failures return to the old generation and restart VMs paused by the attempt. After the rendezvous, the code is intentionally fail-stop: it does not resume normal old-kernel work, free memory beneath a slow AP, or continue if PCI Bus Master Enable cannot be cleared.

## Disk semantics

Candidate kernel/ESP/partition data are not changed by `update live`. TRUEOSFS writes are limited to portable VM checkpoints for active apps.

## Validation requested

- normal build + formatting + linker inspection;
- QEMU single-core then SMP handoff;
- VM checkpoint/restore/resume;
- installed-kernel disk hash unchanged;
- subsystem-by-subsystem bare-metal warm initialization.
