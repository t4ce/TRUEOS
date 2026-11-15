# Public selector with GitHub Actions builds

The existing selector submits to the private API. A separate Actions adapter
claims approved jobs, dispatches `.github/workflows/cloudbuild.yml`, polls the
actual run, verifies its artifact and provenance, and copies the system archive
into the API's private state. The existing receipt finalizer signs licensed
results. Closing the browser does not stop compilation or collection.

PDJB on the LAN is the intended API host. Server access, DNS, HTTPS and firewall
configuration are a later joint deployment step. The public selector belongs at `https://trueos.eu/build/`, next to the existing
home page and `/apps`. The Node site proxies `/build/` to Python on loopback,
removing that prefix while preserving the public Host and Origin. No server deployment or public workflow run was performed to implement
this integration.

## Public and private data

On a public build repository, configuration, source revisions, Actions logs,
provenance and uploaded system archives are public under GitHub's access rules.
The selector discloses this before submission. API bearer tickets protect the
service's job records and downloads, not the repository's copy of an archive.

Public keys, signatures, fingerprints, entitlements, personal receipts and
bearer tickets are never passed into the workflow. GitHub receives the
normalized configuration, immutable dependency commits, and an opaque request
ID. The GitHub App private key and receipt issuer key remain on PDJB, outside
build runners. This is authorization to request a build and a signed receipt;
it does not implement kernel activation or prevent image copying.

The first deployment should keep anonymous builds disabled and issue explicit
key grants. Enabling `--allow-anonymous` permits unsigned requests subject to the
existing queue and ingress limits. No visitor selects repositories, source refs,
commands, environment variables or toolchains.

## One key identity, two execution models

The intended interface supports hosted builds and builds in the user's GitHub
account. The existing operator-hosted route is implemented. Multi-account
GitHub authorization, installation ownership checks and per-user dispatch are
not enabled yet. Do not point the operator configuration at visitor-supplied
repositories to imitate those missing checks.

The SSH key proves possession and authorizes an exact request. It is independent
of GitHub login, repository access, build provenance and permission to log into
a running TRUEOS machine. A public key listed on GitHub is not proof that the
current visitor controls its private half. Private SSH keys stay on the user's
machine, and signing does not grant a runner access to the user's SSH agent.

Actions challenges use schema `trueos-build-request/v2` and bind the execution
destination as well as the existing origin, nonce, expiry, configuration and
source. The destination includes the operator repository, workflow, ref and
immutable dependency pins. Changing these requires a new challenge and SSH
signature. Jobs retain that destination, and Actions rejects an unbound legacy
job or one intended for another worker. The receipt records the destination.
The local compiler worker rejects requests bound to Actions.

GitHub App installation tokens are requested for one repository only, with
Actions write and Contents read permissions. The adapter checks the returned
repository and permission scope before using a token, even when an installation
has broader repository access. No App private key or installation token enters
the build workflow. The receipt issuer key is a separate secret.

The required multi-account flow is:

1. Authenticate the GitHub user through the App's authorization flow with
   single-use state and a secure server-side session. Validate callback data.
2. Confirm installation membership and access to the selected repository using
   GitHub's APIs. Bind immutable user, installation and repository IDs; never
   trust a callback installation ID or repository name by itself.
3. Have the user prove the SSH key with a separate expiring, single-use
   `github-link@trueos.eu` challenge binding those verified IDs. Linking, key
   replacement, account changes and unlinking require explicit authentication.
   Uninstalling the App revokes execution access but does not reassign key ownership.
4. Resolve either an operator-hosted or verified user-owned execution target.
   Display the destination before signing. The signed build challenge binds the
   target IDs and source/workflow pins, preserving one key identity across both.
5. Verify the actual artifact's trusted build provenance before issuing a
   TRUEOS receipt. A key signature proves request authorization; a hash checks
   integrity. Neither proves that an arbitrary user-controlled workflow built
   the claimed image. User-account results need a pinned trusted builder and
   verified attestations/identity policy before receiving an official receipt.

Keep entitlement and execution provenance distinct in receipts. A self-built
image must not acquire an official build claim merely by uploading matching
JSON. Repositories, logs and artifacts follow the selected GitHub repository's
visibility; personal linkage records, challenges and receipts stay private.

Account/key unlinking blocks future requests. Existing signed offline receipts
retain their recorded validity unless a verifier applies a separate revocation
policy. Key loss must have a documented recovery policy; GitHub login alone must
not silently replace an entitled SSH key. Neither execution model adds boot-time
activation, hardware binding, server login access or copy protection.

References: [OpenSSH signatures and namespaces](https://man.openbsd.org/ssh-keygen.1),
[GitHub setup URL validation](https://docs.github.com/en/apps/creating-github-apps/registering-a-github-app/about-the-setup-url),
[installation token scope](https://docs.github.com/en/apps/creating-github-apps/authenticating-with-a-github-app/generating-an-installation-access-token-for-a-github-app),
[artifact attestations](https://docs.github.com/en/actions/concepts/security/artifact-attestations).

## Prepare the approved source and GitHub App

1. Commit and review the integrated sources. Publish the custom workflow on
   the repository's default branch so manual dispatch is available. Create an
   immutable approved tag containing the same workflow and implementation.
   Custom builds do not use the official release counter or publish releases.
2. Choose compatible committed sibling revisions for `TRUEOS-Buildins` and
   `TRUEOS-Blueprints`, and the archived toolchain repository revision. These
   are repository commit hashes; the compiler's internal Rust commit is
   checked separately. Make sure GitHub can fetch every selected source.
3. Register a GitHub App with repository permissions **Actions: read/write**
   and **Contents: read-only**. Install it for the build repository only. No
   webhook receiver is needed: this adapter polls the Actions API. Record the
   App client ID and installation ID, and provision its RSA PEM private key.
4. Copy `deploy/actions.example.json` to an operator-owned `actions.json` and
   replace every placeholder with real full lowercase commit SHAs. `ref` is the
   approved branch or tag. The adapter resolves it and checks its exact SHA
   before dispatch; the workflow validates the manifest against its checkout.
   A moved ref causes rejection. Visitors cannot override these pins.

GitHub App tokens are minted with OpenSSL, expire automatically, and are
refreshed by the adapter. No Python packages are required. The API and adapter
need Python 3.11+, OpenSSH, OpenSSL, CA certificates and outbound HTTPS. PDJB
does not need the TRUEOS Rust compiler or the sibling source trees for this
backend. Both processes need the same trusted Cloudbuild code and private state.

## Run locally or under supervision

The Python service serves the selector and API internally. The existing Node
site mounts it at `/build/`: browser API requests go to `/build/api/...`, and
Node strips `/build` before forwarding to Python. Other site routes retain
their current handlers. Fedora needs Node (for the existing site), Python,
OpenSSH, OpenSSL and CA certificates; it does not need Rust, Cargo, a C compiler
or a TRUEOS source build toolchain.

For selector-only browsing, `export_site.py --site /path/to/TRUEOS_SITE` packages
just the public runtime into the website. With no `TRUEOS_CLOUDBUILD_URL`, Node
starts this Python service automatically on the first `/build/` visit. Builds
remain disabled. State defaults to the service account's private
`~/.local/state/trueos-cloudbuild`, outside public files. Python 3.11+ must exist.
`TRUEOS_CLOUDBUILD_ORIGIN` defaults to `https://trueos.eu`; set it explicitly for
local testing. `TRUEOS_CLOUDBUILD_STATE_DIR` selects private writable state,
`TRUEOS_CLOUDBUILD_PORT` changes loopback port 8787, and `TRUEOS_PYTHON` selects the
interpreter. Restart Node after packaging. Production sets
`TRUEOS_CLOUDBUILD_URL=http://127.0.0.1:8787` to use the separately supervised API
and Actions adapter instead of automatic selector-only startup.

The shared `TRUEOS_SITE` now loads `tools/cloudbuild-proxy.js` from `App.js`.
Its upstream defaults to `http://127.0.0.1:8787`; set `TRUEOS_CLOUDBUILD_URL` in
the Node service environment if that internal address differs. The maintained
copy of this adapter is `deploy/site-proxy.cjs`. It streams downloads and
forwards only the headers needed by Cloudbuild, without website cookies.

Start Python with the public origin, which has no `/build` path:

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  serve --actions-config /etc/trueos-cloudbuild/actions.json \
  --origin https://trueos.eu \
  --issuer-key /etc/trueos-cloudbuild/build-issuer
```

The source revision and public artifact disclosure come from `--actions-config`.
Create the operator issuer key and grant entitled public keys using the existing
[Cloudbuild CLI](README.md#configure-key-bound-builds). The issuer is an Ed25519
key; the GitHub App key is a different RSA key downloaded from GitHub.

Run the adapter as a separate supervised process:

```sh
python3 tools/cloudbuild/cloudbuild.py --state-dir /srv/trueos-build-state \
  actions-worker --config /etc/trueos-cloudbuild/actions.json \
  --app-client-id APP_CLIENT_ID --installation-id INSTALLATION_ID \
  --app-key /etc/trueos-cloudbuild/github-app.pem
```

`--once` performs one dispatch/poll cycle; it does not wait for a build to finish.
Do not run the local compilation `worker` against this same queue. The Actions
adapter serializes remote builds, and a file lock prevents two adapter processes
from dispatching concurrently. API submissions retain the existing global and
per-key queue limits.

The `deploy/` directory contains systemd service templates for a future Linux
installation under `/srv/trueos-cloudbuild`, state under
`/var/lib/trueos-cloudbuild`, and operator files under `/etc/trueos-cloudbuild`.
Create a dedicated unprivileged `trueos-cloudbuild` account. Keep configuration
and keys outside the repository, owned by the operator and readable only by
that account. The Actions unit's `github-app.env` contains:

```text
GITHUB_APP_CLIENT_ID=YOUR_APP_CLIENT_ID
GITHUB_APP_INSTALLATION_ID=YOUR_INSTALLATION_ID
```

The Python templates expose only loopback port 8787. The Caddy example
terminates TLS for `trueos.eu` and proxies the existing Node site on port 14000.
Node handles the `/build/` mount and forwards the public Host/Origin. Adapt it to the
existing reverse proxy instead of starting a second proxy on occupied ports.
The systemd units hide the other process's private key. They need matching
paths before activation. Add ingress rate limits, disk monitoring, state backup,
and artifact retention appropriate to the eventual public deployment.

A loopback health check on the server, before public firewall changes:

```sh
curl --fail -H 'Host: trueos.eu' http://127.0.0.1:8787/api/catalog
```

DNS, certificate issuance and external port forwarding remain deployment steps
to perform together. No firewall opening is needed for the adapter's outbound
GitHub dispatch and polling.

## Recovery, progress and verification

The adapter writes its dispatch intent into SQLite before contacting GitHub.
Successful dispatch responses supply the run ID. If a response is lost or the
process restarts, it discovers the matching workflow run using the opaque ID
in `run-name`, exact source revision and dispatch event. It never blindly
repeats an uncertain POST. After ten minutes without an identifiable run, the
request fails and requires operator inspection before requesting another build.

The browser shows queueing, submission, waiting for a runner, compilation,
collection and verified completion. These are actual states, not simulated
percentages. GitHub failure or cancellation produces a failed request. A known
run exceeding three hours is cancelled; network outages beyond the deadline
require operator inspection. GitHub's build job itself has a two-hour timeout.

Successful collection checks the event, repository, workflow, source, request
ID and first run attempt. It accepts exactly the expected archive files within
size bounds, verifies GitHub's ZIP digest, exact manifest, source pins, compiler
identity, workflow run identity and system archive SHA-256. It never extracts
archive paths or forwards GitHub authorization to artifact storage redirects.
A failed verification never produces a licensed or successful job.

Do not manually rerun an API-linked workflow: reruns are rejected. Submit a new
request after investigating a failure. Restore the original operator config
while recovering an existing pending job; change approved pins only after the
queue drains. Keep the App credentials and state available across restarts.
An ambiguous dispatch may leave a GitHub run requiring manual cancellation.
Operator diagnostics are recorded in `STATE/jobs/JOB_ID/actions-error.txt`;
public responses omit transport details and host paths.

Actions artifacts expire after fourteen days. The adapter imports completed
archives promptly; the API copy then follows the operator's retention policy.
There is currently no shared artifact cache, automatic build retry or automatic
API archive deletion. Per-person receipts remain separate from build inputs.

## First live hosted test: 2026-11-15

The dedicated TRUEOS GitHub App successfully dispatched and collected a real
[kernel-only build](https://github.com/t4ce/TRUEOS/actions/runs/43653888949)
from `t4ce/TRUEOS` at `f991e43bd07604a7e896d8332afe38900822b4c0`.
The HTTP challenge/SSH proof, repository-scoped App authentication, source and
compiler checks, Actions ZIP digest, manifest/provenance checks, authenticated
HTTP artifact download and a separate test-issuer receipt were verified.
The compressed archive is 15,237,718 bytes; its SHA-256 is
`506eb76d76ac80326f4d2211986e88831f45914b97f262c1e35bffc6b444edbc`.
Bounded decompression confirmed the 22,441,296-byte ELF64 kernel, UEFI loader
and GPU firmware. This test used dedicated test subject/issuer keys. It does
not establish bootability, production receipt trust, multi-account OAuth, or
public Fedora deployment. See the machine-readable
[verification record](validation/live-hosted-2026-11-15.json).

## Validation before public launch

```sh
python3 -m unittest discover -s tools/cloudbuild -p 'test_*.py'
python3 -m unittest discover -s tools/testpy -p test_rig_commands.py
TRUEOS_SITE_DIR=/path/to/TRUEOS_SITE node --test tools/cloudbuild/test_site_proxy.cjs
```

The Actions tests use a fake transport and explicitly marked sentinel bytes.
They cover restart recovery, no duplicate dispatch, source/run/provenance
binding, deadlines, failures, path traversal, credential stripping and private
job access. They do not establish live App permissions, cloud build success,
network reachability or bootability.

Before opening the firewall, exercise one live zero-app build and one selected
app build with the provisioned App and approved sources, verify the downloaded
receipts, and boot the resulting images in QEMU or on the intended hardware.
Full graphics removal and a stripped Server preset remain separate work.

API references: [workflow dispatch](https://docs.github.com/en/rest/actions/workflows#create-a-workflow-dispatch-event),
[workflow runs](https://docs.github.com/en/rest/actions/workflow-runs),
[artifact downloads](https://docs.github.com/en/rest/actions/artifacts).
