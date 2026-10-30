//! Pointer-free Hull image transport and owner-scoped retained sprite storage.
//! Network, cache and decode work belongs to hull_image_service. Only immutable
//! decoded pixels reach this module, and no pixel bytes cross the Blueprint ABI.

use super::{
    BlueprintSpriteSource, ERROR_BUSY, ERROR_CONTEXT, ERROR_INVALID, ERROR_NOT_FOUND, ERROR_UI4,
    GpgpuRgba8Surface, OwnedRgba8Surface, SPRITE_MUTATION, SURFACES, WindowOwner,
    allocate_sprite_gpu_va, blueprint_owner, destroy_rgba8_surface, guest_status, surface_mut,
};
use crate::r::services::hull_image_service;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use v::bp_abi::TrueosUi4HullImageInfoV1;

const MAX_URL_BYTES: usize = 8_192;
const MAX_ROOT_BYTES: usize = 2_048;
const MAX_HOST_BYTES: usize = 253;
const MAX_EXTENT: u32 = 4_096;
const WIRE_HEADER_BYTES: usize = 28;
pub(super) const SERVICE_SPRITE_BASE: u32 = 0xC000_0000;
const MAX_RETAINED_IMAGES: usize = 128;
const MAX_RETAINED_BYTES: usize = 32 * 1024 * 1024;
const MAX_GLOBAL_IMAGE_BYTES: usize = 64 * 1024 * 1024;
// Account physical pitched allocations throughout preparation, publication,
// deferred retirement and permanent GPU quarantine. Reset never resets this.
static IMAGE_BYTES: AtomicUsize = AtomicUsize::new(0);

fn reserve_image_bytes(bytes: usize) -> Result<(), i32> {
    IMAGE_BYTES
        .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current
                .checked_add(bytes)
                .filter(|total| *total <= MAX_GLOBAL_IMAGE_BYTES)
        })
        .map(|_| ())
        .map_err(|_| ERROR_BUSY)
}

pub(super) fn release_image_bytes(bytes: usize) {
    if bytes != 0 {
        IMAGE_BYTES.fetch_sub(bytes, Ordering::Relaxed);
    }
}
static NEXT_INCARNATION: AtomicU64 = AtomicU64::new(1);
// At most MAX_SURFACES * MAX_RETAINED_IMAGES unique live sources can need
// retirement. Closing a surface owns its sources and removes stale entries.
const MAX_RETIREMENTS: usize = super::MAX_SURFACES * MAX_RETAINED_IMAGES;
#[derive(Clone, Copy, PartialEq, Eq)]
struct Retirement {
    owner: u32,
    window_id: u32,
    incarnation: u64,
    sprite_id: u32,
}
static RETIREMENTS: spin::Mutex<Vec<Retirement>> = spin::Mutex::new(Vec::new());

pub(super) struct ImageResources {
    pub(super) incarnation: u64,
    next_sprite: u32,
}

impl ImageResources {
    pub(super) fn new() -> Self {
        // Never wrap an incarnation back to an identity a delayed worker held.
        let incarnation = NEXT_INCARNATION
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |value| {
                value.checked_add(1)
            })
            .unwrap_or(0);
        Self {
            incarnation,
            next_sprite: SERVICE_SPRITE_BASE,
        }
    }
}

fn owner_identity(owner: WindowOwner) -> Result<u32, i32> {
    match owner {
        WindowOwner::Vm(id) => Ok(u32::from(id)),
        _ => Err(ERROR_CONTEXT),
    }
}

fn owner_from_id(owner: u32) -> Result<WindowOwner, i32> {
    u8::try_from(owner)
        .map(WindowOwner::Vm)
        .map_err(|_| ERROR_CONTEXT)
}

/// Capture the exact live foreground surface, so close/reuse cannot admit a
/// completion from an older request. Called again when installing pixels.
pub(crate) fn authenticate_window(owner: u32, window_id: u32) -> Result<u64, i32> {
    let owner = owner_from_id(owner)?;
    let mut surfaces = SURFACES.try_lock().ok_or(ERROR_BUSY)?;
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if surface.window.raw() != window_id || surface.hull_images.incarnation == 0 {
        return Err(ERROR_NOT_FOUND);
    }
    Ok(surface.hull_images.incarnation)
}

struct WireRequest<'a> {
    url: &'a str,
    cache_root: &'a str,
    page_host: &'a str,
    width: u32,
    height: u32,
    generation: u64,
}

fn decode_wire(payload: &[u8]) -> Result<WireRequest<'_>, i32> {
    let header = payload.get(..WIRE_HEADER_BYTES).ok_or(ERROR_INVALID)?;
    let word = |index| u32::from_le_bytes(header[index..index + 4].try_into().unwrap());
    let width = word(0);
    let height = word(4);
    if !(1..=MAX_EXTENT).contains(&width) || !(1..=MAX_EXTENT).contains(&height) {
        return Err(ERROR_INVALID);
    }
    let lengths = [word(16) as usize, word(20) as usize, word(24) as usize];
    let limits = [MAX_URL_BYTES, MAX_ROOT_BYTES, MAX_HOST_BYTES];
    let mut offset = WIRE_HEADER_BYTES;
    let mut strings = [""; 3];
    for (index, (&length, &limit)) in lengths.iter().zip(limits.iter()).enumerate() {
        if length == 0 || length > limit {
            return Err(ERROR_INVALID);
        }
        let end = offset.checked_add(length).ok_or(ERROR_INVALID)?;
        let bytes = payload.get(offset..end).ok_or(ERROR_INVALID)?;
        if bytes.contains(&0) {
            return Err(ERROR_INVALID);
        }
        strings[index] = core::str::from_utf8(bytes).map_err(|_| ERROR_INVALID)?;
        offset = end;
    }
    if offset != payload.len() {
        return Err(ERROR_INVALID);
    }
    Ok(WireRequest {
        url: strings[0],
        cache_root: strings[1],
        page_host: strings[2],
        width,
        height,
        generation: u64::from_le_bytes(header[8..16].try_into().unwrap()),
    })
}

pub(crate) fn request_wire(owner: WindowOwner, window_id: u32, payload: &[u8]) -> i32 {
    let owner = match owner_identity(owner) {
        Ok(owner) => owner,
        Err(error) => return error,
    };
    if let Err(error) = authenticate_window(owner, window_id) {
        return error;
    }
    let request = match decode_wire(payload) {
        Ok(request) => request,
        Err(error) => return error,
    };
    hull_image_service::request(
        owner,
        window_id,
        request.url,
        request.cache_root,
        request.page_host,
        request.width,
        request.height,
        request.generation,
    )
}

pub unsafe extern "C" fn trueos_cabi_ui4_hull_image_request_v1(
    window_id: u32,
    url: *const u8,
    url_len: usize,
    cache_root: *const u8,
    cache_root_len: usize,
    page_host: *const u8,
    page_host_len: usize,
    width: u32,
    height: u32,
    generation: u64,
) -> i32 {
    let guest = crate::hv::current_hull_guest_context_vm_id().is_some();
    let owner = if guest {
        None
    } else {
        let Some(owner) = blueprint_owner() else {
            return ERROR_CONTEXT;
        };
        let id = match owner_identity(owner) {
            Ok(id) => id,
            Err(error) => return error,
        };
        if let Err(error) = authenticate_window(id, window_id) {
            return error;
        }
        Some(owner)
    };
    let parts = [
        (url, url_len, MAX_URL_BYTES),
        (cache_root, cache_root_len, MAX_ROOT_BYTES),
        (page_host, page_host_len, MAX_HOST_BYTES),
    ];
    if !(1..=MAX_EXTENT).contains(&width)
        || !(1..=MAX_EXTENT).contains(&height)
        || parts
            .iter()
            .any(|(pointer, length, limit)| pointer.is_null() || *length == 0 || length > limit)
    {
        return ERROR_INVALID;
    }
    let mut payload =
        Vec::with_capacity(WIRE_HEADER_BYTES + url_len + cache_root_len + page_host_len);
    payload.extend_from_slice(&width.to_le_bytes());
    payload.extend_from_slice(&height.to_le_bytes());
    payload.extend_from_slice(&generation.to_le_bytes());
    for (_, length, _) in parts {
        payload.extend_from_slice(&(length as u32).to_le_bytes());
    }
    for (pointer, length, _) in parts {
        // SAFETY: C callers supply readable local bytes; the guest copies its
        // own memory into a bounded wire record, never sends raw pointers.
        payload.extend_from_slice(unsafe { core::slice::from_raw_parts(pointer, length) });
    }
    if guest {
        guest_status(
            trueos_vm::vmcall::OP_BP_UI4_HULL_IMAGE_REQUEST_V1,
            window_id as u64,
            0,
            &payload,
        )
    } else {
        request_wire(owner.unwrap(), window_id, &payload)
    }
}

pub(crate) fn status(
    owner: WindowOwner,
    window_id: u32,
    ticket: u32,
) -> Result<TrueosUi4HullImageInfoV1, i32> {
    let owner = owner_identity(owner)?;
    authenticate_window(owner, window_id)?;
    hull_image_service::status(owner, window_id, ticket)
}

pub unsafe extern "C" fn trueos_cabi_ui4_hull_image_status_v1(
    window_id: u32,
    ticket: u32,
    out: *mut TrueosUi4HullImageInfoV1,
) -> i32 {
    if out.is_null() || ticket == 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        let mut response = [0u8; core::mem::size_of::<TrueosUi4HullImageInfoV1>()];
        let (status, code) = trueos_vm::vmcall::call_with_payload(
            trueos_vm::vmcall::OP_BP_UI4_HULL_IMAGE_STATUS_V1,
            window_id as u64,
            ticket as u64,
            &[],
            &mut response,
        );
        if status != trueos_vm::vmcall::STATUS_OK {
            return ERROR_UI4;
        }
        let code = code as i64 as i32;
        if code != 0 {
            return code;
        }
        unsafe {
            out.write_unaligned(core::ptr::read_unaligned(response.as_ptr().cast()));
        }
        return 0;
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    match status(owner, window_id, ticket) {
        Ok(info) => {
            unsafe {
                out.write_unaligned(info);
            }
            0
        }
        Err(error) => error,
    }
}

pub(crate) fn release(owner: WindowOwner, window_id: u32, ticket: u32) -> i32 {
    let owner = match owner_identity(owner) {
        Ok(owner) => owner,
        Err(error) => return error,
    };
    if let Err(error) = authenticate_window(owner, window_id) {
        return error;
    }
    hull_image_service::release(owner, window_id, ticket)
}

pub extern "C" fn trueos_cabi_ui4_hull_image_release_v1(window_id: u32, ticket: u32) -> i32 {
    if ticket == 0 {
        return ERROR_INVALID;
    }
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_HULL_IMAGE_RELEASE_V1,
            window_id as u64,
            ticket as u64,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    release(owner, window_id, ticket)
}

pub(crate) fn reset(owner: WindowOwner, window_id: u32, generation: u64) -> i32 {
    let owner = match owner_identity(owner) {
        Ok(owner) => owner,
        Err(error) => return error,
    };
    if let Err(error) = authenticate_window(owner, window_id) {
        return error;
    }
    hull_image_service::reset(owner, window_id, generation)
}

pub extern "C" fn trueos_cabi_ui4_hull_image_reset_v1(window_id: u32, generation: u64) -> i32 {
    if crate::hv::current_hull_guest_context_vm_id().is_some() {
        return guest_status(
            trueos_vm::vmcall::OP_BP_UI4_HULL_IMAGE_RESET_V1,
            window_id as u64,
            generation,
            &[],
        );
    }
    let Some(owner) = blueprint_owner() else {
        return ERROR_CONTEXT;
    };
    reset(owner, window_id, generation)
}

/// Immutable prepared DMA pixels. Until publication, cancellation/drop frees
/// the allocation without reaching the sprite registry or invoking the service.
pub(crate) struct PreparedImage {
    phys: u64,
    virt: *mut u8,
    bytes: usize,
    width: u32,
    height: u32,
    pitch: u32,
    opaque: bool,
    premultiplied: bool,
}
unsafe impl Send for PreparedImage {}
impl Drop for PreparedImage {
    fn drop(&mut self) {
        if !self.virt.is_null() {
            crate::dma::dealloc(self.virt, self.bytes);
            release_image_bytes(self.bytes);
        }
    }
}

pub(crate) fn prepare_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<PreparedImage, i32> {
    if !(1..=MAX_EXTENT).contains(&width)
        || !(1..=MAX_EXTENT).contains(&height)
        || rgba.len() != width as usize * height as usize * 4
    {
        return Err(ERROR_INVALID);
    }
    let pitch = crate::intel::align_up(width as usize * 4, 64).ok_or(ERROR_INVALID)?;
    let bytes = crate::intel::align_up(pitch * height as usize, crate::intel::WARM_ALIGN)
        .ok_or(ERROR_INVALID)?;
    if bytes > MAX_RETAINED_BYTES {
        return Err(ERROR_INVALID);
    }
    reap_retired();
    reserve_image_bytes(bytes)?;
    let Some((phys, virt)) = crate::dma::alloc_ppgtt(bytes, crate::intel::WARM_ALIGN) else {
        release_image_bytes(bytes);
        return Err(ERROR_UI4);
    };
    unsafe {
        core::ptr::write_bytes(virt, 0, bytes);
    }
    for y in 0..height as usize {
        unsafe {
            core::ptr::copy_nonoverlapping(
                rgba.as_ptr().add(y * width as usize * 4),
                virt.add(y * pitch),
                width as usize * 4,
            );
        }
    }
    let opaque = rgba.chunks_exact(4).all(|pixel| pixel[3] == 255);
    // The image decoder's contract is straight alpha. Do not infer pixel
    // semantics from RGB <= alpha: dark translucent straight pixels satisfy it.
    let premultiplied = false;
    crate::intel::dma_flush(virt, bytes);
    Ok(PreparedImage {
        phys,
        virt,
        bytes,
        width,
        height,
        pitch: pitch as u32,
        opaque,
        premultiplied,
    })
}

/// Publish after service cancellation/generation validation under its operations
/// lock. Busy retains prepared pixels for retry; success transfers their storage
/// into the registry. No allocation/copy/flush or service callback occurs here.
pub(crate) fn install_prepared(
    owner: u32,
    window_id: u32,
    incarnation: u64,
    _ticket: u32,
    image: &mut PreparedImage,
) -> Result<u32, i32> {
    let owner = owner_from_id(owner)?;
    reap_retired();
    if image.virt.is_null() {
        return Err(ERROR_INVALID);
    }
    let _mutation = SPRITE_MUTATION.try_lock().ok_or(ERROR_BUSY)?;
    let mut surfaces = SURFACES.try_lock().ok_or(ERROR_BUSY)?;
    let surface = surface_mut(&mut surfaces, owner, window_id).ok_or(ERROR_NOT_FOUND)?;
    if incarnation == 0
        || surface.hull_images.incarnation != incarnation
        || surface.window.raw() != window_id
    {
        return Err(ERROR_NOT_FOUND);
    }
    if surface.gpu_submission_unretired {
        return Err(ERROR_BUSY);
    }
    let (count, retained) = surface
        .sprites
        .iter()
        .filter(|(id, _)| *id >= SERVICE_SPRITE_BASE)
        .fold((0usize, 0usize), |(count, total), (_, source)| {
            (count + 1, total + source.bytes())
        });
    if count >= MAX_RETAINED_IMAGES || retained.saturating_add(image.bytes) > MAX_RETAINED_BYTES {
        return Err(ERROR_BUSY);
    }
    let id = surface.hull_images.next_sprite;
    let next = id.checked_add(1).ok_or(ERROR_BUSY)?;
    let gpu = allocate_sprite_gpu_va(surface, id, image.bytes).ok_or(ERROR_BUSY)?;
    let gpu_surface = GpgpuRgba8Surface::new(
        image.phys,
        gpu,
        image.bytes,
        image.width,
        image.height,
        image.pitch,
    )
    .ok_or(ERROR_UI4)?;
    surface.sprites.push((
        id,
        BlueprintSpriteSource::Uploaded(OwnedRgba8Surface {
            opaque: image.opaque,
            premultiplied: image.premultiplied,
            nonzero_alpha_runs: core::ptr::null(),
            nonzero_alpha_run_count: 0,
            surface: gpu_surface,
            virt: image.virt,
            bytes: image.bytes,
            hull_image_budget_bytes: image.bytes,
        }),
    ));
    surface.hull_images.next_sprite = next;
    image.virt = core::ptr::null_mut();
    Ok(id)
}

/// Queue retirement without waiting for an active GPU draw or guest upload.
/// The registry keeps sources resident (and budgeted) until the exact draw
/// retires. Accepted GPU timeouts therefore follow normal surface quarantine.
pub(crate) fn release_rgba(owner: u32, window_id: u32, incarnation: u64, sprite_id: u32) -> i32 {
    if sprite_id < SERVICE_SPRITE_BASE || incarnation == 0 {
        return ERROR_INVALID;
    }
    if let Err(error) = owner_from_id(owner) {
        return error;
    }
    let retirement = Retirement {
        owner,
        window_id,
        incarnation,
        sprite_id,
    };
    {
        let mut retired = RETIREMENTS.lock();
        if !retired.contains(&retirement) {
            // The bound equals all admitted sources across all live surfaces.
            // Source IDs are never reused; duplicates are coalesced above.
            if retired.len() >= MAX_RETIREMENTS {
                return ERROR_BUSY;
            }
            retired.push(retirement);
        }
    }
    reap_retired();
    0
}

/// Opportunistically collect releases. Every lock required by GPU work uses
/// try_lock; request/status/reset never waits for a GPU or an uploading caller.
pub(super) fn reap_retired() {
    let Some(_mutation) = SPRITE_MUTATION.try_lock() else {
        return;
    };
    let Some(mut surfaces) = SURFACES.try_lock() else {
        return;
    };
    let mut retired = RETIREMENTS.lock();
    let mut index = 0;
    while index < retired.len() {
        let request = retired[index];
        let owner = WindowOwner::Vm(request.owner as u8);
        let Some(surface) = surface_mut(&mut surfaces, owner, request.window_id)
            .filter(|surface| surface.hull_images.incarnation == request.incarnation)
        else {
            retired.swap_remove(index);
            continue;
        };
        if surface.gpu_submission_unretired {
            index += 1;
            continue;
        }
        if let Some(slot) = surface
            .sprites
            .iter()
            .position(|(id, _)| *id == request.sprite_id)
        {
            let BlueprintSpriteSource::Uploaded(owned) = surface.sprites.remove(slot).1;
            destroy_rgba8_surface(owned);
        }
        retired.swap_remove(index);
    }
}

pub(super) fn forget_surface(owner: WindowOwner, window_id: u32, incarnation: u64) {
    let Ok(owner) = owner_identity(owner) else {
        return;
    };
    RETIREMENTS
        .lock()
        .retain(|r| r.owner != owner || r.window_id != window_id || r.incarnation != incarnation);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wire() -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&64u32.to_le_bytes());
        bytes.extend_from_slice(&32u32.to_le_bytes());
        bytes.extend_from_slice(&0x1234_5678_9ABC_DEF0u64.to_le_bytes());
        for part in ["https://example.org/a.png", "/cache", "example.org"] {
            bytes.extend_from_slice(&(part.len() as u32).to_le_bytes());
        }
        for part in ["https://example.org/a.png", "/cache", "example.org"] {
            bytes.extend_from_slice(part.as_bytes());
        }
        bytes
    }

    #[test]
    fn image_wire_is_exact_bounded_utf8_and_retains_generation() {
        let payload = wire();
        let request = decode_wire(&payload).unwrap();
        assert_eq!((request.width, request.height), (64, 32));
        assert_eq!(request.generation, 0x1234_5678_9ABC_DEF0);
        assert_eq!(request.url, "https://example.org/a.png");
        for end in 0..payload.len() {
            assert!(decode_wire(&payload[..end]).is_err());
        }
        let mut bad = payload.clone();
        bad.push(0);
        assert!(decode_wire(&bad).is_err());
        let mut bad = payload.clone();
        bad[28] = 0;
        assert!(decode_wire(&bad).is_err());
        let mut bad = payload.clone();
        bad[28] = 0xFF;
        assert!(decode_wire(&bad).is_err());
        let mut bad = payload.clone();
        bad[..4].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode_wire(&bad).is_err());
        let mut bad = payload.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_wire(&bad).is_err());
    }
}
