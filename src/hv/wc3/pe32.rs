use alloc::{string::String, vec, vec::Vec};

use super::imports::{self, LauncherImport};

pub(crate) const IMAGE_BASE: u32 = 0x0040_0000;
pub(crate) const ENTRY_RVA: u32 = 0x2144;
pub(crate) const IMAGE_BYTES: usize = 0x44_000;
pub(crate) const HEADERS_BYTES: usize = 0x1_000;

pub(crate) struct Materialized {
    pub(crate) image: Vec<u8>,
    pub(crate) imports: Vec<LauncherImport>,
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, &'static str> {
    Ok(u16::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(2).ok_or("pe offset overflow")?)
            .ok_or("pe truncated")?
            .try_into()
            .map_err(|_| "pe u16")?,
    ))
}
fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, &'static str> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset.checked_add(4).ok_or("pe offset overflow")?)
            .ok_or("pe truncated")?
            .try_into()
            .map_err(|_| "pe u32")?,
    ))
}
fn c_string(bytes: &[u8], offset: usize) -> Result<String, &'static str> {
    let tail = bytes.get(offset..).ok_or("pe string offset")?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or("pe unterminated string")?;
    core::str::from_utf8(&tail[..end])
        .map_err(|_| "pe non-ascii import")
        .map(String::from)
}

pub(crate) fn materialize(bytes: &[u8]) -> Result<Materialized, &'static str> {
    if bytes.get(..2) != Some(b"MZ") {
        return Err("pe DOS signature");
    }
    let pe = usize::try_from(read_u32(bytes, 0x3C)?).map_err(|_| "pe header offset")?;
    if bytes.get(pe..pe.checked_add(4).ok_or("pe header overflow")?) != Some(b"PE\0\0") {
        return Err("pe signature");
    }
    if read_u16(bytes, pe + 4)? != 0x14C {
        return Err("pe machine is not i386");
    }
    let sections = usize::from(read_u16(bytes, pe + 6)?);
    let optional_bytes = usize::from(read_u16(bytes, pe + 20)?);
    let optional = pe.checked_add(24).ok_or("pe optional overflow")?;
    if optional_bytes < 0xE0 || read_u16(bytes, optional)? != 0x10B {
        return Err("pe optional header is not PE32");
    }
    if read_u32(bytes, optional + 16)? != ENTRY_RVA
        || read_u32(bytes, optional + 28)? != IMAGE_BASE
        || usize::try_from(read_u32(bytes, optional + 56)?).ok() != Some(IMAGE_BYTES)
        || usize::try_from(read_u32(bytes, optional + 60)?).ok() != Some(HEADERS_BYTES)
    {
        return Err("pe fixed launcher header mismatch");
    }
    for directory in [5usize, 9] {
        let directory_offset = optional + 96 + directory * 8;
        if read_u32(bytes, directory_offset)? != 0 || read_u32(bytes, directory_offset + 4)? != 0 {
            return Err("pe unsupported relocation or TLS directory");
        }
    }
    if bytes.len() < HEADERS_BYTES {
        return Err("pe headers truncated");
    }
    let mut image = vec![0; IMAGE_BYTES];
    image[..HEADERS_BYTES].copy_from_slice(&bytes[..HEADERS_BYTES]);
    let section_table = optional
        .checked_add(optional_bytes)
        .ok_or("pe section table overflow")?;
    for index in 0..sections {
        let section = section_table
            .checked_add(index.checked_mul(40).ok_or("pe section count overflow")?)
            .ok_or("pe section offset overflow")?;
        let virtual_size = usize::try_from(read_u32(bytes, section + 8)?)
            .map_err(|_| "pe section virtual size")?;
        let virtual_address = usize::try_from(read_u32(bytes, section + 12)?)
            .map_err(|_| "pe section virtual address")?;
        let raw_size =
            usize::try_from(read_u32(bytes, section + 16)?).map_err(|_| "pe section raw size")?;
        let raw_offset =
            usize::try_from(read_u32(bytes, section + 20)?).map_err(|_| "pe section raw offset")?;
        let section_bytes = virtual_size.max(raw_size);
        if virtual_address
            .checked_add(section_bytes)
            .filter(|end| *end <= IMAGE_BYTES)
            .is_none()
            || raw_offset
                .checked_add(raw_size)
                .filter(|end| *end <= bytes.len())
                .is_none()
        {
            return Err("pe section range");
        }
        image[virtual_address..virtual_address + raw_size]
            .copy_from_slice(&bytes[raw_offset..raw_offset + raw_size]);
    }
    let import_rva =
        usize::try_from(read_u32(bytes, optional + 96 + 8)?).map_err(|_| "pe import rva")?;
    let import_size =
        usize::try_from(read_u32(bytes, optional + 96 + 12)?).map_err(|_| "pe import size")?;
    let mut imports = imports::new_table();
    if import_rva == 0 || import_size == 0 {
        return Err("pe import directory missing");
    }
    let import_end = import_rva
        .checked_add(import_size)
        .filter(|end| *end <= IMAGE_BYTES)
        .ok_or("pe import directory range")?;
    let mut descriptor = import_rva;
    while descriptor
        .checked_add(20)
        .filter(|end| *end <= import_end)
        .is_some()
    {
        let lookup =
            usize::try_from(read_u32(&image, descriptor)?).map_err(|_| "pe import lookup")?;
        let module_rva =
            usize::try_from(read_u32(&image, descriptor + 12)?).map_err(|_| "pe import module")?;
        let iat =
            usize::try_from(read_u32(&image, descriptor + 16)?).map_err(|_| "pe import IAT")?;
        if lookup == 0 && module_rva == 0 && iat == 0 {
            break;
        }
        let module = c_string(&image, module_rva)?;
        let mut index = 0usize;
        loop {
            let name_rva = usize::try_from(read_u32(
                &image,
                lookup
                    .checked_add(index.checked_mul(4).ok_or("pe import index")?)
                    .ok_or("pe import lookup overflow")?,
            )?)
            .map_err(|_| "pe import name")?;
            if name_rva == 0 {
                break;
            }
            if name_rva & 0x8000_0000 != 0 {
                return Err("pe ordinal import unsupported");
            }
            let symbol =
                c_string(&image, name_rva.checked_add(2).ok_or("pe import name overflow")?)?;
            let iat_rva = u32::try_from(
                iat.checked_add(index.checked_mul(4).ok_or("pe IAT index")?)
                    .ok_or("pe IAT overflow")?,
            )
            .map_err(|_| "pe IAT rva")?;
            if usize::try_from(iat_rva)
                .ok()
                .and_then(|offset| image.get(offset..offset + 4))
                .is_none()
            {
                return Err("pe IAT outside image");
            }
            imports.push(LauncherImport {
                id: u32::try_from(imports.len()).map_err(|_| "pe import count")?,
                module: module.clone(),
                symbol,
                iat_rva,
            });
            index += 1;
        }
        descriptor += 20;
    }
    if !imports::find_get_version(&imports) {
        return Err("pe expected KERNEL32.dll!GetVersion missing");
    }
    if !imports::find_initialize_critical_section(&imports) {
        return Err("pe expected unique KERNEL32.dll!InitializeCriticalSection missing");
    }
    Ok(Materialized { image, imports })
}
