extern crate alloc;

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use spin::Mutex;
use trueos_time::{Duration, Timer};

pub const FORMAT_JPEG: u32 = 1;
pub const FORMAT_PNG: u32 = 2;
pub const FORMAT_BMP: u32 = 3;
pub const PIXEL_FORMAT_RGBA8: u32 = 1;

pub const BACKEND_PNG: u32 = 1;
pub const BACKEND_ZUNE_JPEG: u32 = 2;
pub const BACKEND_BMP: u32 = 3;
pub const BACKEND_XELP_JPEG: u32 = 4;

pub const STATUS_PENDING: i32 = 0;
pub const STATUS_READY: i32 = 1;
pub const ERR_NOT_FOUND: i32 = -1;
pub const ERR_FAILED: i32 = -2;
pub const ERR_INVALID: i32 = -3;
pub const ERR_BUSY: i32 = -4;
pub const ERR_TOO_LARGE: i32 = -7;
pub const ERR_UNSUPPORTED: i32 = -8;

const OPERATION_CAP: usize = 32;
const REQUEST_CAP: usize = 16;
const MAX_ENCODED_BYTES: usize = 16 * 1024 * 1024;
const MAX_RGBA_BYTES: usize = 64 * 1024 * 1024;
const MAX_DIMENSION: u32 = 8_192;
const IDLE_MS: u64 = 10;
const XELP_JPEG_TIMEOUT_MS: u64 = 1_000;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);
static OPERATIONS: Mutex<Vec<Operation>> = Mutex::new(Vec::new());
static REQUESTS: Mutex<VecDeque<DecodeRequest>> = Mutex::new(VecDeque::new());

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[repr(C)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub byte_len: u32,
    pub source_format: u32,
    pub pixel_format: u32,
    pub backend: u32,
    pub revision: u32,
}

struct DecodedImage {
    info: ImageInfo,
    rgba: Vec<u8>,
}

enum OperationState {
    Receiving {
        format: u32,
        total_len: usize,
        encoded: Vec<u8>,
    },
    Queued,
    Running,
    Complete(Result<DecodedImage, i32>),
}

struct Operation {
    owner: u32,
    id: u32,
    state: OperationState,
}

struct DecodeRequest {
    owner: u32,
    id: u32,
    format: u32,
    encoded: Vec<u8>,
}

fn next_id(operations: &[Operation]) -> Result<u32, i32> {
    for _ in 0..=OPERATION_CAP {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) & i32::MAX as u32;
        if id != 0 && operations.iter().all(|operation| operation.id != id) {
            return Ok(id);
        }
    }
    Err(ERR_BUSY)
}

fn valid_format(format: u32) -> bool {
    matches!(format, FORMAT_JPEG | FORMAT_PNG | FORMAT_BMP)
}

pub fn begin(owner: u32, format: u32, total_len: usize) -> i32 {
    if !valid_format(format) || total_len == 0 {
        return ERR_INVALID;
    }
    if total_len > MAX_ENCODED_BYTES {
        return ERR_TOO_LARGE;
    }
    let mut operations = OPERATIONS.lock();
    if operations.len() >= OPERATION_CAP {
        return ERR_BUSY;
    }
    let id = match next_id(operations.as_slice()) {
        Ok(id) => id,
        Err(error) => return error,
    };
    let mut encoded = Vec::new();
    if encoded.try_reserve_exact(total_len).is_err() {
        return ERR_TOO_LARGE;
    }
    operations.push(Operation {
        owner,
        id,
        state: OperationState::Receiving {
            format,
            total_len,
            encoded,
        },
    });
    id as i32
}

pub fn write(owner: u32, id: u32, offset: usize, bytes: &[u8]) -> i32 {
    if bytes.is_empty() {
        return ERR_INVALID;
    }
    let mut operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return ERR_NOT_FOUND;
    };
    let OperationState::Receiving {
        total_len, encoded, ..
    } = &mut operation.state
    else {
        return ERR_INVALID;
    };
    if offset != encoded.len()
        || offset
            .checked_add(bytes.len())
            .is_none_or(|end| end > *total_len)
    {
        return ERR_INVALID;
    }
    encoded.extend_from_slice(bytes);
    0
}

pub fn commit(owner: u32, id: u32) -> i32 {
    let mut requests = REQUESTS.lock();
    if requests.len() >= REQUEST_CAP {
        return ERR_BUSY;
    }
    let mut operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return ERR_NOT_FOUND;
    };
    let state = core::mem::replace(&mut operation.state, OperationState::Queued);
    let OperationState::Receiving {
        format,
        total_len,
        encoded,
    } = state
    else {
        operation.state = state;
        return ERR_INVALID;
    };
    if encoded.len() != total_len {
        operation.state = OperationState::Receiving {
            format,
            total_len,
            encoded,
        };
        return ERR_INVALID;
    }
    requests.push_back(DecodeRequest {
        owner,
        id,
        format,
        encoded,
    });
    0
}

pub fn status(owner: u32, id: u32) -> i32 {
    let operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return ERR_NOT_FOUND;
    };
    match &operation.state {
        OperationState::Receiving { .. } | OperationState::Queued | OperationState::Running => {
            STATUS_PENDING
        }
        OperationState::Complete(Ok(_)) => STATUS_READY,
        OperationState::Complete(Err(error)) => *error,
    }
}

pub fn info(owner: u32, id: u32) -> Result<ImageInfo, i32> {
    let operations = OPERATIONS.lock();
    let operation = operations
        .iter()
        .find(|operation| operation.owner == owner && operation.id == id)
        .ok_or(ERR_NOT_FOUND)?;
    match &operation.state {
        OperationState::Complete(Ok(image)) => Ok(image.info),
        OperationState::Complete(Err(error)) => Err(*error),
        _ => Err(ERR_BUSY),
    }
}

pub fn read(owner: u32, id: u32, offset: usize, out: &mut [u8]) -> Result<usize, i32> {
    if out.is_empty() {
        return Err(ERR_INVALID);
    }
    let operations = OPERATIONS.lock();
    let operation = operations
        .iter()
        .find(|operation| operation.owner == owner && operation.id == id)
        .ok_or(ERR_NOT_FOUND)?;
    let image = match &operation.state {
        OperationState::Complete(Ok(image)) => image,
        OperationState::Complete(Err(error)) => return Err(*error),
        _ => return Err(ERR_BUSY),
    };
    if offset > image.rgba.len() {
        return Err(ERR_INVALID);
    }
    let copied = out.len().min(image.rgba.len() - offset);
    out[..copied].copy_from_slice(&image.rgba[offset..offset + copied]);
    Ok(copied)
}

pub fn discard(owner: u32, id: u32) -> i32 {
    let mut requests = REQUESTS.lock();
    let mut operations = OPERATIONS.lock();
    let Some(index) = operations
        .iter()
        .position(|operation| operation.owner == owner && operation.id == id)
    else {
        return ERR_NOT_FOUND;
    };
    operations.swap_remove(index);
    requests.retain(|request| !(request.owner == owner && request.id == id));
    0
}

/// Revoke every upload, queued request, and completed raster owned by one
/// principal. A request already executing may finish its private work, but its
/// result can no longer be published after the operation entry is removed.
pub fn release_owner(owner: u32) -> usize {
    let mut requests = REQUESTS.lock();
    let mut operations = OPERATIONS.lock();
    let before = operations.len();
    operations.retain(|operation| operation.owner != owner);
    requests.retain(|request| request.owner != owner);
    before.saturating_sub(operations.len())
}

pub fn release_vm(vm_id: u8) -> usize {
    release_owner(crate::r::io::async_fs_cabi::owner_for_vm(vm_id))
}

fn mark_running(owner: u32, id: u32) -> bool {
    let mut operations = OPERATIONS.lock();
    let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
    else {
        return false;
    };
    if !matches!(operation.state, OperationState::Queued) {
        return false;
    }
    operation.state = OperationState::Running;
    true
}

fn complete(owner: u32, id: u32, result: Result<DecodedImage, i32>) {
    let mut operations = OPERATIONS.lock();
    if let Some(operation) = operations
        .iter_mut()
        .find(|operation| operation.owner == owner && operation.id == id)
        && matches!(operation.state, OperationState::Running)
    {
        operation.state = OperationState::Complete(result);
    }
}

fn validated_image(
    format: u32,
    backend: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
) -> Result<DecodedImage, i32> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(ERR_TOO_LARGE);
    }
    let byte_len = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|expected| *expected == rgba.len() && *expected <= MAX_RGBA_BYTES)
        .ok_or(ERR_TOO_LARGE)?;
    let stride_bytes = width.checked_mul(4).ok_or(ERR_TOO_LARGE)?;
    Ok(DecodedImage {
        info: ImageInfo {
            width,
            height,
            stride_bytes,
            byte_len: u32::try_from(byte_len).map_err(|_| ERR_TOO_LARGE)?,
            source_format: format,
            pixel_format: PIXEL_FORMAT_RGBA8,
            backend,
            revision: 1,
        },
        rgba,
    })
}

async fn decode(request: &DecodeRequest) -> Result<DecodedImage, i32> {
    match request.format {
        FORMAT_PNG => crate::graphics::png_codec::decode_png_rgba(request.encoded.as_slice())
            .map_err(|error| match error {
                crate::graphics::png_codec::PngDecodeError::Invalid => ERR_INVALID,
                crate::graphics::png_codec::PngDecodeError::Unsupported => ERR_UNSUPPORTED,
                crate::graphics::png_codec::PngDecodeError::DecodeFailed => ERR_FAILED,
            })
            .and_then(|image| {
                validated_image(FORMAT_PNG, BACKEND_PNG, image.width, image.height, image.rgba)
            }),
        FORMAT_JPEG => {
            if crate::intel::has_media_decode_engine()
                && let Ok(image) = crate::intel::hw_jpeg_decode_rgba(
                    request.encoded.as_slice(),
                    XELP_JPEG_TIMEOUT_MS,
                )
                .await
                && image.width.checked_mul(4) == Some(image.stride_bytes)
                && let Ok(decoded) = validated_image(
                    FORMAT_JPEG,
                    BACKEND_XELP_JPEG,
                    image.width,
                    image.height,
                    image.rgba,
                )
            {
                return Ok(decoded);
            }

            crate::graphics::jpeg_codec::decode_jpeg_rgba(request.encoded.as_slice())
                .map_err(|error| match error {
                    crate::graphics::jpeg_codec::JpegDecodeError::Invalid => ERR_INVALID,
                    crate::graphics::jpeg_codec::JpegDecodeError::Unsupported => ERR_UNSUPPORTED,
                    crate::graphics::jpeg_codec::JpegDecodeError::DecodeFailed => ERR_FAILED,
                })
                .and_then(|image| {
                    validated_image(
                        FORMAT_JPEG,
                        BACKEND_ZUNE_JPEG,
                        image.width,
                        image.height,
                        image.rgba,
                    )
                })
        }
        FORMAT_BMP => crate::graphics::bmp_codec::decode_bmp_rgba(request.encoded.as_slice())
            .map_err(|error| match error {
                crate::graphics::bmp_codec::BmpDecodeError::Invalid => ERR_INVALID,
                crate::graphics::bmp_codec::BmpDecodeError::Unsupported => ERR_UNSUPPORTED,
                crate::graphics::bmp_codec::BmpDecodeError::LimitExceeded => ERR_TOO_LARGE,
            })
            .and_then(|image| {
                validated_image(FORMAT_BMP, BACKEND_BMP, image.width, image.height, image.rgba)
            }),
        _ => Err(ERR_UNSUPPORTED),
    }
}

#[trueos_executor::task(pool_size = 2)]
pub async fn worker_task(worker_id: usize, worker_slot: u32, core_kind: u8) {
    crate::log_info!(
        target: "service";
        "vmedia: worker={} online image=png,jpeg,bmp pool=2 worker_slot={} core_kind={} jpeg_backend=xelp-vdbox-owned-rgba+zune-fallback\n",
        worker_id,
        worker_slot,
        core_kind,
    );
    loop {
        let request = REQUESTS.lock().pop_front();
        if let Some(request) = request {
            if mark_running(request.owner, request.id) {
                let result = decode(&request).await;
                complete(request.owner, request.id, result);
            }
        } else {
            Timer::after(Duration::from_millis(IDLE_MS)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owner_scoped_upload_is_strictly_sequential() {
        let id = begin(7, FORMAT_BMP, 4);
        assert!(id > 0);
        let id = id as u32;
        assert_eq!(write(8, id, 0, b"BMxx"), ERR_NOT_FOUND);
        assert_eq!(write(7, id, 1, b"BM"), ERR_INVALID);
        assert_eq!(write(7, id, 0, b"BM"), 0);
        assert_eq!(commit(7, id), ERR_INVALID);
        assert_eq!(write(7, id, 2, b"xx"), 0);
        assert_eq!(discard(7, id), 0);
    }

    #[test]
    fn owner_release_revokes_all_visible_operations() {
        let first = begin(21, FORMAT_JPEG, 4);
        let second = begin(21, FORMAT_PNG, 4);
        let other = begin(22, FORMAT_BMP, 4);
        assert!(first > 0 && second > 0 && other > 0);
        assert_eq!(release_owner(21), 2);
        assert_eq!(status(21, first as u32), ERR_NOT_FOUND);
        assert_eq!(status(21, second as u32), ERR_NOT_FOUND);
        assert_eq!(status(22, other as u32), STATUS_PENDING);
        assert_eq!(discard(22, other as u32), 0);
    }
}
