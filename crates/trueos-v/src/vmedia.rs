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

use crate::bp_abi::{TrueosVmediaImageInfo, TrueosVmediaRetainedTextureInfo};
use crate::vgpu::Device;
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

impl ImageFormat {
    /// Recognize the intentionally bounded raster asset set. SVG and every
    /// unrecognized extension fail closed before an operation is created.
    pub fn from_asset_name(name: &str) -> Option<Self> {
        let extension = name.rsplit_once('.')?.1;
        if extension.eq_ignore_ascii_case("jpg") || extension.eq_ignore_ascii_case("jpeg") {
            Some(Self::Jpeg)
        } else if extension.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else if extension.eq_ignore_ascii_case("bmp") {
            Some(Self::Bmp)
        } else {
            None
        }
    }
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct TextureId(u64);

impl TextureId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TextureResidency {
    PicassoRender1 = 2,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RetainedTextureInfo {
    pub id: TextureId,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: u32,
    pub byte_len: u32,
    pub source_format: ImageFormat,
    pub backend: DecodeBackend,
    pub revision: u32,
    pub residency: TextureResidency,
}

/// One owner-scoped decoded texture already resident in Picasso's Render1
/// carrier. Dropping it releases the generation-checked kernel handle.
pub struct RetainedTexture {
    device: Device,
    info: RetainedTextureInfo,
    live: bool,
}

impl RetainedTexture {
    pub const fn info(&self) -> RetainedTextureInfo {
        self.info
    }

    pub const fn id(&self) -> TextureId {
        self.info.id
    }
}

impl Drop for RetainedTexture {
    fn drop(&mut self) {
        if self.live {
            let _ = unsafe {
                vcabi::trueos_cabi_vmedia_texture_release(
                    self.device.raw(),
                    self.info.id.raw(),
                )
            };
            self.live = false;
        }
    }
}

struct Operation {
    id: u32,
}

impl Operation {
    fn start(format: ImageFormat, bytes: &[u8]) -> Result<Self, i32> {
        Self::start_for_device(None, format, bytes)
    }

    fn start_retained(device: Device, format: ImageFormat, bytes: &[u8]) -> Result<Self, i32> {
        Self::start_for_device(Some(device), format, bytes)
    }

    fn start_for_device(
        device: Option<Device>,
        format: ImageFormat,
        bytes: &[u8],
    ) -> Result<Self, i32> {
        if bytes.is_empty() {
            return Err(ERR_INVALID);
        }
        let id = unsafe {
            match device {
                Some(device) => vcabi::trueos_cabi_vmedia_texture_decode_begin(
                    device.raw(),
                    format as u32,
                    bytes.len(),
                ),
                None => vcabi::trueos_cabi_vmedia_image_decode_begin(format as u32, bytes.len()),
            }
        };
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
        self.wait().await?;

        let mut raw = TrueosVmediaImageInfo::default();
        let info_status = unsafe {
            vcabi::trueos_cabi_vmedia_image_decode_info(self.id, core::ptr::addr_of_mut!(raw))
        };
        if info_status != 0 {
            return Err(info_status);
        }
        let (source_format, backend) = validate_info_contract(
            raw.width,
            raw.height,
            raw.stride_bytes,
            raw.byte_len,
            raw.source_format,
            raw.pixel_format,
            raw.backend,
        )?;
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

    async fn finish_retained(&mut self, device: Device) -> Result<RetainedTexture, i32> {
        self.wait().await?;
        let mut raw = TrueosVmediaRetainedTextureInfo::default();
        let status = unsafe {
            vcabi::trueos_cabi_vmedia_texture_decode_info(
                self.id,
                core::ptr::addr_of_mut!(raw),
            )
        };
        if status != 0 {
            return Err(status);
        }
        let (source_format, backend) = validate_info_contract(
            raw.width,
            raw.height,
            raw.stride_bytes,
            raw.byte_len,
            raw.source_format,
            raw.pixel_format,
            raw.backend,
        )?;
        if raw.texture_id == 0 || raw.residency != TextureResidency::PicassoRender1 as u32 {
            return Err(ERR_FAILED);
        }
        let retained = RetainedTexture {
            device,
            info: RetainedTextureInfo {
                id: TextureId(raw.texture_id),
                width: raw.width,
                height: raw.height,
                stride_bytes: raw.stride_bytes,
                byte_len: raw.byte_len,
                source_format,
                backend,
                revision: raw.revision,
                residency: TextureResidency::PicassoRender1,
            },
            live: true,
        };
        self.discard();
        Ok(retained)
    }

    async fn wait(&self) -> Result<(), i32> {
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
        .await
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

fn validate_info_contract(
    width: u32,
    height: u32,
    stride_bytes: u32,
    byte_len: u32,
    source_format: u32,
    pixel_format: u32,
    backend: u32,
) -> Result<(ImageFormat, DecodeBackend), i32> {
    let source_format = match source_format {
        1 => ImageFormat::Jpeg,
        2 => ImageFormat::Png,
        3 => ImageFormat::Bmp,
        _ => return Err(ERR_FAILED),
    };
    let backend = match backend {
        1 => DecodeBackend::Png,
        2 => DecodeBackend::ZuneJpeg,
        3 => DecodeBackend::Bmp,
        4 => DecodeBackend::XeLpJpeg,
        _ => return Err(ERR_FAILED),
    };
    if pixel_format != 1
        || width == 0
        || height == 0
        || stride_bytes != width.checked_mul(4).ok_or(ERR_TOO_LARGE)?
        || byte_len != stride_bytes.checked_mul(height).ok_or(ERR_TOO_LARGE)?
    {
        return Err(ERR_FAILED);
    }
    Ok((source_format, backend))
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

/// Decode and publish directly into the device's retained Picasso carrier.
/// Awaiting this future yields a mapped opaque `TextureId`; decoded RGBA bytes
/// are never copied into Blueprint memory.
pub async fn decode_retained(
    device: Device,
    format: ImageFormat,
    bytes: &[u8],
) -> Result<RetainedTexture, i32> {
    let mut operation = Operation::start_retained(device, format, bytes)?;
    operation.finish_retained(device).await
}

/// Asset-change front door for the supported raster extensions. `.jpg` and
/// `.jpeg` share one contract; SVG and all other formats fail closed.
pub async fn decode_retained_asset(
    device: Device,
    name: &str,
    bytes: &[u8],
) -> Result<RetainedTexture, i32> {
    let format = ImageFormat::from_asset_name(name).ok_or(ERR_UNSUPPORTED)?;
    decode_retained(device, format, bytes).await
}
