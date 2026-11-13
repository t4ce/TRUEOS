# TRUEOS § Kernel only

**Kernel only is a regular configuration option: build TRUEOS with zero buildins.**

The choice controls the bundled application payload. It does not remove kernel
capabilities. A kernel-only image starts without the bundled file explorer,
app store, text editor, or other buildin apps. There is no app-store application
waiting to install the rest of the desktop for the user.

This is a valid starting point for someone who wants the kernel and its
selected runtime capabilities, supplies their own applications, or wants to
measure the kernel separately from the bundled application payload.

## Choose the configuration

In the subsystem selector:

1. Select **Kernel only**, or select any other preset and choose **None** in
   the bundled-app checklist. Unchecking every individual buildin is equivalent.
2. Choose the subsystems you want: USB, Intel iGPU, Virgl, and the other kernel
   features remain independent choices.
3. Review the resolved manifest. Its `buildins` array is empty, its selected
   feature list omits `buildins`, and its Cargo feature list omits `buildin-apps`.
4. Request the build through the normal anonymous or key-bound flow, according
   to the server's policy.

The Kernel only preset has the same kernel feature selection as Universal,
with the app payload omitted. It does not silently choose a reduced hardware
configuration. It has no experimental label or extra opt-in gate.

Each buildin is also individually selectable. Keeping one tool, several tools,
or all 12 is an equally regular configuration; see [Individual buildins](BUILDINS.md).

For a direct local build:

```sh
bash tools/trueos-boot/prepare.sh --release --no-buildins
```

For a development kernel:

```sh
bash tools/trueos-boot/prepare.sh --dev --no-buildins
```

Both option orders are accepted. A cloud manifest is authoritative: if it asks
for bundled apps, adding `--no-buildins` fails before compilation instead of
silently changing the signed configuration. Resolve a new manifest with
an empty `buildins` list for that request. The existing
`"features": {"buildins": false}` shorthand still resolves to the same result.

## Combine it with subsystem deselection

The simplest request preserves the normal kernel capabilities:

```json
{"preset": "kernel-only", "profile": "release"}
```

For example, a QEMU/Virgl configuration with neither bundled apps nor USB is:

```json
{
  "preset": "qemu-virgl",
  "profile": "release",
  "features": {"usb": false},
  "buildins": []
}
```

With no buildins, there are no bundled applications to compile or check for
compatibility with deselected subsystems. Constraints inside the kernel still
apply: the current H.264 and native GPGPU features require Intel iGPU, for
example. The selector resolves those dependencies as usual.

The existing USB/iGPU/Virgl switches control driver initialization. This option
adds actual omission of the embedded app payload; it does not change those
driver switches into complete driver-code removal.

An image with no apps is therefore distinct from a GPU-free Server build.
[TRUEOS Server](SERVER.md) records the separate configuration target that
retains sh3 and services while excluding UI4 and GPU implementations.

## What the build and boot paths do

| Area | Kernel-only behavior |
| --- | --- |
| Buildin compilation | Skip the selected-buildins adapter and sibling app builder entirely. |
| Editor API reference | Skip the app-specific API reference generation step. |
| Buildin validation | Do not inspect `buildins.json`, require a prepared buildin set, or run its check command. |
| Embedded app payload | Do not include a compressed app database in the kernel. Clear generated payload files so a previous app bundle cannot leak into this build. |
| App database at boot | Create empty, writable `apps`, `hashes`, and `updated` tables in RAM. |
| Default app startup | The unchanged embedded startup configuration skips unavailable local apps; it does not download them through an app-store fallback. |
| Runtime capabilities | Keep explicit Blueprint launch, application insertion, filesystem, and other selected kernel APIs. |
| Cloud worker and CI | Do not require or check out TRUEOS-Buildins. Record that buildins are disabled in provenance. |

The kernel still has source dependencies in `TRUEOS-Blueprints`, so that sibling
checkout and the archived TRUEOS toolchain remain build prerequisites. They
are distinct from compiling and embedding the bundled applications.

The ordinary default build includes the `buildin-apps` Cargo feature and keeps
its existing prepared-app validation. Direct Cargo invocations that omit it
use the empty-catalog path. The preparation script and cloud planner choose the
remaining Cargo features explicitly so disabling app bundling preserves the
chosen kernel configuration.

The zero-app default does not disable user-provided startup configuration or
explicit application launches. Supplying applications later still requires an
available platform entry point; the absent graphical app store cannot perform
that step. Application data added to the current RAM app database remains
volatile, as it is in the ordinary build.

## Measure the kernel separately

The selector's initial [size reference](SIZE_ESTIMATES.md) derives a
**20,388 kB** kernel-plus-assets baseline by subtracting the embedded compressed
AppDB from the supplied TRUEOS 0.0.235 ELF. That helps show selected app weights,
but a separately compiled zero-app image is required for an actual kernel-only
size. The following comparison produces that measurement.

Compare the same source commit, compiler, dependency revisions, profile, and
kernel feature selection. Change only whether the app payload is bundled.
For the default kernel feature set, on a provisioned Linux builder:

```sh
# Default kernel features with bundled apps.
bash tools/trueos-boot/prepare.sh --release
stat -Lc '%n: %s bytes' tgt/pxe-current/TRUEOS.elf tgt/rigreal.tar.lz4

# The same kernel features with zero bundled apps.
bash tools/trueos-boot/prepare.sh --release --no-buildins
stat -Lc '%n: %s bytes' tgt/pxe-current/TRUEOS.elf tgt/rigreal.tar.lz4
```

`TRUEOS.elf` is the stripped runtime kernel. The compressed archive also
contains the loader and firmware, so report those two sizes separately.
Record measured values after building; this option does not promise a fixed
number of kilobytes saved. Removing additional subsystems is a separate
comparison and should list the changed features.

## Extending the configuration guide

Keep the individual app checklist and kernel subsystem choices independent as more
configurations are added. The guided process should explain the applications
available at first boot, show the selected kernel capabilities, and report
measured artifact sizes for the exact resolved manifest. Add real dependencies
to the shared catalog when needed; an empty buildin set must not acquire a
dependency on a bundled desktop app.

Host tests cover manifest composition, skipping app build and validation
steps, avoiding stale payloads, conditional worker/CI dependencies, and
licensing compatibility. Full kernel compilation, boot validation, and size
measurements require the provisioned TRUEOS builder and target hardware.
