//! Volatile application catalog and Blueprint byte store.
//!
//! `app.db` is deliberately RAM-only. The kernel seeds it from build-time
//! embedded Blueprints and online downloads add or replace rows until reboot.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use sha2::{Digest, Sha256};
use spin::Mutex;

const BUILDINS_BUNDLE: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/app-buildins.bin"));
const BUILDINS_MAGIC: &[u8; 8] = b"TAPPDB2\0";
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

fn buildins() -> Result<(u64, Vec<(&'static str, &'static [u8])>), &'static str> {
    if BUILDINS_BUNDLE.get(..BUILDINS_MAGIC.len()) != Some(BUILDINS_MAGIC) {
        return Err("invalid build-ins bundle magic");
    }
    let mut cursor = BUILDINS_MAGIC.len();
    let build_timestamp = read_u64(BUILDINS_BUNDLE, &mut cursor)?;
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
    Ok((build_timestamp, entries))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut text = String::with_capacity(digest.len().saturating_mul(2));
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn build_timestamp_text(timestamp: u64) -> String {
    const SECONDS_PER_DAY: u64 = 86_400;

    let days = i64::try_from(timestamp / SECONDS_PER_DAY).unwrap_or(i64::MAX);
    let seconds = timestamp % SECONDS_PER_DAY;
    let hour = seconds / 3_600;
    let minute = (seconds % 3_600) / 60;

    // Howard Hinnant's civil-from-days conversion, with day zero at the Unix
    // epoch. Keeping it here avoids a wall-clock dependency in the kernel.
    let z = days.saturating_add(719_468);
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }

    alloc::format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}Z")
}

/// Create volatile `app.db` and seed every byte-embedded Blueprint.
///
/// This runs on the BSP after the heap is installed and before Shell2 tasks
/// can accept app-launch requests.
pub(crate) fn init_bsp() -> Result<usize, String> {
    let (build_timestamp, buildins) = buildins().map_err(String::from)?;
    let updated = build_timestamp_text(build_timestamp);
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
            let mut hashes = write
                .open_table(HASHES)
                .map_err(|err| alloc::format!("open app.db hashes table: {err}"))?;
            let mut updated_table = write
                .open_table(UPDATED)
                .map_err(|err| alloc::format!("open app.db updated table: {err}"))?;
            for (archive, bytes) in &buildins {
                apps.insert(*archive, *bytes)
                    .map_err(|err| alloc::format!("seed app.db {archive}: {err}"))?;
                let sha256 = sha256_hex(bytes);
                hashes
                    .insert(*archive, sha256.as_str())
                    .map_err(|err| alloc::format!("seed app.db hash {archive}: {err}"))?;
                updated_table
                    .insert(*archive, updated.as_str())
                    .map_err(|err| alloc::format!("seed app.db update {archive}: {err}"))?;
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
        .map_err(|err| alloc::format!("commit app.db {archive}: {err}"))
}
