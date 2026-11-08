# Access key model

AccessKeyDB relates native file keys to the authenticated user and the app
container that user starts. AppDB still holds one executable entry per app.
Two Codex VMs can reuse one container and its storage key without duplicating
app code or copying that user's persisted login.

## Current integration

The experimental Codex bridge provisions one owner/key relation for its existing
`apps/codex` workspace, or the selected flat `apps/codex_<containerID>` workspace.
The first authenticated preparation records the owner and generated key in
`users/configuration.redb`. Later starts reuse that exact relation, including
after reboot. Another user cannot claim the same workspace. Code download or
AppDB replacement alone does not grant access to an existing workspace.

Preparation saves and reads back the configuration image before stamping the
workspace directory. It preserves directory identity and existing descendant
contents. Both the primary root and Codex's supporting RAM root carry the same
key; `.codex` remains durable. Only after both are ready does trusted host code
install the owner's app grant and container membership. Any failure aborts
preparation; it never falls back to public storage.

`/apps` itself stays FFA. New files and directories inherit the nearest keyed ancestor
identity under the same filesystem lease as their publication. Existing records keep
their keys when updated. Old FFA descendants remain protected by the keyed
ancestor, without rewriting a user's existing data.

The software key references are authorization metadata. They do not encrypt
file payloads or replace the missing TPM. Cry's current software machine-key
provider remains experimental.

## Permission relations

| Relation | Meaning |
| --- | --- |
| workspace to owner/key | Durable owner and one generated key for that workspace |
| container to app and ID | Which app and container use that physical workspace |
| container policy | Private owner, or explicitly registered shared workspace |
| account to container | Which authenticated account may use that container |
| account and app to keys | Keys granted across that account's admitted containers of an app |
| account and container to keys | Keys granted in one exact container |

For a non-FFA record, access requires an authenticated launching account,
container membership, a registered key, and that account's app or container
grant. The kernel adapter treats FFA as public. The pure policy database can
also represent explicit FFA grants, but native filesystem access does not require
one for public records.

A record has exactly one `RecordKey`. Sharing adds grants to that key; it does
not attach a second key to a file. Splitting access between files that share a
key requires assigning a distinct key through a trusted metadata transaction.
There is no guest operation for inventing an account or granting itself a key.

## Captured identity

A human frontend receives an account only after successful Cry sign-in, an
SSH request signed by that account's enrolled key, or a verified SSH Cry code.
The SSH code route durably consumes the replay step before accepting the session
and does not sign in any other terminal. An unsigned SSH key probe,
username text, or an anonymous development connection does not authenticate an
account. Persisted SSH usernames must prove their identity by enrolled key or Cry code,
even when no SSH key has been added. They cannot bypass proof through
development none-auth.

Successful Cry login binds the exact initiating frontend after durable success.
Other terminals keep their own bindings. A frontend must park an active protected
run before changing account or logging out; detach clears only its own binding. Existing stored account IDs remain unchanged, including legacy
`t4ce`/ROOT profiles; new accounts persist distinct IDs derived from their newly
generated credential public keys.

Launch requests capture their frontend's account before queuing. Guest launches
and child terminals inherit the parent's captured principal. They never consult
a later global login or adopt a presentation destination's account. The VM
binding is cleared at teardown. Queued work retains a unique binding epoch;
reusing a VM slot cannot lend the new run's grants to old work. Protected retained
runs reject another account's
terminal takeover and lifecycle control.

## Filesystem boundary

Native file objects and caller-aware path operations check both ancestor and
leaf keys while holding the same root lease as the read or publication. A
retained unlinked file keeps its original authorization keys. Listings and tree
exports filter unauthorized entries. Recursive changes check affected descendants;
rename cannot turn an inherited private FFA child into public data by moving it
out of its keyed ancestor.

Codex explicitly asserts native private storage before accessing `auth.json` and
its lock paths. The assertion uses filesystem object opcode 14 through the
existing native API. The resolved path must be within the captured workspace,
whose root must match the provisioned owner/key relation. This replaces Unix
mode/nlink assumptions at those call sites. A redirected public `CODEX_HOME`
fails instead of silently accepting weaker storage.

The unauthenticated HTTP filesystem export uses a public actor, so only FFA
paths and contents are available there. Its reads, uploads, rename, delete, and
browser tree use the same storage authority. `users/`, the configuration catalog,
and the software machine-key/history records remain kernel-owned through these
app and public filesystem interfaces.

Cached native IDE source images retain the keys captured with their bytes and
recheck the requesting actor. Trusted kernel filesystem operations remain
explicitly privileged. This integration does not claim that every kernel service
or raw device interface has received a complete confinement audit.

## Containers and next steps

Ordinary container 0 uses `apps/<app>`. Additional containers use flat sibling
`apps/<app>_<containerID>` directories. `apps/<app>_0` is not a separate alias for
container 0. The loader and policy use the same archive-name normalization;
AppDB content hashes and recyclable VM IDs are not permission identities.

The same user may start two Codex instances against the same stored workspace;
they receive the same key. Separate workspaces can hold different persisted
state while sharing the same AppDB code entry. The generic policy supports
explicit app/container grants, but a permission-management UI, account-driven
container selection, and broader app provisioning are future work. Existing
public development apps do not acquire private-container launch requirements.

`users/configuration.redb` is a separate durable image beside Cry profiles,
not volatile `app.db`. Its versioned workspace table preserves unrelated Redb
tables and rejects corrupt records, duplicate keys, schema conflicts, and owner
replacement. Cry profile migration into that image is separate work.
