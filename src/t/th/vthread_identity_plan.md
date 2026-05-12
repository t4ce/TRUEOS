TRUEOS virtual-thread identity patch
====================================

This is the narrow first patch for making kernel-hosted virtual threads look
like real execution identities where it matters most: TLS and thread-local
runtime state.

Non-goals
---------

This is not a zkVM, TRUEOS hull, guest blueprint, or VM identity path.

Do not route this through:

- `hv::current_vm_id()`
- `hv::guest_boot_active()`
- VMCS guest fields
- blueprint launch state
- app/hull lifecycle state

Those remain VM and guest concepts. This patch is only about the host-side
virtual thread currently executing on a physical AP carrier.

Identity split
--------------

Use the x86_64 segment-base registers with a strict meaning:

```text
IA32_GS_BASE = TRUEOS PerCpu pointer
IA32_FS_BASE = TRUEOS virtual-thread record pointer
```

`GS_BASE` already backs `percpu::this_cpu_ptr()`. Leave it as the carrier/CPU
identity.

`FS_BASE` becomes the virtual-thread identity register while host code is
running inside a virtual thread. It must point at a TRUEOS-owned record with a
magic value. If the magic is missing, the current context is not a TRUEOS
virtual thread.

Record shape
------------

The first record should stay boring and fixed-size:

```rust
#[repr(C, align(64))]
pub struct VThreadRecord {
    magic: u64,
    version: u32,
    vtid: u32,
    cpu_slot_hint: u32,
    lane_id: u32,
    tls_epoch: u32,
    tls_slot: u32,
    tls_base: usize,
}
```

Suggested magic: `0x4452_4854_4555_5254`, ASCII-ish `TRUETHRD` in a stable
constant. The exact value matters less than never accepting untagged FS values.

New module
----------

`src/th/vthread.rs` owns:

```rust
pub fn current_record() -> Option<&'static VThreadRecord>;
pub fn current_id() -> u32;
pub fn current_tls_slot() -> u32;
pub fn enter(record: &'static VThreadRecord) -> VThreadGuard;
```

On x86_64, `enter()` reads old `IA32_FS_BASE`, writes the new record pointer,
and restores the old value in `Drop`.

Fallback behavior:

```text
valid FS_BASE VThreadRecord -> record.tls_slot
otherwise -> no virtual thread
```

Tokio can keep its old lane/CPU fallback for one patch so boot stays stable,
but the preferred source must be the virtual-thread record.

Tokio lane integration
----------------------

Each Tokio blocking lane should own or reference one `VThreadRecord`.

When `trueos_tokio_worker` enters a blocking job:

```rust
let _vthread = crate::th::vthread::enter(lane.vthread_record());
let _tokio = crate::stackkeeper::enter_tokio_lane(lane, purpose);
job();
```

That makes Tokio's patched TLS, future `std::thread_local!` shims, diagnostics,
and later Rayon experiments see the same virtual-thread identity.

Readiness bit
-------------

Add a readiness flag for the hardware-tag path, preferably named by what it
means rather than by a VM feature:

```rust
pub const VTHREAD_HW_TAG_READY: u32 = 1 << 27;
```

Set it after AP2+ successfully enters the VMX root contract, because that is
the point where the CPU-side VMX/hardware assumptions for these carriers have
been proven.

One-shot probe
--------------

The boot probe is gated on:

```text
VTHREAD_HW_TAG_READY | BACKGROUND_AP_WORKER_READY | TOKIO_RUNTIME_READY
```

The probe should spawn two blocking jobs and log:

- virtual-thread id
- FS_BASE record address
- TLS slot
- address of a TLS cell
- value before and after
- carrier CPU slot

Success:

```text
left.fs_record != right.fs_record
left.tls_addr != right.tls_addr
left.value remains left label
right.value remains right label
```

Failure names should be explicit:

- `bad_magic`
- `same_fs_record`
- `same_tls_addr`
- `value_leak`
- `timeout`

Expected success log shape:

```text
vthread-probe: success left_vtid=1 left_fs=0x... left_tls=0x... left_cpu=3 right_vtid=2 right_fs=0x... right_tls=0x... right_cpu=4
```

Patch order
-----------

1. Add `th::vthread` identity record and FS_BASE enter guard.
2. Add two static probe records.
3. Teach Tokio TLS lookup to prefer `th::vthread::current_tls_slot()`.
4. Wrap Tokio blocking lane execution with `th::vthread::enter()`.
5. Add `VTHREAD_HW_TAG_READY`.
6. Add the one-shot probe.

That is the patch that turns "Tokio lane TLS" into a general virtual-thread
identity primitive.
