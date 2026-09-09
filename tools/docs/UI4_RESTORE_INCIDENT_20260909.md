# Cubes restore / stop incident, 2026-09-09

Evidence was collected before reboot; no reset, app launch, engine probe or
live kernel replacement was performed. The rig remains on its original boot.

Local evidence is in `bld/resize-incident-20260909T013945Z/`:

- Original and later boot logs, reset identity receipt and orchestration log.
- The exact boot runtime ELF, SHA-256
  `e2c965c73c5c40657cd54d1c63cf485bb4196853b41870888094f4023b197463`.
- Shell2 `vgpu status`, `smp`, `ram` responses, raw and ANSI-stripped.
- Fresh `tlb dump`, 194241 bytes, downloaded from discovered TRUEOSFS root 1
  only after the command reported writing that exact size.
- Filesystem discovery trees and a SHA-256 manifest of the captured files.

## What the current boot establishes

The user observed a failure when dragging Cubes back from maximized. The log
then records a requested VM stop, the Hull returning at a VMCALL boundary,
`native-worker: draining vm=0 jobs=1 resources=retained`, and repeated UI4
`PresentFailed` retries (thousands), while audio, networking and Shell2 remain
alive. Subsequent stop/eject attempts leave Cubes stop-pending.

The GPU status reports no lost adapter, quarantined lane, memory-category
fault or GT fault. Host and physical RAM have ample space. The later hardware
dump shows the execution context submission count advancing beyond the earlier
status snapshot; an outstanding timeline point alone is not evidence of a
permanently hung GPU. The native worker still owns service lane 3.

The old error record does not name a failing window, layer or presentation
stage. It cannot establish which resize event or surface first triggered the
failure. In particular, no completed Cubes resize log is present in the saved
boot. The code defects below are reproducible and consistent with the observed
restore/stop state; a rebuilt rig run is needed to confirm the complete chain.

## Corrections

1. A restored window's logical extent can be smaller while its published pair
   is still fullscreen. Dragging previously translated that old fullscreen
   presentation off the output, invalidating direct scanout. Both owner moves
   and UI4 dragging now clamp the held presentation to the output until the
   pair commits its replacement. Logical placement still follows interaction.
2. Dirty/double Blueprint backgrounds were omitted from shared-composition
   admission. A lone background needing fallback could return `PresentFailed`
   for the entire four-plane transaction. Its stable published front is now
   admitted to the compositor, like its streaming/triple foreground.
3. An A → B → A resize acknowledgment advanced only the foreground's staged
   epoch. Both members now receive the newer epoch under the same surface lock,
   including when one replacement has already published.
4. Forced Hull stop does not execute Cubes' Rust destructors, so its detached
   background loop never saw the local stop flag. The native worker API now
   exposes closed-admission cancellation; Cubes checks it on every loop,
   including idle turns. Accepted GPU work still retires before the job returns
   and before VM storage is released. This adds one Rust ABI symbol and VMCALL
   0x207, with matching kernel/SDK declarations and a loader export.
5. Presentation errors now identify completion/flip stages, queue slot/error,
   or the window/layer/source/placement rejected by direct-only admission.

## Validation

- `python3 tools/test_ui4_restore_hold.py`: actual geometry helper and fallback
  policy, including reproduction of the invalid old fullscreen translation.
- `python3 tools/test_ui4_layered_resize.py`: paired publication/rollback and
  ABA epoch refresh with either producer completing first.
- Existing weighted lease and close-scaler checks.
- `python3 tools/check_native_worker_contract.py --blueprints ../TRUEOS-Blueprints`.
- `python3 tools/test_native_worker_lifetime.py`: isolated host tests of the
  actual SDK worker module and kernel admission/drain state (five tests).
- `cargo check --bin TRUEOS` and actual `cargo bp cubes` passed.

The full Blueprint API host test binary cannot link because its unrelated
shutdown/write CABI symbols require the kernel. The isolated worker harness
avoids that linkage without adding production stubs.

The running kernel has not been patched in place. Rebuild/deploy the kernel and
use the newly packed Cubes for physical restore/drag/stop verification; the new
Cubes worker import requires the matching kernel export.
