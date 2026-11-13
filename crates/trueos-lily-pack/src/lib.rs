//! Lossless indexed sprite packs. Runtime decoder uses only Rust + alloc,
//! with no filesystem, OS, image decoder, C library or std requirement.
#![no_std]
extern crate alloc;
use alloc::{string::String, vec, vec::Vec};

pub const MAGIC: &[u8; 8] = b"LILYPK01";
pub const PIXELS: usize = 256 * 256;
pub const HEADER: usize = 32;
const MAX_BODY: usize = 512 * 1024 * 1024;
const NO_REF: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid(&'static str),
    Compression,
    NeedsVideoDecoder,
}
impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl core::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct FrameInfo {
    pub path: String,
    /// [left, top, width, height] in a 256x256 canvas.
    pub rect: [u16; 4],
    pub reference: u32,
    pub index_crc32: u32,
}

pub struct DecodedPack {
    pub palette: [[u8; 4]; 32],
    pub frames: Vec<FrameInfo>,
    /// Frame-major 256x256 palette indices, already aligned and reconstructed.
    pub indices: Vec<u8>,
}
impl DecodedPack {
    /// Straight RGBA8, tightly packed rows, original top-left 256x256 canvas.
    pub fn frame_rgba8(&self, frame: usize) -> Result<Vec<u8>, Error> {
        let src = self.frame_indices(frame)?;
        let mut rgba = Vec::with_capacity(PIXELS * 4);
        for &index in src {
            rgba.extend_from_slice(&self.palette[index as usize]);
        }
        Ok(rgba)
    }
    /// Crop metadata supplies source bounds; place the retained pixels at
    /// bottom center of a transparent 256x256 canvas for the cursor pipeline.
    pub fn frame_rgba8_bottom_center(&self, frame: usize) -> Result<Vec<u8>, Error> {
        let src = self.frame_indices(frame)?;
        let [x, y, w, h] = self.frames[frame].rect.map(usize::from);
        let left = (256 - w) / 2;
        let top = 256 - h;
        let mut rgba = vec![0; PIXELS * 4];
        for row in 0..h {
            for col in 0..w {
                let index = src[(y + row) * 256 + x + col] as usize;
                let dst = ((top + row) * 256 + left + col) * 4;
                rgba[dst..dst + 4].copy_from_slice(&self.palette[index]);
            }
        }
        Ok(rgba)
    }

    pub fn frame_indices(&self, frame: usize) -> Result<&[u8], Error> {
        if frame >= self.frames.len() {
            return Err(Error::Invalid("frame index"));
        }
        Ok(&self.indices[frame * PIXELS..(frame + 1) * PIXELS])
    }
    /// Straight RGBA8 matching the cropped PNG, with dimensions in FrameInfo.
    pub fn cropped_rgba8(&self, frame: usize) -> Result<Vec<u8>, Error> {
        let src = self.frame_indices(frame)?;
        let [x, y, w, h] = self.frames[frame].rect.map(usize::from);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for row in y..y + h {
            for &index in &src[row * 256 + x..row * 256 + x + w] {
                rgba.extend_from_slice(&self.palette[index as usize]);
            }
        }
        Ok(rgba)
    }
}

const fn crc_table() -> [u32; 256] {
    let mut table = [0; 256];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut j = 0;
        while j < 8 {
            c = if c & 1 != 0 {
                0xedb88320 ^ (c >> 1)
            } else {
                c >> 1
            };
            j += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
}
const CRC_TABLE: [u32; 256] = crc_table();
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &v in data {
        crc = (crc >> 8) ^ CRC_TABLE[((crc ^ (v as u32)) & 255) as usize];
    }
    !crc
}
struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}
impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(Error::Invalid("length overflow"))?;
        let out = self
            .data
            .get(self.pos..end)
            .ok_or(Error::Invalid("truncated body"))?;
        self.pos = end;
        Ok(out)
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

/// Decode an ordinary pack. For the video experiment use decode_with_video.
pub fn decode(pack: &[u8]) -> Result<DecodedPack, Error> {
    decode_with_video(pack, |_| Err(Error::NeedsVideoDecoder))
}

/// TRUEOS may supply its video decoder here; the callback must return exact
/// frame-major 256x256 8-bit grayscale samples (palette indices, no YUV scaling).
pub fn decode_with_video<F>(pack: &[u8], video_decoder: F) -> Result<DecodedPack, Error>
where
    F: FnOnce(&[u8]) -> Result<Vec<u8>, Error>,
{
    if pack.len() < HEADER || &pack[..8] != MAGIC {
        return Err(Error::Invalid("magic/header"));
    }
    let codec = pack[8];
    let transform = pack[9];
    let tile = u16::from_le_bytes(pack[10..12].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(pack[12..16].try_into().unwrap()) as usize;
    let len64 = u64::from_le_bytes(pack[16..24].try_into().unwrap());
    if len64 > MAX_BODY as u64 || count == 0 || count > 4096 {
        return Err(Error::Invalid("resource limit"));
    }
    let len = len64 as usize;
    let crc = u32::from_le_bytes(pack[24..28].try_into().unwrap());
    if pack[28..32] != [0; 4] {
        return Err(Error::Invalid("reserved header"));
    }
    let payload = &pack[HEADER..];
    let body = match codec {
        0 => miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(payload, len)
            .map_err(|_| Error::Compression)?,
        1 => {
            let mut out = vec![0; len];
            let n = ruzstd::decoding::FrameDecoder::new()
                .decode_all(payload, &mut out)
                .map_err(|_| Error::Compression)?;
            if n != len {
                return Err(Error::Invalid("zstd decoded length"));
            }
            out
        }
        2 => {
            use lzma_rust2::Read;
            let mut reader = lzma_rust2::XzReader::new(payload, false);
            let mut out = vec![0; len];
            reader
                .read_exact(&mut out)
                .map_err(|_| Error::Compression)?;
            let mut extra = [0];
            if reader.read(&mut extra).map_err(|_| Error::Compression)? != 0 {
                return Err(Error::Invalid("xz decoded length"));
            }
            out
        }
        3 => {
            if transform != 0 || payload.len() < 4 {
                return Err(Error::Invalid("video header"));
            }
            let meta = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            if meta > len || meta > payload.len() - 4 {
                return Err(Error::Invalid("video metadata"));
            }
            let indices = video_decoder(&payload[4 + meta..])?;
            if indices.len() != count * PIXELS {
                return Err(Error::Invalid("video decoded length"));
            }
            let mut out = Vec::with_capacity(len);
            out.extend_from_slice(&payload[4..4 + meta]);
            out.extend_from_slice(&indices);
            out
        }
        _ => return Err(Error::Invalid("codec")),
    };
    if body.len() != len || crc32(&body) != crc {
        return Err(Error::Invalid("body checksum/length"));
    }
    let mut reader = Reader {
        data: &body,
        pos: 0,
    };
    let mut palette = [[0u8; 4]; 32];
    for color in &mut palette {
        color.copy_from_slice(reader.take(4)?);
    }
    if palette[0] != [0; 4] || palette[1..].iter().any(|c| c[3] != 255) {
        return Err(Error::Invalid("palette alpha"));
    }
    let mut frames = Vec::with_capacity(count);
    for i in 0..count {
        let n = reader.u16()? as usize;
        let path = core::str::from_utf8(reader.take(n)?)
            .map_err(|_| Error::Invalid("path UTF8"))?
            .into();
        let rect = [reader.u16()?, reader.u16()?, reader.u16()?, reader.u16()?];
        let [x, y, w, h] = rect.map(usize::from);
        if w == 0 || h == 0 || x + w > 256 || y + h > 256 {
            return Err(Error::Invalid("crop rectangle"));
        }
        let reference = reader.u32()?;
        if reference != NO_REF && reference as usize >= i {
            return Err(Error::Invalid("forward reference"));
        }
        frames.push(FrameInfo {
            path,
            rect,
            reference,
            index_crc32: reader.u32()?,
        });
    }
    let data = &body[reader.pos..];
    let expected = count * PIXELS;
    let mut indices = match transform {
        0 | 1 | 4 => {
            if data.len() != expected {
                return Err(Error::Invalid("index data length"));
            }
            data.to_vec()
        }
        2 | 3 => {
            if data.len() != expected * 5 / 8 {
                return Err(Error::Invalid("bitplane data length"));
            }
            let mut out = vec![0u8; expected];
            for f in 0..count {
                for bit in 0..5 {
                    for j in 0..PIXELS / 8 {
                        let b = data[f * PIXELS * 5 / 8 + bit * PIXELS / 8 + j];
                        for k in 0..8 {
                            out[f * PIXELS + j * 8 + k] |= ((b >> k) & 1) << bit;
                        }
                    }
                }
            }
            out
        }
        5 => {
            if ![4, 8, 16].contains(&tile) {
                return Err(Error::Invalid("tile size"));
            }
            let mut r = Reader { data, pos: 0 };
            let dict_count = r.u32()? as usize;
            let tile_bytes = tile * tile;
            let dict = r.take(
                dict_count
                    .checked_mul(tile_bytes)
                    .ok_or(Error::Invalid("tile overflow"))?,
            )?;
            let mut out = vec![0u8; expected];
            for f in 0..count {
                for ty in (0..256).step_by(tile) {
                    for tx in (0..256).step_by(tile) {
                        let id = r.u32()? as usize;
                        if id >= dict_count {
                            return Err(Error::Invalid("tile index"));
                        }
                        for row in 0..tile {
                            out[f * PIXELS + (ty + row) * 256 + tx
                                ..f * PIXELS + (ty + row) * 256 + tx + tile]
                                .copy_from_slice(
                                    &dict[id * tile_bytes + row * tile
                                        ..id * tile_bytes + (row + 1) * tile],
                                );
                        }
                    }
                }
            }
            if r.pos != data.len() {
                return Err(Error::Invalid("tile trailing data"));
            }
            out
        }
        6 => {
            if data.len() != expected * 5 / 8 {
                return Err(Error::Invalid("packed data length"));
            }
            let mut out = vec![0; expected];
            for (i, v) in out.iter_mut().enumerate() {
                let bit = i * 5;
                let byte = bit / 8;
                let shift = bit % 8;
                let word =
                    data[byte] as u16 | (data.get(byte + 1).copied().unwrap_or(0) as u16) << 8;
                *v = ((word >> shift) & 31) as u8;
            }
            out
        }
        _ => return Err(Error::Invalid("transform")),
    };
    if matches!(transform, 1 | 3 | 4) {
        for (i, info) in frames.iter().enumerate() {
            if info.reference != NO_REF {
                let r = info.reference as usize * PIXELS;
                for j in 0..PIXELS {
                    let predicted = indices[r + j];
                    let v = &mut indices[i * PIXELS + j];
                    *v = if transform == 4 {
                        v.wrapping_add(predicted) & 31
                    } else {
                        *v ^ predicted
                    };
                }
            }
        }
    }
    for (i, info) in frames.iter().enumerate() {
        let frame = &indices[i * PIXELS..(i + 1) * PIXELS];
        if frame.iter().any(|&v| v >= 32) || crc32(frame) != info.index_crc32 {
            return Err(Error::Invalid("frame checksum/index"));
        }
    }
    Ok(DecodedPack {
        palette,
        frames,
        indices,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(codec: u8, transform: u8) -> Vec<u8> {
        let frame = vec![1; PIXELS];
        let mut body = vec![0; 128];
        for c in body[4..].chunks_mut(4) {
            c.copy_from_slice(&[12, 34, 56, 255]);
        }
        body.extend_from_slice(&1u16.to_le_bytes());
        body.push(b'a');
        for v in [0u16, 0, 256, 256] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        body.extend_from_slice(&NO_REF.to_le_bytes());
        body.extend_from_slice(&crc32(&frame).to_le_bytes());
        body.extend_from_slice(&frame);
        let mut pack = MAGIC.to_vec();
        pack.extend_from_slice(&[codec, transform, 0, 0]);
        pack.extend_from_slice(&1u32.to_le_bytes());
        pack.extend_from_slice(&(body.len() as u64).to_le_bytes());
        pack.extend_from_slice(&crc32(&body).to_le_bytes());
        pack.extend_from_slice(&[0; 4]);
        pack.extend_from_slice(&miniz_oxide::deflate::compress_to_vec_zlib(&body, 6));
        pack
    }
    #[test]
    fn rgba_and_crop_roundtrip() {
        let p = decode(&fixture(0, 0)).unwrap();
        assert_eq!(p.frame_rgba8(0).unwrap(), [12, 34, 56, 255].repeat(PIXELS));
        assert_eq!(p.cropped_rgba8(0).unwrap(), p.frame_rgba8(0).unwrap());
    }
    #[test]
    fn rejects_corrupt_and_truncated() {
        let mut p = fixture(0, 0);
        p[24] ^= 1;
        assert!(decode(&p).is_err());
        for n in [0, 7, 31, 40] {
            assert!(decode(&p[..n]).is_err());
        }
    }
    #[test]
    fn rejects_unknown_transform() {
        assert!(decode(&fixture(0, 255)).is_err());
    }
    #[test]
    fn crc_known_answer() {
        assert_eq!(crc32(b"123456789"), 0xcbf43926);
    }
}
