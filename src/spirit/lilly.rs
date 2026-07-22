//! Embedded Lilly frame archive and its persistent render-PPGTT catalog.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const LILLY_ARCHIVE_7Z: &[u8] = include_bytes!("../../tools/Lilly.7z");
const LILLY_EXPECTED_ANIMATIONS: usize = 72;
const LILLY_FRAMES_PER_ANIMATION: usize = 4;
const LILLY_EXPECTED_FRAMES: usize = LILLY_EXPECTED_ANIMATIONS * LILLY_FRAMES_PER_ANIMATION;
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";
const PNG_IHDR: &[u8; 4] = b"IHDR";
const GPU_PAGE_BYTES: usize = 4096;

static LILLY_RESIDENT: Mutex<Option<LillyResidentAssets>> = Mutex::new(None);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentFrame {
    pub(crate) phys: u64,
    pub(crate) gpu: u64,
    pub(crate) bytes: usize,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) pitch_bytes: u32,
}

struct LillyFrameEntry {
    name: String,
    surface: LillyResidentFrame,
}

struct LillyResidentAssets {
    allocation: crate::intel::render::ResidentRenderBuffer,
    frames: Vec<LillyFrameEntry>,
    rgba_bytes: usize,
}

#[derive(Debug)]
enum LillyLoadError {
    Archive(crate::z7::SevenZError),
    ArchiveShape,
    PngHeader,
    PngDecode(i32),
    AddressOverflow,
    Resident(&'static str),
    ResidentWrite,
}

impl From<crate::z7::SevenZError> for LillyLoadError {
    fn from(value: crate::z7::SevenZError) -> Self {
        Self::Archive(value)
    }
}

/// Spirit's cold first job. The archive, decoded PNGs, DMA allocation, and
/// render PPGTT mapping are all established before continuous frame work.
pub(super) fn prepare_resident_once() -> bool {
    if LILLY_RESIDENT.lock().is_some() {
        return true;
    }

    crate::log_info!(
        target: "gfx";
        "trueos-spirit: first-job start job=lilly-resident-assets source=embedded-7z archive_bytes=0x{:X} decode=kernel-z7+png target=intel-render-ppgtt lifetime=runtime\n",
        LILLY_ARCHIVE_7Z.len(),
    );
    let assets = match load_resident_assets() {
        Ok(assets) => assets,
        Err(error) => {
            crate::log_error!(
                target: "gfx";
                "trueos-spirit: first-job failed job=lilly-resident-assets error={:?} archive_bytes=0x{:X} action=retain-existing-spirit-stream\n",
                error,
                LILLY_ARCHIVE_7Z.len(),
            );
            return false;
        }
    };
    crate::log_info!(
        target: "gfx";
        "trueos-spirit: first-job complete job=lilly-resident-assets frames={} animations={} rgba_bytes=0x{:X} mapped_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} ppgtt=render pat=0 cache=wb cpu_uploads=1 persistent=1\n",
        assets.frames.len(),
        assets.frames.len() / LILLY_FRAMES_PER_ANIMATION,
        assets.rgba_bytes,
        assets.allocation.storage_bytes(),
        assets.allocation.storage_phys(),
        assets.allocation.gpu_base(),
    );
    *LILLY_RESIDENT.lock() = Some(assets);
    true
}

/// Resolve one decoded frame without exposing the CPU mapping or allocation
/// owner. Both archive-relative and `Lilly/`-prefixed names are accepted.
#[allow(dead_code)]
pub(crate) fn resident_frame(name: &str) -> Option<LillyResidentFrame> {
    let resident = LILLY_RESIDENT.lock();
    resident.as_ref()?.frames.iter().find_map(|frame| {
        let archive_relative = frame.name.strip_prefix("Lilly/").unwrap_or(&frame.name);
        (frame.name == name || archive_relative == name).then_some(frame.surface)
    })
}

#[allow(dead_code)]
pub(crate) fn resident_frame_count() -> usize {
    LILLY_RESIDENT
        .lock()
        .as_ref()
        .map_or(0, |assets| assets.frames.len())
}

fn load_resident_assets() -> Result<LillyResidentAssets, LillyLoadError> {
    let mut entries = crate::z7::extract_all_to_vec(LILLY_ARCHIVE_7Z)?;
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    validate_archive_shape(entries.as_slice())?;

    let mut storage_bytes = 0usize;
    for entry in &entries {
        let (width, height) = png_ihdr_dimensions(entry.bytes.as_slice())?;
        validate_dimensions(width, height)?;
        storage_bytes = storage_bytes
            .checked_add(frame_storage_bytes(width, height)?)
            .ok_or(LillyLoadError::AddressOverflow)?;
    }

    let allocation = crate::intel::render::allocate_resident_render_buffer(storage_bytes)
        .map_err(LillyLoadError::Resident)?;
    let populated = populate_resident_frames(&allocation, entries, storage_bytes);
    let (frames, rgba_bytes) = match populated {
        Ok(result) => result,
        Err(error) => {
            if !crate::intel::render::release_resident_render_buffer(&allocation) {
                crate::log_error!(
                    target: "gfx";
                    "trueos-spirit: lilly partial resident allocation retained reason=ppgtt-unmap-failed phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
                    allocation.storage_phys(),
                    allocation.gpu_base(),
                    allocation.storage_bytes(),
                );
            }
            return Err(error);
        }
    };
    allocation.flush();
    Ok(LillyResidentAssets {
        allocation,
        frames,
        rgba_bytes,
    })
}

fn populate_resident_frames(
    allocation: &crate::intel::render::ResidentRenderBuffer,
    entries: Vec<crate::z7::SevenZEntry>,
    expected_storage_bytes: usize,
) -> Result<(Vec<LillyFrameEntry>, usize), LillyLoadError> {
    let mut frames = Vec::with_capacity(entries.len());
    let mut offset = 0usize;
    let mut rgba_bytes = 0usize;
    for entry in entries {
        let decoded = crate::graphics::png_codec::decode_png_rgba(entry.bytes.as_slice())
            .map_err(|error| LillyLoadError::PngDecode(error.code()))?;
        validate_dimensions(decoded.width, decoded.height)?;
        let expected_rgba_bytes = rgba_bytes_for(decoded.width, decoded.height)?;
        if decoded.rgba.len() != expected_rgba_bytes || !offset.is_multiple_of(GPU_PAGE_BYTES) {
            return Err(LillyLoadError::ArchiveShape);
        }
        if !allocation.write(offset, decoded.rgba.as_slice()) {
            return Err(LillyLoadError::ResidentWrite);
        }
        let phys = allocation
            .storage_phys()
            .checked_add(offset as u64)
            .ok_or(LillyLoadError::AddressOverflow)?;
        let gpu = allocation
            .gpu_base()
            .checked_add(offset as u64)
            .ok_or(LillyLoadError::AddressOverflow)?;
        frames.push(LillyFrameEntry {
            name: entry.name,
            surface: LillyResidentFrame {
                phys,
                gpu,
                bytes: expected_rgba_bytes,
                width: decoded.width,
                height: decoded.height,
                pitch_bytes: decoded
                    .width
                    .checked_mul(4)
                    .ok_or(LillyLoadError::AddressOverflow)?,
            },
        });
        rgba_bytes = rgba_bytes
            .checked_add(expected_rgba_bytes)
            .ok_or(LillyLoadError::AddressOverflow)?;
        offset = offset
            .checked_add(frame_storage_bytes(decoded.width, decoded.height)?)
            .ok_or(LillyLoadError::AddressOverflow)?;
    }
    if offset != expected_storage_bytes || offset != allocation.storage_bytes() {
        return Err(LillyLoadError::ArchiveShape);
    }
    Ok((frames, rgba_bytes))
}

fn validate_archive_shape(entries: &[crate::z7::SevenZEntry]) -> Result<(), LillyLoadError> {
    if entries.len() != LILLY_EXPECTED_FRAMES {
        return Err(LillyLoadError::ArchiveShape);
    }
    for animation in entries.chunks_exact(LILLY_FRAMES_PER_ANIMATION) {
        let (expected_directory, first_index) =
            split_frame_name(animation[0].name.as_str()).ok_or(LillyLoadError::ArchiveShape)?;
        if first_index != 1 {
            return Err(LillyLoadError::ArchiveShape);
        }
        for (position, entry) in animation.iter().enumerate() {
            let (directory, index) =
                split_frame_name(entry.name.as_str()).ok_or(LillyLoadError::ArchiveShape)?;
            if directory != expected_directory || index as usize != position + 1 {
                return Err(LillyLoadError::ArchiveShape);
            }
        }
    }
    Ok(())
}

fn split_frame_name(name: &str) -> Option<(&str, u8)> {
    let relative = name.strip_prefix("Lilly/")?;
    let (directory, file) = relative.rsplit_once('/')?;
    if !directory.ends_with("_frames") {
        return None;
    }
    let index = match file {
        "frame_01.png" => 1,
        "frame_02.png" => 2,
        "frame_03.png" => 3,
        "frame_04.png" => 4,
        _ => return None,
    };
    Some((directory, index))
}

fn png_ihdr_dimensions(bytes: &[u8]) -> Result<(u32, u32), LillyLoadError> {
    if bytes.get(..8) != Some(PNG_SIGNATURE)
        || bytes.get(12..16) != Some(PNG_IHDR)
        || bytes.get(8..12) != Some(&13u32.to_be_bytes())
    {
        return Err(LillyLoadError::PngHeader);
    }
    let width = u32::from_be_bytes(
        bytes
            .get(16..20)
            .ok_or(LillyLoadError::PngHeader)?
            .try_into()
            .map_err(|_| LillyLoadError::PngHeader)?,
    );
    let height = u32::from_be_bytes(
        bytes
            .get(20..24)
            .ok_or(LillyLoadError::PngHeader)?
            .try_into()
            .map_err(|_| LillyLoadError::PngHeader)?,
    );
    Ok((width, height))
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), LillyLoadError> {
    match (width, height) {
        (64, 64) | (128, 128) => Ok(()),
        _ => Err(LillyLoadError::ArchiveShape),
    }
}

fn rgba_bytes_for(width: u32, height: u32) -> Result<usize, LillyLoadError> {
    (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(LillyLoadError::AddressOverflow)
}

fn frame_storage_bytes(width: u32, height: u32) -> Result<usize, LillyLoadError> {
    let rgba_bytes = rgba_bytes_for(width, height)?;
    crate::intel::align_up(rgba_bytes, GPU_PAGE_BYTES).ok_or(LillyLoadError::AddressOverflow)
}
