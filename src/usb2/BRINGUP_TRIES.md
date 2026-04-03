# USB2 / CrabUSB Bring-Up Tries

Current reference commits:

- `08bbc091` `crab usb done`
- `4fe0b668` `crab usb win first time`
- `1f33be6e` `ciao usb2`
- `1ad0fa01` `battlefield patchnam`

Purpose of this note:

- keep the stage model stable
- list what was already tried
- avoid repeating low-value flag toggles

## Current stage picture

The current Intel bring-up stages we can actually hit are:

1. `USBHost::new_xhci(...)`
2. `host.init().await`
3. clean vendor init succeeds:
   - halt
   - ready
   - HC reset
   - scratchpads
   - run
4. wrapper logs `init successful ... ports=...`
5. deferred Intel settled probe fires
6. vendor `kcore::_probe_devices()`
7. vendor `xhci/hub::changed_ports()`
8. if the event handler is alive too early, controller eventually trips `USBSTS.HSE`
9. if the event handler is kept off and vendor `handle_uninit()` targeted reset is skipped, first settled probe completes cleanly with `devices=0`

Current most stable Intel state:

```text
init successful
intel deferred probe before event pump
servicing settled probe
probe ctrl=0 devices=0
descriptor check ctrl=0 none
intel keeping event pump disabled after settled probe
```

Current stable old-flow Intel state:

```text
init successful
Resetting all ports of xHCI Root Hub
intel deferred probe before event pump
servicing settled probe
uninit port=X waiting for reset
uninit port=X reset complete enable=false connect=true speed=1
reseted port=X skipped (not connect+enabled)
probe ctrl=0 devices=0
intel keeping event pump disabled after settled probe
intel periodic reprobe
```

## Strong facts learned

- Bare-metal Intel can pass `host.init()` successfully.
- Clean vendor init now works without the staged-run host init hacks.
- Clean scratchpad setup works on bare metal Intel.
- The wrapper was re-simplified; giant lifecycle logic is still not needed.
- Poll-only mode proved the real IRQ-signaling path is not required for forward progress.
- The earliest toxic vendor hub path found so far was proactive `handle_uninit()` targeted reset on Intel FS ports.
- Retry failures like `while waiting for controller halt` are secondary fallout after the first runtime loss.

## Wrapper changes that were worth keeping

- Minimal wrapper restored after the big delete:
  - one host per controller
  - one init
  - one event handler
  - one tiny event pump
  - reprobe on request
- Vendor `log` facade is bridged into `crabusb:` logging.
- Most noisy logs are gated behind `USB_LOG_ALL`.
- Repeated HID interrupt timeout logs are gated behind `USB_LOG_ALL`.
- Lost-wake race fixed in vendor async waiters:
  - `backend/kmod/queue.rs`
  - `backend/ty/ep/mod.rs`

## Things we tried and what they told us

### 1. Huge wrapper / lifecycle / settle / quarantine logic

Result:

- produced lots of visibility
- not needed for basic controller bring-up
- not the right long-term architecture

Conclusion:

- keep wrapper small
- only preserve the tiny event pump idea

### 2. Immediate probe after init

Result:

- Intel often hit changed-port handling too early

Conclusion:

- deferred settled probe is better
- keep a quiet root-port window before probing Intel

### 3. Delayed settle / interrupt masking / delayed event-handler install

Result:

- not the core issue
- event path still failed in other forms

Conclusion:

- not the main bug

### 4. `host.init()` halt-wait / ready-wait strict failure

Tried:

- allow `"controller halt"` to succeed if `halted=true` even with `HSE`
- allow `"controller ready after stop"` to succeed if `cnr=false` even with `HSE`

Result:

- this let some runs progress deeper
- but did not solve the runtime loss

Conclusion:

- useful to reach later stages
- not the final fix

### 5. `aggressive_usb_reset` default feature

Tried:

- changed vendored `Cargo.toml` default features from:
  - `default = ["aggressive_usb_reset"]`
  - to `default = []`

Result:

- helped us get clean successful init runs

Conclusion:

- likely relevant to friendly bring-up
- not enough by itself

### 6. Clean vendor host-init baseline

Tried:

- removed staged-run conditional host-init behavior
- restored unconditional:
  - `setup_dcbaap()`
  - `set_cmd_ring()`
  - `setup_runtime_ring()`
  - `setup_scratchpads()`

Result:

- clean init still works on bare metal Intel
- staged-run host-init hacks were real contamination
- but removing them did not by itself fix later runtime/probe loss

Conclusion:

- keep the clean host-init baseline
- do not go back to staged-run host-init hacks

### 7. Reset request write in vendor `hub.rs`

Question:

- do we even write `PR=1`?

Answer:

- yes, vendor `reset_port()` does call `set_port_reset()`

Experiment:

- skip the `PR` write entirely

Result:

- no meaningful improvement
- controller still died in the same changed-port window

Conclusion:

- `PR=1` itself is not the primary culprit

### 8. Extra `PED=0` write next to port reset

Question:

- is `set_0_port_enabled_disabled()` poisoning the transition?

Experiment:

- removed / bypassed it

Result:

- no meaningful improvement

Conclusion:

- not the primary culprit

### 9. PORTSC change-bit clear write in `handle_changed()`

Question:

- is the ack write itself killing Intel?

Experiment:

- skip clearing `CSC/PEDC/WRC/OCC/PRC/PLC/CEC`

Result:

- no meaningful improvement

Conclusion:

- ack clear write is not the primary culprit

### 10. Event-ring / drain suspicion

Question:

- is `handle_event()` dying while draining the ring?

Probe added:

- `event stop pre-drain ...`
- `event stop post-drain ...`

Result:

- stop is pre-drain
- `HSE` already set before ring processing

Conclusion:

- not an event-ring drain bug
- not a wake/drain ordering issue at this stage

### 11. Port Link State suspicion

Question:

- are we misreading `PLS` / link-state during reset?

Result:

- not the main issue
- the stronger hardware-visible facts are:
  - `reset request never latched` in the newer targeted-reset flow
  - `reset complete enable=false connect=true` in the older blanket-reset flow

Conclusion:

- do not over-focus on `PLS`
- the real blocker is still `PED` never becoming set on Intel FS ports

### 12. Newer targeted-reset flow honesty patch

Tried:

- stop treating a reset as success if `PR` never visibly latched and `PRC` never fired

Result:

- exposed the real failure in the newer flow:
  - `port=X reset request never latched speed=1`
- this happened on multiple connected FS ports, not just one

Conclusion:

- newer targeted reset path is not really progressing on Intel FS ports

### 13. Older March 21-style root-hub flow

Tried:

- restore the older vendor root-hub shape:
  - `init()` powers and blanket-resets all ports
  - `changed_ports()` only does:
    - `handle_uninit()`
    - `handle_reseted()`
  - `handle_changed()` is not in the active path

Result:

- much more stable
- no early `HSE` spiral
- failure mode changed from:
  - `reset request never latched`
  - to:
  - `reset complete enable=false connect=true speed=1`

Conclusion:

- this old flow is a better Intel baseline than the newer targeted-reset flow
- but it still does not get `PED=1`

### 14. Old in-tree `bootstrap_ports()` behavior

Checked:

- old in-tree stack in commit `a46c12b0`
- `src/xhci.rs` had `bootstrap_ports()` right after controller reset:
  - power port if needed
  - if connected and not enabled, assert reset
  - poll until reset clears or enable appears

Tried:

- port that same idea into vendor `hub.rs` `init()`
- log:
  - `bootstrap port=X connect=... enabled=... reset=... speed=...`

Result:

- connected Intel FS ports still land at:
  - `connect=true enabled=false reset=false speed=1`
- so the missing piece was not just “do an early bootstrap pass once”

Conclusion:

- early bootstrap alone is not enough

### 15. Old in-tree second direct per-port reset

Checked:

- old `src/usb.rs` did another strict per-port reset later:
  - assert reset
  - poll until `PR` clears and `PED=1`
  - bail if `PED` never appears

Tried:

- in vendor `handle_reseted()`, if:
  - `connect=true`
  - `enabled=false`
  - then do one explicit direct reset retry instead of skipping forever

Result:

- ports repeatedly cycle through:
  - `reset complete enable=false connect=true speed=1`
  - `reseted port=X retrying direct reset after bootstrap`
- but still never promote to `enabled=true`

Conclusion:

- the second-chance direct reset retry also does not recover Intel FS ports
- current blocker remains:
  - post-reset `PED` never comes up on ports `1/3/11`

### 16. Current best Intel baseline

Current best-known stable setup:

- clean vendor host init
- scratchpads enabled
- old March 21-style blanket root-hub reset flow
- deferred first Intel settled probe
- no event pump before probe
- no event pump after probe
- periodic reprobe allowed

What it gives:

- stable controller bring-up
- stable reprobes
- no immediate `HSE` death

What it does not give:

- any enumerated devices
- because connected FS ports remain:
  - `connect=true enabled=false`

Question:

- are we using `PLS` during reset even though spec says ignore it?

Answer:

- vendor `hub.rs` logs `PLC`
- but does not appear to branch on `PLS` during `port_reset()`

Conclusion:

- not a strong current suspect

### 12. Poll-only event handler

Tried:

- event handler exists but does not arm/enable real interrupt signaling

Result:

- removed one whole class of failure
- controller can survive much farther in some runs
- but did not by itself solve Intel FS enumeration

Conclusion:

- keep poll-only as the default experimental runtime mode
- real IRQ path is not needed for current debugging

### 13. Disable event pump before first Intel settled probe

Tried:

- no event pump before the first settled Intel probe
- later also tested leaving it disabled even after that probe

Result:

- this produced the first stable, non-crashing Intel baseline
- but also left the system non-enumerating

Conclusion:

- useful stable floor
- but not sufficient for device discovery on its own

### 14. Tighten reset success criteria

Tried:

- `reset_port()` now only succeeds if reset actually shows progress
- if `PR` never latches, log it explicitly

Result:

- surfaced a real state:
  - `port=X reset request never latched speed=1`

Conclusion:

- useful for the newer targeted-reset flow
- but not the whole story, because the older flow behaves differently

### 15. Continue per-port after targeted reset failure

Tried:

- do not abort the whole `handle_uninit()` pass on one port failure

Result:

- confirmed the same reset-latch issue appears on more than one connected FS port

Conclusion:

- Intel problem is not isolated to one root port

### 16. Restore older March 21-style root-hub flow

Tried:

- blanket reset all root ports in `init()`
- remove `handle_changed()` from active scan path
- make `handle_uninit()` only wait for reset completion and then mark ports `Reseted`

Result:

- this no longer crashes
- and it changed the failure from:
  - `reset request never latched`
- to:
  - `reset complete enable=false connect=true speed=1`

Conclusion:

- the older flow is a better stable baseline than the newer targeted-reset flow
- in this older flow, reset appears to complete, but connected FS ports still never reach `PED=1`
- the current blocker has narrowed from “reset request won’t latch” to:
  - **post-reset enable never happens on connected FS ports**

### 17. Old-flow periodic reprobe

Result:

- ports stay in:
  - `connect=true`
  - `enabled=false`
- repeated reprobes do not change that state

Conclusion:

- no hidden later progression is happening
- the stable old-flow baseline is truly stuck at post-reset disabled state

## Current best read

At this point the most honest current state is:

- clean Intel init works
- old-flow blanket reset works without crashing
- connected FS root ports survive reset completion
- but they remain `enabled=false`
- no leaf devices are discovered

## Update after minimizing too far

What the minimized tree proved:

- the `PORTSC`-only write path is real and necessary
- but keeping only that fix and going back to the newer targeted-reset path regressed Intel immediately back to:
  - `targeted reset still settling connect=true enabled=false reset=false speed=1`
- so the earlier success was crossplay, not a single isolated fix

Current better read:

- Intel needs at least this combination:
  - `PORTSC`-only writes in vendor `hub.rs`
  - old bootstrap root-port reset flow in `init()`
  - deferred settled first probe
  - poll-only event handling
- removing the bootstrap reset flow while keeping `PORTSC`-only writes is not enough
- removing `PORTSC`-only writes while keeping bootstrap reset is also not enough

Working hypothesis now:

- the successful run depended on both:
  - the right write granularity (`PORTSC` only)
  - the right port-state-machine shape (bootstrap reset first, not newer targeted-reset progression)

Rule for next edits:

- preserve the combo baseline first
- only vary one dimension at a time from there

The current live blocker is:

- **post-reset port enable never happens on Intel FS root ports**

That is narrower and different from the earlier:

- `HSE` during changed-port handling
- `reset request never latched`

Those older problems were real in other flows, but the current stable old-flow baseline has moved past them.

Tried:

- `create_event_handler()` in poll-only mode
- no `arm_irq()`
- no `enable_irq()`

Result:

- Intel got materially farther in some runs
- proved that real interrupt signaling is not required for progress
- but poll-only by itself did not eliminate later `HSE`

Conclusion:

- poll-only is a valid runtime mode
- IRQ signaling is not required right now
- but the real bug was not only “interrupts enabled”

### 13. Intel `handle_uninit()` targeted reset quirk

Tried:

- skip vendor proactive targeted reset for `PortState::Uninit`

Result:

- first settled probe can complete cleanly
- no immediate `HSE` during that first settled probe
- but result is `devices=0` because nothing advances disabled ports

Conclusion:

- proactive `handle_uninit()` targeted reset is toxic on Intel
- keeping it skipped is useful for a stable baseline

### 14. No event pump before first settled probe

Tried:

- defer event handler install until after first settled Intel probe

Result:

- prevents early death before the settled probe
- first settled probe can finish cleanly

Conclusion:

- keep event pump disabled before the first settled probe on Intel

### 15. Delayed event pump after settled probe

Tried:

- after a clean settled probe, wait again and then start poll-only event pump

Result:

- controller still died immediately when the handler came up

Conclusion:

- the mere presence of the event handler after probe is still toxic right now
- do not start the event pump yet on Intel

## Things that look like bait

These consumed time but now look mostly unrelated to the real stop:

- `PR` write itself
- `PED=0` neighbor write
- PORTSC change-bit clear write
- generic event-ring drain suspicion
- repeated wrapper retry behavior
- staged-run host-init hacks as the root cause

## Things still genuinely in play

Only a few buckets still look real:

1. Intel FS root-port progression is still incomplete.
   Evidence:
   - with the stable baseline, we reach settled probe
   - vendor `hub.rs` sees connected-but-disabled FS ports
   - first settled probe still returns `devices=0`

2. Event-handler lifetime is still toxic after probe.
   Evidence:
   - first settled probe succeeds while event pump is off
   - delayed event handler startup still immediately trips `HSE`

3. The next useful progress may come from repeated static reprobe rather than event-driven progression.
   Evidence:
   - stable/no-pump baseline does not crash
   - but also does not enumerate

## What not to do next

- do not reintroduce the giant wrapper
- do not keep permuting unrelated feature flags
- do not keep adding generic visibility everywhere
- do not start a custom TRUEOS xHCI implementation unless vendor path is conclusively dead

## Best current next focus

- keep the wrapper small
- keep clean host init
- keep Intel first probe static and handler-free
- keep the Intel `handle_uninit()` targeted-reset quirk
- avoid more low-signal flag toggles unless they answer a binary question
