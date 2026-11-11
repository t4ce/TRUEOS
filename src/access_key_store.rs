//! Durable workspace access identities in the shared configuration Redb image.
//!
//! This module edits images only. The caller serializes storage, durably saves
//! a returned image, and only then installs grants or stamps filesystem roots.
//! Key references are public authorization metadata, never encryption material.
use alloc::{string::String, vec::Vec};
use trueos_crypto::{AccountId, KeyHandle, KeyRef, ProviderId};
use trueos_fs::RecordKey;
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, TableDefinition, TableError},
};

pub(crate) const SOFTWARE_PROVIDER_BYTES: [u8; 16] = *b"trueos.fs.key.v1";
const WORKSPACES: TableDefinition<&str, &[u8]> = TableDefinition::new("access_key_workspace_v1");
const VERSION: u32 = 1;
const MAGIC: &[u8; 4] = b"AKWS";
const RECORD_BYTES: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkspaceRecord {
    pub(crate) owner: AccountId,
    pub(crate) key: RecordKey,
}

#[derive(Debug)]
pub(crate) struct WorkspaceUpdate {
    pub(crate) record: WorkspaceRecord,
    /// `None` means the existing durable record is authoritative and unchanged.
    pub(crate) image: Option<Vec<u8>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StoreError {
    InvalidWorkspace,
    InvalidKey,
    Corrupt,
    ForeignOwner,
    KeyAlreadyAssigned,
    Database,
}

pub(crate) fn software_key(handle: [u8; 32]) -> Result<RecordKey, StoreError> {
    let handle = KeyHandle::new(handle).ok_or(StoreError::InvalidKey)?;
    let provider = ProviderId::new(SOFTWARE_PROVIDER_BYTES).ok_or(StoreError::InvalidKey)?;
    Ok(RecordKey::Key(KeyRef::new(provider, handle)))
}

fn valid_workspace(workspace: &str) -> bool {
    workspace.len() <= 1024
        && workspace.starts_with("apps/")
        && !workspace.contains(['\0', '\\'])
        && workspace
            .split('/')
            .all(|part| !part.is_empty() && !matches!(part, "." | ".."))
}

fn checked_key(key: RecordKey) -> Result<KeyRef, StoreError> {
    match key {
        RecordKey::Key(key) if key.provider.as_bytes() == &SOFTWARE_PROVIDER_BYTES => Ok(key),
        _ => Err(StoreError::InvalidKey),
    }
}

fn encode(record: WorkspaceRecord) -> Result<[u8; RECORD_BYTES], StoreError> {
    let key = checked_key(record.key)?;
    let mut bytes = [0; RECORD_BYTES];
    bytes[..4].copy_from_slice(MAGIC);
    bytes[4..8].copy_from_slice(&VERSION.to_le_bytes());
    bytes[8..16].copy_from_slice(&record.owner.raw().to_le_bytes());
    bytes[16..32].copy_from_slice(key.provider.as_bytes());
    bytes[32..64].copy_from_slice(key.handle.as_bytes());
    Ok(bytes)
}

fn decode(bytes: &[u8]) -> Result<WorkspaceRecord, StoreError> {
    if bytes.len() != RECORD_BYTES
        || &bytes[..4] != MAGIC
        || u32::from_le_bytes(bytes[4..8].try_into().unwrap()) != VERSION
        || bytes[16..32] != SOFTWARE_PROVIDER_BYTES
    {
        return Err(StoreError::Corrupt);
    }
    let owner = AccountId::new(u64::from_le_bytes(bytes[8..16].try_into().unwrap()));
    let key = software_key(bytes[32..64].try_into().unwrap()).map_err(|_| StoreError::Corrupt)?;
    Ok(WorkspaceRecord { owner, key })
}

/// Provision one owner-qualified workspace identity without changing an existing
/// assignment. Empty bytes mean a missing configuration file; nonempty corrupt
/// images are errors. Callers must not convert a read error into empty bytes.
///
/// A newly generated candidate is used only when no record exists. This allows
/// two serialized launches, and launches after reboot, to reuse exactly one key.
pub(crate) fn ensure_workspace(
    image: &[u8],
    workspace: &str,
    actor: AccountId,
    generated_key: RecordKey,
) -> Result<WorkspaceUpdate, StoreError> {
    if !valid_workspace(workspace) {
        return Err(StoreError::InvalidWorkspace);
    }
    let store = ImageDatabase::open(image).map_err(|_| StoreError::Corrupt)?;
    let mut assigned = None;
    let mut candidate_taken = false;
    let mut used_keys = Vec::new();
    {
        let read = store
            .database()
            .begin_read()
            .map_err(|_| StoreError::Database)?;
        match read.open_table(WORKSPACES) {
            Ok(table) => {
                // Validate every row, including unknown owners, before a grant
                // is returned. A damaged catalog cannot selectively disappear.
                for row in table.iter().map_err(|_| StoreError::Database)? {
                    let (name, value) = row.map_err(|_| StoreError::Database)?;
                    if !valid_workspace(name.value()) {
                        return Err(StoreError::Corrupt);
                    }
                    let record = decode(value.value())?;
                    if used_keys.contains(&record.key) {
                        return Err(StoreError::Corrupt);
                    }
                    used_keys.push(record.key);
                    if name.value() == workspace {
                        assigned = Some(record);
                    } else if record.key == generated_key {
                        candidate_taken = true;
                    }
                }
            }
            Err(TableError::TableDoesNotExist(_)) => {}
            Err(_) => return Err(StoreError::Corrupt),
        }
    }
    if let Some(record) = assigned {
        if record.owner != actor {
            return Err(StoreError::ForeignOwner);
        }
        return Ok(WorkspaceUpdate {
            record,
            image: None,
        });
    }
    checked_key(generated_key)?;
    if candidate_taken {
        return Err(StoreError::KeyAlreadyAssigned);
    }
    let record = WorkspaceRecord {
        owner: actor,
        key: generated_key,
    };
    let bytes = encode(record)?;
    let write = store
        .database()
        .begin_write()
        .map_err(|_| StoreError::Database)?;
    {
        write
            .open_table(WORKSPACES)
            .map_err(|_| StoreError::Database)?
            .insert(workspace, bytes.as_slice())
            .map_err(|_| StoreError::Database)?;
    }
    write.commit().map_err(|_| StoreError::Database)?;
    let image = store.into_image().map_err(|_| StoreError::Database)?;
    Ok(WorkspaceUpdate {
        record,
        image: Some(image),
    })
}

// Administrative metadata shares the existing image, preserving other tables.
const ALIASES: TableDefinition<&[u8], &str> = TableDefinition::new("access_key_alias_v1");
const APP_GRANTS: TableDefinition<&str, &[u8]> = TableDefinition::new("access_key_app_grant_v1");

#[derive(Clone, Debug, Default)]
pub(crate) struct Catalog {
    pub workspaces: Vec<(String, WorkspaceRecord)>,
    pub aliases: Vec<(RecordKey, String)>,
    pub grants: Vec<(AccountId, String, RecordKey)>,
}

#[derive(Clone, Debug)]
pub(crate) enum AdminChange {
    Alias {
        key: RecordKey,
        name: String,
    },
    Grant {
        account: AccountId,
        app: String,
        key: RecordKey,
        enabled: bool,
    },
}

pub(crate) fn valid_app_name(app: &str) -> bool {
    !app.is_empty()
        && app.len() <= 128
        && !app.starts_with(['.', '_'])
        && !app.ends_with(['.', '_'])
        && app
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '-' | '.'))
}

fn key_bytes(key: RecordKey) -> [u8; 49] {
    let mut bytes = [0; 49];
    if let RecordKey::Key(key) = key {
        bytes[0] = 1;
        bytes[1..17].copy_from_slice(key.provider.as_bytes());
        bytes[17..].copy_from_slice(key.handle.as_bytes());
    }
    bytes
}

fn read_key(bytes: &[u8]) -> Result<RecordKey, StoreError> {
    if bytes.len() != 49 {
        return Err(StoreError::Corrupt);
    }
    match bytes[0] {
        0 if bytes[1..].iter().all(|b| *b == 0) => Ok(RecordKey::Ffa),
        1 => Ok(RecordKey::Key(KeyRef::new(
            ProviderId::new(bytes[1..17].try_into().unwrap()).ok_or(StoreError::Corrupt)?,
            KeyHandle::new(bytes[17..].try_into().unwrap()).ok_or(StoreError::Corrupt)?,
        ))),
        _ => Err(StoreError::Corrupt),
    }
}

fn valid_alias(name: &str) -> bool {
    !name.trim().is_empty() && name.len() <= 64 && !name.chars().any(char::is_control)
}

pub(crate) fn catalog(image: &[u8]) -> Result<Catalog, StoreError> {
    let store = ImageDatabase::open(image).map_err(|_| StoreError::Corrupt)?;
    let read = store
        .database()
        .begin_read()
        .map_err(|_| StoreError::Database)?;
    let mut result = Catalog::default();
    match read.open_table(WORKSPACES) {
        Ok(table) => {
            for row in table.iter().map_err(|_| StoreError::Database)? {
                let (path, value) = row.map_err(|_| StoreError::Database)?;
                let record = decode(value.value())?;
                if !valid_workspace(path.value())
                    || result.workspaces.iter().any(|(_, r)| r.key == record.key)
                {
                    return Err(StoreError::Corrupt);
                }
                result.workspaces.push((String::from(path.value()), record));
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(_) => return Err(StoreError::Corrupt),
    }
    match read.open_table(ALIASES) {
        Ok(table) => {
            for row in table.iter().map_err(|_| StoreError::Database)? {
                let (key, name) = row.map_err(|_| StoreError::Database)?;
                if !valid_alias(name.value())
                    || result.aliases.iter().any(|(_, n)| n == name.value())
                {
                    return Err(StoreError::Corrupt);
                }
                result
                    .aliases
                    .push((read_key(key.value())?, String::from(name.value())));
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(_) => return Err(StoreError::Corrupt),
    }
    match read.open_table(APP_GRANTS) {
        Ok(table) => {
            for row in table.iter().map_err(|_| StoreError::Database)? {
                let (subject, keys) = row.map_err(|_| StoreError::Database)?;
                let (account, app) = subject.value().split_once('/').ok_or(StoreError::Corrupt)?;
                let account = AccountId::new(account.parse().map_err(|_| StoreError::Corrupt)?);
                if !valid_app_name(app) || keys.value().is_empty() || keys.value().len() % 49 != 0 {
                    return Err(StoreError::Corrupt);
                }
                for bytes in keys.value().chunks_exact(49) {
                    let grant = (account, String::from(app), read_key(bytes)?);
                    if result.grants.contains(&grant) {
                        return Err(StoreError::Corrupt);
                    }
                    result.grants.push(grant);
                }
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(_) => return Err(StoreError::Corrupt),
    }
    Ok(result)
}

pub(crate) fn edit_admin(image: &[u8], change: &AdminChange) -> Result<Vec<u8>, StoreError> {
    let mut state = catalog(image)?;
    let store = ImageDatabase::open(image).map_err(|_| StoreError::Corrupt)?;
    let write = store
        .database()
        .begin_write()
        .map_err(|_| StoreError::Database)?;
    match change {
        AdminChange::Alias { key, name } => {
            if !valid_alias(name) || state.aliases.iter().any(|(k, n)| k != key && n == name) {
                return Err(StoreError::InvalidKey);
            }
            let bytes = key_bytes(*key);
            write
                .open_table(ALIASES)
                .map_err(|_| StoreError::Database)?
                .insert(bytes.as_slice(), name.as_str())
                .map_err(|_| StoreError::Database)?;
        }
        AdminChange::Grant {
            account,
            app,
            key,
            enabled,
        } => {
            if !valid_app_name(app) {
                return Err(StoreError::InvalidWorkspace);
            }
            state
                .grants
                .retain(|(a, p, k)| a != account || p != app || k != key);
            if *enabled {
                state.grants.push((*account, app.clone(), *key));
            }
            let mut bytes = Vec::new();
            for (_, _, key) in state
                .grants
                .iter()
                .filter(|(a, p, _)| a == account && p == app)
            {
                bytes.extend_from_slice(&key_bytes(*key));
            }
            let subject = alloc::format!("{}/{app}", account.raw());
            let mut table = write
                .open_table(APP_GRANTS)
                .map_err(|_| StoreError::Database)?;
            if bytes.is_empty() {
                table
                    .remove(subject.as_str())
                    .map_err(|_| StoreError::Database)?;
            } else {
                table
                    .insert(subject.as_str(), bytes.as_slice())
                    .map_err(|_| StoreError::Database)?;
            }
        }
    }
    write.commit().map_err(|_| StoreError::Database)?;
    store.into_image().map_err(|_| StoreError::Database)
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos_redb::redb::TableDefinition;

    const OTHER: TableDefinition<&str, &[u8]> = TableDefinition::new("unrelated_settings_v9");
    fn key(n: u8) -> RecordKey {
        software_key([n; 32]).unwrap()
    }
    fn account(n: u64) -> AccountId {
        AccountId::new(n)
    }
    fn provision(image: &[u8], path: &str, owner: u64, n: u8) -> WorkspaceUpdate {
        ensure_workspace(image, path, account(owner), key(n)).unwrap()
    }
    fn write_raw(image: &[u8], path: &str, bytes: &[u8]) -> Vec<u8> {
        let store = ImageDatabase::open(image).unwrap();
        let write = store.database().begin_write().unwrap();
        {
            write
                .open_table(WORKSPACES)
                .unwrap()
                .insert(path, bytes)
                .unwrap();
        }
        write.commit().unwrap();
        store.into_image().unwrap()
    }

    #[test]
    fn admin_edits_preserve_unrelated_tables_and_workspace_identities() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        write
            .open_table(OTHER)
            .unwrap()
            .insert("theme", b"opaque\0settings".as_slice())
            .unwrap();
        write.commit().unwrap();
        let first = provision(&store.into_image().unwrap(), "apps/codex", 41, 1);
        let record = first.record;
        let image = edit_admin(
            &first.image.unwrap(),
            &AdminChange::Alias {
                key: record.key,
                name: "codex-workspace".into(),
            },
        )
        .unwrap();
        let image = edit_admin(
            &image,
            &AdminChange::Grant {
                account: account(41),
                app: "termdir".into(),
                key: record.key,
                enabled: true,
            },
        )
        .unwrap();
        let state = catalog(&image).unwrap();
        assert_eq!(
            state.workspaces,
            alloc::vec![(String::from("apps/codex"), record)]
        );
        assert_eq!(
            state.aliases,
            alloc::vec![(record.key, String::from("codex-workspace"))]
        );
        assert_eq!(
            state.grants,
            alloc::vec![(account(41), String::from("termdir"), record.key)]
        );
        let reopened = ImageDatabase::open(&image).unwrap();
        let read = reopened.database().begin_read().unwrap();
        assert_eq!(
            read.open_table(OTHER)
                .unwrap()
                .get("theme")
                .unwrap()
                .unwrap()
                .value(),
            b"opaque\0settings"
        );
        assert_eq!(provision(&image, "apps/codex", 41, 2).record, record);
        assert!(
            edit_admin(
                &image,
                &AdminChange::Alias {
                    key: key(2),
                    name: "codex-workspace".into()
                }
            )
            .is_err()
        );
        assert!(
            edit_admin(
                b"damaged database",
                &AdminChange::Alias {
                    key: key(2),
                    name: "other".into()
                }
            )
            .is_err()
        );
    }

    #[test]
    fn malformed_admin_tables_are_errors_not_empty_catalogs() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        write
            .open_table(APP_GRANTS)
            .unwrap()
            .insert("41/termdir", &[1u8; 48][..])
            .unwrap();
        write.commit().unwrap();
        assert_eq!(
            catalog(&store.into_image().unwrap()).unwrap_err(),
            StoreError::Corrupt
        );
    }

    #[test]
    fn two_instances_and_reboot_reuse_the_durable_identity() {
        let first = provision(&[], "apps/codex", 41, 1);
        let image = first.image.unwrap();
        let second = provision(&image, "apps/codex", 41, 2);
        assert_eq!(second.record, first.record);
        assert!(second.image.is_none());
        let reboot = provision(&image.clone(), "apps/codex", 41, 3);
        assert_eq!(reboot.record, first.record);
        assert!(reboot.image.is_none());
    }

    #[test]
    fn two_accounts_own_distinct_workspaces_and_cannot_take_each_others() {
        let alice = provision(&[], "apps/codex", 41, 1);
        let image = alice.image.unwrap();
        let bob = provision(&image, "apps/codex_bob", 42, 2);
        let image = bob.image.unwrap();
        assert_eq!(provision(&image, "apps/codex", 41, 4).record, alice.record);
        assert_eq!(
            provision(&image, "apps/codex_bob", 42, 4).record,
            bob.record
        );
        assert_eq!(
            ensure_workspace(&image, "apps/codex", account(42), key(3)).unwrap_err(),
            StoreError::ForeignOwner
        );
        assert_eq!(
            ensure_workspace(&image, "apps/codex_bob", account(41), key(3)).unwrap_err(),
            StoreError::ForeignOwner
        );
    }

    #[test]
    fn unrelated_configuration_tables_survive_a_new_workspace() {
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        {
            let mut table = write.open_table(OTHER).unwrap();
            table.insert("theme", b"\0\xffnative\n".as_slice()).unwrap();
            table
                .insert("opaque_future_fields", [7u8; 8192].as_slice())
                .unwrap();
        }
        write.commit().unwrap();
        let old = store.into_image().unwrap();
        let updated = provision(&old, "apps/codex", 41, 1).image.unwrap();
        let store = ImageDatabase::open(&updated).unwrap();
        let read = store.database().begin_read().unwrap();
        let table = read.open_table(OTHER).unwrap();
        assert_eq!(
            table.get("theme").unwrap().unwrap().value(),
            b"\0\xffnative\n"
        );
        assert_eq!(
            table.get("opaque_future_fields").unwrap().unwrap().value(),
            &[7u8; 8192]
        );
    }

    #[test]
    fn corrupt_nonempty_database_is_never_reset() {
        assert_eq!(
            ensure_workspace(b"broken configuration", "apps/codex", account(41), key(1))
                .unwrap_err(),
            StoreError::Corrupt
        );
    }

    #[test]
    fn corrupt_or_future_rows_fail_closed_even_for_another_workspace() {
        let valid = encode(WorkspaceRecord {
            owner: account(41),
            key: key(1),
        })
        .unwrap();
        let mut cases = alloc::vec![alloc::vec![], valid[..63].to_vec()];
        for index in [0, 4, 16, 32] {
            let mut bad = valid;
            if index == 32 {
                bad[32..].fill(0);
            } else {
                bad[index] ^= 0x80;
            }
            cases.push(bad.to_vec());
        }
        for bytes in cases {
            let image = write_raw(&[], "apps/other", &bytes);
            assert_eq!(
                ensure_workspace(&image, "apps/codex", account(41), key(2)).unwrap_err(),
                StoreError::Corrupt
            );
        }
    }

    #[test]
    fn table_schema_conflict_fails_without_recreating_the_table() {
        let conflicting: TableDefinition<&str, u64> =
            TableDefinition::new("access_key_workspace_v1");
        let store = ImageDatabase::open(&[]).unwrap();
        let write = store.database().begin_write().unwrap();
        {
            write
                .open_table(conflicting)
                .unwrap()
                .insert("apps/old", 77)
                .unwrap();
        }
        write.commit().unwrap();
        let image = store.into_image().unwrap();
        assert_eq!(
            ensure_workspace(&image, "apps/codex", account(41), key(1)).unwrap_err(),
            StoreError::Corrupt
        );
        let store = ImageDatabase::open(&image).unwrap();
        assert_eq!(
            store
                .database()
                .begin_read()
                .unwrap()
                .open_table(conflicting)
                .unwrap()
                .get("apps/old")
                .unwrap()
                .unwrap()
                .value(),
            77
        );
    }

    #[test]
    fn opaque_software_keys_exclude_ffa_zero_and_other_providers() {
        assert_eq!(software_key([0; 32]), Err(StoreError::InvalidKey));
        assert_eq!(
            ensure_workspace(&[], "apps/codex", account(41), RecordKey::Ffa).unwrap_err(),
            StoreError::InvalidKey
        );
        let foreign = RecordKey::Key(KeyRef::new(
            ProviderId::new([1; 16]).unwrap(),
            KeyHandle::new([2; 32]).unwrap(),
        ));
        assert_eq!(
            ensure_workspace(&[], "apps/codex", account(41), foreign).unwrap_err(),
            StoreError::InvalidKey
        );
        let image = provision(&[], "apps/codex", 41, 1).image.unwrap();
        assert_eq!(
            ensure_workspace(&image, "apps/codex", account(41), RecordKey::Ffa)
                .unwrap()
                .record
                .key,
            key(1)
        );
    }

    #[test]
    fn keys_cannot_be_reassigned_or_duplicated_between_workspaces() {
        let image = provision(&[], "apps/codex", 41, 1).image.unwrap();
        assert_eq!(
            ensure_workspace(&image, "apps/codex_bob", account(42), key(1)).unwrap_err(),
            StoreError::KeyAlreadyAssigned
        );
        let duplicate = encode(WorkspaceRecord {
            owner: account(42),
            key: key(1),
        })
        .unwrap();
        let image = write_raw(&image, "apps/codex_bob", &duplicate);
        assert_eq!(
            ensure_workspace(&image, "apps/codex", account(41), key(3)).unwrap_err(),
            StoreError::Corrupt
        );
    }

    #[test]
    fn canonical_workspace_paths_and_root_account_are_explicit() {
        for path in [
            "",
            "/apps/codex",
            "apps",
            "apps/",
            "apps//codex",
            "apps/./codex",
            "apps/../codex",
            "apps/codex/",
            "apps/codex\0",
            "apps\\codex",
            "users/codex",
        ] {
            assert_eq!(
                ensure_workspace(&[], path, account(41), key(1)).unwrap_err(),
                StoreError::InvalidWorkspace,
                "{path:?}"
            );
        }
        // AccountId::ROOT is a valid native owner, never an absent principal.
        assert_eq!(
            provision(&[], "apps/codex", 0, 1).record.owner,
            AccountId::ROOT
        );
    }
}
