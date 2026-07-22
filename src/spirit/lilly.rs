//! Embedded Lilly frame archive and its persistent render-PPGTT catalog.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const LILLY_ARCHIVE_7Z: &[u8] = include_bytes!("../../tools/Lilly.7z");
const LILLY_CATALOG: &str = include_str!("../../tools/Lilly.catalog");
const LILLY_EXPECTED_ASSETS: usize = 72;
const LILLY_PARTS_PER_ASSET: usize = 4;
const LILLY_EXPECTED_FRAMES: usize = LILLY_EXPECTED_ASSETS * LILLY_PARTS_PER_ASSET;
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

/// Default sequencing behavior described by the reviewed Lilly catalog.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyPlayback {
    Loop,
    Once,
    OnceHold,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyPose {
    CrossedArms,
    UncrossedArms,
}

/// The four archive members are usually animation frames. The one exception,
/// `static.crossed_arms`, is a 2x2 grid of 64x64 tiles for one 128x128 still.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyAssetKind {
    Animation {
        playback: LillyPlayback,
        frame_period_ms: u16,
    },
    TileGrid2x2,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentPart {
    /// Meaning within the asset, such as `mouth_wide` or `top_left`.
    pub(crate) semantic: &'static str,
    pub(crate) surface: LillyResidentFrame,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAsset {
    /// Stable semantic identity, such as `talk.calm.crossed`.
    pub(crate) key: &'static str,
    pub(crate) kind: LillyAssetKind,
    pub(crate) entry_pose: LillyPose,
    pub(crate) exit_pose: LillyPose,
    pub(crate) parts: [LillyResidentPart; LILLY_PARTS_PER_ASSET],
}

#[allow(dead_code)]
impl LillyResidentAsset {
    pub(crate) fn part(self, semantic: &str) -> Option<LillyResidentFrame> {
        self.parts
            .iter()
            .find_map(|part| (part.semantic == semantic).then_some(part.surface))
    }
}

/// Uniform view of one ordinary four-frame animation. Callers do not need
/// clip-specific code: the catalog supplies cadence, playback, pose, and the
/// semantic names of all four frames.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAnimation {
    pub(crate) key: &'static str,
    pub(crate) playback: LillyPlayback,
    pub(crate) frame_period_ms: u16,
    pub(crate) entry_pose: LillyPose,
    pub(crate) exit_pose: LillyPose,
    pub(crate) frames: [LillyResidentPart; LILLY_PARTS_PER_ASSET],
}

#[allow(dead_code)]
impl LillyResidentAnimation {
    pub(crate) const fn cycle_duration_ms(self) -> u64 {
        self.frame_period_ms as u64 * LILLY_PARTS_PER_ASSET as u64
    }

    /// Shared inner-loop helper for every clip. `Loop` wraps, `Once` ends, and
    /// `OnceHold` keeps its final frame; callers need no tag-specific logic.
    pub(crate) fn frame_at_elapsed(self, elapsed_ms: u64) -> Option<LillyResidentPart> {
        let period = u64::from(self.frame_period_ms);
        if period == 0 {
            return None;
        }
        let frame = elapsed_ms / period;
        let index = match self.playback {
            LillyPlayback::Loop => frame % LILLY_PARTS_PER_ASSET as u64,
            LillyPlayback::Once if frame >= LILLY_PARTS_PER_ASSET as u64 => return None,
            LillyPlayback::Once => frame,
            LillyPlayback::OnceHold => frame.min(LILLY_PARTS_PER_ASSET as u64 - 1),
        };
        Some(self.frames[index as usize])
    }
}

struct LillyFrameEntry {
    name: String,
    surface: LillyResidentFrame,
}

struct LillyResidentAssets {
    allocation: crate::intel::render::ResidentRenderBuffer,
    frames: Vec<LillyFrameEntry>,
    semantic_assets: Vec<LillyResidentAsset>,
    rgba_bytes: usize,
}

#[derive(Copy, Clone)]
struct LillyCatalogEntry {
    key: &'static str,
    directory: &'static str,
    kind: LillyAssetKind,
    entry_pose: LillyPose,
    exit_pose: LillyPose,
    parts: [&'static str; LILLY_PARTS_PER_ASSET],
}

#[derive(Debug)]
enum LillyLoadError {
    Archive(crate::z7::SevenZError),
    ArchiveShape,
    PngHeader,
    PngDecode(i32),
    Catalog(&'static str),
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
        "trueos-spirit: first-job complete job=lilly-resident-assets frames={} semantic_assets={} animated_clips={} tiled_stills={} rgba_bytes=0x{:X} mapped_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} ppgtt=render pat=0 cache=wb cpu_uploads=1 persistent=1\n",
        assets.frames.len(),
        assets.semantic_assets.len(),
        assets
            .semantic_assets
            .iter()
            .filter(|asset| matches!(asset.kind, LillyAssetKind::Animation { .. }))
            .count(),
        assets
            .semantic_assets
            .iter()
            .filter(|asset| matches!(asset.kind, LillyAssetKind::TileGrid2x2))
            .count(),
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

/// Resolve a complete resident asset by semantic identity. This is the normal
/// Spirit-facing API; archive paths and frame numbers remain an implementation
/// detail of the warm loader.
#[allow(dead_code)]
pub(crate) fn resident_asset(key: &str) -> Option<LillyResidentAsset> {
    LILLY_RESIDENT
        .lock()
        .as_ref()?
        .semantic_assets
        .iter()
        .copied()
        .find(|asset| asset.key == key)
}

/// Resolve one meaningful part directly, for example
/// `resident_part("talk.calm.crossed", "mouth_wide")`.
#[allow(dead_code)]
pub(crate) fn resident_part(key: &str, semantic: &str) -> Option<LillyResidentFrame> {
    resident_asset(key)?.part(semantic)
}

/// Resolve any normal animation through one central path. Tiled stills are
/// intentionally rejected because treating quadrants as temporal frames would
/// produce corrupt presentation.
#[allow(dead_code)]
pub(crate) fn resident_animation(key: &str) -> Option<LillyResidentAnimation> {
    let asset = resident_asset(key)?;
    let LillyAssetKind::Animation {
        playback,
        frame_period_ms,
    } = asset.kind
    else {
        return None;
    };
    Some(LillyResidentAnimation {
        key: asset.key,
        playback,
        frame_period_ms,
        entry_pose: asset.entry_pose,
        exit_pose: asset.exit_pose,
        frames: asset.parts,
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
    let catalog = parse_catalog()?;
    let mut entries = crate::z7::extract_all_to_vec(LILLY_ARCHIVE_7Z)?;
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    validate_archive_shape(entries.as_slice())?;
    validate_catalog(entries.as_slice(), catalog.as_slice())?;

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
    let semantic_assets = match build_semantic_assets(frames.as_slice(), catalog.as_slice()) {
        Ok(assets) => assets,
        Err(error) => {
            if !crate::intel::render::release_resident_render_buffer(&allocation) {
                crate::log_error!(
                    target: "gfx";
                    "trueos-spirit: lilly catalog allocation retained reason=ppgtt-unmap-failed phys=0x{:X} gpu=0x{:X} bytes=0x{:X}\n",
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
        semantic_assets,
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

fn parse_catalog() -> Result<Vec<LillyCatalogEntry>, LillyLoadError> {
    let mut catalog: Vec<LillyCatalogEntry> = Vec::with_capacity(LILLY_EXPECTED_ASSETS);
    for raw_line in LILLY_CATALOG.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut fields = line.split('|');
        let key = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-key"))?;
        let directory = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-directory"))?;
        let kind_name = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-kind"))?;
        let period = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-period"))?
            .parse::<u16>()
            .map_err(|_| LillyLoadError::Catalog("bad-period"))?;
        let pose_name = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-pose"))?;
        let part_names = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-parts"))?;
        if fields.next().is_some() {
            return Err(LillyLoadError::Catalog("extra-field"));
        }
        if !is_semantic_key(key)
            || directory.is_empty()
            || directory.starts_with('/')
            || directory.starts_with("Lilly/")
            || !directory.ends_with("_frames")
        {
            return Err(LillyLoadError::Catalog("bad-identity"));
        }
        if catalog
            .iter()
            .any(|entry| entry.key == key || entry.directory == directory)
        {
            return Err(LillyLoadError::Catalog("duplicate-identity"));
        }

        let kind = match kind_name {
            "loop" if period != 0 => LillyAssetKind::Animation {
                playback: LillyPlayback::Loop,
                frame_period_ms: period,
            },
            "once" if period != 0 => LillyAssetKind::Animation {
                playback: LillyPlayback::Once,
                frame_period_ms: period,
            },
            "once_hold" if period != 0 => LillyAssetKind::Animation {
                playback: LillyPlayback::OnceHold,
                frame_period_ms: period,
            },
            "tile_2x2" if period == 0 => LillyAssetKind::TileGrid2x2,
            _ => return Err(LillyLoadError::Catalog("bad-kind-or-period")),
        };
        let (entry_pose, exit_pose) = parse_pose(pose_name)?;

        let mut names = part_names.split(',');
        let parts = [
            names
                .next()
                .ok_or(LillyLoadError::Catalog("missing-part"))?,
            names
                .next()
                .ok_or(LillyLoadError::Catalog("missing-part"))?,
            names
                .next()
                .ok_or(LillyLoadError::Catalog("missing-part"))?,
            names
                .next()
                .ok_or(LillyLoadError::Catalog("missing-part"))?,
        ];
        if names.next().is_some()
            || parts.iter().any(|part| !is_semantic_part(part))
            || parts
                .iter()
                .enumerate()
                .any(|(index, part)| parts[..index].contains(part))
        {
            return Err(LillyLoadError::Catalog("bad-parts"));
        }
        catalog.push(LillyCatalogEntry {
            key,
            directory,
            kind,
            entry_pose,
            exit_pose,
            parts,
        });
    }
    if catalog.len() != LILLY_EXPECTED_ASSETS {
        return Err(LillyLoadError::Catalog("asset-count"));
    }
    Ok(catalog)
}

fn parse_pose(value: &str) -> Result<(LillyPose, LillyPose), LillyLoadError> {
    match value {
        "crossed" => Ok((LillyPose::CrossedArms, LillyPose::CrossedArms)),
        "uncrossed" => Ok((LillyPose::UncrossedArms, LillyPose::UncrossedArms)),
        "crossed>uncrossed" => Ok((LillyPose::CrossedArms, LillyPose::UncrossedArms)),
        "uncrossed>crossed" => Ok((LillyPose::UncrossedArms, LillyPose::CrossedArms)),
        _ => Err(LillyLoadError::Catalog("bad-pose")),
    }
}

fn is_semantic_key(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('.')
        && !value.ends_with('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"._".contains(&byte))
}

fn is_semantic_part(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn validate_catalog(
    entries: &[crate::z7::SevenZEntry],
    catalog: &[LillyCatalogEntry],
) -> Result<(), LillyLoadError> {
    for archive_asset in entries.chunks_exact(LILLY_PARTS_PER_ASSET) {
        let (directory, _) =
            split_frame_name(archive_asset[0].name.as_str()).ok_or(LillyLoadError::ArchiveShape)?;
        if !catalog.iter().any(|entry| entry.directory == directory) {
            return Err(LillyLoadError::Catalog("archive-asset-unmapped"));
        }
    }
    for catalog_entry in catalog {
        let archive_asset = entries
            .chunks_exact(LILLY_PARTS_PER_ASSET)
            .find(|asset| {
                split_frame_name(asset[0].name.as_str())
                    .is_some_and(|(directory, _)| directory == catalog_entry.directory)
            })
            .ok_or(LillyLoadError::Catalog("catalog-asset-missing"))?;
        let expected_dimensions = match catalog_entry.kind {
            LillyAssetKind::Animation { .. } => (128, 128),
            LillyAssetKind::TileGrid2x2 => (64, 64),
        };
        for part in archive_asset {
            if png_ihdr_dimensions(part.bytes.as_slice())? != expected_dimensions {
                return Err(LillyLoadError::Catalog("layout-dimensions"));
            }
        }
    }
    Ok(())
}

fn build_semantic_assets(
    frames: &[LillyFrameEntry],
    catalog: &[LillyCatalogEntry],
) -> Result<Vec<LillyResidentAsset>, LillyLoadError> {
    let mut assets = Vec::with_capacity(catalog.len());
    for catalog_entry in catalog {
        let source = frames
            .chunks_exact(LILLY_PARTS_PER_ASSET)
            .find(|asset| {
                split_frame_name(asset[0].name.as_str())
                    .is_some_and(|(directory, _)| directory == catalog_entry.directory)
            })
            .ok_or(LillyLoadError::Catalog("resident-asset-missing"))?;
        assets.push(LillyResidentAsset {
            key: catalog_entry.key,
            kind: catalog_entry.kind,
            entry_pose: catalog_entry.entry_pose,
            exit_pose: catalog_entry.exit_pose,
            parts: [
                LillyResidentPart {
                    semantic: catalog_entry.parts[0],
                    surface: source[0].surface,
                },
                LillyResidentPart {
                    semantic: catalog_entry.parts[1],
                    surface: source[1].surface,
                },
                LillyResidentPart {
                    semantic: catalog_entry.parts[2],
                    surface: source[2].surface,
                },
                LillyResidentPart {
                    semantic: catalog_entry.parts[3],
                    surface: source[3].surface,
                },
            ],
        });
    }
    Ok(assets)
}

fn validate_archive_shape(entries: &[crate::z7::SevenZEntry]) -> Result<(), LillyLoadError> {
    if entries.len() != LILLY_EXPECTED_FRAMES {
        return Err(LillyLoadError::ArchiveShape);
    }
    for asset in entries.chunks_exact(LILLY_PARTS_PER_ASSET) {
        let (expected_directory, first_index) =
            split_frame_name(asset[0].name.as_str()).ok_or(LillyLoadError::ArchiveShape)?;
        if first_index != 1 {
            return Err(LillyLoadError::ArchiveShape);
        }
        for (position, entry) in asset.iter().enumerate() {
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
