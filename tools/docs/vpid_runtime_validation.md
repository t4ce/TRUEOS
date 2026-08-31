# TRUEOS lane-local VPID runtime validation

VPID is required hardware acceleration for VM hull execution. It is not a VM
principal, a guest ABI value, or snapshot state. The executor lane owns the
active tuple `(vpid, vm_id, run_generation)` and may return to the carrier pool
only after a successful single-context `INVVPID`.

## Static contract

- `vm_id + 1` supplies a nonzero 16-bit VPID for the current VM run.
- Every AP2+ VMX lane must advertise `ENABLE_VPID`, `INVVPID`, `INVEPT`, and
  both single-context invalidation types. VMX lane initialization fails closed
  if any part is absent.
- VMXON/re-entry invalidates all VPIDs TRUEOS can allocate.
- Assignment invalidates the selected VPID before `VMLAUNCH`, covering first
  entry, restore, migration, and reuse.
- After TRUEOS rebuilds the assignment's EPT tree in place, single-context
  `INVEPT` fences that EPTP before VM entry. This is separate because
  `INVVPID` is not required to invalidate guest-physical mappings.
- VM exits followed by `VMRESUME` keep the active assignment and its cached
  translations. The executor revalidates the lane tuple before every entry, so
  an unexpected async-task migration is rejected before touching another
  lane's VMCS.
- Teardown invalidates the VPID before the executor lease is released.
- VMXOFF drains all allocatable VPIDs. Any ambiguous assignment or failed
  invalidation quarantines the lane until reboot.
- EPT remains the memory-isolation boundary. VPID only tags cached linear and
  combined translations.

## Physical-rig evidence

After rebuilding and deploying, inspect the new capture rather than an older
artifact. A healthy boot/run has all of the following:

1. One capability record for every AP2+ lane with
   `enable_vpid=1 invvpid=1 invvpid_single=1 invept=1 invept_single=1
   contract=required`.
2. A matching `vpid lane ready` record with `invalidated=64` for every lane.
3. Before the VMCS-ready/VMLAUNCH records, an assignment such as
   `vpid assigned vm=0 vpid=1 generation=N slot=S ... invalidation=single-context`,
   followed after EPT construction by a matching `vpid ept fenced` record.
4. The VMCS controls record contains `vpid=1` for VM 0 and has bit 5 set in
   `proc2` (`proc2 & 0x20 != 0`). On the currently captured rig, adding VPID to
   `proc2=0x00002002` should therefore produce `proc2=0x00002022`.
5. At teardown, `vpid retired` repeats the same VM, VPID, generation, and slot
   before the lane becomes reusable.
6. No `vpid lane quarantined` or `lane quarantine ... vpid_state=` record.

For a reuse/restore exercise, confirm that the next run has a new run
generation and that its assignment invalidation appears before its first
entry, whether the scheduler selects the same lane or a different one. During
one run, ordinary VM-exit/VMRESUME traffic must not emit another assignment or
retirement record.

The current host capture path is discoverable with `trueos-doc topic logs`.
