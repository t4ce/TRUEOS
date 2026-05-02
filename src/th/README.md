TRUEOS virtual-thread direction
===============================

This folder is for the current TRUEOS thread direction.

The `*_onlyasreference/` directories are preserved idea/reference material. They
are not the plan for the active implementation path.

Current goal
------------

Build a small virtual-thread identity substrate, not a guest VM, not a
TRUEOS hull, and not a second hypervisor personality.

The core idea is:

```text
GS_BASE = physical CPU carrier / PerCpu
FS_BASE = virtual thread identity / TLS persona
```

An Embassy AP task may carry work, but the work executes as a virtual thread
when FS_BASE points at a TRUEOS-owned, magic-validated virtual-thread record.
Libraries and runtime glue should eventually ask "which virtual thread am I?"
instead of guessing from CPU slot, VM id, or Tokio lane state.

See `vthread_identity_plan.md` for the first patch boundary.
