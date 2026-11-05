//! Only remote SVG conversions are persisted here, as validated encoded PNG.
//! redb runs in RAM; TRUEOSFS owns the asynchronous image read/commit. Admission
//! captures the disk/path before dispatch, so workers never resolve ambient paths.
use alloc::{vec, vec::Vec};
use embassy_sync::mutex::Mutex;
use trueos_redb::{
    ImageDatabase,
    redb::{ReadableDatabase, ReadableTable, ReadableTableMetadata, TableDefinition},
};

use super::hull_image_policy as policy;
use crate::{disc::block::DeviceHandle, r::fs::trueosfs as fs};

const PNGS: TableDefinition<&str, &[u8]> = TableDefinition::new("svg_png_v1");
const MAX_DATABASE_BYTES: usize = 64 * 1024 * 1024;
const MAX_PAYLOAD_BYTES: usize = 32 * 1024 * 1024;
const MAX_ENTRIES: u64 = 256;
const MAX_KEY_BYTES: usize = 8192;
const MAX_PATH_BYTES: usize = 1024;
const READ_CHUNK_BYTES: usize = 64 * 1024;
const ERR_IO: i32 = -6;

// A single serializer also prevents two windows from overwriting each other's
// fresh images. No redb transaction or spinlock is held across filesystem work.
static SERIAL: Mutex<crate::wait::EmbassySpinRawMutex, ()> = Mutex::new(());

fn host<T>(f: impl FnOnce() -> T) -> T {
    crate::allocators::with_host_alloc_domain_strong(f)
}

fn valid_path_key(path: &str, key: &str) -> bool {
    !path.is_empty()
        && path.len() <= MAX_PATH_BYTES
        && !path.contains('\0')
        && !key.is_empty()
        && key.len() <= MAX_KEY_BYTES
}

fn valid_png(bytes: &[u8]) -> bool {
    policy::preflight(bytes).is_ok_and(|info| info.format == 2)
}

async fn image(disk: DeviceHandle, path: &str) -> Result<Option<Vec<u8>>, i32> {
    let Some(handle) = fs::file_read_open_async(disk, path)
        .await
        .map_err(|_| ERR_IO)?
    else {
        return Ok(None);
    };
    let len = handle.data_len();
    if len == 0 || len > MAX_DATABASE_BYTES as u64 {
        return Ok(None);
    }
    // The handle pins the inspected file record. A replacement between length
    // inspection and reading cannot turn this into an unbounded file read.
    let mut bytes = host(|| vec![0; len as usize]);
    let mut offset = 0;
    while offset < bytes.len() {
        let end = offset.saturating_add(READ_CHUNK_BYTES).min(bytes.len());
        let count =
            fs::file_read_handle_range_async(handle, offset as u64, &mut bytes[offset..end])
                .await
                .map_err(|_| ERR_IO)?
                .ok_or(ERR_IO)?;
        if count == 0 || count > end - offset {
            return Err(ERR_IO);
        }
        offset += count;
    }
    Ok(Some(bytes))
}

pub(super) async fn read(disk: DeviceHandle, path: &str, key: &str) -> Option<Vec<u8>> {
    if !valid_path_key(path, key) {
        return None;
    }
    let _serial = SERIAL.lock().await;
    let bytes = image(disk, path).await.ok()??;
    host(|| {
        let store = ImageDatabase::open(&bytes).ok()?;
        drop(bytes);
        let read = store.database().begin_read().ok()?;
        let table = read.open_table(PNGS).ok()?;
        let value = table.get(key).ok()??;
        valid_png(value.value()).then(|| value.value().to_vec())
    })
}

fn reset_needed(store: &ImageDatabase, key: &str, bytes: usize) -> Result<bool, i32> {
    let read = store.database().begin_read().map_err(|_| ERR_IO)?;
    let table = match read.open_table(PNGS) {
        Ok(table) => table,
        Err(trueos_redb::redb::TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(_) => return Err(ERR_IO),
    };
    let count = table.len().map_err(|_| ERR_IO)?;
    let previous = table.get(key).map_err(|_| ERR_IO)?.map(|v| v.value().len());
    if count > MAX_ENTRIES || (count == MAX_ENTRIES && previous.is_none()) {
        return Ok(true);
    }
    let mut payload = bytes;
    for row in table.iter().map_err(|_| ERR_IO)? {
        let (stored_key, value) = row.map_err(|_| ERR_IO)?;
        if stored_key.value() != key {
            payload = payload.saturating_add(value.value().len());
            if payload > MAX_PAYLOAD_BYTES {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn inserted_image(store: ImageDatabase, key: &str, png: &[u8]) -> Result<Vec<u8>, i32> {
    let write = store.database().begin_write().map_err(|_| ERR_IO)?;
    {
        write
            .open_table(PNGS)
            .map_err(|_| ERR_IO)?
            .insert(key, png)
            .map_err(|_| ERR_IO)?;
    }
    write.commit().map_err(|_| ERR_IO)?;
    // Consuming the store closes redb and finalizes its headers before I/O.
    store.into_image().map_err(|_| ERR_IO)
}

pub(super) async fn write(
    disk: DeviceHandle,
    path: &str,
    key: &str,
    png: &[u8],
) -> Result<(), i32> {
    if !valid_path_key(path, key) || !valid_png(png) {
        return Err(policy::ERR_INVALID);
    }
    let _serial = SERIAL.lock().await;
    let bytes = image(disk, path).await?;
    // This file is a disposable conversion cache: a corrupt old image is a
    // miss and may be replaced only after a new PNG has passed validation.
    let bytes = host(|| {
        let mut store = bytes
            .as_deref()
            .and_then(|b| ImageDatabase::open(b).ok())
            .map_or_else(|| ImageDatabase::open(&[]), Ok)
            .map_err(|_| ERR_IO)?;
        drop(bytes);
        if reset_needed(&store, key, png.len()).unwrap_or(true) {
            store = ImageDatabase::open(&[]).map_err(|_| ERR_IO)?;
        }
        let mut bytes = inserted_image(store, key, png)?;
        if bytes.len() > MAX_DATABASE_BYTES {
            bytes = inserted_image(ImageDatabase::open(&[]).map_err(|_| ERR_IO)?, key, png)?;
        }
        if bytes.len() > MAX_DATABASE_BYTES {
            return Err(policy::ERR_TOO_LARGE);
        }
        Ok(bytes)
    })?;
    if let Some((parent, _)) = path.rsplit_once('/')
        && !parent.is_empty()
        && !matches!(fs::dir_create_all_async(disk, parent).await, Ok(true))
    {
        return Err(ERR_IO);
    }
    if matches!(fs::file_in_async(disk, path, &bytes).await, Ok(true)) {
        Ok(())
    } else {
        Err(ERR_IO)
    }
}
