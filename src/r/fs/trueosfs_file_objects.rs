//! Native open-file objects. IDs are owner-scoped capabilities, never Unix FDs.
//! Every linked read and every read/modify/publication holds the root lease.
//! Namespace removals retain actual unlinked objects before publishing tombstones.
use super::*;
use crate::r::cabi_codes::*;

const MAX_HANDLES: usize = 2048;
const MAX_OWNER_HANDLES: usize = 512;
const MAX_TRANSFER: usize = 1024 * 1024;
const MAX_OBJECT_BYTES: u64 = 4 * 1024 * 1024 * 1024;
pub const ERR_WOULD_BLOCK: i32 = -18;
pub const ERR_PERMISSION: i32 = -19;
pub const ERR_UNSUPPORTED: i32 = -20;
const READ: u32 = 1;
const WRITE: u32 = 2;
const APPEND: u32 = 4;
const TRUNCATE: u32 = 8;
const CREATE: u32 = 16;
const CREATE_NEW: u32 = 32;

#[derive(Debug)]
pub struct Command { pub op: u32, pub flags: u32, pub object: u64, pub arg: u64, pub payload: Vec<u8> }
impl Command {
    pub fn decode(bytes: &[u8]) -> Result<Self, i32> {
        if bytes.len() < 24 || bytes.len() > 24 + MAX_TRANSFER { return Err(FS_ERR_BAD_PARAM); }
        let command = Self { op: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            flags: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            object: u64::from_le_bytes(bytes[8..16].try_into().unwrap()),
            arg: u64::from_le_bytes(bytes[16..24].try_into().unwrap()), payload: bytes[24..].to_vec() };
        match command.op {
            1 if command.object == 0 && command.arg == 0 && command.flags & !63 == 0 && !command.payload.is_empty() && command.payload.len() <= BLUEPRINT_ASYNC_FS_MAX_PATH => {
                let writable = command.flags & (WRITE | APPEND) != 0;
                if command.flags & READ == 0 && !writable || !writable && command.flags & (TRUNCATE | CREATE | CREATE_NEW) != 0 { return Err(FS_ERR_BAD_PARAM); }
                core::str::from_utf8(&command.payload).map_err(|_| FS_ERR_BAD_UTF8)?;
            }
            2 if command.object != 0 && command.flags == 0 && command.arg <= MAX_TRANSFER as u64 && command.payload.is_empty() => {}
            12 if command.object != 0 && command.flags <= MAX_TRANSFER as u32 && command.payload.is_empty() => {}
            13 if command.object != 0 && command.flags == 0 => {}
            3 if command.object != 0 && command.flags == 0 && command.arg == 0 => {}
            4 if command.object != 0 && command.flags <= 2 && command.payload.is_empty() => {}
            5 | 7 | 8 | 10 | 11 if command.object != 0 && command.flags == 0 && command.arg == 0 && command.payload.is_empty() => {}
            6 if command.object != 0 && command.flags == 0 && command.arg <= MAX_OBJECT_BYTES && command.payload.is_empty() => {}
            9 if command.object != 0 && matches!(command.flags, 1 | 2) && command.arg == 0 && command.payload.is_empty() => {}
            _ => return Err(FS_ERR_BAD_PARAM),
        }
        Ok(command)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct Identity { root: u32, epoch: u32, file: u64 }
#[derive(Clone)]
struct Description { owner: u32, identity: Identity, position: u64, flags: u32, refs: usize, lock: u32 }
struct NativeFile { disk: block::DeviceHandle, path: String, kind: NodeKind, orphan: Option<Vec<u8>>, revision: u64 }
struct Registry { next: u64, handles: BTreeMap<u64, u64>, descriptions: BTreeMap<u64, Description>, files: BTreeMap<Identity, NativeFile> }
impl Registry {
    const fn new() -> Self { Self { next: 1, handles: BTreeMap::new(), descriptions: BTreeMap::new(), files: BTreeMap::new() } }
    fn allocate(&mut self, owner: u32) -> Result<u64, i32> {
        if self.handles.len() >= MAX_HANDLES || self.handles.values().filter(|id| self.descriptions.get(id).is_some_and(|d| d.owner == owner)).count() >= MAX_OWNER_HANDLES { return Err(FS_ERR_NO_SPACE); }
        let next = self.next.checked_add(1).ok_or(FS_ERR_NO_SPACE)?;
        let id = self.next; self.next = next; Ok(id)
    }
    fn description(&self, owner: u32, handle: u64) -> Result<(u64, Description), i32> {
        let id = *self.handles.get(&handle).ok_or(FS_ERR_NOT_FOUND)?;
        let description = self.descriptions.get(&id).filter(|d| d.owner == owner).ok_or(FS_ERR_NOT_FOUND)?;
        Ok((id, description.clone()))
    }
    fn close(&mut self, owner: u32, handle: u64) -> Result<(), i32> {
        let (id, old) = self.description(owner, handle)?;
        self.handles.remove(&handle);
        let description = self.descriptions.get_mut(&id).unwrap();
        description.refs -= 1;
        if description.refs == 0 { self.descriptions.remove(&id); }
        if !self.descriptions.values().any(|d| d.identity == old.identity) { self.files.remove(&old.identity); }
        Ok(())
    }
}
static OBJECTS: Mutex<Registry> = Mutex::new(Registry::new());

pub fn release_owner(owner: u32) -> usize {
    let mut state = OBJECTS.lock();
    let handles: Vec<_> = state.handles.iter().filter_map(|(handle,id)| state.descriptions.get(id).filter(|d| d.owner == owner).map(|_| *handle)).collect();
    for handle in &handles { let _ = state.close(owner, *handle); }
    handles.len()
}
fn epoch(disk: block::DeviceHandle) -> Result<u32, i32> {
    ROOTS.lock().iter().find(|m| m.disk_id == disk.id() && m.index.is_some()).map(|m| m.seq).filter(|s| *s != 0).ok_or(FS_ERR_NOT_FOUND)
}
fn error(error: block::Error) -> i32 {
    match error { block::Error::NotReady => FS_ERR_NOT_FOUND, block::Error::InvalidParam | block::Error::OutOfBounds => FS_ERR_BAD_PARAM, block::Error::NotSupported => ERR_UNSUPPORTED, _ => FS_ERR_IO }
}
fn record_from_node(record: trueos_fs::NodeRecordRef) -> trueos_fs::FileRecordRef {
    trueos_fs::FileRecordRef { file_id: record.file_id, entry_lba: record.entry_lba, data_lba: record.data_lba, data_len: record.data_len, content_type: record.content_type, record_key: record.record_key }
}
async fn record_at(disk: block::DeviceHandle, params: &trueos_fs::FsParams, path: &str) -> Result<Option<trueos_fs::NodeRecordRef>, i32> {
    let indexed = indexed_removal_entry(disk.id(), path).map_err(error)?;
    match indexed { Some(entry) => trueos_fs::get_node_record_by_lba(&KernelBlockIo::new(disk), params, entry.entry_lba).await.map_err(|e| error(map_engine_err(e))).and_then(|r| r.ok_or(FS_ERR_IO).map(Some)), None => Ok(None) }
}
async fn bytes_at(disk: block::DeviceHandle, params: &trueos_fs::FsParams, record: trueos_fs::FileRecordRef) -> Result<Vec<u8>, i32> {
    if record.data_len > MAX_OBJECT_BYTES || record.data_len > isize::MAX as u64 { return Err(FS_ERR_TOO_LARGE); }
    trueos_fs::read_file_at_record(&KernelBlockIo::new(disk), params, &record).await.map_err(|e| error(map_engine_err(e)))?.ok_or(FS_ERR_IO)
}
async fn publish(disk: block::DeviceHandle, params: &trueos_fs::FsParams, path: &str, old: Option<trueos_fs::FileRecordRef>, bytes: &[u8]) -> Result<trueos_fs::FileRecordRef, i32> {
    let io = KernelBlockIo::new(disk);
    let metadata = old.map_or(trueos_fs::FileWriteMetadata { file_id: 0, content_type: ContentTypeId::BLOB, record_key: RecordKey::Ffa }, |r| trueos_fs::FileWriteMetadata { file_id: r.file_id, content_type: r.content_type, record_key: r.record_key });
    let mut stream = trueos_fs::begin_write_file_stream_prevalidated(&io, params, path, bytes.len() as u64, metadata).await.map_err(|e| error(map_engine_err(e)))?.ok_or(FS_ERR_NO_SPACE)?;
    for chunk in bytes.chunks(64 * 1024) { trueos_fs::write_file_stream_chunk(&io, &mut stream, chunk).await.map_err(|e| error(map_engine_err(e)))?; }
    let record = trueos_fs::write_stream_record_ref(&stream);
    trueos_fs::finish_write_file_stream(&io, params, stream).await.map_err(|e| error(map_engine_err(e)))?;
    record_successful_content_commit(record.content_type, false);
    bump_root_cache_gen(disk.id()); file_record_cache_invalidate_path(disk.id(), path); file_record_cache_insert(disk.id(), path, record);
    if !update_root_index_put(disk.id(), path, record) { invalidate_root_index(disk.id()); }
    super::super::file_watch::changed(disk.id(), path);
    Ok(record)
}

/// Capture live objects while the caller holds RootWriteLease and before it
/// removes their namespace entries. Nothing changes if the disk commit fails.
pub struct PreparedOrphans(Vec<(Identity, Vec<u8>, u64)>);
pub async fn prepare_orphans(disk: block::DeviceHandle, params: &trueos_fs::FsParams, path: &str, recursive: bool) -> Result<PreparedOrphans, block::Error> {
    let prefix = alloc::format!("{}/", path.trim_end_matches('/'));
    let candidates: Vec<_> = OBJECTS.lock().files.iter().filter(|(id,file)| id.root == disk.id().raw() && file.orphan.is_none() && (file.path == path || recursive && file.path.starts_with(&prefix))).map(|(id,file)| (*id,file.path.clone(),file.kind)).collect();
    let mut output = Vec::new();
    for (identity,path,kind) in candidates {
        let node = record_at(disk, params, &path).await.map_err(|_| block::Error::Io)?.ok_or(block::Error::NotReady)?;
        if node.file_id != identity.file || epoch(disk).ok() != Some(identity.epoch) { return Err(block::Error::NotReady); }
        let bytes = if kind == NodeKind::File { bytes_at(disk, params, record_from_node(node)).await.map_err(|_| block::Error::Io)? } else { Vec::new() };
        output.push((identity, bytes, node.entry_lba));
    }
    Ok(PreparedOrphans(output))
}
pub fn commit_orphans(prepared: PreparedOrphans) {
    let mut state = OBJECTS.lock();
    for (identity,bytes,revision) in prepared.0 {
        if let Some(file) = state.files.get_mut(&identity) { file.orphan = Some(bytes); file.revision = revision; }
    }
}
pub fn renamed(disk: block::DeviceHandle, source: &str, destination: &str) {
    let prefix = alloc::format!("{}/", source.trim_end_matches('/'));
    let mut state = OBJECTS.lock();
    for (id,file) in &mut state.files {
        if id.root != disk.id().raw() || file.orphan.is_some() { continue; }
        if file.path == source { file.path = String::from(destination); }
        else if let Some(suffix) = file.path.strip_prefix(&prefix) { file.path = alloc::format!("{}/{suffix}", destination.trim_end_matches('/')); }
    }
}

/// Opens are path-authorized by the async-FS broker before reaching this API.
pub async fn open(owner: u32, disk: block::DeviceHandle, path: &str, flags: u32) -> Result<Vec<u8>, i32> {
    let _activity = crate::disc::access::Activity::begin(disk).map_err(error)?;
    let placement = placement_for_io_async(disk).await.map_err(error)?.ok_or(FS_ERR_NOT_FOUND)?;
    let params = trueos_fs::FsParams { super_lba: placement.super_lba, data_lba: placement.data_lba, data_end_lba_exclusive: placement.data_end_lba_exclusive };
    let _lease = loop {
        ensure_index_async(disk, &placement).await.map_err(error)?;
        let lease = write_gate::RootWriteLease::acquire(disk.id()).await;
        if epoch(disk).is_ok() { break lease; }
        drop(lease);
    };
    // An open directory is useful for metadata and sync, never byte I/O.
    let node = if path.is_empty() { Some(trueos_fs::NodeRecordRef { file_id: 0, entry_lba: 0, data_lba: 0, data_len: 0, content_type: ContentTypeId::NONE, record_key: RecordKey::Ffa, kind: NodeKind::Directory }) }
        else { record_at(disk, &params, path).await? };
    if node.is_some() && flags & CREATE_NEW != 0 { return Err(FS_ERR_ALREADY_EXISTS); }
    if node.as_ref().is_some_and(|n| n.kind == NodeKind::Directory) && flags & (WRITE | APPEND | TRUNCATE | CREATE | CREATE_NEW) != 0 { return Err(FS_ERR_BAD_PARAM); }
    let node = match node {
        Some(node) if flags & TRUNCATE == 0 => node,
        Some(node) => {
            let record = publish(disk, &params, path, Some(record_from_node(node)), &[]).await?;
            trueos_fs::NodeRecordRef { file_id: record.file_id, entry_lba: record.entry_lba, data_lba: record.data_lba, data_len: record.data_len, content_type: record.content_type, record_key: record.record_key, kind: NodeKind::File }
        }
        None if flags & (CREATE | CREATE_NEW) != 0 => {
            if !validate_indexed_file_target(disk.id(), path, false).map_err(error)? { return Err(FS_ERR_IO); }
            let record = publish(disk, &params, path, None, &[]).await?;
            trueos_fs::NodeRecordRef { file_id: record.file_id, entry_lba: record.entry_lba, data_lba: record.data_lba, data_len: record.data_len, content_type: record.content_type, record_key: record.record_key, kind: NodeKind::File }
        }
        None => return Err(FS_ERR_NOT_FOUND),
    };
    let identity = Identity { root: disk.id().raw(), epoch: epoch(disk)?, file: node.file_id };
    let mut state = OBJECTS.lock();
    let handle = state.allocate(owner)?;
    state.files.entry(identity).or_insert_with(|| NativeFile { disk, path: String::from(path), kind: node.kind, orphan: None, revision: node.entry_lba });
    state.descriptions.insert(handle, Description { owner, identity, position: 0, flags, refs: 1, lock: 0 });
    state.handles.insert(handle, handle);
    Ok(handle.to_le_bytes().to_vec())
}

pub async fn operate(owner: u32, command: &Command) -> Result<Vec<u8>, i32> {
    let (description_id, description) = OBJECTS.lock().description(owner, command.object)?;
    if command.op == 7 {
        let mut state = OBJECTS.lock(); let handle = state.allocate(owner)?;
        state.descriptions.get_mut(&description_id).ok_or(FS_ERR_NOT_FOUND)?.refs += 1;
        state.handles.insert(handle, description_id); return Ok(handle.to_le_bytes().to_vec());
    }
    if command.op == 8 { OBJECTS.lock().close(owner, command.object)?; return Ok(Vec::new()); }
    if command.op == 9 || command.op == 10 {
        let mut state = OBJECTS.lock();
        if command.op == 9 && state.descriptions.iter().any(|(id,d)| *id != description_id && d.identity == description.identity && d.lock != 0 && (command.flags == 2 || d.lock == 2)) { return Err(ERR_WOULD_BLOCK); }
        state.descriptions.get_mut(&description_id).ok_or(FS_ERR_NOT_FOUND)?.lock = if command.op == 10 { 0 } else { command.flags };
        return Ok(Vec::new());
    }
    let (disk,kind) = { let state = OBJECTS.lock(); let file = state.files.get(&description.identity).ok_or(FS_ERR_NOT_FOUND)?; (file.disk,file.kind) };
    let _activity = crate::disc::access::Activity::begin(disk).map_err(error)?;
    let placement = placement_for_io_async(disk).await.map_err(error)?.ok_or(FS_ERR_NOT_FOUND)?;
    let params = trueos_fs::FsParams { super_lba: placement.super_lba, data_lba: placement.data_lba, data_end_lba_exclusive: placement.data_end_lba_exclusive };
    let _lease = loop {
        ensure_index_async(disk, &placement).await.map_err(error)?;
        let lease = write_gate::RootWriteLease::acquire(disk.id()).await;
        if epoch(disk).is_ok() { break lease; }
        drop(lease);
    };
    if epoch(disk)? != description.identity.epoch { return Err(FS_ERR_NOT_FOUND); }
    let (path,orphan) = { let state = OBJECTS.lock(); let file = state.files.get(&description.identity).ok_or(FS_ERR_NOT_FOUND)?; (file.path.clone(),file.orphan.is_some()) };
    let node = if orphan || path.is_empty() { None } else { record_at(disk, &params, &path).await? };
    if !orphan && !path.is_empty() && node.as_ref().is_none_or(|n| n.file_id != description.identity.file) { return Err(FS_ERR_NOT_FOUND); }
    let length = if orphan { OBJECTS.lock().files.get(&description.identity).unwrap().orphan.as_ref().unwrap().len() as u64 } else { node.as_ref().map_or(0, |n| n.data_len) };
    match command.op {
        5 => {
            let mut bytes = Vec::with_capacity(32);
            bytes.extend_from_slice(&(if kind == NodeKind::File { 1_u32 } else { 2 }).to_le_bytes()); bytes.extend_from_slice(&length.to_le_bytes()); bytes.extend_from_slice(&description.identity.file.to_le_bytes()); bytes.extend_from_slice(&description.identity.root.to_le_bytes());
            bytes.extend_from_slice(&node.map_or_else(|| OBJECTS.lock().files.get(&description.identity).unwrap().revision, |n| n.entry_lba).to_le_bytes()); Ok(bytes)
        }
        11 => Ok(Vec::new()), // Every successful write already completed storage publication.
        4 => {
            let base = match command.flags { 0 => 0, 1 => description.position, 2 => length, _ => return Err(FS_ERR_BAD_PARAM) };
            let position = if command.flags == 0 { command.arg } else { base.checked_add_signed(command.arg as i64).ok_or(FS_ERR_BAD_PARAM)? };
            OBJECTS.lock().descriptions.get_mut(&description_id).ok_or(FS_ERR_NOT_FOUND)?.position = position;
            Ok(position.to_le_bytes().to_vec())
        }
        2 | 12 => {
            let position = if command.op == 12 { command.arg } else { description.position };
            let capacity = if command.op == 12 { u64::from(command.flags) } else { command.arg };
            if description.flags & READ == 0 || kind != NodeKind::File { return Err(ERR_PERMISSION); }
            let count = capacity.min(length.saturating_sub(position)) as usize;
            let mut bytes = alloc::vec![0; count];
            let got = if count == 0 { 0 } else if orphan {
                let state = OBJECTS.lock(); let data = state.files.get(&description.identity).unwrap().orphan.as_ref().unwrap();
                bytes.copy_from_slice(&data[position as usize..position as usize + count]); count
            } else { trueos_fs::read_file_range_at(&KernelBlockIo::new(disk), &params, &record_from_node(node.unwrap()), position, &mut bytes).await.map_err(|e| error(map_engine_err(e)))?.ok_or(FS_ERR_IO)? };
            if got > count { return Err(FS_ERR_IO); }
            bytes.truncate(got); if command.op == 2 { OBJECTS.lock().descriptions.get_mut(&description_id).ok_or(FS_ERR_NOT_FOUND)?.position = position + got as u64; } Ok(bytes)
        }
        3 | 6 | 13 => {
            if description.flags & (WRITE | APPEND) == 0 || kind != NodeKind::File { return Err(ERR_PERMISSION); }
            if command.op != 6 && command.payload.is_empty() { return Ok(0_u64.to_le_bytes().to_vec()); }
            let position = if description.flags & APPEND != 0 { length } else if command.op == 13 { command.arg } else { description.position };
            let size = if command.op == 6 { command.arg } else { length.max(position.checked_add(command.payload.len() as u64).ok_or(FS_ERR_TOO_LARGE)?) };
            if size > MAX_OBJECT_BYTES || size > isize::MAX as u64 { return Err(FS_ERR_TOO_LARGE); }
            let mut bytes = if orphan { OBJECTS.lock().files.get(&description.identity).unwrap().orphan.as_ref().unwrap().clone() } else { bytes_at(disk, &params, record_from_node(node.unwrap())).await? };
            bytes.try_reserve((size as usize).saturating_sub(bytes.len())).map_err(|_| FS_ERR_NO_SPACE)?;
            bytes.resize(size as usize, 0);
            if command.op != 6 { bytes[position as usize..position as usize + command.payload.len()].copy_from_slice(&command.payload); }
            if orphan { OBJECTS.lock().files.get_mut(&description.identity).unwrap().orphan = Some(bytes); }
            else { publish(disk, &params, &path, node.map(record_from_node), &bytes).await?; }
            if command.op == 3 { OBJECTS.lock().descriptions.get_mut(&description_id).ok_or(FS_ERR_NOT_FOUND)?.position = position + command.payload.len() as u64; }
            if command.op != 6 { Ok((command.payload.len() as u64).to_le_bytes().to_vec()) } else { Ok(Vec::new()) }
        }
        _ => Err(FS_ERR_BAD_PARAM),
    }
}
