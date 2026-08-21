//! Volatile application catalog and Blueprint byte store.
//!
//! `app.db` is deliberately RAM-only. The kernel seeds it from build-time
//! embedded Blueprints and online downloads add or replace rows until reboot.

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use spin::Mutex;

const BUILDINS_BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app-buildins.bin"));
const BUILDINS_MAGIC: &[u8; 8] = b"TAPPDB1\0";
const APPS: TableDefinition<&str, &[u8]> = TableDefinition::new("apps");
const SOURCES: TableDefinition<&str, &str> = TableDefinition::new("sources");

static APP_DB: Mutex<Option<Database>> = Mutex::new(None);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AppEntry {
    pub archive: String,
    pub source: String,
}

fn read_u16(bytes: &[u8], cursor: &mut usize) -> Result<u16, &'static str> {
    let end = cursor.checked_add(2).ok_or("bundle cursor overflow")?;
    let raw: [u8; 2] = bytes
        .get(*cursor..end)
        .ok_or("truncated build-ins bundle")?
        .try_into()
        .map_err(|_| "invalid build-ins u16")?;
    *cursor = end;
    Ok(u16::from_le_bytes(raw))
}

fn read_u32(bytes: &[u8], cursor: &mut usize) -> Result<u32, &'static str> {
    let end = cursor.checked_add(4).ok_or("bundle cursor overflow")?;
    let raw: [u8; 4] = bytes
        .get(*cursor..end)
        .ok_or("truncated build-ins bundle")?
        .try_into()
        .map_err(|_| "invalid build-ins u32")?;
    *cursor = end;
    Ok(u32::from_le_bytes(raw))
}

fn read_u64(bytes: &[u8], cursor: &mut usize) -> Result<u64, &'static str> {
    let end = cursor.checked_add(8).ok_or("bundle cursor overflow")?;
    let raw: [u8; 8] = bytes
        .get(*cursor..end)
        .ok_or("truncated build-ins bundle")?
        .try_into()
        .map_err(|_| "invalid build-ins u64")?;
    *cursor = end;
    Ok(u64::from_le_bytes(raw))
}

fn buildins() -> Result<Vec<(&'static str, &'static [u8])>, &'static str> {
    if BUILDINS_BUNDLE.get(..BUILDINS_MAGIC.len()) != Some(BUILDINS_MAGIC) {
        return Err("invalid build-ins bundle magic");
    }
    let mut cursor = BUILDINS_MAGIC.len();
    let count = usize::try_from(read_u32(BUILDINS_BUNDLE, &mut cursor)?)
        .map_err(|_| "build-ins count overflow")?;
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let name_len = usize::from(read_u16(BUILDINS_BUNDLE, &mut cursor)?);
        let data_len = usize::try_from(read_u64(BUILDINS_BUNDLE, &mut cursor)?)
            .map_err(|_| "build-in size overflow")?;
        let name_end = cursor
            .checked_add(name_len)
            .ok_or("build-in name overflow")?;
        let name = core::str::from_utf8(
            BUILDINS_BUNDLE
                .get(cursor..name_end)
                .ok_or("truncated build-in name")?,
        )
        .map_err(|_| "build-in name is not UTF-8")?;
        cursor = name_end;
        let data_end = cursor
            .checked_add(data_len)
            .ok_or("build-in data overflow")?;
        let data = BUILDINS_BUNDLE
            .get(cursor..data_end)
            .ok_or("truncated build-in data")?;
        cursor = data_end;
        entries.push((name, data));
    }
    if cursor != BUILDINS_BUNDLE.len() {
        return Err("trailing bytes in build-ins bundle");
    }
    Ok(entries)
}

/// Create volatile `app.db` and seed every byte-embedded Blueprint.
///
/// This runs on the BSP after the heap is installed and before Shell2 tasks
/// can accept app-launch requests.
pub(crate) fn init_bsp() -> Result<usize, String> {
    let buildins = buildins().map_err(String::from)?;
    let database = Database::builder()
        .create_with_backend(redb::backends::InMemoryBackend::new())
        .map_err(|err| alloc::format!("create app.db: {err}"))?;
    {
        let write = database
            .begin_write()
            .map_err(|err| alloc::format!("begin app.db seed transaction: {err}"))?;
        {
            let mut apps = write
                .open_table(APPS)
                .map_err(|err| alloc::format!("open app.db apps table: {err}"))?;
            let mut sources = write
                .open_table(SOURCES)
                .map_err(|err| alloc::format!("open app.db sources table: {err}"))?;
            for (archive, bytes) in &buildins {
                apps.insert(*archive, *bytes)
                    .map_err(|err| alloc::format!("seed app.db {archive}: {err}"))?;
                sources
                    .insert(*archive, "built-in")
                    .map_err(|err| alloc::format!("seed app.db source {archive}: {err}"))?;
            }
        }
        write
            .commit()
            .map_err(|err| alloc::format!("commit app.db seed: {err}"))?;
    }
    let count = buildins.len();
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
    let sources = read
        .open_table(SOURCES)
        .map_err(|err| alloc::format!("open app.db sources table: {err}"))?;
    let mut entries = Vec::new();
    for row in apps
        .iter()
        .map_err(|err| alloc::format!("iterate app.db: {err}"))?
    {
        let (archive, _) = row.map_err(|err| alloc::format!("read app.db row: {err}"))?;
        let archive = archive.value();
        let source = sources
            .get(archive)
            .map_err(|err| alloc::format!("read app.db source: {err}"))?
            .map(|value| value.value().to_string())
            .unwrap_or_else(|| String::from("memory"));
        entries.push(AppEntry {
            archive: archive.to_string(),
            source,
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
        let mut sources = write
            .open_table(SOURCES)
            .map_err(|err| alloc::format!("open app.db sources table: {err}"))?;
        apps.insert(archive, bytes)
            .map_err(|err| alloc::format!("write app.db {archive}: {err}"))?;
        sources
            .insert(archive, "downloaded")
            .map_err(|err| alloc::format!("write app.db source {archive}: {err}"))?;
    }
    write
        .commit()
        .map_err(|err| alloc::format!("commit app.db {archive}: {err}"))
}
