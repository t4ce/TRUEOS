# TRUEOS Cloud Build

A subsystem and bundled-app selector, strict build manifest, private job queue, and SSH-key
challenge flow for operator-issued personal TRUEOS builds. It uses Python 3.11+
and OpenSSH, with no Python packages or frontend build step.

See [Conversation and implementation handoff](CONVERSATION.md) for the
maintainer's requests, the decisions implemented here, validation results,
and the known integration work against newer upstream revisions.

The selector offers Universal, Intel desktop, QEMU / Virgl, and Kernel only presets. Each
resolves into the same allowlisted feature catalog used by the server and
builder. Each of the 12 existing buildins also has its own checkbox, with All
and None shortcuts. A selection becomes a real queued build; it never produces simulated
progress or a fabricated download.

## What a feature switch does

| Selection | Current effect |
| --- | --- |
| Individual buildins | Compile and embed exactly the selected applications. All keeps the normal payload; None gives Kernel only. |
| USB | Controls xHCI controller admission and USB device workers. |
| Intel iGPU | Controls Intel GPU claim, initialization, and dependent readiness. |
| Virgl | Controls virtio GPU discovery before DMA or bus mastering. |
| H.264 streaming | Selects the existing streaming Cargo feature; requires iGPU. |
| Native GPGPU | Selects the existing Odin native compute feature; requires iGPU. |
| Task profiling | Selects the existing task profiling feature. |
| Experimental speech | Selects the existing speech feature; runtime models are separate. |

USB/iGPU/Virgl are **device initialization gates**. Shared interfaces and driver
code remain compiled. This does not establish full binary pruning, measured
size reductions, or a hardened isolation boundary. Disabling both graphics
backends leaves the existing boot-framebuffer logo fallback, not a validated
headless desktop. The existing automatic Intel HDA audio probe depends on an
Intel GPU being claimed; the selector warns about that coupling.

Normal maintainer builds retain their existing defaults. A cloud build passes
an explicit `--no-default-features` and a validated feature list to Cargo.
No Blueprint is promoted to a builtin, and the existing release workflow and
license text remain unchanged.

## Kernel only: no buildins

**Kernel only is a regular build option.** It skips buildin compilation and
embedding, and starts with an empty writable app catalog. Selecting it keeps
the default kernel capabilities and subsystem switches; you can then deselect
USB, iGPU, Virgl, or other features independently. Choosing **None** in the
bundled-app checklist on any other preset has the same app-payload effect.

There is no bundled app store to launch, and no bundled file explorer or text
editor. The kernel's filesystem, Blueprint/runtime APIs, and selected hardware
capabilities remain available. With zero buildins, preparation does not check
whether a bundled application can compile against the selected subsystems.
Kernel-to-kernel feature dependencies still apply.

```sh
bash tools/trueos-boot/prepare.sh --release --no-buildins
```

The cloud request is `{"preset":"kernel-only"}`, or add an exact
`"buildins": []` list to any request. The original
`"features": {"buildins": false}` shorthand remains supported. The manifest hash,
signed challenge, receipt and provenance distinguish the app-payload choice.
App bundling adds no new license capability requirement; existing grants for
the selected kernel features remain valid with or without bundled apps.

See [TRUEOS § Kernel only](KERNEL_ONLY.md) for the normal selection flow,
precise build/boot behavior, and a same-configuration size comparison.

## Server configuration

A GPU-free Server configuration is a separate build choice from omitting
apps. The loader and incoming sh3 terminal already have display-independent
paths, but UI4 and GPU implementation modules are still compiled in this
branch. There is not yet a stripped Server preset. See
[TRUEOS Server](SERVER.md) for the intended regular configuration, the
existing source support, and the remaining shell, graphics, API, packaging,
and hardware validation work.

## Choose individual buildins

The app checklist is independent of kernel capabilities. Keep just Edit and
Termdir, for example:

```sh
bash tools/trueos-boot/prepare.sh --release --buildins edit,termdir
```

The equivalent cloud request with QEMU/Virgl kernel settings is:

```json
{"preset":"qemu-virgl","profile":"release","buildins":["edit","termdir"]}
```

The exact app list enters the configuration hash, signed challenge, receipt,
and provenance. A nonempty list enables the existing `buildin-apps` Cargo
feature; the packaging adapter compiles and seeds only those app packages.
An empty list skips the entire buildin step, source checkout, and validation.
No app is automatically added, including Appstore and Hull. The embedded
default startup does not fetch an omitted Hull through Appstore; a custom
startup configuration can still explicitly request online apps.

See [Individual buildins](BUILDINS.md) for the 12-app list, exact-set request
rules, local commands, and the build/boot contract. Normalized manifests now
use schema 2; resolve and sign a fresh request when upgrading from schema 1.

## Live size estimates

Each app checkbox shows its measured packed size from the TRUEOS 0.0.235
release. The size panel sums the selected apps, shows the omitted app payload,
and places that sum beside a derived kernel baseline. It uses decimal kB and
rounds only after summing exact bytes. Edit + Termdir, for example, select
**812 kB** of packed apps and omit **5,303 kB** of the full app set.

The baseline is the measured full ELF minus its actual embedded compressed
AppDB; it is not a separately built kernel-only image. The kernel-plus-apps
total is an estimate, and the actual reference download size is shown
separately. Subsystem savings remain **unmeasured**. See
[Build size estimates](SIZE_ESTIMATES.md) for the measurements, formulas,
provenance limits, and the process for adding measured subsystem comparisons.

## Try the selector

From the TRUEOS repository root:

```sh
python3 tools/cloudbuild/cloudbuild.py serve
```

Open <http://127.0.0.1:8787>. Presets, checkboxes, dependency validation, and
manifest downloads work immediately. Build submission requires the operator to
configure licensed builds or explicitly enable anonymous builds. A worker must
also be running to process queued jobs.

The source revision defaults to this checkout's full Git commit. Commit the
implementation before building: the worker rejects dirty or sparse checkouts.

## Configure key-bound builds

There are two independent keys:

1. The **person's key** authenticates a build request. The person supplies only
   its public half; their private key stays on their machine.
2. The **operator's issuer key** signs the resulting license receipt. It must
   be unavailable to build scripts and worker containers.

Create or provision an ordinary Ed25519 issuer key outside the build worker.
For example, on the control-plane host, after creating a private key directory:

```sh
ssh-keygen -t ed25519 -N '' -f /srv/trueos-keys/build-issuer
```

This reference service requires an unencrypted service key because it signs
without an interactive agent. Protect it with filesystem/container isolation.
Provision its public key independently to receipt verifiers; a key included
inside a receipt is not its own trust anchor.

Grant a person's public key an entitlement using the local operator CLI:

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  grant --public-key person.pub --license-id personal-evaluation-001
```

Optional `--features usb,virgl,task_profile` limits the allowed selected features.
An optional `--expires-at` takes a future Unix timestamp. A grant can be revoked:

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  revoke --fingerprint 'SHA256:THE_PUBLIC_KEY_FINGERPRINT'
```

Start the service with its issuer key:

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  serve --issuer-key /srv/trueos-keys/build-issuer
```

The user selects features, pastes an `ssh-ed25519` public key, and downloads the
challenge. They sign its exact bytes locally:

```sh
ssh-keygen -Y sign -f ~/.ssh/id_ed25519 \
  -n build-request@trueos.eu trueos-challenge.json
```

They upload or paste `trueos-challenge.json.sig` and choose **Verify & queue**.
The service verifies their signature, checks their grant, consumes the challenge
once, and queues the exact stored manifest in one SQLite transaction.

Proof of possession establishes control of a key, not payment, a unique person,
or a right to every feature. Grants are intentionally operator-controlled. New
keys receive no automatic license. Anonymous builds can be enabled explicitly
with `serve --allow-anonymous`; those jobs do not receive a license receipt.

## Provision and run the cloud worker

Use a dedicated Linux builder with a complete, clean TRUEOS checkout, its
committed submodules, and a pinned sibling `TRUEOS-Blueprints` checkout.
`TRUEOS-Buildins` is needed only when bundled apps are selected; kernel-only
jobs do not inspect or require it. Blueprints still supplies kernel path
dependencies even when no apps are bundled. Install the same host dependencies and archived
Rust toolchain as [the existing release workflow](../../.github/workflows/release.yml).
The pinned Rust compiler commit is
`af3d95584dbddcae597890340995509a7fb47a50`.

The control plane and worker use the same private state directory; on a
containerized host this can be an operator-owned shared volume. They must use
the same service/catalog code version. The worker uses its independently
provisioned source checkout, and rejects jobs for a different commit. Set
`serve --source-commit FULL_SHA` when its build checkout is elsewhere.

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  worker --repository /srv/build/TRUEOS
```

Use `--once` for a single queue attempt and `--timeout` to change the default
two-hour build deadline. The worker serializes builds within each checkout,
records failures, and copies a successful archive into the private job
directory before making it available. It invokes the existing preparation
script, never the physical rig reset. Logs are operator-only under
`STATE/jobs/JOB_ID/`; the API returns bounded failure messages.

The worker checks clean source revisions, initialized submodules, compiler
identity, and required host commands before building. It records sibling
commits, submodule revisions, `rustc -vV`, and kernel/loader lockfile digests in
provenance. Missing worker dependencies produce a failed job, not a pretend
successful build. A worker killed abruptly can leave a `building` job behind;
this first version does not implement automatic lease recovery or retries.

### Hosting boundary

This is a reference control plane for a **trusted, fixed-source builder**, not
an adversarial multi-tenant execution sandbox. The worker is trusted with queue
state. Build subprocesses use a minimal environment, but environment filtering
does not isolate their filesystem or network. Run them in an operator-managed
container/VM without the issuer key, cloud credentials, or unrelated user data.
Do not give visitors source uploads, commands, environment settings, or mutable
repository refs. Resource limits, dependency provisioning, storage retention,
backups, and worker restart supervision remain deployment responsibilities.

The HTTP adapter defaults to loopback. For a public deployment, use a TLS reverse
proxy with connection/body limits, set `--origin https://build.example.org`, and
bind the service to a private interface (or `--host 0.0.0.0` inside a private
container network). The proxy must preserve that exact Host and Origin. There
is no CORS wildcard. Built-in body/concurrency/rate/queue limits are a starting
point; enforce operational quotas at ingress too. The Python HTTP adapter is
not a replacement for a production ingress server.

Separate processes under the same host user are not a security boundary. The
issuer key must be mounted only into the control-plane container. The SQLite
database, challenge records, bearer tokens and artifacts must remain private.
The UI holds the returned build token in memory and offers a **Save build
access** file for explicit recovery. Keep that file private: it grants access
to the build. **Resume build** restores a saved ticket on the same origin.
Changing settings keeps the previous request accessible. No token is embedded
in a URL or stored in browser local storage.

## Signed receipt and download

After a successful licensed build, the control plane rechecks the entitlement
and artifact hash, then signs a receipt with namespace
`build-license@trueos.eu`. The receipt binds the subject fingerprint, license ID,
kernel features, exact bundled-app list, source commit, configuration hash, artifact SHA-256/size, expiry, and
build provenance. A background finalizer handles completed jobs even when the
requesting browser has closed. Download the archive and receipt from the job panel.

Verify them with an independently trusted issuer public key:

```sh
python3 tools/cloudbuild/cloudbuild.py verify-receipt \
  --receipt trueos-license.json \
  --issuer-public-key trusted-build-issuer.pub \
  --subject-public-key ~/.ssh/id_ed25519.pub \
  --artifact TrueOS.tar.lz4
```

This implements authenticated build authorization and a signed, key-bound
receipt. It does **not** add boot-time activation, kernel enforcement, hardware
binding, or an uncopyable image. Existing TRUEOS machine-login/credential flows
are not changed. Revoking a grant blocks new jobs and unissued receipts;
previously issued offline receipts retain their signed validity interval.

Keep personal keys and license IDs out of the OS build inputs. Two people can
request the same configuration while receiving separate receipts. The
configuration hash covers source, profile, kernel features, and selected apps;
it excludes personal identity. It is not a
complete dependency cache key, and this version has no shared artifact cache.
Lockfile recording and pinned siblings improve auditability; they do not prove
bit-for-bit reproducibility. The existing pipeline may generate fresh lockfiles.

## Export a build plan or use GitHub Actions

A request file contains only known preset, feature, app, and profile choices:

```json
{
  "preset": "qemu-virgl",
  "profile": "release",
  "features": {"usb": true},
  "buildins": ["apps", "hull", "termdir", "edit"]
}
```

```sh
python3 tools/cloudbuild/cloudbuild.py plan \
  --request request.json --output manifest.json
TRUEOS_CLOUDBUILD_SPEC="$PWD/manifest.json" \
  bash tools/trueos-boot/prepare.sh --release
```

The manual [Cloud build workflow](../../.github/workflows/cloudbuild.yml)
accepts that normalized manifest and exact sibling/toolchain commit pins.
`buildins_commit` is required only for builds with bundled apps. Kernel-only
jobs skip that checkout and record only their actual source dependencies. It
validates the manifest against the workflow's checked-out commit before
compilation and uploads workflow artifacts and provenance under the repository's
access policy. These Actions artifacts are not per-person private downloads. It does
not publish a GitHub release or issue licenses. The web service processes its
own queue through `worker`; it does not automatically dispatch this alternate
Actions workflow.

## Protocol and verification

`GET /api/catalog` returns the catalog and server capabilities.
`POST /api/plan` resolves selections. `POST /api/challenges` accepts a request
and public key. `POST /api/builds` accepts a challenge ID and SSH signature, or
an anonymous request when explicitly enabled. Job polling and artifact downloads
require `Authorization: Bearer TOKEN` on `/api/jobs/ID` and
`/api/jobs/ID/artifact`.

The nonce is cryptographically random, expires after five minutes, and is bound
to the origin, source, configuration, operation, and fingerprint. Wrong keys,
altered messages, other signing namespaces, expired challenges, concurrent
replays, revoked grants, and cross-job tokens are rejected. Public-key comments
are discarded before creating an allowed-signers entry. OpenSSH receives an
argument array; user strings are never executed as shell syntax.

Run the host-side suite without compiling the OS:

```sh
python3 -m unittest discover -s tools/cloudbuild -p 'test_*.py' -v
```

Worker tests use a tiny temporary Git fixture and clearly marked bytes, not a
TRUEOS image. Full feature-matrix builds and physical/QEMU boot validation need
the provisioned toolchain and hardware and remain separate acceptance gates.

Primary references: [OpenSSH signing and namespaces](https://man.openbsd.org/ssh-keygen.1),
[NixOS reproducibility boundaries](https://reproducible.nixos.org/).
