//! Size-keyed, color-free glyph masks produced on the shared blocking service lanes.

use alloc::{
    boxed::Box,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    vec::Vec,
};
use microemoji::{
    Bundle,
    raster_stroke::{self, Outline},
};

mod native;

const SOURCE_BYTES: &[u8] = include_bytes!("../../../vendor/microemoji/data/outline-alpha.bin.lz4");
// Enough for any 256 standalone glyphs from the supplied Twemoji font.
const MAX_SOURCE_BYTES: usize = 1024 * 1024;
const MAX_SIZE: u32 = 256;
const MAX_PENDING: usize = 8;
const MAX_TILES: usize = 256;
const MAX_MASK_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    InvalidSize,
    MissingGlyph,
    Pending,
    Build(&'static str),
}

type Key = (char, u32);
type Mask = Arc<[u8]>;
type Tile = Result<Mask, Error>;

struct Entry {
    tile: Tile,
    last_used: u64,
}

struct Cache {
    entries: BTreeMap<Key, Entry>,
    pending: BTreeSet<Key>,
    bytes: usize,
    clock: u64,
}

impl Cache {
    const fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            pending: BTreeSet::new(),
            bytes: 0,
            clock: 0,
        }
    }

    fn get(&mut self, key: Key) -> Option<Tile> {
        let entry = self.entries.get_mut(&key)?;
        self.clock = self.clock.wrapping_add(1);
        entry.last_used = self.clock;
        Some(entry.tile.clone())
    }

    fn insert(&mut self, key: Key, tile: Tile) {
        let bytes = tile.as_ref().map_or(0, |mask| mask.len());
        while self.entries.len() >= MAX_TILES || self.bytes + bytes > MAX_MASK_BYTES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(&key, _)| key);
            let Some(oldest) = oldest else { break };
            if let Ok(mask) = self.entries.remove(&oldest).unwrap().tile {
                self.bytes -= mask.len();
            }
        }
        self.clock = self.clock.wrapping_add(1);
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                tile,
                last_used: self.clock,
            },
        );
    }
}

static SOURCE: spin::Once<Result<Bundle<'static>, Error>> = spin::Once::new();
static CACHE: spin::Mutex<Cache> = spin::Mutex::new(Cache::new());

/// Look up or queue a square, tightly packed R8 mask. Retrying a pending
/// request shares its job. A returned mask remains valid after eviction.
pub(crate) fn glyph(character: char, size: u32) -> Result<Mask, Error> {
    crate::allocators::with_host_alloc_domain_strong(|| request(character, size))
}

/// Return the glyph scalars in the embedded bundle's Unicode order.
pub(crate) fn characters() -> Result<impl ExactSizeIterator<Item = char>, Error> {
    let bundle = crate::allocators::with_host_alloc_domain_strong(load_source)?;
    Ok(bundle.glyphs().map(|glyph| glyph.scalar()))
}

/// Check a loaded bundle without starting decompression on the draw path.
pub(crate) fn has_glyph(character: char) -> bool {
    SOURCE
        .get()
        .is_some_and(|bundle| bundle.is_ok_and(|bundle| bundle.glyph(character).is_some()))
}

fn request(character: char, size: u32) -> Tile {
    if !(1..=MAX_SIZE).contains(&size) {
        return Err(Error::InvalidSize);
    }
    if let Some(source) = SOURCE.get() {
        (*source)?.glyph(character).ok_or(Error::MissingGlyph)?;
    }

    let key = (character, size);
    {
        let mut cache = CACHE.lock();
        if let Some(tile) = cache.get(key) {
            return tile;
        }
        if cache.pending.len() >= MAX_PENDING || !cache.pending.insert(key) {
            return Err(Error::Pending);
        }
    }

    let job = Box::new(move || {
        let result = build(character, size as usize);
        if let Err(Error::Build(reason)) = result {
            crate::log_error!(target: "microemoji-mask";
                "microemoji-mask: build failed glyph=U+{:04X} size={} error={}\n",
                character as u32, size, reason);
        }
        let mut cache = CACHE.lock();
        cache.pending.remove(&key);
        if !matches!(result, Err(Error::Pending | Error::MissingGlyph)) {
            cache.insert(key, result);
        }
    });
    if let Err(job) =
        crate::r::blocking::try_spawn_blocking_job_with_purpose(job, "microemoji-mask")
    {
        drop(job);
        CACHE.lock().pending.remove(&key);
    }
    Err(Error::Pending)
}

fn load_source() -> Result<Bundle<'static>, Error> {
    *SOURCE.call_once(|| {
        let bytes = crate::r::lz4::decompress_frame_cpu(SOURCE_BYTES, MAX_SOURCE_BYTES)
            .map_err(|_| Error::Build("microemoji-source-decode"))?;
        Bundle::parse(&bytes).map_err(|_| Error::Build("microemoji-source-format"))?;
        // Owner and immutable views live for the service lifetime.
        Bundle::parse(Box::leak(bytes.into_boxed_slice()))
            .map_err(|_| Error::Build("microemoji-source-format"))
    })
}

fn buffer<T: Clone>(len: usize, initial: T) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(len)
        .map_err(|_| Error::Build("microemoji-mask-allocation"))?;
    values.resize(len, initial);
    Ok(values)
}

fn build(character: char, size: usize) -> Tile {
    let glyph = load_source()?.glyph(character).ok_or(Error::MissingGlyph)?;
    let outline =
        Outline::new(glyph, size, size).map_err(|_| Error::Build("microemoji-outline-viewport"))?;
    let mut pixels = buffer(size * size, 0u8)?;

    match native::render(&outline, size, size, &mut pixels) {
        Ok(()) => return Ok(Arc::from(pixels)),
        Err(native::Error::Retry) => return Err(Error::Pending),
        Err(native::Error::Failed(reason)) => {
            crate::log_warn!(target: "microemoji-mask";
                "microemoji-mask: native outline failed glyph=U+{:04X} size={} error={} fallback=cpu\n",
                character as u32, size, reason);
        }
        Err(native::Error::Unavailable) => {}
    }

    let coverage_len = raster_stroke::coverage_scratch_size(size, size)
        .map_err(|_| Error::Build("microemoji-mask-scratch"))?;
    let mut coverage = buffer(coverage_len, 0u8)?;
    raster_stroke::rasterize(outline, &mut pixels, size, &mut coverage)
        .map_err(|_| Error::Build("microemoji-mask-raster"))?;
    Ok(Arc::from(pixels))
}
