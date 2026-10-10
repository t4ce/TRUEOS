fn parse_string_package(
    bytes: &[u8],
    list_index: usize,
    package_index: usize,
) -> Result<StringPackageRecord, String> {
    const FIXED_HEADER_BYTES: usize = 46;
    if bytes.len() < FIXED_HEADER_BYTES + 1 {
        return Err(String::from("string package header is truncated"));
    }
    let raw = read_u32(bytes, 0)?;
    if (raw >> 24) as u8 != HII_STRINGS || (raw & 0x00ff_ffff) as usize != bytes.len() {
        return Err(String::from("string package header length/type mismatch"));
    }
    let header_size = read_u32(bytes, 4)? as usize;
    let string_info_offset = read_u32(bytes, 8)? as usize;
    if header_size < FIXED_HEADER_BYTES + 1
        || header_size > bytes.len()
        || string_info_offset < header_size
        || string_info_offset >= bytes.len()
    {
        return Err(String::from("string package header offsets are invalid"));
    }
    let language_name_id = read_u16(bytes, 44)?;
    let language_end = bytes[FIXED_HEADER_BYTES..header_size]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| FIXED_HEADER_BYTES + relative)
        .ok_or_else(|| String::from("string package language is not NUL terminated"))?;
    let language = decode_ascii_metadata(&bytes[FIXED_HEADER_BYTES..language_end], 80);
    if language.is_empty() {
        return Err(String::from("string package language is empty"));
    }

    let mut package = StringPackageRecord {
        list_index,
        package_index,
        language,
        language_name_id,
        strings: BTreeMap::new(),
        max_string_id: 0,
        duplicate_blocks: 0,
        unresolved_duplicates: 0,
        skipped_ids: 0,
        extension_blocks: 0,
        truncated_strings: 0,
    };
    let mut current_id = 1u32;
    let mut cursor = string_info_offset;
    let mut ended = false;
    while cursor < bytes.len() {
        let block_type = bytes[cursor];
        match block_type {
            SIBT_END => {
                cursor += 1;
                ended = true;
                break;
            }
            SIBT_STRING_SCSU => {
                let decoded = decode_ascii_string(bytes, cursor + 1)?;
                let consumed = decoded.consumed;
                insert_decoded(&mut package, &mut current_id, decoded, StringSource::Scsu)?;
                cursor = checked_advance(cursor + 1, consumed, bytes.len(), "SCSU string")?;
            }
            SIBT_STRING_SCSU_FONT => {
                checked_end(cursor, 2, bytes.len(), "SCSU-font header")?;
                let decoded = decode_ascii_string(bytes, cursor + 2)?;
                let consumed = decoded.consumed;
                insert_decoded(&mut package, &mut current_id, decoded, StringSource::Scsu)?;
                cursor = checked_advance(cursor + 2, consumed, bytes.len(), "SCSU-font string")?;
            }
            SIBT_STRINGS_SCSU => {
                let count = read_u16(bytes, cursor + 1)? as usize;
                let mut text_cursor = checked_advance(cursor, 3, bytes.len(), "SCSU strings header")?;
                for _ in 0..count {
                    let decoded = decode_ascii_string(bytes, text_cursor)?;
                    let consumed = decoded.consumed;
                    insert_decoded(&mut package, &mut current_id, decoded, StringSource::Scsu)?;
                    text_cursor = checked_advance(text_cursor, consumed, bytes.len(), "SCSU strings")?;
                }
                cursor = text_cursor;
            }
            SIBT_STRINGS_SCSU_FONT => {
                checked_end(cursor, 4, bytes.len(), "SCSU-font strings header")?;
                let count = read_u16(bytes, cursor + 2)? as usize;
                let mut text_cursor = cursor + 4;
                for _ in 0..count {
                    let decoded = decode_ascii_string(bytes, text_cursor)?;
                    let consumed = decoded.consumed;
                    insert_decoded(&mut package, &mut current_id, decoded, StringSource::Scsu)?;
                    text_cursor = checked_advance(text_cursor, consumed, bytes.len(), "SCSU-font strings")?;
                }
                cursor = text_cursor;
            }
            SIBT_STRING_UCS2 => {
                let decoded = decode_ucs2_string(bytes, cursor + 1)?;
                let consumed = decoded.consumed;
                insert_decoded(&mut package, &mut current_id, decoded, StringSource::Ucs2)?;
                cursor = checked_advance(cursor + 1, consumed, bytes.len(), "UCS2 string")?;
            }
            SIBT_STRING_UCS2_FONT => {
                checked_end(cursor, 2, bytes.len(), "UCS2-font header")?;
                let decoded = decode_ucs2_string(bytes, cursor + 2)?;
                let consumed = decoded.consumed;
                insert_decoded(&mut package, &mut current_id, decoded, StringSource::Ucs2)?;
                cursor = checked_advance(cursor + 2, consumed, bytes.len(), "UCS2-font string")?;
            }
            SIBT_STRINGS_UCS2 => {
                let count = read_u16(bytes, cursor + 1)? as usize;
                let mut text_cursor = checked_advance(cursor, 3, bytes.len(), "UCS2 strings header")?;
                for _ in 0..count {
                    let decoded = decode_ucs2_string(bytes, text_cursor)?;
                    let consumed = decoded.consumed;
                    insert_decoded(&mut package, &mut current_id, decoded, StringSource::Ucs2)?;
                    text_cursor = checked_advance(text_cursor, consumed, bytes.len(), "UCS2 strings")?;
                }
                cursor = text_cursor;
            }
            SIBT_STRINGS_UCS2_FONT => {
                checked_end(cursor, 4, bytes.len(), "UCS2-font strings header")?;
                let count = read_u16(bytes, cursor + 2)? as usize;
                let mut text_cursor = cursor + 4;
                for _ in 0..count {
                    let decoded = decode_ucs2_string(bytes, text_cursor)?;
                    let consumed = decoded.consumed;
                    insert_decoded(&mut package, &mut current_id, decoded, StringSource::Ucs2)?;
                    text_cursor = checked_advance(text_cursor, consumed, bytes.len(), "UCS2-font strings")?;
                }
                cursor = text_cursor;
            }
            SIBT_DUPLICATE => {
                checked_end(cursor, 3, bytes.len(), "duplicate block")?;
                let source_id = read_u16(bytes, cursor + 1)?;
                let target_id = next_string_id(current_id)?;
                package.duplicate_blocks = package.duplicate_blocks.saturating_add(1);
                if source_id != target_id {
                    if let Some(source) = package.strings.get(&source_id).cloned() {
                        package.strings.insert(
                            target_id,
                            ResolvedString {
                                text: source.text,
                                source: StringSource::Duplicate,
                                truncated: source.truncated,
                            },
                        );
                    } else {
                        package.unresolved_duplicates =
                            package.unresolved_duplicates.saturating_add(1);
                    }
                } else {
                    package.unresolved_duplicates =
                        package.unresolved_duplicates.saturating_add(1);
                }
                current_id = current_id
                    .checked_add(1)
                    .ok_or_else(|| String::from("string ID overflow"))?;
                cursor += 3;
            }
            SIBT_SKIP1 => {
                checked_end(cursor, 2, bytes.len(), "skip1 block")?;
                let skip = bytes[cursor + 1] as u32;
                current_id = advance_string_id(current_id, skip)?;
                package.skipped_ids = package.skipped_ids.saturating_add(skip);
                cursor += 2;
            }
            SIBT_SKIP2 => {
                checked_end(cursor, 3, bytes.len(), "skip2 block")?;
                let skip = read_u16(bytes, cursor + 1)? as u32;
                current_id = advance_string_id(current_id, skip)?;
                package.skipped_ids = package.skipped_ids.saturating_add(skip);
                cursor += 3;
            }
            SIBT_EXT1 => {
                checked_end(cursor, 3, bytes.len(), "extension1 header")?;
                let length = bytes[cursor + 2] as usize;
                cursor = skip_extension(bytes, cursor, length, 3, "extension1")?;
                package.extension_blocks = package.extension_blocks.saturating_add(1);
            }
            SIBT_EXT2 => {
                checked_end(cursor, 4, bytes.len(), "extension2 header")?;
                let length = read_u16(bytes, cursor + 2)? as usize;
                cursor = skip_extension(bytes, cursor, length, 4, "extension2")?;
                package.extension_blocks = package.extension_blocks.saturating_add(1);
            }
            SIBT_EXT4 => {
                checked_end(cursor, 6, bytes.len(), "extension4 header")?;
                let length = read_u32(bytes, cursor + 2)? as usize;
                cursor = skip_extension(bytes, cursor, length, 6, "extension4")?;
                package.extension_blocks = package.extension_blocks.saturating_add(1);
            }
            _ => {
                return Err(alloc::format!(
                    "unknown string block 0x{:02X} has no conservative length",
                    block_type
                ));
            }
        }
    }
    if !ended {
        return Err(String::from("string package has no END block"));
    }
    if bytes[cursor..].iter().any(|byte| *byte != 0) {
        return Err(String::from("string package contains nonzero bytes after END"));
    }
    package.max_string_id = if current_id <= 1 {
        0
    } else {
        u16::try_from(current_id - 1)
            .map_err(|_| String::from("maximum string ID exceeds u16"))?
    };
    Ok(package)
}

