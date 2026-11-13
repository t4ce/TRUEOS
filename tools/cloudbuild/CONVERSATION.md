# Conversation and implementation handoff

Prepared as a source handoff for `t4ce/TRUEOS` on **2026-11-12**.
Working branch: `feat/subsystem-cloudbuild`.

This record preserves the maintainer's requests and the resulting work from
the conversation. Quoted user messages retain their wording, with whitespace
and clipboard formatting normalized. Assistant responses are condensed
implementation reports checked against the branch. The operational details
live in the linked documentation and source files.

The implementation was integrated with TRUEOS **v0.0.235**, commit
`75e9b1dc08189d3bba546213b4d5f3b4f3ee9757`. The newer target branch and its
integration issues are recorded under [Upstream integration](#upstream-integration).

## 1. Subsystem selector, personal builds, and key possession

### User request

> make like a subsystem selector, and a cloudbuild-tool
>
> people should be able to select the presets with checkboxes, toggling entire features like USB or iGPU or Virgl
>
> then, these will cause a build for the "person" where the person could aswell put in his public crypto key, then we had this license thing cool
>
> we will just make it the other way arround, if there is actually someone that has that key, he can solve our challenge for the license ?
>
> that is like, by far the dankest way to kind of give the NIX ppl a hint that i am on the better tracks?

Repository supplied: <https://github.com/t4ce/TRUEOS>.

### Implemented response

- A browser selector backed by one allowlisted feature catalog and a strict
  normalized manifest. Presets are Universal, Intel desktop, QEMU/Virgl, and
  Kernel only.
- A Python standard-library control plane, private SQLite queue, and a worker
  that runs the existing preparation pipeline against a clean, provisioned
  source checkout.
- SSH Ed25519 proof of possession: the requester submits a public key, signs
  the exact issued challenge locally, and submits the detached signature.
  The private key remains on the requester's machine.
- Operator-controlled entitlements, single-use expiring challenges, private
  job access tokens, and a separate issuer key for artifact-bound receipts.
- A manually dispatched GitHub Actions path with immutable dependency inputs
  and recorded build provenance. It is separate from the web service queue.

Proof of possession demonstrates control of a key. It does not establish
payment, a unique human, or an automatic entitlement. The branch implements
build authorization and receipts; it does not add boot-time activation or
kernel license enforcement, and does not change the repository's license.

The USB/iGPU/Virgl switches currently control device admission and
initialization. Shared interfaces and driver implementations remain compiled.
Complete driver-code removal was not implemented or claimed.

See [Cloud Build](README.md), [catalog](catalog.json),
[identity protocol](identity.py), [service](service.py), and [worker](worker.py).

## 2. Zero buildins as a regular configuration

### User request

> lets add to the local branch the idea of "no buildins" what is entirely possible
> but will get the person to a situation, of actually not even the appstore
>
> it does not remove any capabilities but going without a fileexplorer, appstore, texteditor thats only 500 kilobytes anyway... is entirely valid for someone that wants a TrueOS § Kernel only
>
> note that this stacks obviously nice with feature deselection because if there is 0 buildins, the subsystem removal doesnt have to check if a buildin is impossible to compile
>
> but i will figgure out more configurations relevant things and do documentation and a guided process anyway, that needs be in the branch as md or experimental option, or just regular option actually, not experimental:
>
> it really is that simple -> omit the buildins step so ppl can whitness how slim that kernel actually is

### Implemented response

Kernel only is an ordinary option. The default-on `buildin-apps` Cargo feature
controls whether the selected app payload is prepared and embedded. With zero
apps, preparation skips the app builder, app checks, and editor reference
generation, and the kernel initializes an empty writable app database.
Kernel capability selections remain independent.

```sh
bash tools/trueos-boot/prepare.sh --release --no-buildins
```

There is no mandatory store, explorer, editor, or desktop application. The
unchanged embedded startup configuration skips missing local apps rather than
fetching an omitted Hull through Appstore. Explicit custom startup and online
launch requests retain their normal behavior.

See [Kernel only](KERNEL_ONLY.md). This is an app-payload choice; it does not
mean UI4 and GPU implementation code have been removed from the kernel.

## 3. Each existing buildin is independently selectable

### User request

> actually its perfectly legit to make every buildin a choice in itself, like a 12 tool list so to say?

### Implemented response

All twelve existing buildins have independent checkboxes, with All and None
shortcuts. No new Blueprint was promoted to a buildin. The app IDs are:

`apps`, `commander`, `hull`, `trauma`, `ssh`, `img`, `os`, `backup`, `termdir`,
`edit`, `share`, and `paper`.

Schema 2 manifests carry the exact sorted `buildins` array. Membership is
bound into the configuration hash, challenge, receipt, and provenance; list
order does not change identity. The original `features.buildins` boolean
remains an all/none shorthand.

```sh
bash tools/trueos-boot/prepare.sh --release --buildins edit,termdir
bash tools/trueos-boot/prepare.sh --release --buildins all
bash tools/trueos-boot/prepare.sh --release --buildins none
```

The selected-buildins adapter compiles, validates, packages, and seeds exactly
the requested set. It detects stale or mismatched readiness records and
removes obsolete packages. Edit's generated API reference is prepared in a
private output dependency tree so the pinned source checkout remains clean.

The v0.0.235 integration preserves upstream native-only validation without
silently filtering the selected apps. It also fingerprints external kernel
vendor dependencies, including the shared SSH implementation.

See [Individual buildins](BUILDINS.md) and [the adapter](buildins.py).

## 4. Release measurements and visible savings

### User-supplied comparison table

These are the **older pasted values**, preserved for context. The selector
uses the measurements from the subsequently attached release instead.

| Buildin | Unix/POSIX imports | All imports | ELF bytes | Packed BP bytes |
| --- | ---: | ---: | ---: | ---: |
| SSH | 27 | 63 | 2,920,648 | 1,384,524 |
| Hull | 0 | 101 | 6,882,488 | 3,022,846 |
| Share | 0 | 46 | 1,364,960 | 734,913 |
| Edit | 0 | 39 | 1,053,448 | 475,287 |
| Trauma | 0 | 50 | 682,008 | 378,514 |
| Termdir | 0 | 53 | 609,032 | 336,525 |
| Apps | 0 | 42 | 234,024 | 125,516 |
| Img | 0 | 49 | 217,968 | 128,800 |
| OS | 0 | 35 | 174,752 | 92,135 |
| Backup | 0 | 30 | 173,784 | 94,810 |
| Commander | 0 | 31 | 111,240 | 56,633 |
| Paper | 0 | 41 | 40,624 | 24,426 |

### User request

> ive pushed with some decisive patching some sticky c pointer registry and such, unify crypto stack for ssh in and out a bit
>
> and its better values, ssh is aswell good
>
> the completele current release is attached so you can see it
>
> for the selector buildfeature i had in mind that we can do a total sum next to the kernel, for what of the buildins was selected, as estimate, by just round it to kilobytes and then its aswell nice for ppl to see what they save if they turn of buildins, for sure
>
> and perhaps what they save, if they compile without the subsystems you have in already

### Measured and implemented response

The supplied `TrueOS-0.0.235(1).lz4` was inspected without executing its
binaries. Exact measurements and hashes are retained in
[measurements/0.0.235.json](measurements/0.0.235.json); the release binary itself
is not added to this branch.

| Measurement | Attached release |
| --- | ---: |
| SSH packed BP | 647,986 bytes |
| SSH ELF | 1,150,840 bytes |
| SSH total undefined imports | 48 |
| SSH Unix/POSIX classifier matches | 0 |
| All twelve packed apps | 6,115,201 bytes |
| Complete kernel ELF | 26,213,712 bytes |
| Embedded compressed AppDB | 5,825,237 bytes |
| Derived kernel/assets remainder | 20,388,475 bytes |
| Compressed release archive | 19,447,079 bytes |

SSH's packed size is 53.2% below the older pasted value. All twelve app modules
have zero Unix/POSIX matches under the v0.0.235 source classifier. The archive
does not contain a provenance sidecar, so its exact source/dependency revisions
are not asserted as verified.

The selector now shows per-app size badges, a selected-app sum, omitted app
payload, a derived kernel baseline, and an additive kernel-plus-apps estimate.
It uses decimal kB and sums exact bytes before rounding. Edit + Termdir selects
812 kB and omits 5,303 kB of reference app payload. Queued/resumed jobs use their
recorded selections; development builds label these figures as a release
comparison.

The derived baseline is not a separately rebuilt kernel-only image. AppDB and
archive compression prevent treating package weights as exact ELF or download
deltas. Subsystem savings remain unmeasured; no numeric savings were invented.

See [Build size estimates](SIZE_ESTIMATES.md).

## 5. A real Server configuration without UI4 or an iGPU

### User question and clarification

> do we have a server version, that just has the shell sh3 and servers
> but require no kinds of shaders or igpu perse, beacuse on some intel server chips its just maybe entirely headless with the blade not even have a monitor out

The follow-up clarified the intended compilation boundary:

> the big subsystems that fall out are like ui4, intel igpu driver for the 4 engines etc
> so it is actually one of the single biggest decisions for os kind of other major distros dualr elease server and kind of regular os aswell

### Audited response and recorded design

A stripped Server preset is **not implemented in this branch**. At the audited
revision, the UEFI loader accepts an absent framebuffer and the incoming sh3
SSH path uses text terminal cells without a compositor readiness gate. The
shell's graphical and network implementations still share UI4 types and
modules, and the kernel still declares graphics implementation modules
unconditionally.

The documented target is a regular Server preset using the same kernel and
build pipeline: keep sh3/session ownership, networking, storage, crypto, CPU
and runtime services, while excluding UI4, GPU implementations, shader assets,
and unnecessary firmware. Buildins remain independently selectable.

[SERVER.md](SERVER.md) records the shell/frontend separation, generated ABI
resolver changes, service selection, firmware/update packaging, hardware
validation, and administrator provisioning still required. Its source audit
is explicitly tied to the branch revision; it is not a claim about later
upstream behavior or a completed blade boot test.

## 6. Pull request and local follow-up

### User request

> can you file a PR against TrueOS repo with the markdown of conversation and the craft we did here
>
> then i can resolve the details locally after i eventually merge it

### Handoff scope

The handoff carries the implementation, host tests, measured reference,
documentation, and this conversation record together. The Server design and
unmeasured subsystem savings remain explicitly separate from working selector
and packaging behavior. The maintainer will reconcile the newer upstream
interfaces and perform full build/boot checks locally.

### Follow-up: ZIP instead of publishing a PR

> a zip is fine if you can bundle all of it, with diff and reference, we can resolve it equally easy dont hussle the api

The chosen delivery is a ZIP containing the branch's changed files, a patch
against its integrated upstream base, a Git history bundle, exact reference
commits and source snapshots, the conversation/documents, and measurement
and validation evidence. Publishing a GitHub PR is no longer required for
this handoff.

### Implementation history

| Commit | Work |
| --- | --- |
| `ab90b61` | Subsystem selector and key-bound cloud build service. |
| `c8055da` | Regular kernel-only builds without bundled apps. |
| `3a520be` | Each existing buildin independently selectable. |
| `c0ce544` | Merge v0.0.235 and preserve exact selection/native-only preparation. |
| `9c4f452` | Measured app sizes, live estimates, and evidence. |
| `1a19449` | GPU-free Server configuration design and remaining boundaries. |

### Validation recorded during implementation

The following results were obtained on 2026-11-07; they apply to this branch's
implementation, not to an integration with subsequent upstream changes:

- Cloudbuild host suite: **121 passed, 3 skipped** because host Rust was absent
  (124 tests discovered).
- Rig/preparation checks: **20 passed**; the separate Cargo-discovery check
  could not run because Cargo was unavailable. The PXE check passed with an
  existing temporary directory supplied to Node.
- Size selector: **10 grouped jsdom/real HTTP API checks passed**, including
  rounding, all/none/subsets, profile/reference labels, frozen queued
  selections, absent measurements, unchanged request identity, and no JS
  exceptions. This was not a visual-browser assertion.
- Resolver coverage includes all 4,096 app subsets. Configuration acceptance
  does not mean every combination has been compiled and booted.
- Python/JavaScript/Bash syntax and whitespace checks passed.

Full OS compilation and hardware boot verification require complete clean
checkouts, submodules, pinned sibling repositories and the archived TRUEOS
compiler. They were not completed in this environment. No production service
or public build worker was deployed.

### Upstream integration

The default target branch is `true`. On 2026-11-12 it was fetched at
`94af018df093b18eceb2b00b3072ed6c358021c6`, with 55 target-branch commits beyond
the branch's existing integration point. A read-only merge simulation found
content conflicts in:

- `src/usb3/mod.rs`
- `tools/trueos-boot/prepare.sh`
- `tools/testpy/test_rig_commands.py`

There is also a semantic AppDB integration issue: newer upstream adds
`trueos-appbundle` and requires a `metadata/appbundle.json` contract, while
this branch's zero-app initialization creates the older `apps`, `hashes`,
and `updated` tables. In the merge preview, `src/app_db.rs` merges textually
but then attempts the new unconditional metadata read: the empty database
fails initialization before publication. The zero-app path needs a valid
bundle metadata record.

The newer `tools/appdb-seed` requires five positional arguments:
`BUILDINS_JSON PACKAGE_DIR OUTPUT_DB UPDATED_UTC APPBUNDLE_JSON`. This branch's
adapter supplies four. Adapt the call and bind the bundle-definition input
into its fingerprints/readiness records. The new seeder also requires bundle
companion members to be present, so the old resolver's unrestricted subset
contract needs reconciliation with those dependencies. Do not silently add
apps that were absent from the user's signed selection.

Resolve both the textual conflicts and the AppDB contract before treating
the combined tree as validated. Recheck dependency pins and the selected
builder's strict interfaces against the intended sibling revisions. Keep the
release size dataset labeled as a historical 0.0.235 reference until newer
artifacts are measured.

After the final merge, resolve fresh schema 2 manifests and challenges against
the resulting commit. The source commit is part of build identity, so earlier
manifests and signatures must not be silently reused for a different tree.

## Review entry points

| Concern | Files |
| --- | --- |
| User flow and operator setup | [README.md](README.md), [web/index.html](web/index.html), [web/app.js](web/app.js) |
| Catalog and normalized build identity | [catalog.json](catalog.json), [build_plan.py](build_plan.py) |
| Key proof, grants, queue, receipts | [identity.py](identity.py), [service.py](service.py) |
| Worker and CI | [worker.py](worker.py), [ci.py](ci.py), [cloudbuild.yml](../../.github/workflows/cloudbuild.yml) |
| Exact-set packaging | [buildins.py](buildins.py), [prepare.sh](../trueos-boot/prepare.sh), [build.rs](../../build.rs) |
| Empty payload and startup | [app_db.rs](../../src/app_db.rs), [restart.rs](../../src/r/restart.rs) |
| Size reference | [SIZE_ESTIMATES.md](SIZE_ESTIMATES.md), [0.0.235.json](measurements/0.0.235.json) |
| Server architecture | [SERVER.md](SERVER.md) |
