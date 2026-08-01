//! Embedded Lilly frame archive and its execution-context-owned catalog.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

const LILLY_ARCHIVE_7Z: &[u8] = include_bytes!("../../tools/Lilly.7z");
const LILLY_CATALOG: &str = include_str!("../../tools/Lilly.catalog");
const LILLY_EXPECTED_ASSETS: usize = 70;
const LILLY_FRAMES_PER_ASSET: usize = 7;
const LILLY_EXPECTED_LOGICAL_FRAMES: usize = LILLY_EXPECTED_ASSETS * LILLY_FRAMES_PER_ASSET;
const LILLY_IDENTITY_SOURCE_MAP: [u8; LILLY_FRAMES_PER_ASSET] = [1, 2, 3, 4, 5, 6, 7];
const PNG_SIGNATURE: &[u8; 8] = b"\x89PNG\r\n\x1A\n";
const PNG_IHDR: &[u8; 4] = b"IHDR";
const GPU_PAGE_BYTES: usize = 4096;
const LILLY_EXECUTION_GPU_BASE: u64 = 0x3000_0000;
const LILLY_EXECUTION_GPU_LIMIT: u64 = 0x4000_0000;
const _: () = assert!(LILLY_EXECUTION_GPU_BASE.is_multiple_of(GPU_PAGE_BYTES as u64));
const _: () = assert!(LILLY_EXECUTION_GPU_LIMIT.is_multiple_of(GPU_PAGE_BYTES as u64));
const _: () = assert!(LILLY_EXECUTION_GPU_BASE < LILLY_EXECUTION_GPU_LIMIT);
const _: () =
    assert!(LILLY_EXECUTION_GPU_LIMIT <= crate::intel::gpgpu::DIRECT_RCS_PPGTT_LIMIT_BYTES);

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
pub(crate) struct LillyResidentPart {
    /// Meaning within the animation, such as `mouth_wide` or `raise_fists`.
    pub(crate) semantic: &'static str,
    pub(crate) surface: LillyResidentFrame,
}

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAsset {
    /// Stable semantic identity, such as `talk.calm.crossed`.
    pub(crate) key: &'static str,
    pub(crate) playback: LillyPlayback,
    pub(crate) frame_period_ms: u16,
    pub(crate) parts: [LillyResidentPart; LILLY_FRAMES_PER_ASSET],
}

#[allow(dead_code)]
impl LillyResidentAsset {
    pub(crate) fn part(self, semantic: &str) -> Option<LillyResidentFrame> {
        self.parts
            .iter()
            .find_map(|part| (part.semantic == semantic).then_some(part.surface))
    }
}

/// Uniform view of one ordinary seven-frame animation. Callers do not need
/// clip-specific code: the catalog supplies cadence, playback, and the semantic
/// names of all seven frames.
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAnimation {
    pub(crate) key: &'static str,
    pub(crate) playback: LillyPlayback,
    pub(crate) frame_period_ms: u16,
    pub(crate) frames: [LillyResidentPart; LILLY_FRAMES_PER_ASSET],
}

#[allow(dead_code)]
impl LillyResidentAnimation {
    pub(crate) const fn cycle_duration_ms(self) -> u64 {
        self.frame_period_ms as u64 * LILLY_FRAMES_PER_ASSET as u64
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
            LillyPlayback::Loop => frame % LILLY_FRAMES_PER_ASSET as u64,
            LillyPlayback::Once if frame >= LILLY_FRAMES_PER_ASSET as u64 => return None,
            LillyPlayback::Once => frame,
            LillyPlayback::OnceHold => frame.min(LILLY_FRAMES_PER_ASSET as u64 - 1),
        };
        Some(self.frames[index as usize])
    }
}

struct LillyFrameEntry {
    name: String,
    surface: LillyResidentFrame,
}

struct LillyResidentAssets {
    allocation: crate::gpu::resident::ResidentDmaBuffer,
    frames: Vec<LillyFrameEntry>,
    semantic_assets: Vec<LillyResidentAsset>,
    rgba_bytes: usize,
}

#[derive(Copy, Clone)]
struct LillyCatalogEntry {
    key: &'static str,
    directory: &'static str,
    playback: LillyPlayback,
    frame_period_ms: u16,
    parts: [&'static str; LILLY_FRAMES_PER_ASSET],
    sources: [u8; LILLY_FRAMES_PER_ASSET],
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
/// engine-neutral DMA storage are established before continuous frame work.
/// The Spirit execution PPGTT maps the physical pages under its own VA when
/// the VFX context starts; Render is not part of this cold path.
pub(super) fn prepare_resident_once() -> bool {
    if LILLY_RESIDENT.lock().is_some() {
        return true;
    }

    crate::log_info!(
        target: "gfx";
        "trueos-spirit: first-job start job=lilly-resident-assets source=embedded-7z archive_bytes=0x{:X} decode=kernel-z7+png target=spirit-execution-ppgtt lifetime=runtime render_dependency=0\n",
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
        "trueos-spirit: first-job complete job=lilly-resident-assets logical_frames={} decoded_frames={} animations={} rgba_bytes=0x{:X} resident_bytes=0x{:X} phys=0x{:X} gpu=0x{:X} ppgtt=spirit-execution pat=0 cache=wb cpu_uploads=1 persistent=1 render_dependency=0\n",
        LILLY_EXPECTED_LOGICAL_FRAMES,
        assets.frames.len(),
        assets.semantic_assets.len(),
        assets.rgba_bytes,
        assets.allocation.bytes(),
        assets.allocation.phys(),
        LILLY_EXECUTION_GPU_BASE,
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

/// Resolve an animation through one central path.
#[allow(dead_code)]
pub(crate) fn resident_animation(key: &str) -> Option<LillyResidentAnimation> {
    let asset = resident_asset(key)?;
    Some(LillyResidentAnimation {
        key: asset.key,
        playback: asset.playback,
        frame_period_ms: asset.frame_period_ms,
        frames: asset.parts,
    })
}

#[allow(dead_code)]
pub(crate) fn resident_frame_count() -> usize {
    LILLY_RESIDENT
        .lock()
        .as_ref()
        .map_or(0, |_| LILLY_EXPECTED_LOGICAL_FRAMES)
}

fn load_resident_assets() -> Result<LillyResidentAssets, LillyLoadError> {
    let catalog = parse_catalog()?;
    let mut entries = crate::z7::extract_all_to_vec(LILLY_ARCHIVE_7Z)?;
    entries.sort_unstable_by(|left, right| left.name.cmp(&right.name));
    validate_archive_shape(entries.as_slice(), catalog.as_slice())?;

    let mut storage_bytes = 0usize;
    for entry in &entries {
        let (width, height) = png_ihdr_dimensions(entry.bytes.as_slice())?;
        validate_dimensions(width, height)?;
        storage_bytes = storage_bytes
            .checked_add(frame_storage_bytes(width, height)?)
            .ok_or(LillyLoadError::AddressOverflow)?;
    }

    let storage_bytes_u64 =
        u64::try_from(storage_bytes).map_err(|_| LillyLoadError::AddressOverflow)?;
    if LILLY_EXECUTION_GPU_BASE
        .checked_add(storage_bytes_u64)
        .is_none_or(|end| end > LILLY_EXECUTION_GPU_LIMIT)
    {
        return Err(LillyLoadError::Resident("spirit-execution-va-capacity"));
    }
    let allocation =
        crate::gpu::resident::ResidentDmaBuffer::allocate_zeroed(storage_bytes, GPU_PAGE_BYTES)
            .ok_or(LillyLoadError::Resident("spirit-resident-dma"))?;
    let (frames, rgba_bytes) = populate_resident_frames(&allocation, entries, storage_bytes)?;
    let semantic_assets = build_semantic_assets(frames.as_slice(), catalog.as_slice())?;
    allocation.flush();
    Ok(LillyResidentAssets {
        allocation,
        frames,
        semantic_assets,
        rgba_bytes,
    })
}

fn populate_resident_frames(
    allocation: &crate::gpu::resident::ResidentDmaBuffer,
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
            .phys()
            .checked_add(offset as u64)
            .ok_or(LillyLoadError::AddressOverflow)?;
        let gpu = LILLY_EXECUTION_GPU_BASE
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
    if offset != expected_storage_bytes || offset != allocation.bytes() {
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
        let part_names = fields
            .next()
            .ok_or(LillyLoadError::Catalog("missing-parts"))?;
        let source_names = fields.next();
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

        let playback = match kind_name {
            "loop" if period != 0 => LillyPlayback::Loop,
            "once" if period != 0 => LillyPlayback::Once,
            "once_hold" if period != 0 => LillyPlayback::OnceHold,
            _ => return Err(LillyLoadError::Catalog("bad-kind-or-period")),
        };

        let mut parts = [""; LILLY_FRAMES_PER_ASSET];
        let mut part_count = 0usize;
        for part in part_names.split(',') {
            let Some(slot) = parts.get_mut(part_count) else {
                return Err(LillyLoadError::Catalog("bad-parts"));
            };
            *slot = part;
            part_count += 1;
        }
        if part_count != LILLY_FRAMES_PER_ASSET
            || parts.iter().any(|part| !is_semantic_part(part))
            || parts
                .iter()
                .enumerate()
                .any(|(index, part)| parts[..index].contains(part))
        {
            return Err(LillyLoadError::Catalog("bad-parts"));
        }
        let sources = parse_source_map(source_names)?;
        catalog.push(LillyCatalogEntry {
            key,
            directory,
            playback,
            frame_period_ms: period,
            parts,
            sources,
        });
    }
    if catalog.len() != LILLY_EXPECTED_ASSETS {
        return Err(LillyLoadError::Catalog("asset-count"));
    }
    Ok(catalog)
}

fn parse_source_map(
    source_names: Option<&str>,
) -> Result<[u8; LILLY_FRAMES_PER_ASSET], LillyLoadError> {
    let Some(source_names) = source_names else {
        return Ok(LILLY_IDENTITY_SOURCE_MAP);
    };
    let mut sources = [0u8; LILLY_FRAMES_PER_ASSET];
    let mut source_count = 0usize;
    for source_name in source_names.split(',') {
        let Some(slot) = sources.get_mut(source_count) else {
            return Err(LillyLoadError::Catalog("bad-source-map"));
        };
        *slot = source_name
            .parse::<u8>()
            .map_err(|_| LillyLoadError::Catalog("bad-source-map"))?;
        source_count += 1;
    }
    if source_count != LILLY_FRAMES_PER_ASSET
        || sources.iter().enumerate().any(|(logical_index, source)| {
            *source == 0
                || *source > LILLY_FRAMES_PER_ASSET as u8
                || usize::from(*source) > logical_index + 1
        })
    {
        return Err(LillyLoadError::Catalog("bad-source-map"));
    }
    Ok(sources)
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

fn find_source_frame<'a>(
    frames: &'a [LillyFrameEntry],
    directory: &str,
    source_index: u8,
) -> Option<&'a LillyFrameEntry> {
    frames.iter().find(|frame| {
        split_frame_name(frame.name.as_str()).is_some_and(|(frame_directory, frame_index)| {
            frame_directory == directory && frame_index == source_index
        })
    })
}

fn build_semantic_assets(
    frames: &[LillyFrameEntry],
    catalog: &[LillyCatalogEntry],
) -> Result<Vec<LillyResidentAsset>, LillyLoadError> {
    let mut assets = Vec::with_capacity(catalog.len());
    for catalog_entry in catalog {
        let mut parts = Vec::with_capacity(LILLY_FRAMES_PER_ASSET);
        for logical_index in 0..LILLY_FRAMES_PER_ASSET {
            let source = find_source_frame(
                frames,
                catalog_entry.directory,
                catalog_entry.sources[logical_index],
            )
            .ok_or(LillyLoadError::Catalog("resident-asset-missing"))?;
            parts.push(LillyResidentPart {
                semantic: catalog_entry.parts[logical_index],
                surface: source.surface,
            });
        }
        assets.push(LillyResidentAsset {
            key: catalog_entry.key,
            playback: catalog_entry.playback,
            frame_period_ms: catalog_entry.frame_period_ms,
            parts: parts
                .try_into()
                .map_err(|_| LillyLoadError::Catalog("resident-part-count"))?,
        });
    }
    Ok(assets)
}

fn expected_archive_frame_count(catalog: &[LillyCatalogEntry]) -> usize {
    catalog
        .iter()
        .map(|entry| {
            let mut seen = [false; LILLY_FRAMES_PER_ASSET + 1];
            let mut count = 0usize;
            for source in entry.sources {
                let slot = &mut seen[usize::from(source)];
                if !*slot {
                    *slot = true;
                    count += 1;
                }
            }
            count
        })
        .sum()
}

fn validate_archive_shape(
    entries: &[crate::z7::SevenZEntry],
    catalog: &[LillyCatalogEntry],
) -> Result<(), LillyLoadError> {
    if entries.len() != expected_archive_frame_count(catalog) {
        return Err(LillyLoadError::ArchiveShape);
    }
    for catalog_entry in catalog {
        for (position, source_index) in catalog_entry.sources.iter().enumerate() {
            if catalog_entry.sources[..position].contains(source_index) {
                continue;
            }
            if !entries.iter().any(|entry| {
                split_frame_name(entry.name.as_str()).is_some_and(|(directory, frame_index)| {
                    directory == catalog_entry.directory && frame_index == *source_index
                })
            }) {
                return Err(LillyLoadError::Catalog("catalog-frame-missing"));
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
    let number = file.strip_prefix("frame_")?.strip_suffix(".png")?;
    if number.len() != 2 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let index = number.parse::<u8>().ok()?;
    if !(1..=LILLY_FRAMES_PER_ASSET as u8).contains(&index) {
        return None;
    }
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
        (128, 128) => Ok(()),
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
