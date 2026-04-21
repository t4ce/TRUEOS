# VM Lane Substrate For Tokio And Hull Execution

The current TRUEOS integration point with Tokio is intentionally not a host-thread shim.

The design choice is:

- treat executor-backed VM lanes as the native carrier substrate
- keep VM hull execution on that substrate
- target the same substrate for future Tokio blocking/offload work
- avoid growing a parallel abstraction that tries to look like POSIX threads

## What Is Already Proven

The current-thread Tokio probe on `target_os="zkvm"` is now green for:

- runtime construction and `block_on`
- core task scheduling
- `tokio::sync`
- `tokio::time`
- `tokio::io-util`
- `tokio::io-std`
- `tokio::fs` surface compilation
- `tokio::parking_lot`
- `tokio::test-util`

That means the remaining boundary is no longer the Tokio task/scheduler model in general.

The remaining boundary is carrier execution for:

- `spawn_blocking`
- real `tokio::fs` runtime operations
- eventual multi-worker Tokio launch paths

## Why VM Lanes

TRUEOS already has the thing Tokio actually wants more than it wants host threads:

- stable execution lanes
- per-lane executor entry
- wakeable background carriers
- explicit slot placement
- the ability to keep critical and disposable work separated

This makes VM lanes the obvious native substrate for both:

- VM hull execution
- future Tokio blocking carriers

## Lane Roles

`src/hv/guest_work.rs` now defines four lane roles:

- `vm-hull`
- `tokio-blocking`
- `worker`
- `service`

These are not separate runtimes. They are named uses of the same carrier-lane substrate.

The important invariant is that `vm-hull` and `tokio-blocking` should continue to converge on the same underlying executor-backed lane model unless a later hard technical constraint proves otherwise.

## Placement Rules

The current placement story is intentionally simple:

- slot `0` is BSP/local and not a VM lane target
- slot `1` is AP1/service
- slots `>= 2` are disposable worker lanes
- reserved VM lane work prefers perf cores on slots `>= 2`

This means hull execution stays off BSP and AP1, and future Tokio blocking work can use the same disposable-lane substrate without inventing a separate scheduling story.

## Why This Matters

If later Tokio vendor work needs a custom blocking spawn backend, the first obvious TRUEOS hook should be:

- `pick_tokio_blocking_lane()`

If later VM work needs a hull lane, the obvious hook should be:

- `pick_vm_hull_lane()`

That keeps the codebase aligned with the integration steps that were actually validated during bring-up instead of rediscovering them through drift.