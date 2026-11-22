//! Packed LillyDS frames and the unchanged Spirit playback catalog.

extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Once;

const LILLY_PACK: &[u8] = include_bytes!("../../tools/Lilly.lpak");
const LILLY_SOURCES: &str = include_str!("../../tools/Lilly.sources");
const LILLY_CATALOG: &str = include_str!("../../tools/Lilly.catalog");
const LILLY_EXPECTED_ASSETS: usize = 70;
const LILLY_FRAMES_PER_ASSET: usize = 7;
const LILLY_EXPECTED_LOGICAL_FRAMES: usize = LILLY_EXPECTED_ASSETS * LILLY_FRAMES_PER_ASSET;
const LILLY_IDENTITY_SOURCE_MAP: [u8; LILLY_FRAMES_PER_ASSET] = [1, 2, 3, 4, 5, 6, 7];
const FRAME_BYTES: usize = 256 * 256 * 4;

static LILLY_RESIDENT: Once<LillyResidentAssets> = Once::new();

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentFrame {
    offset: usize,
}

/// Default sequencing behavior described by the reviewed Lilly catalog.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum LillyPlayback {
    Loop,
    Once,
    OnceHold,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentPart {
    /// Meaning within the animation, such as `mouth_wide` or `raise_fists`.
    pub(crate) semantic: &'static str,
    pub(crate) surface: LillyResidentFrame,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAsset {
    /// Stable semantic identity, such as `talk.calm.crossed`.
    pub(crate) key: &'static str,
    pub(crate) playback: LillyPlayback,
    pub(crate) frame_period_ms: u16,
    pub(crate) parts: [LillyResidentPart; LILLY_FRAMES_PER_ASSET],
}

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
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct LillyResidentAnimation {
    pub(crate) key: &'static str,
    pub(crate) playback: LillyPlayback,
    pub(crate) frame_period_ms: u16,
    pub(crate) frames: [LillyResidentPart; LILLY_FRAMES_PER_ASSET],
}

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
    pixels: Vec<u8>,
    frames: Vec<LillyFrameEntry>,
    semantic_assets: Vec<LillyResidentAsset>,
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
    Pack(trueos_lily_pack::Error),
    PackShape,
    Catalog(&'static str),
    AddressOverflow,
    ResidentAllocation,
}

/// Decode into immutable cursor pixels before continuous playback.
pub(super) fn prepare_resident_once() -> bool {
    match LILLY_RESIDENT.try_call_once(|| {
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: first-job start job=lilly-resident-assets source=embedded-lillyds-pack pack_bytes=0x{:X} decode=rust-xz+shared-tiles+palette placement=bottom-center-256x256 format=premultiplied-bgra8\n",
            LILLY_PACK.len(),
        );
        let assets = load_resident_assets()?;
        crate::log_info!(
            target: "gfx";
            "trueos-spirit: first-job complete job=lilly-resident-assets logical_frames={} decoded_frames={} animations={} resident_bytes=0x{:X} format=premultiplied-bgra8 storage=host lifetime=runtime\n",
            LILLY_EXPECTED_LOGICAL_FRAMES,
            assets.frames.len(),
            assets.semantic_assets.len(),
            assets.pixels.len(),
        );
        Ok::<_, LillyLoadError>(assets)
    }) {
        Ok(_) => true,
        Err(error) => {
            crate::log_error!(
                target: "gfx";
                "trueos-spirit: first-job failed job=lilly-resident-assets error={:?} pack_bytes=0x{:X} action=retain-existing-spirit-stream\n",
                error,
                LILLY_PACK.len(),
            );
            false
        }
    }
}

/// Resolve one decoded frame without exposing the CPU mapping or allocation
/// owner. Both archive-relative and `Lilly/`-prefixed names are accepted.
pub(crate) fn resident_frame(name: &str) -> Option<LillyResidentFrame> {
    let frames = &LILLY_RESIDENT.get()?.frames;
    let relative = name.strip_prefix("Lilly/").unwrap_or(name);
    if let Some(frame) = frames.iter().find(|frame| frame.name == relative) {
        return Some(frame.surface);
    }
    let (directory, index) = split_frame_name(name)?;
    find_source_frame(frames, directory, index).map(|frame| frame.surface)
}

/// Copy the already converted frame directly into the cursor back buffer.
pub(super) fn copy_frame(frame: LillyResidentFrame, destination: &mut [u8]) -> bool {
    if destination.len() != FRAME_BYTES {
        return false;
    }
    let Some(resident) = LILLY_RESIDENT.get() else {
        return false;
    };
    let Some(pixels) = resident
        .pixels
        .get(frame.offset..frame.offset + FRAME_BYTES)
    else {
        return false;
    };
    destination.copy_from_slice(pixels);
    true
}

/// Resolve a complete resident asset by semantic identity. This is the normal
/// Spirit-facing API; archive paths and frame numbers remain an implementation
/// detail of the warm loader.
pub(crate) fn resident_asset(key: &str) -> Option<LillyResidentAsset> {
    LILLY_RESIDENT
        .get()?
        .semantic_assets
        .iter()
        .copied()
        .find(|asset| asset.key == key)
}

/// Resolve one meaningful part directly, for example
/// `resident_part("talk.calm.crossed", "mouth_wide")`.
pub(crate) fn resident_part(key: &str, semantic: &str) -> Option<LillyResidentFrame> {
    resident_asset(key)?.part(semantic)
}

/// Resolve an animation through one central path.
pub(crate) fn resident_animation(key: &str) -> Option<LillyResidentAnimation> {
    let asset = resident_asset(key)?;
    Some(LillyResidentAnimation {
        key: asset.key,
        playback: asset.playback,
        frame_period_ms: asset.frame_period_ms,
        frames: asset.parts,
    })
}

pub(crate) fn resident_frame_count() -> usize {
    LILLY_RESIDENT
        .get()
        .map_or(0, |_| LILLY_EXPECTED_LOGICAL_FRAMES)
}

fn load_resident_assets() -> Result<LillyResidentAssets, LillyLoadError> {
    let catalog = parse_catalog()?;
    let pack = trueos_lily_pack::decode(LILLY_PACK).map_err(LillyLoadError::Pack)?;
    validate_packed_sources(&pack, &catalog)?;
    let storage_bytes = pack
        .frames
        .len()
        .checked_mul(FRAME_BYTES)
        .ok_or(LillyLoadError::AddressOverflow)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(storage_bytes)
        .map_err(|_| LillyLoadError::ResidentAllocation)?;
    let mut frames = Vec::with_capacity(pack.frames.len());
    for (index, entry) in pack.frames.iter().enumerate() {
        let mut rgba = pack
            .frame_rgba8_bottom_center(index)
            .map_err(LillyLoadError::Pack)?;
        if rgba.len() != FRAME_BYTES {
            return Err(LillyLoadError::PackShape);
        }
        for pixel in rgba.as_chunks_mut::<4>().0 {
            let [red, green, blue, alpha] = *pixel;
            let premultiply = |channel| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8;
            *pixel = [
                premultiply(blue),
                premultiply(green),
                premultiply(red),
                alpha,
            ];
        }
        let offset = pixels.len();
        pixels.extend_from_slice(&rgba);
        frames.push(LillyFrameEntry {
            name: entry.path.clone(),
            surface: LillyResidentFrame { offset },
        });
    }
    let semantic_assets = build_semantic_assets(&frames, &catalog)?;
    Ok(LillyResidentAssets {
        pixels,
        frames,
        semantic_assets,
    })
}

fn replacement_directory(directory: &str) -> Option<&'static str> {
    LILLY_SOURCES
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (legacy, packed) = line.split_once('|')?;
            (legacy == directory).then_some(packed)
        })
        .next()
}

fn validate_packed_sources(
    pack: &trueos_lily_pack::DecodedPack,
    catalog: &[LillyCatalogEntry],
) -> Result<(), LillyLoadError> {
    let mut seen = Vec::new();
    for line in LILLY_SOURCES
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let (legacy, packed) = line
            .split_once('|')
            .ok_or(LillyLoadError::Catalog("replacement-map-format"))?;
        if packed.is_empty()
            || packed.starts_with('/')
            || packed
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
            || packed.contains('|')
            || seen.contains(&legacy)
            || !catalog.iter().any(|entry| entry.directory == legacy)
        {
            return Err(LillyLoadError::Catalog("replacement-map-identity"));
        }
        seen.push(legacy);
    }
    if seen.len() != catalog.len() {
        return Err(LillyLoadError::Catalog("replacement-map-count"));
    }
    for entry in catalog {
        let directory = replacement_directory(entry.directory)
            .ok_or(LillyLoadError::Catalog("replacement-directory-missing"))?;
        for index in 2..=8 {
            let path = alloc::format!("{directory}/frame_{index:02}.png");
            if !pack.frames.iter().any(|frame| frame.path == path) {
                return Err(LillyLoadError::Catalog("replacement-frame-missing"));
            }
        }
    }
    // Extra frames remain resident, but are never added to the playback selector.
    Ok(())
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
    let directory = replacement_directory(directory)?;
    let path = alloc::format!("{directory}/frame_{:02}.png", source_index + 1);
    frames.iter().find(|frame| frame.name == path)
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

fn split_frame_name(name: &str) -> Option<(&str, u8)> {
    let relative = name.strip_prefix("Lilly/").unwrap_or(name);
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
