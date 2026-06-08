use alloc::vec::Vec;

const BMP_FILE_HEADER_BYTES: usize = 14;
const BMP_INFO_HEADER_BYTES: usize = 40;
const BMP_HEADER_BYTES: usize = BMP_FILE_HEADER_BYTES + BMP_INFO_HEADER_BYTES;

#[derive(Clone, Debug)]
pub(crate) struct Ui3DebugImage {
    pub(crate) content_type: &'static str,
    pub(crate) filename: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) bytes: Vec<u8>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Ui3DebugCaptureError {
    NoFrame,
    NoSurface,
    TooLarge,
}

pub(crate) fn latest_pixi_primary_bmp() -> Result<Ui3DebugImage, Ui3DebugCaptureError> {
    if super::pixi_service_frame_count() == 0 {
        return Err(Ui3DebugCaptureError::NoFrame);
    }

    let snapshot =
        crate::intel::capture_primary_surface_bgra8().ok_or(Ui3DebugCaptureError::NoSurface)?;
    let bytes = encode_bgra8_bmp(snapshot.width, snapshot.height, snapshot.pixels.as_slice())?;
    Ok(Ui3DebugImage {
        content_type: "image/bmp",
        filename: "ui3-latest.bmp",
        width: snapshot.width,
        height: snapshot.height,
        bytes,
    })
}

fn encode_bgra8_bmp(width: u32, height: u32, bgra: &[u8]) -> Result<Vec<u8>, Ui3DebugCaptureError> {
    if width == 0 || height == 0 || height > i32::MAX as u32 {
        return Err(Ui3DebugCaptureError::TooLarge);
    }

    let width_usize = width as usize;
    let height_usize = height as usize;
    let row_bytes = width_usize
        .checked_mul(4)
        .ok_or(Ui3DebugCaptureError::TooLarge)?;
    let pixel_bytes = row_bytes
        .checked_mul(height_usize)
        .ok_or(Ui3DebugCaptureError::TooLarge)?;
    if bgra.len() < pixel_bytes {
        return Err(Ui3DebugCaptureError::NoSurface);
    }

    let file_bytes = BMP_HEADER_BYTES
        .checked_add(pixel_bytes)
        .ok_or(Ui3DebugCaptureError::TooLarge)?;
    let file_bytes_u32 = u32::try_from(file_bytes).map_err(|_| Ui3DebugCaptureError::TooLarge)?;
    let pixel_bytes_u32 = u32::try_from(pixel_bytes).map_err(|_| Ui3DebugCaptureError::TooLarge)?;

    let mut out = Vec::new();
    if out.try_reserve_exact(file_bytes).is_err() {
        return Err(Ui3DebugCaptureError::TooLarge);
    }

    out.extend_from_slice(b"BM");
    push_u32_le(&mut out, file_bytes_u32);
    push_u16_le(&mut out, 0);
    push_u16_le(&mut out, 0);
    push_u32_le(&mut out, BMP_HEADER_BYTES as u32);
    push_u32_le(&mut out, BMP_INFO_HEADER_BYTES as u32);
    push_i32_le(&mut out, width as i32);
    push_i32_le(&mut out, -(height as i32));
    push_u16_le(&mut out, 1);
    push_u16_le(&mut out, 32);
    push_u32_le(&mut out, 0);
    push_u32_le(&mut out, pixel_bytes_u32);
    push_i32_le(&mut out, 0);
    push_i32_le(&mut out, 0);
    push_u32_le(&mut out, 0);
    push_u32_le(&mut out, 0);
    out.extend_from_slice(&bgra[..pixel_bytes]);

    Ok(out)
}

#[inline]
fn push_u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[inline]
fn push_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[inline]
fn push_i32_le(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}
