//! Sole owner of IDE source images. Editors hold project leases, never redb files.
use alloc::{collections::BTreeMap, string::{String, ToString}, vec::Vec};
use embassy_sync::mutex::Mutex as AsyncMutex;
use spin::Mutex;
use trueos_ide::{source_image::SourceImage, store_protocol::{self, Command, Reply, Request}};
use super::ide_workspace_service::{self as workspace, Identity};
use crate::r::{cabi_codes::*, fs::trueosfs as fs};

const MAX_DATABASES: usize = 8;
const MAX_IMAGE_BYTES: u64 = 256 * 1024 * 1024;
const VM_LIMIT: usize = crate::allcaps::hv::VM_ID_LIMIT;
static IMAGES: AsyncMutex<crate::wait::EmbassySpinRawMutex, BTreeMap<String, SourceImage>> = AsyncMutex::new(BTreeMap::new());
static REPLIES: [Mutex<Option<(Identity, Vec<u8>)>>; VM_LIMIT] = [const { Mutex::new(None) }; VM_LIMIT];
static BACKEND_LEASES: Mutex<Vec<(String, crate::r::fs::advisory_lock::Owner)>> = Mutex::new(Vec::new());

fn host<T>(f: impl FnOnce() -> T) -> T { crate::allocators::with_host_alloc_domain_strong(f) }
fn failure(code: &str, error: impl core::fmt::Display) -> Reply {
    Reply::Error { code: code.into(), message: error.to_string() }
}

// Coordinate with file-based redb users too. The kernel keeps this image lease
// while individual Edit VMs acquire and release their independent project leases.
fn reserve_backend(key: &str) -> Result<(), i32> {
    let mut leases = BACKEND_LEASES.lock();
    if leases.iter().any(|(path, _)| path == key) { return Ok(()); }
    if leases.len() >= MAX_DATABASES { return Err(FS_ERR_NO_SPACE); }
    let (disk, path) = key.strip_prefix("trueosfs:disc").and_then(|key| key.split_once('/')).ok_or(FS_ERR_BAD_PATH)?;
    let disk: u32 = disk.parse().map_err(|_| FS_ERR_BAD_PATH)?;
    let owner = crate::r::fs::advisory_lock::Owner { realm: u64::MAX, description: leases.len() as u64 };
    if !crate::r::fs::advisory_lock::LOCKS.lock().try_set(disk, path, owner, Some(true)) {
        return Err(FS_ERR_ALREADY_EXISTS);
    }
    leases.push((key.into(), owner));
    Ok(())
}

fn apply(image: &mut SourceImage, command: Command) -> Reply {
    let db = &mut image.database;
    let result = match command {
        Command::List {} => db.list_projects().map(|projects| Reply::Projects { projects }),
        Command::Get { project, revision } => db.get(&project, revision).map(|source| Reply::Source { source }),
        Command::GetConfig { project } => db.project_build_config(&project).map(|config| Reply::Config { config }),
        Command::Create { project, files } => {
            match db.get(&project, None) {
                Ok(Some(_)) => return failure("already_exists", "A project with that name already exists"),
                Err(error) => return failure("store_failed", error),
                Ok(None) => {},
            }
            db.import_local(&project, None, files).map(|result| Reply::Saved { revision: result.revision })
        }
        Command::Save { project, expected_revision_id, files } => db.save_files(&project, expected_revision_id, files)
            .map(|revision| Reply::Saved { revision }),
        Command::SetConfig { project, config } => db.set_project_build_config(&project, config).map(|_| Reply::Done {}),
    };
    result.unwrap_or_else(|error| failure("store_failed", error))
}

pub(crate) async fn write_for_owner(owner: u32, bytes: &[u8]) -> Result<(), i32> {
    if bytes.len() > store_protocol::MAX_FRAME_BYTES { return Err(FS_ERR_TOO_LARGE); }
    let identity = workspace::caller(owner)?;
    let request: Request = host(|| serde_json::from_slice(bytes)).map_err(|_| FS_ERR_BAD_PARAM)?;
    let resolved = host(|| crate::hv::with_guest_broker_context(identity.vm, || {
        crate::r::io::env::resolve_fs_path(&request.database, false)
    })).ok_or(FS_ERR_BAD_PATH)?;
    let key = host(|| crate::r::io::async_fs_cabi::canonical_file_key(&resolved))?;
    let mut images = IMAGES.lock().await;
    // Recheck after waiting: a VM or its project lease may have changed.
    if workspace::caller(owner)? != identity { return Err(FS_ERR_NOT_FOUND); }
    if REPLIES[identity.vm as usize].lock().as_ref().is_some_and(|(owner, _)| *owner == identity) {
        return Err(FS_ERR_ALREADY_EXISTS);
    }
    let writing = request.command.write_project().is_some();
    let reply = if let Some(project) = request.command.write_project()
        && !workspace::owns_project(identity, &key, project)? {
        host(|| failure("not_owner", "This Edit window does not own that project"))
    } else {
        execute(&mut images, &resolved, &key, request.command, writing).await
    };
    let encoded = host(|| serde_json::to_vec(&reply)).map_err(|_| FS_ERR_BAD_PARAM)?;
    if encoded.len() > store_protocol::MAX_FRAME_BYTES { return Err(FS_ERR_TOO_LARGE); }
    if workspace::caller(owner)? == identity {
        host(|| *REPLIES[identity.vm as usize].lock() = Some((identity, encoded)));
    }
    Ok(())
}

async fn execute(images: &mut BTreeMap<String, SourceImage>, resolved: &str, key: &str, command: Command, writing: bool) -> Reply {
    let (disk, path) = match crate::r::io::async_fs_cabi::selected_disk(resolved) {
        Ok(selected) => selected, Err(code) => return host(|| failure("store_failed", code)),
    };
    if !images.contains_key(key) {
        if images.len() >= MAX_DATABASES { return host(|| failure("store_busy", "Too many open source images")); }
        if let Err(code) = host(|| reserve_backend(key)) {
            return host(|| failure("store_busy", alloc::format!("The source image is open outside the source service ({code})")));
        }
        let bytes = match fs::file_info_async(disk, path).await {
            Ok(Some(info)) if info.data_len > MAX_IMAGE_BYTES => return host(|| failure("store_failed", "Source image is too large")),
            Ok(Some(_)) => match fs::file_out_async(disk, path).await {
                Ok(Some(bytes)) => bytes,
                _ => return host(|| failure("store_failed", "Cannot read source image")),
            },
            Ok(None) => Vec::new(),
            Err(error) => return host(|| failure("store_failed", alloc::format!("Source image read failed: {error:?}"))),
        };
        match host(|| SourceImage::open(&bytes)) {
            Ok(image) => { host(|| images.insert(key.into(), image)); },
            Err(error) => return host(|| failure("store_failed", error)),
        }
    }
    if !writing { return host(|| apply(images.get_mut(key).unwrap(), command)); }
    // No project edit is acknowledged until the complete image is durable.
    // On failure discard the candidate; the next request reloads disk state.
    let mut image = host(|| images.remove(key).unwrap());
    let reply = host(|| apply(&mut image, command));
    if matches!(reply, Reply::Error { .. }) {
        host(|| images.insert(key.into(), image));
        return reply;
    }
    let bytes = match host(|| image.into_bytes()) {
        Ok(bytes) if bytes.len() as u64 <= MAX_IMAGE_BYTES => bytes,
        _ => return host(|| failure("store_failed", "Cannot finish source image")),
    };
    if let Some((parent, _)) = path.rsplit_once('/')
        && !matches!(fs::dir_create_all_async(disk, parent).await, Ok(true)) {
        return host(|| failure("store_failed", "Cannot create source image directory"));
    }
    if !matches!(fs::file_in_async(disk, path, &bytes).await, Ok(true)) {
        return host(|| failure("store_failed", "Cannot persist source image"));
    }
    match host(|| SourceImage::open(&bytes)) {
        Ok(image) => { host(|| images.insert(key.into(), image)); reply },
        Err(error) => host(|| failure("store_failed", error)),
    }
}

/// Async-FS reserves capacity before this consuming read.
pub(crate) fn read_for_owner(owner: u32) -> Result<Vec<u8>, i32> {
    let identity = workspace::caller(owner)?;
    let slot = REPLIES[identity.vm as usize].lock().take();
    match slot {
        Some((owner, bytes)) if owner == identity => Ok(bytes),
        _ => Err(FS_ERR_NOT_FOUND),
    }
}

pub(crate) fn release_vm(vm: u8, run: u64) {
    if let Some(slot) = REPLIES.get(vm as usize) {
        let mut slot = slot.lock();
        if slot.as_ref().is_some_and(|(owner, _)| owner.vm == vm && owner.run == run) {
            slot.take();
        }
    }
}
