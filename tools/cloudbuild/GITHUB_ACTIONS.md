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
