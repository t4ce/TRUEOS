# Build size estimates

The selector shows the measured packed size beside each of the 12 buildins,
the total for the selected apps, the payload omitted by deselecting apps, and
an estimated kernel-plus-apps total. **All**, **None**, presets, and individual
checkboxes update these values immediately. App choices and subsystem choices
remain independent.

The initial reference is the supplied **TRUEOS 0.0.235** release. Exact byte
counts, package and artifact hashes, import counts, and validation details are
recorded in [`measurements/0.0.235.json`](measurements/0.0.235.json). The smaller
presentation dataset lives in [`catalog.json`](catalog.json) under
`size_estimates`, served through the existing catalog API.

## What each number means

| Display | Basis |
| --- | --- |
| Size beside an app | That app's complete packed `.bp` package from the reference release. |
| Selected apps | Sum of the selected apps' exact packed byte counts. |
| Omitted app payload | All 12 packed packages minus the selected package sum. |
| Kernel + assets baseline | Reference kernel ELF size minus the actual embedded compressed AppDB byte range. |
| Kernel + selected apps estimate | Derived kernel baseline plus the selected package sum. |
| Reference release download | Actual compressed size of the complete supplied release, shown separately. |
| Subsystem savings | Unmeasured; no speculative byte amount is deducted. |

All displayed kilobytes use **decimal kB: 1 kB = 1,000 bytes**, rounded to the
nearest whole kB. Totals are computed from exact bytes before rounding. The
individually rounded app labels sum to 6,116 kB, while the correctly rounded
aggregate is **6,115 kB**. Adding already rounded labels would introduce error.

### Measured app payloads

| Buildin | Packed BP bytes | Displayed kB | ELF bytes | All imports | Unix/POSIX imports |
| --- | ---: | ---: | ---: | ---: | ---: |
| SSH | 647,986 | 648 | 1,150,840 | 48 | 0 |
| Hull | 3,019,130 | 3,019 | 6,876,568 | 101 | 0 |
| Share | 735,292 | 735 | 1,364,960 | 46 | 0 |
| Edit | 475,282 | 475 | 1,053,448 | 39 | 0 |
| Trauma | 378,732 | 379 | 682,016 | 50 | 0 |
| Termdir | 336,520 | 337 | 609,032 | 53 | 0 |
| Appstore | 125,526 | 126 | 234,024 | 42 | 0 |
| Image viewer | 128,797 | 129 | 217,968 | 49 | 0 |
| OS | 92,018 | 92 | 174,752 | 35 | 0 |
| Backup | 94,810 | 95 | 173,784 | 30 | 0 |
| Commander | 56,626 | 57 | 111,240 | 31 | 0 |
| Paper | 24,482 | 24 | 40,624 | 41 | 0 |
| **Total** | **6,115,201** | **6,115** | **12,689,256** | — | **0** |

Relative to the older table supplied with the release, SSH's packed size fell
from 1,384,524 to 647,986 bytes: **736,538 bytes smaller, a 53.2% reduction**.
Its ELF fell from 2,920,648 to 1,150,840 bytes, and its undefined import count
fell from 63 to 48. The older artifact was not remeasured; those comparisons
use the older pasted counts.

The Unix/POSIX column intersects each ELF's unique undefined symbol names with
`is_unix_import` in `src/unix_compat.rs` at the v0.0.235 tag commit
`75e9b1dc08189d3bba546213b4d5f3b4f3ee9757`. All 12 app modules have zero matches.
This uses the upstream native-only audit's classification. Hull's
`__assert_fail`, `modf`, `lrint`, and `atanh` helpers are outside that classifier.
Import counts describe these app modules, not every dependency or capability
in the kernel.

## Kernel estimate and its limits

The release contains a 26,213,712-byte `TRUEOS.elf`. Its compressed AppDB is a
5,825,237-byte LZ4 frame starting at ELF file offset 14,615,049. Subtracting that
actual region gives a **20,388,475-byte derived baseline**.

```text
selected_app_bytes = sum(packed_bp_bytes for selected apps)
omitted_app_bytes = 6,115,201 - selected_app_bytes
derived_kernel_baseline = 26,213,712 - 5,825,237 = 20,388,475
estimated_kernel_plus_apps = derived_kernel_baseline + selected_app_bytes
```

The baseline includes other embedded assets, kernel support code, ELF headers,
and layout overhead. It is **not a separately compiled kernel-only image**.
The selector identifies it as derived, and the total remains an estimate.
See [Kernel only](KERNEL_ONLY.md#measure-the-kernel-separately) for the real
zero-app build comparison.

This reference still comes from the graphics-capable kernel. It is not a
measured [Server](SERVER.md) baseline with UI4 and GPU implementations removed.

| App selection | Selected app payload | Omitted app payload | Kernel + apps estimate |
| --- | ---: | ---: | ---: |
| All 12 | 6,115 kB | 0 kB | 26,504 kB |
| Edit + Termdir | 812 kB | 5,303 kB | 21,200 kB |
| SSH | 648 kB | 5,467 kB | 21,036 kB |
| None | 0 kB | 6,115 kB | 20,388 kB |

With all apps selected, the additive estimate is 26,503,676 bytes, while the
observed full kernel ELF is 26,213,712 bytes. The **289,964-byte difference**
shows why packed-package weights are not exact marginal ELF savings: the app
database wraps and compresses the packages again, and kernel linking/layout
also affects the result. Removing an app can change database overhead and
compression ratios. The model does not assign these interactions to one app.

The supplied release archive is **19,447,079 bytes** compressed. It also
contains the 61,440-byte EFI loader and 335,360-byte Intel GuC firmware. Its
outer compression is a separate measurement. App package savings must not be
subtracted directly from that download size. None of these file-size values
measures runtime RAM consumption.

The archive has no source/provenance sidecar. Its source commit and sibling
dependency revisions cannot be proven from the archive;
`source_commit_verified_from_archive` in the measurement record and
`source_commit` in the catalog reference are therefore null. `tag_commit` records the source
tag used for comparison and import classification, not verified binary
provenance. The release profile comes from the user's description. Development
builds and builds from different inputs keep an explicit release-reference
label; their final sizes are not predicted as exact.

## Subsystem deselection

USB, Intel iGPU, and Virgl currently gate device initialization, with shared
interfaces and driver code still compiled. The other feature switches may
also change generated code, but one stripped release ELF cannot establish
the amount attributable to each feature. The selector reports subsystem
savings as **unmeasured** and leaves the displayed kernel baseline unchanged
when these switches change. This does not claim the real size difference is
zero.

To add a measured subsystem delta:

1. Pin the same kernel and sibling commits, compiler, dependency lockfiles,
   profile, and app selection. Starting with zero apps helps separate app
   payload effects from kernel feature effects.
2. Build the baseline, then change the intended feature set and rebuild.
   Respect dependencies: turning off iGPU also requires turning off the
   current H.264 and native GPGPU features. Record that whole change.
3. Measure the stripped `TRUEOS.elf` and release archive independently, and
   preserve each exact configuration and artifact hash with the measurement.
4. Present the delta as applying to that pair of configurations. Do not add
   independently measured marginal deltas for combinations that were never
   built; shared code, linking, and compression make them context dependent.

## Updating the reference

Use real prepared `.bp` packages from a known build, its final stripped kernel
ELF, and its compressed release archive. Save the exact app set, bytes, hashes,
profile, source/compiler/dependency provenance, and derivation method under
`measurements/`. Prefer a matched zero-app build for a future measured kernel
baseline. If only a release artifact is available, distinguish its measured
regions from inferred build metadata as this reference does.

The initial attached archive was decoded with bounded LZ4 decompression and
only regular, allowlisted tar members were extracted. The embedded database
frame, redb record bounds, BP headers/digests, nested tar lengths, ELF formats,
and undefined symbols were checked without executing the binaries. The JSON
evidence records the detailed checks and artifact hashes.

Update `catalog.size_estimates` from the measured record. Keep byte counts as
integers, require coverage for every selectable app, and use null/unmeasured
for unknown values. Tests bind the UI dataset to the evidence and exercise
the actual checkbox arithmetic, missing-data handling, profile labels, and
queued selection display.

Size metadata is presentation data. It is not an input to the configuration
hash, signing challenge, license receipt, or build request. Changing a
measurement cannot change the requested app set or kernel features. An active
job's estimate uses the job's resolved manifest, so later checkbox edits do
not relabel an already queued build.
