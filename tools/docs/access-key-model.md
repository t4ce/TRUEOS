# Access key model

AccessKeyDB relates file keys, application workspaces, and the authenticated
account that starts an app. AppDB continues to store application code in RAM.
The AccessKeyDB integration currently **warns and continues**; it does not reject
launches or filesystem operations. Existing reboots and fresh builtin installs
require no access-key setup.

## Invariants

- Every live TRUEOSFS file has exactly one `RecordKey` in its record header.
  One key can label any number of files. Sharing adds database grants, never a
  second key to a file.
- An application has one code entry in AppDB and can use multiple containers.
  Each installation is an app-prefixed container ID with its own files, not
  another copy of its logic. There is no separate static app installation.
- Each key grant names an account and either an app or one exact container.
  App-wide grants cover that account's admitted containers for the app.
- Account-to-container membership grants permission to start that container.
  It is independent of key grants. Installing code, registering a workspace,
  and adding a member grant no file keys.
- A run uses only its launching account's grants. A shared container never
  unions its users' keychains. There is no separate “on behalf of” account.
- A private container cannot gain another owner or become shared in place.
  Sharing starts with a separate clean workspace explicitly registered as
  shared. The trusted host must verify that it is actually clean.

The policy for an existing file is:

```text
allow(user, container, file) =
    authenticated(user)
    AND member(user, container)
    AND registered(file.record_key)
    AND (grant(user, container.app, file.record_key)
         OR grant(user, container, file.record_key))
```

`RecordKey::Ffa` is the existing standard/default file identity. The policy
does not treat it as an implicit grant; the current rollout still permits I/O
after a missing-grant warning. `Key(KeyRef)` names an opaque provider/handle,
not private key bytes. Neither header metadata nor AccessKeyDB implements file
encryption. V3 headers require a valid key extension, including for `Ffa`.

## Database relations

| Relation | Meaning |
| --- | --- |
| key registry | Known `RecordKey` identities; registration gives no access |
| container to app and ID | Which app and installation own this file workspace |
| container policy | Private owner, or explicitly shared from registration |
| account to container | Which authenticated account may start this container |
| account and app to keys | Keys available to that account across admitted containers of an app |
| account and container to keys | Keys available to that account in one exact workspace |

`trueos_fs::access::AccessKeyDb` implements these relations, membership changes,
grant/revoke operations, and separate launch/file decisions. All mutations are
trusted host operations. A guest cannot submit an account ID or modify policy.
`src/access_key_db.rs::configure_policy` is the future built-in permission
management entry point, after user consent. It currently has no command or UI
caller. Runtime policy starts without memberships or grants; launch only
registers the existing standard key identity, without granting it.

## Containers and application identity

Every installation uses `(app ID, container ID)`. Ordinary launch means
container `0`. For compatibility its physical directory remains lowercase
`apps/<app>`; this is the first container, not a different kind of workspace.
Additional containers use flat sibling directories `apps/<app>_<containerID>`.
There is no extra app nesting or user-directory hierarchy in this layout.

| Container | Physical directory | Selection |
| --- | --- | --- |
| notes, 0 | `apps/notes` | ordinary launch or explicit ID 0 |
| notes, 1 | `apps/notes_1` | explicit container ID 1 |
| notes, 2 | `apps/notes_2` | explicit container ID 2 |
| viewer, 0 | `apps/viewer` | ordinary viewer launch |

`apps/notes_0` is not another alias for container 0: selection of ID 0 always
resolves to `apps/notes`. The adapter warns about noncanonical bindings rather
than assigning them container-0 permissions. App names and IDs can contain
underscores, so registration also rejects two different container identities
whose derived physical directories collide.

The adapter derives the app ID using the loader's archive-name normalization.
AppDB hashes and recyclable VM slots are not permission identities. Replacing
an app's code does not create grants or change a selected container ID.
`/common` still resolves to `apps/common`; its location does not confer access.
A key can grant access to files outside the running container, subject to the
existing path rules.
The existing rooted `/apps/<app>/userdata` export explicitly accesses the
app's container-0 userdata; relative userdata paths remain inside the selected
container. File keys still govern the intended sharing permissions.

Three users can share `notes.bp` while using private containers 0, 1, and 2.
Each needs membership in their container and explicit file-key grants. The
user-to-container relationship belongs in configuration, not in the physical
directory name. Later tooling can sort installations by user without changing
their identities or introducing a second installation model.

The host launch request can explicitly select a stable container ID. The
existing named-launch request remains an explicit request for a fresh opaque
ID containing a runtime UUID; repeating a display name creates another
container. All apps follow the same path rule, including Codex. Selected
Codex containers keep their own durable `.codex` subtree. No create/select UI,
allocation of installation IDs, profile-based launch menu, or automatic private
container provisioning is added here. Stable volume identity and a durable
installation catalog remain future work.

## Splitting files that share a key

Suppose files F1 and F2 both carry A, and Editor and Viewer can read A. Later,
Editor must keep both files while Viewer may read only F1:

1. Create/register a distinct key B.
2. Grant the relevant user's Editor scope A and B; leave Viewer with only A.
3. Reassign F2's single header key from A to B, preserving its data, object
   identity, and content type through the filesystem's normal commit mechanism.

| File | Single header key after split | Editor | Viewer |
| --- | --- | --- | --- |
| F1 | A | allowed | allowed |
| F2 | B | allowed | missing grant |

Creating B in the database alone cannot split files while both still carry A.
The policy tests exercise the decision after reassignment; a kernel command
for transactional file rekeying is future work. With encrypted payloads,
provider-aware re-encryption or key wrapping would also need a defined commit
protocol. Keys here are currently access metadata.

## Launch identity

Cry account setup and verified 2FA establish the human launch identity.
Human Shell3 launches capture the authenticated account in their frontend's
transport scope before queuing. Queued work never consults a later global
login to choose its account.

Guest launches, child processes, image viewers, and guest-owned interactive
terminals inherit the parent run's captured account, including an absent
account. Choosing a different presentation destination cannot adopt that
destination's signed-in user. Central Blueprint staging binds the account and
workspace before guest filesystem requests can execute. Teardown clears the
binding; retained pause keeps it. Legacy portable checkpoints have no trusted
authentication origin and restore without an account.

Shell3 resume also compares the initiating account with the retained run's
launch account and checks membership. Even another member of a shared container
cannot take over that run's account or in-memory state. A mismatch warns and
continues in this rollout, leaving the original binding unchanged. Direct
trusted-host lifecycle calls are outside that Shell3 observation point.

Cry currently has one authenticated session and transport-level scopes, not
independent concurrent user sessions for every frontend. Full simultaneous
three-user operation and the first-run membership/consent flow remain future
work. Boot and development launches without Cry continue with warnings during
this rollout; enabling 2FA is not newly required to run existing apps.

## Warn-only integration

`src/access_key_db.rs` translates policy failures to
`log_warn!(target: "filesystem"; ...)`, which uses Logos `Warn`. Records include
the VM, container, captured account, operation, file key, reason, and
`decision=would-deny mode=warn-only`. Duplicate combinations are suppressed per
run, with at most 32 distinct policy warnings per VM. A policy change clears
the suppression history.

The adapter returns no permission result to its filesystem callers. Missing
files retain their existing behavior; a new file is audited using the standard
key. Diagnostic metadata lookup failures log a warning and leave normal I/O
to determine the operation's result. Existing content-type, path, stream-owner,
and storage error checks still apply.

Current observation points are:

- `src/r/io/async_fs_cabi.rs`: resolved guest read/write, stat/metadata/key
  queries, directory targets, remove, rename source and existing destination,
  and the archive read for virtual AppDB installation.
- `src/r/fs/request_broker.rs`: guest-owned native std file reads, range/length
  reads, stat, write-stream begin, directory targets, remove, and exists.
  Later stream chunks retain the established stream ownership checks.
- `src/hv/mod.rs::stage_blueprint_launch`: launch membership decision;
  Shell3 lifecycle control also observes resume identity and membership.

These are best-effort observations, not complete atomic authorization.
Recursive directory operations observe their target, not every descendant's
key; listing/selection results are not filtered. Whole-tree diagnostic exports,
volatile seeding, virtual IDE source services, direct guest archive-launch
reads, and other direct host filesystem calls need separate coverage. Trusted
host operations without a guest owner are outside these hooks. Metadata can
change between observation and an operation's commit. Raw guest calls and
package/app identity also need a complete confinement/trust audit before
enforcement is appropriate.

## Reboot workflow and remaining work

`app.db` remains the host-prepared image opened in writable RAM. Downloads and
replacements last until reboot. AccessKeyDB currently holds boot-local policy;
no file headers are migrated and no memberships or grants are synthesized from
an AppDB install or launch. Ordinary apps still use their existing container-0
directories and standard file keys.

The reserved durable configuration location is **`users/configuration.redb`**
on the primary TRUEOSFS root, beside Cry's `users/<username>` profile
directories. Container registrations, membership, and key grants belong in
tables of that shared configuration image, alongside account/profile
configuration. Keep secret credentials in each user's `secrets` directory.
Do not put this configuration in `apps`, `apps/common`, or volatile AppDB.
The adapter declares the reserved path; it does not create or load the image.
Today's Cry profile remains `users/<username>/account.v1`, with its credential
envelope under `secrets/cry.v1.aes256gcm`; migration into Redb is a separate
storage change. There is no existing configuration Redb being silently reused.

Before switching to denial, implement durable permission storage outside
volatile AppDB, clean container provisioning/reopen, scoped multi-user Cry
sessions, explicit user consent and revocation UI, file-key reassignment,
complete per-record broker coverage, and checks tied to the exact read/commit
record. Then run the full app suite on the rig and enable enforcement in a
separate change. This change has no enforcement switch.
