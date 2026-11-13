# TRUEOS cloudbuild source handoff

This ZIP contains all changes crafted in the conversation, the Markdown
decision record, a Git patch and history bundle, exact source references,
the original measured release, and validation evidence.

**Start here:**

- [Conversation and decisions](changed-files/tools/cloudbuild/CONVERSATION.md)
- [Integration notes for current upstream](INTEGRATION.md)
- [Implemented selector and operator guide](changed-files/tools/cloudbuild/README.md)
- [Server design](changed-files/tools/cloudbuild/SERVER.md)
- [Size measurements](changed-files/tools/cloudbuild/SIZE_ESTIMATES.md)

## Contents

| Path | Purpose |
| --- | --- |
| `changed-files/` | Exact final contents of all 41 changed paths, preserving repository layout and executable modes. |
| `changes.patch` | Full-index binary-capable diff from `75e9b1dc0818` to `11cacfdd1b90`. |
| `history.bundle` | Incremental Git history for `feat/subsystem-cloudbuild`, including its merge commit. |
| `reference.json`, `changed-files.json`, `commits.txt`, `diffstat.txt` | Exact commit/tree/blob references and change inventory. |
| `reference/base/` | Original versions of changed files and selected supporting sources at the patch base. |
| `reference/upstream/` | Corresponding files at observed current `true` (`94af018df093`). |
| `INTEGRATION.md`, `reference/merge-preview.txt` | Text conflicts and AppDB semantic integration work. |
| `evidence/` | Original release archive, measurements, inspection helpers and historical DOM/API checks. |
| `PR_DESCRIPTION.md` | Optional PR description for your eventual local submission. |
| `validation/`, `SHA256SUMS` | Export checks and SHA-256 inventory. |

The snapshots are for review. Import into an existing TRUEOS checkout with
the Git bundle or patch so unchanged repository sources and history remain
available. Use one of the following paths.

## Recommended: import the branch history

From your existing TRUEOS checkout containing the prerequisite history below:

```sh
git bundle verify /path/to/handoff/history.bundle
git fetch /path/to/handoff/history.bundle \
  refs/heads/feat/subsystem-cloudbuild:refs/heads/review/trueos-cloudbuild
git switch review/trueos-cloudbuild
```

Expected imported HEAD: **`11cacfdd1b90b284f11a958f48e0c28742867e01`**.

The bundle is incremental and declares these two prerequisite commits:

- `15e57a4c1898be8b4b3770eaa93705d08729d4a7`
- `75e9b1dc08189d3bba546213b4d5f3b4f3ee9757`

A full checkout containing the patch base's history has both. If either is
missing from a shallow local clone, obtain the required TRUEOS history first.
There is no need to authenticate to GitHub to read this ZIP or import the
bundle into a checkout that already has that history.

Read `INTEGRATION.md`, then reconcile the current `true` branch locally.
The preview found three textual conflicts and a separate new AppDB metadata
contract. The source snapshot has not been rebased or validated against
those later changes.

## Alternative: apply the complete diff at its exact base

From your existing TRUEOS checkout:

```sh
git worktree add -b review/trueos-cloudbuild-patch \
  ../trueos-cloudbuild-review 75e9b1dc08189d3bba546213b4d5f3b4f3ee9757
git -C ../trueos-cloudbuild-review apply --check /path/to/handoff/changes.patch
git -C ../trueos-cloudbuild-review apply --index /path/to/handoff/changes.patch
```

That recreates the exported source tree without preserving individual
implementation commits. Applying to a newer base needs the integration
work documented above. `changed-files/` is also available for direct review.

## Validation and provenance

The patch was applied to a separate temporary Git index initialized from the
exact base; its resulting complete tree hash was checked against the exported
HEAD. The original repository index and working tree were left unchanged.
The bundle was verified and its published branch tip checked. The ZIP member
paths, CRCs, per-file hashes and source snapshots were validated.

The portable final selector check was rerun on 2026-11-12. All ten grouped
DOM/API checks passed; see `validation/portable-ui-check.json`.
Recorded implementation tests and their limits are in the conversation
record. Full OS compilation/boot checks require the provisioned TRUEOS
toolchain and matching sibling checkouts. The Server profile remains a design
target; it is not an implemented graphics-free image.

To verify extracted file contents from this directory:

```sh
sha256sum -c SHA256SUMS
```

`SHA256SUMS` covers every other payload file, including the source reference
and original uploaded release. The ZIP's own SHA-256 is supplied separately.
