# SK hynix Lumen Monopoly Revert

This tree currently has a hardwired rescue boot path for the SK hynix UAS disk
`152E:7001`.

What it does:

- TRUEOSFS still mounts the SK hynix disk and records it as the primary root.
- `lumen-service` starts without waiting for `TRUEOSFS_ROOT_MOUNTED`.
- The SK hynix root mount does **not** set `TRUEOSFS_ROOT_MOUNTED`, so chat,
  webmail, file explorer, http-trueosfs, and similar filesystem consumers stay
  parked while Lumen owns the disk path.
- Emulator NVMe roots still use the normal readiness handoff.

To undo it:

1. In `src/allcaps.rs`, change:

   ```rust
   pub const SKHYNIX_UAS_LUMEN_MONOPOLY: bool = true;
   ```

   to:

   ```rust
   pub const SKHYNIX_UAS_LUMEN_MONOPOLY: bool = false;
   ```

2. In `src/r/spawn_service.rs`, change the `lumen-service` task gate back from:

   ```rust
   0,
   ```

   to:

   ```rust
   crate::r::readiness::TRUEOSFS_ROOT_MOUNTED,
   ```

3. In `src/r/fs/trueosfs.rs`, remove the `skhynix_lumen_monopoly_root(...)`
   branch in `register_root_mount` and leave the plain readiness set:

   ```rust
   crate::r::readiness::set(crate::r::readiness::TRUEOSFS_ROOT_MOUNTED);
   ```

That restores the old flow where mounting the root filesystem releases every
service gated by `TRUEOSFS_ROOT_MOUNTED`.
