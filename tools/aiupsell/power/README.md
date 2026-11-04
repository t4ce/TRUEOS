# Bare-metal power measurement attempt — 2026-11-04

**We have desktop temperature and halt-state observations, but no measured watts yet.** The README's 30% bar counts implemented building blocks; it is not a power-saving percentage.

## Observations from the running rig

Hardware inventory exported by the rig identifies an Intel Core i5-14500T, CPUID `06-bf-02`, microcode `0x3e`, 20 logical CPUs, and a JGINYUE B760I Snow Dream D5 V1.0 motherboard.

| Observation | Result |
| --- | --- |
| Capture window | 2026-11-04 21:03:00–21:03:18 UTC; ten SSH/TLB snapshots about two seconds apart |
| Package temperature | 32–33°C; mean 32.1°C |
| Logical CPUs marked `hlt_now` | 11–18 of 20 per snapshot |
| Package thermal / PROCHOT status | Neither asserted in the ten captured rows |
| Exported RAPL snapshot | Zero updates; `sample_valid=false` |
| Package energy / watts | Unavailable |

[Recorded sensor rows and timestamps](2026-11-04-desktop.json) retain the package CSV and the beginning of each per-core diagnostic row. Collection used the existing ADM → TLB → thermal selector. No energy counters were read during this capture.

This was the existing desktop: Hull and Edit appeared throughout the captured slot list. Spirit's animation and the GPU's power state were not independently verified. SSH and the diagnostic queries add work. The session later disconnected, and the reconnect showed lower kernel uptime, so samples from the two sessions must not be combined. A subsequent Termdir launch attempt did not produce a verified launch. No controlled Cubes, Termdir, or fixed-compute energy comparison was completed.

`hlt_now` records the state at a snapshot, not C-state residency or a percentage of time asleep. AP sensor data were passively cached, sometimes for much longer than the capture window; their temperatures and performance counters cannot establish a fresh load comparison. The package temperature is useful context, not a conversion to watts. Cooling, room temperature, fan settings and display power were not controlled.

## Measurement support added in source

The resident `raple-service` remains disabled. Previously the probe admitted any Intel CPU with MSR instructions and read all five domains unconditionally. CPUID's MSR bit does not establish that a particular register exists.

The initial probe now admits only bare-metal Intel family 6 model `0xbf`, matching this rig, and reads only the RAPL units register (`0x606`) and package energy register (`0x611`). Virtual machines and other CPU profiles are rejected. Optional PP0, graphics, DRAM and platform domains remain **not probed**, with blank energy/power fields rather than fabricated zeroes. This is a narrow model profile, not a general fault-recovering MSR probe; firmware-level register access still needs validation on hardware.

An explicit **`tlb rapl sample`** command, also available as **rapl sample** in the native TLB selector, takes one sample and appends it to the existing memory history. The ordinary RAPL view and full inventory dump remain cached reads. Sampling is serialized. The first sample establishes a baseline; another sample 1–10 seconds later reports average package watts. Zero-duration intervals and gaps longer than 60 seconds do not produce watts. The 32-bit delta handles one counter wrap; repeated short intervals avoid ambiguous long gaps.

Use **`tlb rapl store`** / **rapl store** to persist the history to the mounted root's `rapl.txt`. This avoids writes on every sample. History remains bounded at 5 MiB.

**These changes are local source changes, not a deployed measurement result.** Host arithmetic/admission tests and `cargo check --release --locked --bin TRUEOS` passed. The dev check lacked prepared dev builtins; release checking used the existing prepared release set. No kernel update or reboot was initiated for this attempt.

## Next run on the rig

After booting a kernel containing this change:

1. Record the kernel revision, display resolution/refresh, connected devices, fan policy and room temperature. Keep them fixed. Close unrelated apps and leave Spirit in a specified animation state.
2. Allow the desktop to settle. Take an initial **rapl sample**, then sample every 1–10 seconds for a 60-second baseline. Record thermal snapshots alongside it.
3. Repeat for Termdir open, fullscreen Cubes, and a fixed compute job. Keep the same scene/settings; record frames or completed work and elapsed time. Merely opening an app does not prove that the GPU is computing or that it is asleep afterwards.
4. Discard the transition interval when switching conditions. Repeat baseline → workload → baseline at least three times, allowing temperature to settle between runs. Save history and label each condition's start/end kernel timestamps.
5. Compute interval watts as `delta_joules / elapsed_seconds`, and window watts as `sum(delta_joules) / sum(elapsed_seconds)`. For fixed work, report joules per completed job as well as duration. Keep raw intervals and report the spread across repeats.

Package energy covers the processor package, including its integrated components; it is not whole-machine AC input. Optional domains overlap package scope and should not simply be summed. Intel describes the domain counters as accumulated energy, and Linux converts their deltas over elapsed time to watts. See [Intel's RAPL description](https://www.intel.com/content/www/us/en/developer/articles/technical/software-security-guidance/advisory-guidance/running-average-power-limit-energy-reporting.html) and the [upstream Linux RAPL implementation](https://github.com/torvalds/linux/blob/master/arch/x86/events/rapl.c).

No motherboard-wide or wall-power reading was obtained. To substantiate a whole-rig claim, use an external energy meter alongside RAPL, or implement and validate a documented board sensor interface. To claim a saving, compare equal completed work and equivalent desktop conditions against a stated reference configuration. This capture alone establishes neither savings nor superiority over another OS.
