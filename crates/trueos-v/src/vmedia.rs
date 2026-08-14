//! Owner-scoped asynchronous media services.
//!
//! V1 intentionally exposes one decoded raster result: tightly packed RGBA8.
//! Backend selection is kernel-owned, so clients do not encode Intel/zune
//! policy and a future retained GPU image handle can extend this contract.

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::future::poll_fn;
use core::task::Poll;

use crate::bp_abi::TrueosVmediaImageInfo;
use crate::vcabi;

pub const ERR_NOT_FOUND: i32 = -1;
pub const ERR_FAILED: i32 = -2;
pub const ERR_INVALID: i32 = -3;
pub const ERR_BUSY: i32 = -4;
pub const ERR_TOO_LARGE: i32 = -7;
pub const ERR_UNSUPPORTED: i32 = -8;
pub const ERR_DECODE: i32 = -9;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ImageFormat {
    Jpeg = 1,
    Png = 2,
    Bmp = 3,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum DecodeBackend {
    Png = 1,
    ZuneJpeg = 2,
    Bmp = 3,
    XeLpJpeg = 4,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub source_format: ImageFormat,
    pub backend: DecodeBackend,
    pub revision: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecodedImage {
    pub info: ImageInfo,
    pub rgba: Vec<u8>,
}

struct Operation {
    id: u32,
}

impl Operation {
    fn start(format: ImageFormat, bytes: &[u8]) -> Result<Self, i32> {
        if bytes.is_empty() {
            return Err(ERR_INVALID);
        }
        let id =
            unsafe { vcabi::trueos_cabi_vmedia_image_decode_begin(format as u32, bytes.len()) };
        if id <= 0 {
            return Err(if id == 0 { ERR_INVALID } else { id });
        }
        let mut operation = Self { id: id as u32 };
        let write = unsafe {
            vcabi::trueos_cabi_vmedia_image_decode_write(
                operation.id,
                0,
                bytes.as_ptr(),
                bytes.len(),
            )
        };
        if write != 0 {
            operation.discard();
            return Err(write);
        }
        let commit = unsafe { vcabi::trueos_cabi_vmedia_image_decode_commit(operation.id) };
        if commit != 0 {
            operation.discard();
            return Err(commit);
        }
        Ok(operation)
    }

    async fn finish(&mut self) -> Result<DecodedImage, i32> {
        poll_fn(|context| {
            let status = unsafe { vcabi::trueos_cabi_vmedia_image_decode_status(self.id) };
            match status {
                0 => {
                    context.waker().wake_by_ref();
                    Poll::Pending
                }
                1 => Poll::Ready(Ok(())),
                error => Poll::Ready(Err(error)),
            }
        })
        .await?;

        let mut raw = TrueosVmediaImageInfo::default();
        let info_status = unsafe {
            vcabi::trueos_cabi_vmedia_image_decode_info(self.id, core::ptr::addr_of_mut!(raw))
        };
        if info_status != 0 {
            return Err(info_status);
        }
        let source_format = match raw.source_format {
            1 => ImageFormat::Jpeg,
            2 => ImageFormat::Png,
            3 => ImageFormat::Bmp,
            _ => return Err(ERR_FAILED),
        };
        let backend = match raw.backend {
            1 => DecodeBackend::Png,
            2 => DecodeBackend::ZuneJpeg,
            3 => DecodeBackend::Bmp,
            4 => DecodeBackend::XeLpJpeg,
            _ => return Err(ERR_FAILED),
        };
        if raw.pixel_format != 1
            || raw.width == 0
            || raw.height == 0
            || raw.stride_bytes != raw.width.checked_mul(4).ok_or(ERR_TOO_LARGE)?
            || raw.byte_len
                != raw
                    .stride_bytes
                    .checked_mul(raw.height)
                    .ok_or(ERR_TOO_LARGE)?
        {
            return Err(ERR_FAILED);
        }
        let mut rgba = vec![0u8; raw.byte_len as usize];
        let read = unsafe {
            vcabi::trueos_cabi_vmedia_image_decode_read(self.id, 0, rgba.as_mut_ptr(), rgba.len())
        };
        if read < 0 {
            return Err(read as i32);
        }
        if read as usize != rgba.len() {
            return Err(ERR_FAILED);
        }
        self.discard();
        Ok(DecodedImage {
            info: ImageInfo {
                width: raw.width,
                height: raw.height,
                stride_bytes: raw.stride_bytes,
                source_format,
                backend,
                revision: raw.revision,
            },
            rgba,
        })
    }

    fn discard(&mut self) {
        if self.id != 0 {
            let _ = unsafe { vcabi::trueos_cabi_vmedia_image_decode_discard(self.id) };
            self.id = 0;
        }
    }
}

impl Drop for Operation {
    fn drop(&mut self) {
        self.discard();
    }
}

/// Decode one PNG, JPEG, or BMP through the kernel-owned media service.
///
/// The encoded source and decoded result are bounded by kernel policy. Dropping
/// the future discards its owner-scoped operation; no kernel pointer or GPU
/// address crosses the V boundary.
pub async fn decode(format: ImageFormat, bytes: &[u8]) -> Result<DecodedImage, i32> {
    let mut operation = Operation::start(format, bytes)?;
    operation.finish().await
}
