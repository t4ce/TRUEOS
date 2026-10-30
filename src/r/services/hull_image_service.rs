//! Bounded, owner-scoped URL -> cache -> decode -> UI4 raster service.
//!
//! Two workers may fetch/cache; one may decode/resize. The ABI admits work and
//! polls tickets only. All encoded/decoded bytes remain in the kernel. Cache
//! writes happen after ready publication and cannot fail an already drawn image.
use alloc::{collections::VecDeque, string::String, sync::Arc, vec::Vec};
use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};
use v::bp_abi::TrueosUi4HullImageInfoV1 as ImageInfo;

use super::hull_image_policy::{self as policy, ERR_INVALID, ERR_TOO_LARGE, MAX_ENCODED_BYTES};
use crate::r::fs::request_broker as fs;
use crate::ui4::blueprint_ui4::hull_image_api as ui;

const ERR_NOT_FOUND: i32 = -3;
const ERR_FAILED: i32 = -6;
const ERR_BUSY: i32 = -7;
const MAX_WINDOWS: usize = 16;
const MAX_OPERATIONS: usize = 64;
const MAX_PER_WINDOW: usize = 8;
const WINDOW_BYTES: usize = 32 * 1024 * 1024;
const GLOBAL_BYTES: usize = 64 * 1024 * 1024;
const FETCH_TIMEOUT_MS: u32 = 15_000;
const MAX_CACHE_ROOT_BYTES: usize = 512;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static STARTED: Mutex<bool> = Mutex::new(false);
static DECODING: AtomicBool = AtomicBool::new(false);
static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

struct Life {
    owner: u32,
    window: u32,
    incarnation: u64,
    run: u64,
    generation: u64,
    id: u32,
    cancelled: AtomicBool,
    fetch: crate::r::net::https::MediaFetchCancellation,
}
impl Life {
    fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.fetch.cancel();
    }
    fn live(&self) -> bool {
        !self.cancelled.load(Ordering::Acquire)
            && crate::hv::vm_run_generation(self.owner as u8) == Some(self.run)
            && match ui::authenticate_window(self.owner, self.window) {
                Ok(incarnation) => incarnation == self.incarnation,
                Err(ERR_BUSY) => true, // Publication revalidates after a busy draw.
                Err(_) => false,
            }
    }
}

struct Operation {
    life: Arc<Life>,
    url: String,
    requested_extent: (u32, u32),
    reserved_bytes: usize,
    info: ImageInfo,
}
struct Window {
    owner: u32,
    id: u32,
    incarnation: u64,
    generation: u64,
}
#[derive(Clone)]
struct Cache {
    disk: crate::disc::block::DeviceHandle,
    directory: String,
}
struct Job {
    life: Arc<Life>,
    url: String,
    width: u32,
    height: u32,
    cache: Option<Cache>,
}
struct Registry {
    operations: Vec<Operation>,
    windows: Vec<Window>,
    queue: VecDeque<Job>,
}
impl Registry {
    const fn new() -> Self {
        Self {
            operations: Vec::new(),
            windows: Vec::new(),
            queue: VecDeque::new(),
        }
    }
    fn remove_matching(&mut self, matches: impl Fn(&Life) -> bool) {
        self.operations.retain(|op| {
            if !matches(&op.life) {
                return true;
            }
            op.life.cancel();
            if op.info.sprite_id != 0 {
                let _ = ui::release_rgba(
                    op.life.owner,
                    op.life.window,
                    op.life.incarnation,
                    op.info.sprite_id,
                );
            }
            false
        });
        self.queue
            .retain(|job| !job.life.cancelled.load(Ordering::Acquire));
    }
}

fn ensure_workers() -> Result<(), i32> {
    let mut started = STARTED.lock();
    if *started {
        return Ok(());
    }
    let workers = crate::workers::pick_background_spawners_with_slots(2);
    if workers.is_empty() {
        return Err(ERR_BUSY);
    }
    let first = worker_task().map_err(|_| ERR_BUSY)?;
    workers[0].2.spawn(first);
    // One worker is sufficient if task-pool admission ever fails. It keeps the
    // same memory budget and serialized decode behavior.
    if let Ok(second) = worker_task() {
        workers[1 % workers.len()].2.spawn(second);
    }
    *started = true;
    Ok(())
}

/// Resolve and capture the calling process's filesystem scope at admission,
/// never from a background worker's environment or from a later VM incarnation.
fn cache_scope(root: &str, host: &str, url: &str) -> Result<Option<Cache>, i32> {
    if root.len() > MAX_CACHE_ROOT_BYTES || !policy::valid_page_host(host) {
        return Err(ERR_INVALID);
    }
    crate::r::path::FsPath::parse(root, false).map_err(|_| ERR_INVALID)?;
    let relative = alloc::format!(
        "{}/{host}/images/{}",
        root.trim_end_matches('/'),
        policy::digest(url.as_bytes())
    );
    let resolved = crate::r::io::env::resolve_fs_path(&relative, false).ok_or(ERR_INVALID)?;
    let Ok((disk, directory)) = crate::r::io::async_fs_cabi::selected_disk(&resolved) else {
        // A missing/unavailable disk only disables persistence.
        return Ok(None);
    };
    Ok(Some(Cache {
        disk,
        directory: String::from(directory),
    }))
}

pub(crate) fn request(
    owner: u32,
    window: u32,
    url: &str,
    cache_root: &str,
    page_host: &str,
    width: u32,
    height: u32,
    generation: u64,
) -> i32 {
    crate::allocators::with_host_alloc_domain_strong(|| {
        request_inner(
            owner, window, url, cache_root, page_host, width, height, generation,
        )
        .unwrap_or_else(|error| error)
    })
}

fn request_inner(
    owner: u32,
    window: u32,
    url: &str,
    cache_root: &str,
    page_host: &str,
    width: u32,
    height: u32,
    generation: u64,
) -> Result<i32, i32> {
    let incarnation = ui::authenticate_window(owner, window)?;
    let run = crate::hv::vm_run_generation(owner as u8).ok_or(ERR_NOT_FOUND)?;
    let url = policy::canonical_url(url)?;
    let (width, height, reserved_bytes) = policy::target_extent(width, height)?;
    let cache = cache_scope(cache_root, page_host, &url)?;
    ensure_workers()?;
    let mut registry = REGISTRY.lock();
    if let Some(current) = registry
        .windows
        .iter()
        .find(|entry| entry.owner == owner && entry.id == window)
    {
        if current.incarnation != incarnation || current.generation != generation {
            return Err(ERR_INVALID);
        }
    } else {
        if registry.windows.len() >= MAX_WINDOWS {
            return Err(ERR_BUSY);
        }
        registry.windows.push(Window {
            owner,
            id: window,
            incarnation,
            generation,
        });
    }
    if let Some(op) = registry.operations.iter().find(|op| {
        op.life.owner == owner
            && op.life.window == window
            && op.life.incarnation == incarnation
            && op.life.generation == generation
            && op.url == url
            && op.requested_extent == (width, height)
    }) {
        return Ok(op.life.id as i32);
    }
    let window_ops = registry
        .operations
        .iter()
        .filter(|op| op.life.owner == owner && op.life.window == window);
    if registry.operations.len() >= MAX_OPERATIONS
        || window_ops.clone().count() >= MAX_PER_WINDOW
        || window_ops.map(|op| op.reserved_bytes).sum::<usize>() + reserved_bytes > WINDOW_BYTES
        || registry
            .operations
            .iter()
            .map(|op| op.reserved_bytes)
            .sum::<usize>()
            + reserved_bytes
            > GLOBAL_BYTES
    {
        return Err(ERR_BUSY);
    }
    let id = (0..=MAX_OPERATIONS)
        .find_map(|_| {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) & i32::MAX as u32;
            (id != 0 && registry.operations.iter().all(|op| op.life.id != id)).then_some(id)
        })
        .ok_or(ERR_BUSY)?;
    let life = Arc::new(Life {
        owner,
        window,
        incarnation,
        run,
        generation,
        id,
        cancelled: AtomicBool::new(false),
        fetch: crate::r::net::https::MediaFetchCancellation::new(),
    });
    registry.operations.push(Operation {
        life: life.clone(),
        url: url.clone(),
        requested_extent: (width, height),
        reserved_bytes,
        info: ImageInfo {
            state: 0,
            sprite_id: 0,
            width,
            height,
            generation,
            error: 0,
            reserved: 0,
        },
    });
    registry.queue.push_back(Job {
        life,
        url,
        width,
        height,
        cache,
    });
    Ok(id as i32)
}

pub(crate) fn status(owner: u32, window: u32, id: u32) -> Result<ImageInfo, i32> {
    let incarnation = ui::authenticate_window(owner, window)?;
    // Polling must never wait behind a publication/retirement on another core.
    let registry = REGISTRY.try_lock().ok_or(ERR_BUSY)?;
    let op = registry
        .operations
        .iter()
        .find(|op| {
            op.life.owner == owner
                && op.life.window == window
                && op.life.id == id
                && op.life.incarnation == incarnation
        })
        .ok_or(ERR_NOT_FOUND)?;
    if crate::hv::vm_run_generation(owner as u8) != Some(op.life.run) {
        return Err(ERR_NOT_FOUND);
    }
    Ok(op.info)
}

pub(crate) fn release(owner: u32, window: u32, id: u32) -> i32 {
    let incarnation = match ui::authenticate_window(owner, window) {
        Ok(incarnation) => incarnation,
        Err(error) => return error,
    };
    let mut registry = REGISTRY.lock();
    if !registry.operations.iter().any(|op| {
        op.life.owner == owner
            && op.life.window == window
            && op.life.incarnation == incarnation
            && op.life.id == id
    }) {
        return ERR_NOT_FOUND;
    }
    registry.remove_matching(|life| life.owner == owner && life.window == window && life.id == id);
    0
}

pub(crate) fn reset(owner: u32, window: u32, generation: u64) -> i32 {
    let incarnation = match ui::authenticate_window(owner, window) {
        Ok(value) => value,
        Err(error) => return error,
    };
    let mut registry = REGISTRY.lock();
    registry.remove_matching(|life| life.owner == owner && life.window == window);
    if let Some(current) = registry
        .windows
        .iter_mut()
        .find(|entry| entry.owner == owner && entry.id == window)
    {
        current.incarnation = incarnation;
        current.generation = generation;
    } else {
        if registry.windows.len() >= MAX_WINDOWS {
            return ERR_BUSY;
        }
        registry.windows.push(Window {
            owner,
            id: window,
            incarnation,
            generation,
        });
    }
    0
}

pub(crate) fn release_window(owner: u32, window: u32) {
    let mut registry = REGISTRY.lock();
    registry.remove_matching(|life| life.owner == owner && life.window == window);
    registry
        .windows
        .retain(|entry| entry.owner != owner || entry.id != window);
}

pub(crate) fn release_owner(owner: u32) {
    let mut registry = REGISTRY.lock();
    registry.remove_matching(|life| life.owner == owner);
    registry.windows.retain(|entry| entry.owner != owner);
}

fn complete_error(life: &Life, error: i32) {
    let mut registry = REGISTRY.lock();
    if let Some(op) = registry
        .operations
        .iter_mut()
        .find(|op| op.life.id == life.id && Arc::as_ptr(&op.life) == life as *const Life)
    {
        op.info.state = 2;
        op.info.error = error;
        op.reserved_bytes = 0;
    }
}

struct DecodePermit;
impl Drop for DecodePermit {
    fn drop(&mut self) {
        DECODING.store(false, Ordering::Release);
    }
}
async fn decode_permit(life: &Life) -> Result<DecodePermit, i32> {
    loop {
        if !life.live() {
            return Err(ERR_NOT_FOUND);
        }
        if DECODING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Ok(DecodePermit);
        }
        Timer::after(Duration::from_millis(2)).await;
    }
}

#[trueos_executor::task(pool_size = 2)]
async fn worker_task() {
    loop {
        let job = REGISTRY.lock().queue.pop_front();
        if let Some(job) = job {
            if job.life.live() {
                if let Err(error) = process(&job).await {
                    complete_error(&job.life, error);
                }
            }
        } else {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

async fn process(job: &Job) -> Result<(), i32> {
    let cached = job
        .cache
        .as_ref()
        .and_then(|cache| read_cache(cache, &job.url));
    if !job.life.live() {
        return Err(ERR_NOT_FOUND);
    }
    let cache_hit = cached.is_some();
    let encoded = match cached {
        Some(bytes) => bytes,
        None => crate::r::net::https::get_image_bytes_shared(
            &job.url,
            FETCH_TIMEOUT_MS,
            MAX_ENCODED_BYTES,
            &job.life.fetch,
        )
        .await
        .map_err(|_| ERR_FAILED)?,
    };
    if !job.life.live() {
        return Err(ERR_NOT_FOUND);
    }
    let permit = decode_permit(&job.life).await?;
    let decoded = super::media_service::decode_ui4_bounded(&encoded)?;
    let format = decoded.info.source_format;
    let width = job.width.min(decoded.info.width);
    let height = job.height.min(decoded.info.height);
    let pixels = resize(
        &decoded.rgba,
        decoded.info.width,
        decoded.info.height,
        width,
        height,
    )?;
    drop(decoded);
    drop(permit);
    if !job.life.live() {
        return Err(ERR_NOT_FOUND);
    }
    // Allocation, pixel copy, and cache flush occur before taking the registry
    // lock. Installation only validates/switches the prepared retained source.
    let mut prepared = ui::prepare_rgba(width, height, &pixels)?;
    drop(pixels);
    let mut published = false;
    // A busy GPU/upload preserves prepared pixels. Retry off the caller's
    // thread and revalidate cancellation before each attempted installation.
    for _ in 0..500 {
        let result = {
            let mut registry = REGISTRY.lock();
            let op = registry
                .operations
                .iter_mut()
                .find(|op| Arc::ptr_eq(&op.life, &job.life))
                .ok_or(ERR_NOT_FOUND)?;
            if !job.life.live() {
                return Err(ERR_NOT_FOUND);
            }
            match ui::install_prepared(
                job.life.owner,
                job.life.window,
                job.life.incarnation,
                job.life.id,
                &mut prepared,
            ) {
                Ok(sprite_id) => {
                    op.info.state = 1;
                    op.info.sprite_id = sprite_id;
                    op.info.width = width;
                    op.info.height = height;
                    op.info.error = 0;
                    Ok(())
                }
                Err(error) => Err(error),
            }
        };
        match result {
            Ok(()) => {
                published = true;
                break;
            }
            Err(ERR_BUSY) => Timer::after(Duration::from_millis(10)).await,
            Err(error) => return Err(error),
        }
    }
    if !published {
        return Err(ERR_BUSY);
    }
    // Publication is independent of persistence: a full/offline filesystem
    // cannot turn a ready image into an error or hold initial layout hostage.
    if !cache_hit && job.life.live() {
        if let Some(cache) = &job.cache {
            let _ = write_cache(cache, &job.url, format, &encoded, &job.life);
        }
    }
    Ok(())
}

fn resize(source: &[u8], sw: u32, sh: u32, width: u32, height: u32) -> Result<Vec<u8>, i32> {
    let len = width as usize * height as usize * 4;
    if len == 0 || len > policy::MAX_TARGET_BYTES || source.len() != sw as usize * sh as usize * 4 {
        return Err(ERR_TOO_LARGE);
    }
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(len).map_err(|_| ERR_TOO_LARGE)?;
    pixels.resize(len, 0);
    // Deterministic nearest sample at pixel centres; no upscaling or secondary
    // whole-source scratch buffers. UI4 scales this variant into the frozen box.
    for y in 0..height as usize {
        let sy = ((2 * y + 1) * sh as usize / (2 * height as usize)).min(sh as usize - 1);
        for x in 0..width as usize {
            let sx = ((2 * x + 1) * sw as usize / (2 * width as usize)).min(sw as usize - 1);
            let src = (sy * sw as usize + sx) * 4;
            let dst = (y * width as usize + x) * 4;
            pixels[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
        }
    }
    Ok(pixels)
}

fn read_bounded(cache: &Cache, name: &str, limit: usize) -> Option<Vec<u8>> {
    let path = alloc::format!("{}/{name}", cache.directory);
    let len = fs::read_file_len(cache.disk, path.clone()).ok()?.ok()?;
    if len == 0 || len > limit {
        return None;
    }
    let bytes = fs::read_file_range(cache.disk, path, 0, len).ok()?.ok()?;
    (bytes.len() == len).then_some(bytes)
}

fn read_cache(cache: &Cache, url: &str) -> Option<Vec<u8>> {
    let meta = read_bounded(cache, "metadata", policy::MAX_URL_BYTES + 128)?;
    let (format, len, hash) = policy::cache_descriptor(&meta, url)?;
    let bytes = read_bounded(
        cache,
        &alloc::format!("source.{}", policy::extension(format)?),
        len,
    )?;
    if bytes.len() != len
        || policy::digest(&bytes) != hash
        || policy::preflight(&bytes).ok()?.format != format
    {
        return None;
    }
    Some(bytes)
}

fn write_file(cache: &Cache, name: &str, bytes: &[u8], life: &Life) -> Option<()> {
    if !life.live() {
        return None;
    }
    let path = alloc::format!("{}/{name}", cache.directory);
    let handle = fs::write_file_begin(cache.disk, path, bytes.len() as u64)
        .ok()?
        .ok()?;
    for chunk in bytes.chunks(64 * 1024) {
        if !life.live() || !matches!(fs::write_file_chunk(handle, chunk.to_vec()), Ok(Ok(()))) {
            let _ = fs::write_file_abort(handle);
            return None;
        }
    }
    if !life.live() {
        let _ = fs::write_file_abort(handle);
        return None;
    }
    if !matches!(fs::write_file_finish(handle), Ok(Ok(()))) {
        let _ = fs::write_file_abort(handle);
        return None;
    }
    Some(())
}

fn write_cache(cache: &Cache, url: &str, format: u32, bytes: &[u8], life: &Life) -> Option<()> {
    fs::create_dir_all(cache.disk, cache.directory.clone())
        .ok()?
        .ok()?;
    write_file(
        cache,
        &alloc::format!("source.{}", policy::extension(format)?),
        bytes,
        life,
    )?;
    // No rename assumptions: metadata is the final commit marker and includes
    // the full URL, format, exact length, and source digest. Torn replacements
    // and concurrent writers can only produce a rejected cache entry.
    write_file(
        cache,
        "metadata",
        policy::metadata(url, format, bytes).as_bytes(),
        life,
    )
}
