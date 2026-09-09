# Cubes autostart: missing host opcode declaration

The September 9 boot loaded `cubes.bp` (410136 bytes), resolved all 69 imports,
including `trueos_cabi_ui4_scene_set_display_bottom_color`, entered VM 0, and
created its Picasso carrier. No initial window became visible.

Read-only live diagnostics showed VM 0 still reported running, with five vGPU
buffers and only 364 uploaded bytes. Its SMP lane was idle and no Blueprint
native worker was running. The GPU reported no lost device, engine quarantine,
or memory fault. This placed the failure after foreground resource setup but
before its first seed upload and presentation.

## Cause

The new display-bottom-color operation had a guest opcode declaration (`0x208`)
and a host match arm, but no declaration in the host's separate opcode table.
Rust interpreted the bare match-arm name as a binding that matched every
remaining opcode. That intercepted later handlers, including opacity, native
worker creation (`0x62`), logging, and shutdown. The worker request's arguments
were rejected as invalid display-color arguments, preventing Cubes from
completing background startup and reaching first presentation.

The previous compile check succeeded with unreachable-pattern warnings. Those
warnings were missed; the initial ABI tests exercised the exported function
but not the host dispatch arm.

## Correction and validation

- The host opcode now aliases the guest's authoritative constant.
- `dispatch_inner` denies unreachable patterns, making this failure a build
  error instead of a warning.
- `tools/test_ui4_display_bottom_color.py` compiles the actual host arm and
  host constant. It checks every other opcode through `0x300` bypasses that
  handler, the intended opcode reaches it, and wide arguments are rejected.
  Existing owner, closed-frame, RGB, hardware-failure, and guest-forwarding
  checks also pass (four tests total).
- `cargo check` passed with the dispatcher lint enforced and no unreachable
  pattern diagnostics.

Evidence is preserved under `bld/cubes-autostart-incident/`: boot log,
vGPU/SMP/apps status and Matrix transcript captures, original compiler warnings,
and corrected compile output. The diagnostic connection was returned to the
default Matrix slot and command mode, then closed. No app was stopped or
restarted, and the rig was not rebooted or deployed by the agent.

The fix requires rebuilding/redeploying TRUEOS. The existing Cubes package and
its imported ABI remain valid; no Cubes repack is required for this correction.
Fresh-boot autostart presentation still needs verification with the fixed kernel.
