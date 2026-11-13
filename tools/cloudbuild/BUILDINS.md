# Choose individual buildins

**Every existing buildin is a regular, independent packaging choice.** Choose
all 12, any subset, or none. An app selection does not change USB, Intel iGPU,
Virgl, or the other kernel features. Selecting zero apps is
[TRUEOS § Kernel only](KERNEL_ONLY.md).

## The existing 12-app checklist

The selector catalog follows the existing
[`TRUEOS-Buildins/buildins.json`](https://github.com/t4ce/TRUEOS-Buildins/blob/794a711607a159d97319f73c96ec0d8a29953c9f/buildins.json)
at revision `794a711607a159d97319f73c96ec0d8a29953c9f`.
It does not promote any additional Blueprint to a buildin.

| Checkbox | App ID | Bundled application |
| --- | --- | --- |
| Appstore | `apps` | Browse, download, verify, and launch apps. |
| Commander | `commander` | Terminal-to-VLayer keyboard and mouse control. |
| Hull | `hull` | The default desktop application launched by embedded startup. |
| Trauma | `trauma` | HTTP(S) download manager with retry support. |
| SSH | `ssh` | SSH client for remote hosts. |
| Image viewer | `img` | PNG/JPEG viewer with gallery browsing, pan, and zoom. |
| OS | `os` | Installation, live updates, reboot, and shutdown controls. |
| Backup | `backup` | Whole-disk backup and restore. |
| Termdir | `termdir` | Directory and file explorer. |
| Edit | `edit` | Terminal text editor. |
| Share | `share` | Standalone WebDAV file sharing. |
| Paper | `paper` | Editable gridpaper, linepaper, and whitepaper sheets. |

The **All** and **None** buttons change only the app checklist. Selecting a
preset resets both the kernel and app choices to that preset. Universal, Intel
desktop, and QEMU / Virgl start with all apps; Kernel only starts with none.
After choosing a preset, adjust either checklist independently.

Every app also has a rounded packed-size label. The live size panel sums exact
bytes for the selected set and shows the omitted payload. The initial
TRUEOS 0.0.235 reference has **6,115 kB** of packed apps in total; see
[Build size estimates](SIZE_ESTIMATES.md) for the measured app table and
kernel-baseline derivation.

All 4,096 app subsets are accepted by the configuration resolver. This is a
configuration contract, not a claim that every combination has been compiled
and boot-tested. Apps still need their normal runtime services and appropriate
hardware to perform their functions. Shared libraries required by selected
apps can still compile even when another app using them is omitted.

## Local builds

The ordinary command keeps all current buildins:

```sh
bash tools/trueos-boot/prepare.sh --release
```

Bundle only the file explorer and text editor:

```sh
bash tools/trueos-boot/prepare.sh --release --buildins termdir,edit
```

Bundle only the SSH client in a development build:

```sh
bash tools/trueos-boot/prepare.sh --dev --buildins ssh
```

The explicit whole-set shortcuts are:

```sh
bash tools/trueos-boot/prepare.sh --release --buildins all
bash tools/trueos-boot/prepare.sh --release --buildins none
bash tools/trueos-boot/prepare.sh --release --no-buildins
```

The last two select the same zero-app payload. Unknown names, duplicate names,
and contradictory options fail before compilation. A supplied cloud manifest
is authoritative; a conflicting local selection fails instead of changing
the configuration that was reviewed and signed.

## Cloud requests and identity

Supply `buildins` as an exact array of app IDs:

```json
{
  "preset": "qemu-virgl",
  "profile": "release",
  "features": {"usb": false},
  "buildins": ["termdir", "edit"]
}
```

The resolver sorts the array to `["edit", "termdir"]`. Reordering a request
does not create a different build. Changing its membership does: the exact
list is part of the configuration hash and signed challenge, and is recorded
in the queued manifest, resulting receipt, and build provenance.

| Request form | Meaning |
| --- | --- |
| Omit `buildins` | Use the preset's app selection. |
| `"buildins": []` | Bundle no applications. |
| `"buildins": ["ssh"]` | Bundle exactly SSH. |
| `"features": {"buildins": true}` | Compatibility shorthand: bundle all catalog apps. |
| `"features": {"buildins": false}` | Compatibility shorthand: bundle no apps. |

When both forms are supplied, the boolean must agree with whether the exact
list is nonempty. An explicit list overrides the preset's payload, so a user
can start with Kernel only and check a single app. No app implies another app.

Manifest schema 2 requires the normalized `buildins` array. The derived
`features` list contains `buildins`, and `cargo_features` contains
`buildin-apps`, precisely when that array is nonempty. Existing grants continue
to cover kernel capabilities; choosing bundled apps adds no new license
capability requirement.

After upgrading this branch, regenerate schema 1 manifests and request a new
challenge. The service does not reinterpret an old signature as permission for
a newly resolved configuration. Unsupported old built jobs fail receipt
finalization without stopping other jobs.

## Compilation and embedding

The kernel-owned adapter in [`buildins.py`](buildins.py) reuses the pinned
sibling builder's per-app compilation, object packaging, and ABI validation.
The sibling builder's existing `--app` diagnostic flag alone is insufficient:
it does not produce the complete prepared AppDB expected by the kernel.

For a nonempty selection, preparation:

1. Validates app names against the selector catalog and the pinned sibling's
   authoritative manifest.
2. Stages the shared IDE dependency and generates its API reference inside the
   build output only if Edit is selected. The pinned source checkout stays clean.
3. Compiles and checks only the selected app packages and their dependencies.
4. Writes a selected `buildins.json`, removes obsolete packaged apps, and seeds
   AppDB from that exact manifest.
5. Records selection-specific source and artifact checksums. The kernel build
   rechecks those inputs before embedding the compressed database.

A changed selection cannot reuse a ready marker or database for another set.
A failed build leaves a marker that prevents embedding incomplete output.
The adapter checks its required sibling-builder interfaces and refuses an
incompatible source version. It does not modify the sibling checkout.

Edit's shared IDE crate is copied to
`tgt/buildins/<profile>/dependencies/trueos-ide`; its generated `api-prm`
reference stays there. The staged manifest keeps the crate's other path
dependencies pointed at the pinned sources. Readiness records the staged tree
and API manifest hashes. Completed worker/CI provenance includes those hashes
and the readiness-file hash, while the normal clean-source checks remain in
force. Builds without Edit skip this staging and reference generation.

The preparation script supplies `TRUEOS_BUILDINS_SELECTION` to the kernel
build. A direct Cargo invocation after preparing a subset must supply the same
exact list and keep `buildin-apps` enabled, for example
`TRUEOS_BUILDINS_SELECTION=edit,termdir cargo build --release`. Without that
environment setting, the checked payload defaults to all apps; a prepared
subset is rejected instead of silently embedded under the wrong configuration.

For an empty selection, preparation skips this entire path. There is no
buildin compilation, source compatibility check, API reference generation,
or compressed app payload. The kernel starts with its existing empty, writable
app database. Worker and CI also skip the TRUEOS-Buildins checkout; their
TRUEOS-Blueprints and archived compiler dependencies still apply.

## First boot and later apps

Omitting Appstore leaves no bundled app-store bootstrap. Omitting Termdir or
Edit leaves no corresponding bundled explorer or editor. The selected kernel
filesystem, app database, Blueprint runtime, and hardware APIs remain available.

The unchanged embedded startup configuration skips a launch whose local app
is absent. In particular, keeping Appstore while omitting Hull does not cause
boot to download Hull again. A custom startup manifest or an explicit later
launch can still request online applications through the normal runtime path.

Selecting an app bundles it; it does not add it to autostart. Users choosing a
small tool set should also choose how to launch those tools. Future guided
configuration can describe those entry points and measured sizes without
making a desktop, store, explorer, or editor mandatory.

## Validation and measurements

Host tests cover all 4,096 app subsets, subsystem composition, malformed or
tampered selections, signature binding, partial-set packaging, stale payload
rejection, and conditional CI/worker dependencies. DOM/API checks exercise
the actual checklist and signing flow.

The selector has a measured release reference, not a measured build for every
subset. Full kernel compilation, boot behavior, app functionality, and exact
artifact sizes for a new configuration still require the provisioned TRUEOS
builder and target hardware. Use
the measurement commands in [Kernel only](KERNEL_ONLY.md), holding source,
compiler, dependency revisions, profile, and kernel features fixed while
changing only `--buildins`.
