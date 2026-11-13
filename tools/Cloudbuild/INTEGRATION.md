# Local integration notes

## Exact references

- Exported branch: `feat/subsystem-cloudbuild`
- Exported HEAD: `11cacfdd1b90b284f11a958f48e0c28742867e01`
- Patch base: `75e9b1dc08189d3bba546213b4d5f3b4f3ee9757` (integrated TRUEOS v0.0.235)
- Current `true` observed for this handoff: `94af018df093b18eceb2b00b3072ed6c358021c6`

The source changes preserve the work from the conversation. The newer
upstream tree has not been merged into the exported branch.

## Textual conflicts from the merge preview

| File | Reconciliation |
| --- | --- |
| `src/usb3/mod.rs` | Preserve the `subsystem-usb` admission guard together with upstream's removal of the old MicroFont USB boot-stage call. |
| `tools/trueos-boot/prepare.sh` | Keep upstream's unconditional `cargo metadata --format-version 1` resolution of missing/stale locks, plus the selected-buildins adapter, private Edit staging, native-only forwarding and zero-app skip. |
| `tools/testpy/test_rig_commands.py` | Reconcile fixtures and call traces for both selected apps and the newer metadata/lock step. |

The raw read-only merge simulation is in `reference/merge-preview.txt`.

## Semantic AppDB changes even where Git auto-merges

1. **Zero apps:** upstream now opens the `metadata` table and requires the
   `appbundle.json` entry before publishing the database. The old zero-app
   path creates only `apps`, `hashes` and `updated`. The combined tree will
   fail initialization until the empty database has valid bundle metadata.
2. **Selected apps:** `tools/appdb-seed` now takes five arguments:
   `BUILDINS_JSON PACKAGE_DIR OUTPUT_DB UPDATED_UTC APPBUNDLE_JSON`.
   This branch supplies four. Supply the definition and include it in source
   fingerprints/readiness checks.
3. **Companion requirements:** the new seeder requires each bundle member.
   Reconcile that rule with exact app selection and reject incompatible
   choices clearly. Do not silently extend a signed app list.
4. **Sibling inputs:** the new `trueos-appbundle` crate lives in the sibling
   `TRUEOS-Blueprints/crates/trueos-appbundle`. This ZIP does not claim to pin
   a new compatible sibling revision. Choose complete matching checkouts and
   rerun the strict adapter contract checks.

`reference/base/` and `reference/upstream/` contain exact snapshots of the
affected files and additional sources for these interfaces. The historical
Server audit and release measurements retain their original source scope.

## Acceptance after reconciliation

Run the host suite in the combined checkout, then compile and boot zero-app,
selected-app and normal default builds using the archived TRUEOS compiler.
Check native-only audit mode, empty/new AppDB metadata, startup behavior and
the selected package list. Server remains a documented design: UI4/GPU
compilation exclusion is not implemented here.

Resolve fresh schema 2 manifests and key challenges at the eventual merged
commit. Existing source hashes and signatures identify the old tree.
