//! Volatile application catalog and Blueprint byte store.
//!
//! `app.db` is deliberately RAM-only. With bundled apps, the kernel opens its
//! compressed host-built redb image. Kernel-only builds create the same tables
//! empty. Online downloads can add or replace rows in either mode until reboot.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use redb::{
    Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition,
};
#[cfg(feature = "buildin-apps")]
use redb::StorageBackend;
use sha2::{Digest, Sha256};
use spin::{Mutex, Once};

#[cfg(feature = "buildin-apps")]
const APP_DB_IMAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.db.lz4"));
include!(concat!(env!("OUT_DIR"), "/appdb-seed.rs"));
const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const HASHES: TableDefinition<&str, &str> = TableDefinition::new("hashes");
const UPDATED: TableDefinition<&str, &str> = TableDefinition::new("updated");
const METADATA: TableDefinition<&str, &[u8]> = TableDefinition::new("metadata");

static APP_DB: Mutex<Option<Database>> = Mutex::new(None);
// Only the kernel's embedded seed may approve a controller. Runtime AppDB
// downloads can replace executable bytes, but cannot add or change these roles.
static REMOTE_INPUT_CONTROLLERS: Once<Vec<(String, [u8; 32])>> = Once::new();
const BLUEPRINT_CAP_REMOTE_INPUT_CONTROLLER: u16 = 1 << 12;

fn declares_remote_input_controller(bytes: &[u8]) -> bool {
    bytes.len() >= 24
        && bytes.starts_with(b"TRBP")
        && u16::from_le_bytes([bytes[4], bytes[5]]) == 1
        && u16::from_le_bytes([bytes[6], bytes[7]]) & BLUEPRINT_CAP_REMOTE_INPUT_CONTROLLER != 0
}

/// A role declaration in downloaded bytes is insufficient: the exact archive
/// and executable digest must have been approved by this kernel generation.
pub(crate) fn is_remote_input_controller(archive: &str, bytes: &[u8]) -> bool {
    let Some(controllers) = REMOTE_INPUT_CONTROLLERS.get() else { return false; };
    let Some((_, approved)) = controllers.iter().find(|(name, _)| name == archive) else {
        return false;
    };
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    *approved == actual
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppEntry {
    pub archive: String,
    pub sha256: String,
    pub updated: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Initialize a writable catalog before launch requests, seeded or empty.
pub(crate) fn init_bsp() -> Result<usize, String> {
    let backend = redb::backends::InMemoryBackend::new();
    #[cfg(feature = "buildin-apps")]
    {
        let image = crate::r::lz4::decompress_frame_cpu(APP_DB_IMAGE, APP_DB_IMAGE_LEN)
            .map_err(|err| alloc::format!("decompress app.db image: {err:?}"))?;
        if image.len() != APP_DB_IMAGE_LEN {
            return Err(String::from("app.db image length mismatch"));
        }
        backend
            .set_len(image.len() as u64)
            .map_err(|err| alloc::format!("allocate app.db image: {err}"))?;
        backend
            .write(0, &image)
            .map_err(|err| alloc::format!("load app.db image: {err}"))?;
    }
    let database = Database::builder()
        .set_cache_size(64 * 1024 * 1024)
        .create_with_backend(backend)
        .map_err(|err| alloc::format!("open app.db: {err}"))?;
    #[cfg(not(feature = "buildin-apps"))]
    {
        let write = database
            .begin_write()
            .map_err(|err| alloc::format!("begin empty app.db initialization: {err}"))?;
        {
            let _apps = write
                .open_table(APPS)
                .map_err(|err| alloc::format!("create empty apps table: {err}"))?;
            let _hashes = write
                .open_table(HASHES)
                .map_err(|err| alloc::format!("create empty hashes table: {err}"))?;
            let _updated = write
                .open_table(UPDATED)
                .map_err(|err| alloc::format!("create empty dates table: {err}"))?;
            let mut metadata = write
                .open_table(METADATA)
                .map_err(|err| alloc::format!("create empty metadata table: {err}"))?;
            metadata.insert(trueos_appbundle::METADATA_KEY,
                include_bytes!("../../TRUEOS-Blueprints/appbundle.json").as_slice())
                .map_err(|err| alloc::format!("seed empty app bundle metadata: {err}"))?;
        }
        write
            .commit()
            .map_err(|err| alloc::format!("commit empty app.db initialization: {err}"))?;
    }
    let (count, controllers) = {
        let read = database
            .begin_read()
            .map_err(|err| alloc::format!("begin app.db seed read: {err}"))?;
        let apps = read
            .open_table(APPS)
            .map_err(|err| alloc::format!("open seeded apps: {err}"))?;
        let hashes = read
            .open_table(HASHES)
            .map_err(|err| alloc::format!("open seeded hashes: {err}"))?;
        let updated = read
            .open_table(UPDATED)
            .map_err(|err| alloc::format!("open seeded dates: {err}"))?;
        let count = apps
            .len()
            .map_err(|err| alloc::format!("count seeded apps: {err}"))?;
        if count != APP_DB_EXPECTED_APPS as u64
            || hashes
                .len()
                .map_err(|err| alloc::format!("count seeded hashes: {err}"))?
                != count
            || updated
                .len()
                .map_err(|err| alloc::format!("count seeded dates: {err}"))?
                != count
        {
            return Err(String::from(
                "app.db image does not contain the complete built-in set",
            ));
        }
        let mut controllers = Vec::new();
        for row in apps.iter().map_err(|err| alloc::format!("iterate seeded roles: {err}"))? {
            let (archive, bytes) = row.map_err(|err| alloc::format!("read seeded role: {err}"))?;
            if declares_remote_input_controller(bytes.value()) {
                controllers.push((archive.value().to_string(), Sha256::digest(bytes.value()).into()));
            }
        }
        (count as usize, controllers)
    };
    {
        let read = database
            .begin_read()
            .map_err(|err| alloc::format!("read AppDB metadata: {err}"))?;
        let metadata = read
            .open_table(METADATA)
            .map_err(|err| alloc::format!("open AppDB metadata: {err}"))?;
        let json = metadata
            .get(trueos_appbundle::METADATA_KEY)
            .map_err(|err| alloc::format!("read appbundle.json: {err}"))?
            .ok_or("AppDB is missing appbundle.json")?;
        trueos_appbundle::AppBundles::parse(json.value())?;
    }
    REMOTE_INPUT_CONTROLLERS.call_once(|| controllers);
    *APP_DB.lock() = Some(database);
    Ok(count)
}

pub(crate) fn list() -> Result<Vec<AppEntry>, String> {
    let guard = APP_DB.lock();
    let database = guard
        .as_ref()
        .ok_or_else(|| String::from("app.db is not initialized"))?;
    let read = database
        .begin_read()
        .map_err(|err| alloc::format!("begin app.db read: {err}"))?;
    let apps = read
        .open_table(APPS)
        .map_err(|err| alloc::format!("open app.db apps table: {err}"))?;
    let hashes = read
        .open_table(HASHES)
        .map_err(|err| alloc::format!("open app.db hashes table: {err}"))?;
    let updated = read
        .open_table(UPDATED)
        .map_err(|err| alloc::format!("open app.db updated table: {err}"))?;
    let mut entries = Vec::new();
    for row in apps
        .iter()
        .map_err(|err| alloc::format!("iterate app.db: {err}"))?
    {
        let (archive, bytes) = row.map_err(|err| alloc::format!("read app.db row: {err}"))?;
        let archive = archive.value();
        let sha256 = hashes
            .get(archive)
            .map_err(|err| alloc::format!("read app.db hash: {err}"))?
            .map(|value| value.value().to_string())
            .unwrap_or_else(|| sha256_hex(bytes.value()));
        let updated = updated
            .get(archive)
            .map_err(|err| alloc::format!("read app.db update: {err}"))?
            .map(|value| value.value().to_string())
            .unwrap_or_else(|| String::from("this boot"));
        entries.push(AppEntry {
            archive: archive.to_string(),
            sha256,
            updated,
        });
    }
    Ok(entries)
}

/// Machine-readable names from the same live catalog used by Shell3.
pub(crate) fn catalog_json() -> Result<Vec<u8>, String> {
    let apps: Vec<_> = list()?
        .into_iter()
        .map(|entry| {
            let name = entry.archive.strip_suffix(".bp").unwrap_or(&entry.archive);
            serde_json::json!({ "name": name, "archive": entry.archive })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({ "version": 1, "apps": apps }))
        .map_err(|error| alloc::format!("encode AppDB catalog: {error}"))
}

pub(crate) fn get(archive: &str) -> Result<Option<Vec<u8>>, String> {
    let guard = APP_DB.lock();
    let database = guard
        .as_ref()
        .ok_or_else(|| String::from("app.db is not initialized"))?;
    let read = database
        .begin_read()
        .map_err(|err| alloc::format!("begin app.db read: {err}"))?;
    let table = read
        .open_table(APPS)
        .map_err(|err| alloc::format!("open app.db apps table: {err}"))?;
    table
        .get(archive)
        .map(|value| value.map(|value| value.value().to_vec()))
        .map_err(|err| alloc::format!("read app.db {archive}: {err}"))
}

/// Read the embedded bundle definitions without exposing them as an app.
pub(crate) fn appbundle_json() -> Result<Vec<u8>, String> {
    let guard = APP_DB.lock();
    let database = guard.as_ref().ok_or("app.db is not initialized")?;
    let read = database
        .begin_read()
        .map_err(|err| alloc::format!("read AppDB metadata: {err}"))?;
    let metadata = read
        .open_table(METADATA)
        .map_err(|err| alloc::format!("open AppDB metadata: {err}"))?;
    metadata
        .get(trueos_appbundle::METADATA_KEY)
        .map_err(|err| alloc::format!("read appbundle.json: {err}"))?
        .map(|value| value.value().to_vec())
        .ok_or_else(|| "AppDB is missing appbundle.json".into())
}

/// Publish a complete request in one transaction; failed bundles change no rows.
pub(crate) fn insert_downloads(downloads: &[(String, Vec<u8>)]) -> Result<(), String> {
    let bundles = trueos_appbundle::AppBundles::parse(&appbundle_json()?)?;
    if downloads.is_empty() || downloads.len() > trueos_appbundle::MAX_MEMBERS {
        return Err("Invalid AppDB install batch size".into());
    }
    for (index, (archive, _)) in downloads.iter().enumerate() {
        if !trueos_appbundle::valid_archive(archive)
            || downloads[..index]
                .iter()
                .any(|(name, _)| name.eq_ignore_ascii_case(archive))
        {
            return Err(alloc::format!("Invalid or duplicate Blueprint: {archive}"));
        }
    }
    let guard = APP_DB.lock();
    let database = guard
        .as_ref()
        .ok_or_else(|| String::from("app.db is not initialized"))?;
    let write = database
        .begin_write()
        .map_err(|err| alloc::format!("begin app.db write: {err}"))?;
    {
        let mut apps = write
            .open_table(APPS)
            .map_err(|err| alloc::format!("open app.db apps table: {err}"))?;
        let mut hashes = write
            .open_table(HASHES)
            .map_err(|err| alloc::format!("open app.db hashes table: {err}"))?;
        let mut updated = write
            .open_table(UPDATED)
            .map_err(|err| alloc::format!("open app.db updated table: {err}"))?;
        for (archive, bytes) in downloads {
            let archive = archive.as_str();
            apps.insert(archive, bytes.as_slice())
                .map_err(|err| alloc::format!("write app.db {archive}: {err}"))?;
            let sha256 = sha256_hex(bytes);
            hashes
                .insert(archive, sha256.as_str())
                .map_err(|err| alloc::format!("write app.db hash {archive}: {err}"))?;
            updated
                .insert(archive, "this boot")
                .map_err(|err| alloc::format!("write app.db update {archive}: {err}"))?;
        }
        for (archive, _) in downloads {
            for member in bundles.members(archive)? {
                if apps
                    .get(member.as_str())
                    .map_err(|err| alloc::format!("read app.db {member}: {err}"))?
                    .is_none()
                {
                    return Err(alloc::format!(
                        "{archive}: required Blueprint {member} is unavailable in AppDB"
                    ));
                }
            }
        }
    }
    write
        .commit()
        .map_err(|err| alloc::format!("commit AppDB install batch: {err}"))?;
    drop(guard);
    crate::shell3::service::refresh_appdb_names();
    Ok(())
}
