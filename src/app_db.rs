//! Volatile application catalog and Blueprint byte store.
//!
//! `app.db` is deliberately RAM-only. The kernel opens its compressed host-built
//! redb image; online downloads add or replace rows until reboot.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use redb::{Database, ReadableDatabase, ReadableTable, ReadableTableMetadata, StorageBackend, TableDefinition};
use sha2::{Digest, Sha256};
use spin::Mutex;

const APP_DB_IMAGE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app.db.lz4"));
include!(concat!(env!("OUT_DIR"), "/appdb-seed.rs"));
const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const HASHES: TableDefinition<&str, &str> = TableDefinition::new("hashes");
const UPDATED: TableDefinition<&str, &str> = TableDefinition::new("updated");

static APP_DB: Mutex<Option<Database>> = Mutex::new(None);

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

/// Open the host-built app.db image in writable RAM before launch requests.
pub(crate) fn init_bsp() -> Result<usize, String> {
    let image = crate::r::lz4::decompress_frame_cpu(APP_DB_IMAGE, APP_DB_IMAGE_LEN)
        .map_err(|err| alloc::format!("decompress app.db image: {err:?}"))?;
    if image.len() != APP_DB_IMAGE_LEN {
        return Err(String::from("app.db image length mismatch"));
    }
    let backend = redb::backends::InMemoryBackend::new();
    backend.set_len(image.len() as u64)
        .map_err(|err| alloc::format!("allocate app.db image: {err}"))?;
    backend.write(0, &image)
        .map_err(|err| alloc::format!("load app.db image: {err}"))?;
    drop(image);
    let database = Database::builder()
        .create_with_backend(backend)
        .map_err(|err| alloc::format!("open app.db image: {err}"))?;
    let count = {
        let read = database.begin_read()
            .map_err(|err| alloc::format!("begin app.db seed read: {err}"))?;
        let apps = read.open_table(APPS)
            .map_err(|err| alloc::format!("open seeded apps: {err}"))?;
        let hashes = read.open_table(HASHES)
            .map_err(|err| alloc::format!("open seeded hashes: {err}"))?;
        let updated = read.open_table(UPDATED)
            .map_err(|err| alloc::format!("open seeded dates: {err}"))?;
        let count = apps.len().map_err(|err| alloc::format!("count seeded apps: {err}"))?;
        if count != APP_DB_EXPECTED_APPS as u64
            || hashes.len().map_err(|err| alloc::format!("count seeded hashes: {err}"))? != count
            || updated.len().map_err(|err| alloc::format!("count seeded dates: {err}"))? != count {
            return Err(String::from("app.db image does not contain the complete built-in set"));
        }
        count as usize
    };
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

/// Add or replace a downloaded Blueprint in volatile `app.db`.
pub(crate) fn insert_download(archive: &str, bytes: &[u8]) -> Result<(), String> {
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
        apps.insert(archive, bytes)
            .map_err(|err| alloc::format!("write app.db {archive}: {err}"))?;
        let sha256 = sha256_hex(bytes);
        hashes
            .insert(archive, sha256.as_str())
            .map_err(|err| alloc::format!("write app.db hash {archive}: {err}"))?;
        updated
            .insert(archive, "this boot")
            .map_err(|err| alloc::format!("write app.db update {archive}: {err}"))?;
    }
    write
        .commit()
        .map_err(|err| alloc::format!("commit app.db {archive}: {err}"))?;
    drop(guard);
    crate::shell3::service::refresh_appdb_names();
    Ok(())
}
