//! Standard LZ4 block and frame service. No archive or filesystem semantics.
extern crate alloc;
use alloc::{vec, vec::Vec};
use core::ops::Range;

pub const MAGIC: [u8; 4] = [0x04, 0x22, 0x4d, 0x18];
pub const ENCODE_BLOCK_BYTES: usize = 4096;
const MAX_BLOCKS: usize = 65536;
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Invalid,
    Unsupported,
    Limit,
    Worker,
    Gpu,
}
fn hash(bytes: &[u8]) -> u32 {
    twox_hash::XxHash32::oneshot(0, bytes)
}
fn word(bytes: &[u8]) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        bytes
            .get(..4)
            .ok_or(Error::Invalid)?
            .try_into()
            .map_err(|_| Error::Invalid)?,
    ))
}
struct Block {
    range: Range<usize>,
    raw: bool,
}
struct Frame {
    blocks: Vec<Block>,
    max_block: usize,
    independent: bool,
    content_size: Option<usize>,
    checksum: Option<u32>,
}
fn parse(bytes: &[u8], max_output: usize) -> Result<Frame, Error> {
    if bytes.get(..4) != Some(&MAGIC) {
        return Err(Error::Invalid);
    }
    let flags = *bytes.get(4).ok_or(Error::Invalid)?;
    let bd = *bytes.get(5).ok_or(Error::Invalid)?;
    if flags >> 6 != 1 || flags & 2 != 0 || bd & 0x8f != 0 {
        return Err(Error::Invalid);
    }
    if flags & 1 != 0 {
        return Err(Error::Unsupported);
    } // external dictionary
    let max_block = match (bd >> 4) & 7 {
        4 => 65536,
        5 => 262144,
        6 => 1048576,
        7 => 4194304,
        _ => return Err(Error::Invalid),
    };
    let mut pos = 6usize;
    let content_size = if flags & 8 != 0 {
        let size = u64::from_le_bytes(
            bytes
                .get(pos..pos + 8)
                .ok_or(Error::Invalid)?
                .try_into()
                .map_err(|_| Error::Invalid)?,
        );
        pos += 8;
        let size = usize::try_from(size).map_err(|_| Error::Limit)?;
        if size > max_output {
            return Err(Error::Limit);
        }
        Some(size)
    } else {
        None
    };
    if bytes.get(pos).copied() != Some((hash(&bytes[4..pos]) >> 8) as u8) {
        return Err(Error::Invalid);
    }
    pos += 1;
    let mut blocks = Vec::new();
    loop {
        let descriptor = word(bytes.get(pos..).ok_or(Error::Invalid)?)?;
        pos += 4;
        if descriptor == 0 {
            break;
        }
        let size = (descriptor & 0x7fffffff) as usize;
        if size == 0 || size > max_block {
            return Err(Error::Invalid);
        }
        if blocks.len() == MAX_BLOCKS {
            return Err(Error::Limit);
        }
        let end = pos.checked_add(size).ok_or(Error::Limit)?;
        let data = bytes.get(pos..end).ok_or(Error::Invalid)?;
        blocks.push(Block {
            range: pos..end,
            raw: descriptor >> 31 != 0,
        });
        pos = end;
        if flags & 16 != 0 {
            if word(bytes.get(pos..).ok_or(Error::Invalid)?)? != hash(data) {
                return Err(Error::Invalid);
            }
            pos += 4;
        }
    }
    let checksum = if flags & 4 != 0 {
        let checksum = word(bytes.get(pos..).ok_or(Error::Invalid)?)?;
        pos += 4;
        Some(checksum)
    } else {
        None
    };
    // Concatenated/skippable frames need explicit handling; never silently
    // ignore data after a valid frame or accept a partially consumed archive.
    if pos != bytes.len() {
        return Err(Error::Unsupported);
    }
    Ok(Frame {
        blocks,
        max_block,
        independent: flags & 32 != 0,
        content_size,
        checksum,
    })
}
fn verify(frame: &Frame, output: &[u8]) -> Result<(), Error> {
    if frame.content_size.is_some_and(|size| size != output.len())
        || frame
            .checksum
            .is_some_and(|checksum| hash(output) != checksum)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
fn header(size: usize) -> Vec<u8> {
    let mut out = MAGIC.to_vec();
    out.extend_from_slice(&[0x6c, 0x40]); // independent, size, content checksum; max 64KiB
    out.extend_from_slice(&(size as u64).to_le_bytes());
    out.push((hash(&out[4..]) >> 8) as u8);
    out
}
fn append_block(out: &mut Vec<u8>, source: &[u8], encoded: &[u8]) {
    let (data, flag) = if encoded.len() < source.len() {
        (encoded, 0)
    } else {
        (source, 0x80000000)
    };
    out.extend_from_slice(&((data.len() as u32) | flag).to_le_bytes());
    out.extend_from_slice(data);
}
fn finish(out: &mut Vec<u8>, source: &[u8]) {
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&hash(source).to_le_bytes());
}

/// CPU reference/fallback. The caller is responsible for executor placement.
pub fn compress_frame_cpu(input: &[u8]) -> Vec<u8> {
    let mut out = header(input.len());
    for block in input.chunks(ENCODE_BLOCK_BYTES) {
        append_block(&mut out, block, &lz4_flex::block::compress(block));
    }
    finish(&mut out, input);
    out
}
pub fn decompress_frame_cpu(input: &[u8], max_output: usize) -> Result<Vec<u8>, Error> {
    let frame = parse(input, max_output)?;
    let mut out = Vec::new();
    for block in &frame.blocks {
        let data = &input[block.range.clone()];
        let available = max_output.saturating_sub(out.len()).min(frame.max_block);
        if block.raw {
            if data.len() > available {
                return Err(Error::Limit);
            }
            out.extend_from_slice(data);
        } else {
            let mut decoded = vec![0; available];
            let length = if frame.independent {
                lz4_flex::block::decompress_into(data, &mut decoded)
            } else {
                lz4_flex::block::decompress_into_with_dict(
                    data,
                    &mut decoded,
                    &out[out.len().saturating_sub(65536)..],
                )
            }
            .map_err(|_| Error::Invalid)?;
            out.extend_from_slice(&decoded[..length]);
        }
    }
    verify(&frame, &out)?;
    Ok(out)
}

/// Transform independent blocks on the GPU, with bounded CPU-pool fallback
/// when no supported GPU is available. This is the reusable primitive for
/// archives, asset loaders, and other kernel services. Capacities are explicit.
pub async fn blocks(input: Vec<(Vec<u8>, usize)>, encode: bool) -> Result<Vec<Vec<u8>>, Error> {
    if input.len() > 256 {
        return Err(Error::Limit);
    }
    let mut total_input = 0usize;
    let mut total_output = 0usize;
    for (bytes, capacity) in &input {
        if bytes.len() > if encode { 65536 } else { 4194304 }
            || *capacity > 4194304 + 16464
            || (encode && *capacity < bytes.len() + bytes.len() / 255 + 16)
        {
            return Err(Error::Limit);
        }
        total_input = total_input.checked_add(bytes.len()).ok_or(Error::Limit)?;
        total_output = total_output.checked_add(*capacity).ok_or(Error::Limit)?;
    }
    if total_input > 5 * 1024 * 1024 || total_output > 5 * 1024 * 1024 {
        return Err(Error::Limit);
    }
    if crate::intel::gpgpu::lz4_gpu_available() {
        let borrowed: Vec<_> = input
            .iter()
            .map(|(bytes, cap)| (bytes.as_slice(), *cap))
            .collect();
        return crate::intel::gpgpu::lz4_gpu_blocks(&borrowed, encode)
            .await
            .map_err(|_| Error::Gpu);
    }
    super::codec::CODEC_COMPUTE
        .run("codec/lz4-blocks", move |_| {
            input
                .into_iter()
                .map(|(bytes, cap)| {
                    if bytes.len() > 4194304 || cap > 4194304 + 16464 {
                        return Err(Error::Limit);
                    }
                    if encode {
                        let out = lz4_flex::block::compress(&bytes);
                        if out.len() > cap {
                            return Err(Error::Limit);
                        }
                        Ok(out)
                    } else {
                        let mut out = vec![0; cap];
                        let size = lz4_flex::block::decompress_into(&bytes, &mut out)
                            .map_err(|_| Error::Invalid)?;
                        out.truncate(size);
                        Ok(out)
                    }
                })
                .collect()
        })
        .await
        .map_err(|_| Error::Worker)?
}

pub async fn compress_frame(input: Vec<u8>) -> Result<Vec<u8>, Error> {
    if !crate::intel::gpgpu::lz4_gpu_available() {
        return super::codec::CODEC_COMPUTE
            .run("codec/lz4-encode", move |_| compress_frame_cpu(&input))
            .await
            .map_err(|_| Error::Worker);
    }
    let mut out = header(input.len());
    for batch in input.chunks(ENCODE_BLOCK_BYTES * 256) {
        let blocks: Vec<_> = batch
            .chunks(ENCODE_BLOCK_BYTES)
            .map(|block| (block, block.len() + block.len() / 255 + 16))
            .collect();
        let encoded = crate::intel::gpgpu::lz4_gpu_blocks(&blocks, true)
            .await
            .map_err(|_| Error::Gpu)?;
        for ((source, _), compressed) in blocks.iter().zip(encoded.iter()) {
            append_block(&mut out, source, compressed);
        }
    }
    finish(&mut out, &input);
    Ok(out)
}

pub async fn decompress_frame(input: Vec<u8>, max_output: usize) -> Result<Vec<u8>, Error> {
    let frame = parse(&input, max_output)?;
    if !frame.independent || !crate::intel::gpgpu::lz4_gpu_available() {
        return super::codec::CODEC_COMPUTE
            .run("codec/lz4-decode", move |_| decompress_frame_cpu(&input, max_output))
            .await
            .map_err(|_| Error::Worker)?;
    }
    let mut out = Vec::new();
    let per_batch = (4194304 / frame.max_block).min(256).max(1);
    for batch in frame.blocks.chunks(per_batch) {
        let capacity = frame.max_block.min(max_output.saturating_sub(out.len()));
        let compressed: Vec<_> = batch
            .iter()
            .filter(|block| !block.raw)
            .map(|block| (&input[block.range.clone()], capacity))
            .collect();
        let decoded = crate::intel::gpgpu::lz4_gpu_blocks(&compressed, false)
            .await
            .map_err(|_| Error::Gpu)?;
        let mut decoded = decoded.into_iter();
        for block in batch {
            let owned;
            let bytes = if block.raw {
                &input[block.range.clone()]
            } else {
                owned = decoded.next().ok_or(Error::Invalid)?;
                &owned
            };
            if bytes.len() > max_output.saturating_sub(out.len()) {
                return Err(Error::Limit);
            }
            out.extend_from_slice(bytes);
        }
    }
    verify(&frame, &out)?;
    Ok(out)
}
