//! App-owned shader payloads authenticated before a Blueprint can execute.
//!
//! The kernel contains only SHA-256 trust records and ABI metadata. Immutable
//! payloads arrive in `.trueos.shaders` with the downloaded app and are shared
//! across app instances after validation. Missing packages have no embedded fallback.
use alloc::{boxed::Box, vec::Vec};
use sha2::{Digest, Sha256};
use spin::Once;

include!("app_shader_trust.rs");

const MAGIC: &[u8; 8] = b"APSHDR1\0";
const MAX_PACKAGE_BYTES: usize = 4 * 1024 * 1024;
const MAX_STAGES: usize = 8;

enum Stage {
    Bytes(Box<[u8]>),
    Words(Box<[u32]>),
}
struct Package { stages: Vec<Stage> }
static PACKAGES: [Once<Package>; 10] = [const { Once::new() }; 10];

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?))
}

pub(crate) fn present(id: u32) -> bool {
    id.checked_sub(1).and_then(|i| PACKAGES.get(i as usize)).is_some_and(|p| p.get().is_some())
}

pub(crate) fn bytes(id: u32, stage: usize) -> Option<&'static [u8]> {
    match PACKAGES.get(id.checked_sub(1)? as usize)?.get()?.stages.get(stage)? {
        Stage::Bytes(bytes) => Some(bytes),
        Stage::Words(_) => None,
    }
}

pub(crate) fn words(id: u32, stage: usize) -> Option<&'static [u32]> {
    match PACKAGES.get(id.checked_sub(1)? as usize)?.get()?.stages.get(stage)? {
        Stage::Words(words) => Some(words),
        Stage::Bytes(_) => None,
    }
}

/// Register one or more concatenated packages from a non-ALLOC ELF section.
/// Hash/layout checks precede allocation. The fixed registry bounds lifetime
/// storage, including concurrent launches and repeatedly downloaded packages.
pub(crate) fn register_section(mut section: &[u8]) -> Result<(), &'static str> {
    while !section.is_empty() {
        if section.get(..8) != Some(MAGIC.as_slice()) { return Err("app-shader-magic"); }
        let id = u32_at(section, 8).ok_or("app-shader-header")?;
        let payload = u32_at(section, 12).ok_or("app-shader-header")? as usize;
        let size = 16usize.checked_add(payload).ok_or("app-shader-size")?;
        if size > MAX_PACKAGE_BYTES { return Err("app-shader-size"); }
        let package = section.get(..size).ok_or("app-shader-truncated")?;
        let (_, expected_size, digest) = TRUST.iter().find(|(key, _, _)| *key == id)
            .ok_or("app-shader-untrusted-id")?;
        if size != *expected_size || Sha256::digest(package)[..] != *digest {
            return Err("app-shader-sha256-mismatch");
        }
        let count = u32_at(package, 16).ok_or("app-shader-stage-count")? as usize;
        if count == 0 || count > MAX_STAGES { return Err("app-shader-stage-count"); }
        // Validate all stage ranges before installing anything.
        let mut cursor = 20usize;
        for _ in 0..count {
            let kind = u32_at(package, cursor).ok_or("app-shader-stage-header")?;
            let len = u32_at(package, cursor + 4).ok_or("app-shader-stage-header")? as usize;
            cursor = cursor.checked_add(8).ok_or("app-shader-stage-range")?;
            if kind > 1 || len == 0 || (kind == 1 && len % 4 != 0) {
                return Err("app-shader-stage-layout");
            }
            cursor = cursor.checked_add(len).ok_or("app-shader-stage-range")?;
            if cursor > package.len() { return Err("app-shader-stage-range"); }
        }
        if cursor != package.len() { return Err("app-shader-trailing-data"); }
        let slot = PACKAGES.get(id.checked_sub(1).ok_or("app-shader-id")? as usize)
            .ok_or("app-shader-id")?;
        if slot.get().is_none() {
            let mut stages = Vec::new();
            stages.try_reserve_exact(count).map_err(|_| "app-shader-allocation")?;
            let mut cursor = 20;
            for _ in 0..count {
                let kind = u32_at(package, cursor).ok_or("app-shader-stage-header")?;
                let len = u32_at(package, cursor + 4).ok_or("app-shader-stage-header")? as usize;
                cursor += 8;
                let data = &package[cursor..cursor + len];
                let stage = if kind == 0 {
                    let mut copy = Vec::new();
                    copy.try_reserve_exact(len).map_err(|_| "app-shader-allocation")?;
                    copy.extend_from_slice(data);
                    Stage::Bytes(copy.into_boxed_slice())
                } else {
                    let mut copy = Vec::new();
                    copy.try_reserve_exact(len / 4).map_err(|_| "app-shader-allocation")?;
                    for word in data.chunks_exact(4) {
                        copy.push(u32::from_le_bytes(word.try_into().map_err(|_| "app-shader-word")?));
                    }
                    Stage::Words(copy.into_boxed_slice())
                };
                stages.push(stage);
                cursor += len;
            }
            slot.call_once(|| Package { stages });
        }
        section = &section[size..];
    }
    Ok(())
}

/// Intake runs in the loader's host allocation domain before guest relocation
/// or execution. Shader sections are packaging data, not guest allocations.
pub(crate) fn register_blueprint_elf(bytes: &[u8]) -> Result<(), &'static str> {
    use object::{Object, ObjectSection};
    let file = object::read::elf::ElfFile64::<object::Endianness>::parse(bytes)
        .map_err(|_| "app-shader-elf")?;
    for section in file.sections() {
        if section.name().ok() == Some(".trueos.shaders") {
            let data = section.data().map_err(|_| "app-shader-elf-section")?;
            register_section(data)?;
        }
    }
    Ok(())
}
