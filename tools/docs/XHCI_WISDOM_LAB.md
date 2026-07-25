# xHCI Wisdom Lab

`xhci` is the register-level laboratory for the live CrabUSB controller owner.
It does not create a second MMIO mapping. Requests are queued to the USB
controller service, and the event handler uses the same register lock.

## First run

With external test peripherals unplugged:

```text
xhci stage 1
xhci stage 2
tlb usb
```

Stage 1 prints the controller capability, operational, runtime, Supported
Protocol, and complete physical-port register census. Stage 2 cooperatively
parks new SKHYNIX UAS operations, drains in-flight UAS work, and samples every
port for 500 ms without writing a register. Parked filesystem requests resume
after quarantine rather than receiving a synthetic disk error.

Port 11 is the known fused mainboard LED-controller path. It is always observed
as an ambient actor but automated mutations refuse it unless the command
contains the separate literal `fused`.

## Mutating stages

```text
xhci stage 3 <port> arm
xhci stage 4 <port> arm
xhci stage 5 <port> arm depth=2
```

- Stage 3 proves a neutral PORTSC write and clears only RW1C change bits that
  were observed set.
- Stage 4 applies a protocol-aware power/reset ladder and records every timed
  transition.
- Stage 5 restores a powered/reset baseline before every replay branch, then
  explores neutral, acknowledge, power-off, power-on, disable, and reset. USB3
  ports additionally explore warm reset, RxDetect, and U0.

Depth 2 is exhaustive for that action vocabulary. Depth 3 is capped at 128
branches so a bad controller cannot trap the shell in an unbounded experiment.

Stages 4 and 5 refuse a target with `PORTSC.CCS=1`. Add `live` only when
disrupting the physically connected device is intentional:

```text
xhci stage 4 <port> arm live
```

Targeting the fused LED port requires both acknowledgements:

```text
xhci stage 4 11 arm live fused
```

## Direct wire

Offsets are relative to the mapped xHCI BAR and accept decimal or `0x` syntax:

```text
xhci read <offset>
xhci read64 <offset>
xhci write <offset> <u32-value> arm
xhci write64 <offset> <u64-value> arm
xhci rmw <offset> <u32-clear-mask> <u32-set-mask> arm
```

Raw writes that resolve inside a connected physical port require `live`; port
11 additionally requires `fused`. Controller-global writes require the same
acknowledgements whenever those devices are connected. Aperture bounds and
access alignment are validated inside CrabUSB before volatile access.

## Evidence

Every observation uses the `xhci-wisdom` prefix and includes a run number.
Mutations record BAR offset, before value, requested value, immediate readback,
and delayed observed state. Changes on non-target ports are marked `ambient`;
changes on port 11 are additionally marked `fused_ambient`.

The in-memory journal retains the latest 2,048 lines:

```text
xhci status
xhci journal
```

The USB Trace log remains the complete record if an unusually noisy run wraps
the in-memory journal.
