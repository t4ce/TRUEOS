# power/shell2: expose Intel HFI and Thread Director CPUID metadata

**Base:** `true@725095bfcbe5a159feb4731e9ee118eb838a9d6f`

## Motivation

TRUEOS already identifies each logical processor as a performance, efficiency, or unknown core through `CPUID.1A`, stores that identity in `CpuProfile`, and uses it for explicit worker placement. The next useful layer is to retain Intel Hardware Feedback Interface and Thread Director enumeration data before any scheduler policy starts depending on it.

This change deliberately establishes that observational layer only. It does not enable HFI, program feedback MSRs, allocate a hardware feedback table, register package thermal notifications, or alter task placement.

## Changes

- Add `power::hfi::IntelHfiCpuid`, a CPUID-only decoder for:
  - HFI support (`CPUID.06:EAX[19]`)
  - Intel Thread Director support (`CPUID.06:EAX[23]`)
  - Thread Director class count (`CPUID.06:ECX[15:8]`)
  - advertised HFI capability columns (`CPUID.06:EDX[7:0]`)
  - HFI table page count (`CPUID.06:EDX[11:8] + 1`)
  - per-logical-CPU, package-local HFI row index (`CPUID.06:EDX[31:16]`)
  - raw `CPUID.06` registers for later decoding
- Capture that data on every CPU during the existing `CpuProfile` registration path and publish it with the profile record.
- Add `tlb hfi` for a decoded summary plus one row per registered logical CPU, including x2APIC package/core/SMT identity and the existing P/E core kind.
- Add an `Intel HFI / Thread Director CPUID` section to `tlb dump`.
- Add `hfi` to the live Shell2 `tlb` tool schema, so `trueos-doc command tlb` discovers it.
- Add decoder unit tests and a `trueos-doc` schema regression test.

## Diagnostic contract

`tlb hfi` and `tlb dump` are explicitly read-only:

```text
capture_policy=registration-time-cpuid-only
msr_programming=no
hardware_table=unconfigured
scheduler_consumer=none
```

The per-CPU HFI index is reported as package-local. Raw register values remain in the dump so later HFI-table work does not need to extend the profile ABI merely to recover enumeration data.

## Non-goals

This PR does **not**:

- read or write `IA32_HW_FEEDBACK_PTR` or `IA32_HW_FEEDBACK_CONFIG`
- allocate, map, enable, or consume an HFI table
- handle HFI update interrupts
- classify running tasks
- change `ComputeWorkerPolicy`
- migrate tasks between per-CPU executors

Those steps can follow once physical-core/SMT topology and the observational output have been validated on the target Intel machine.

## Validation performed in this environment

- Unified diff parsed successfully: 6 files, 460 insertions, 7 deletions.
- Patch applied successfully to a generated preimage containing every original hunk context.
- `git diff --check` passed on the resulting postimage.
- The modified `TOOL_JSON_TLB` payload parsed as JSON and contains the `hfi` target.
- Delimiter/string/comment balance checks passed for the new Rust module and the inserted TLB implementation.

## Validation still required in a TRUEOS checkout

This environment has no Rust toolchain and no writable GitHub checkout, so compilation, formatting, tests, and hardware execution were not run here.

Recommended local checks:

```sh
git apply --check trueos-intel-hfi-cpuid.patch
git apply trueos-intel-hfi-cpuid.patch
python3 tools/test_trueos_doc.py
```

Then run the repository's normal non-deploy formatting/check/build path. On the intended Intel target, inspect:

```text
tlb hfi
tlb dump
```

and verify the generated `trueos/pci/tlb.txt` HFI section, especially package-local row indexes across P-core SMT siblings and E-cores.
