fn insert_decoded(
    package: &mut StringPackageRecord,
    current_id: &mut u32,
    decoded: DecodedText,
    source: StringSource,
) -> Result<(), String> {
    let string_id = next_string_id(*current_id)?;
    if decoded.truncated {
        package.truncated_strings = package.truncated_strings.saturating_add(1);
    }
    package.strings.insert(
        string_id,
        ResolvedString {
            text: decoded.text,
            source,
            truncated: decoded.truncated,
        },
    );
    *current_id = (*current_id)
        .checked_add(1)
        .ok_or_else(|| String::from("string ID overflow"))?;
    Ok(())
}

fn decode_ascii_string(bytes: &[u8], start: usize) -> Result<DecodedText, String> {
    if start >= bytes.len() {
        return Err(String::from("SCSU string starts outside package"));
    }
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|relative| start + relative)
        .ok_or_else(|| String::from("SCSU string is not NUL terminated"))?;
    let raw = &bytes[start..end];
    let truncated = raw.len() > MAX_STORED_STRING_CHARS;
    let mut text = String::new();
    for byte in raw.iter().take(MAX_STORED_STRING_CHARS) {
        let ch = match *byte {
            b'\t' => ' ',
            0x20..=0x7e => *byte as char,
            _ => '\u{fffd}',
        };
        text.push(ch);
    }
    Ok(DecodedText {
        text,
        consumed: end - start + 1,
        truncated,
    })
}

fn decode_ucs2_string(bytes: &[u8], start: usize) -> Result<DecodedText, String> {
    if start >= bytes.len() {
        return Err(String::from("UCS2 string starts outside package"));
    }
    let mut cursor = start;
    let mut units = Vec::<u16>::new();
    let mut total_units = 0usize;
    loop {
        let unit = read_u16(bytes, cursor)?;
        cursor = cursor
            .checked_add(2)
            .ok_or_else(|| String::from("UCS2 cursor overflow"))?;
        if unit == 0 {
            break;
        }
        if total_units < MAX_STORED_STRING_CHARS {
            units.push(unit);
        }
        total_units = total_units.saturating_add(1);
    }
    let truncated = total_units > MAX_STORED_STRING_CHARS;
    let text: String = core::char::decode_utf16(units)
        .map(|result| result.unwrap_or('\u{fffd}'))
        .collect();
    Ok(DecodedText {
        text,
        consumed: cursor - start,
        truncated,
    })
}

fn decode_ascii_metadata(bytes: &[u8], max_chars: usize) -> String {
    let mut text = String::new();
    for byte in bytes.iter().take(max_chars) {
        text.push(match *byte {
            0x20..=0x7e => *byte as char,
            _ => '\u{fffd}',
        });
    }
    text
}

fn next_string_id(current_id: u32) -> Result<u16, String> {
    if current_id == 0 || current_id > u16::MAX as u32 {
        return Err(String::from("string ID exceeds u16"));
    }
    Ok(current_id as u16)
}

fn advance_string_id(current_id: u32, count: u32) -> Result<u32, String> {
    let next = current_id
        .checked_add(count)
        .ok_or_else(|| String::from("string ID overflow"))?;
    if next > u16::MAX as u32 + 1 {
        return Err(String::from("string ID exceeds u16 range"));
    }
    Ok(next)
}

fn skip_extension(
    bytes: &[u8],
    cursor: usize,
    length: usize,
    minimum: usize,
    label: &str,
) -> Result<usize, String> {
    if length < minimum {
        return Err(alloc::format!("{} length={} is too small", label, length));
    }
    checked_advance(cursor, length, bytes.len(), label)
}

fn checked_end(
    start: usize,
    length: usize,
    bound: usize,
    label: &str,
) -> Result<usize, String> {
    let end = start
        .checked_add(length)
        .ok_or_else(|| alloc::format!("{} range overflow", label))?;
    if end > bound {
        return Err(alloc::format!("{} crosses its enclosing boundary", label));
    }
    Ok(end)
}

fn checked_advance(
    start: usize,
    length: usize,
    bound: usize,
    label: &str,
) -> Result<usize, String> {
    checked_end(start, length, bound, label)
}

fn read_struct<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    let end = checked_end(offset, size_of::<T>(), bytes.len(), "structure")?;
    let _ = end;
    Ok(unsafe { core::ptr::read_unaligned(bytes.as_ptr().add(offset).cast::<T>()) })
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = checked_end(offset, 4, bytes.len(), "u32")?;
    let slice = &bytes[offset..end];
    Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, String> {
    let end = checked_end(offset, 2, bytes.len(), "u16")?;
    let slice = &bytes[offset..end];
    Ok(u16::from_le_bytes([slice[0], slice[1]]))
}

fn require_range(phys: u64, bytes: usize, label: &str) -> Result<(), String> {
    if crate::limine::memmap_contains_phys_range(phys, bytes) {
        Ok(())
    } else {
        Err(alloc::format!(
            "{} outside one Limine range phys=0x{:X} bytes={}",
            label,
            phys,
            bytes
        ))
    }
}

fn guid_eq(left: &EfiGuid, right: &EfiGuid) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

fn language_priority(language: &str) -> u8 {
    if language.eq_ignore_ascii_case("en-US") {
        0
    } else if language.eq_ignore_ascii_case("en")
        || language
            .get(..3)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("en-"))
    {
        1
    } else {
        2
    }
}

fn package_type_name(package_type: u8) -> &'static str {
    match package_type {
        0x00 => "all",
        0x01 => "guid",
        HII_FORMS => "forms",
        HII_STRINGS => "strings",
        0x05 => "fonts",
        0x06 => "images",
        0x07 => "simple-fonts",
        0x08 => "device-path",
        0x09 => "keyboard-layout",
        0x0a => "animations",
        HII_END => "end",
        0xe0..=0xff => "system",
        _ => "unknown",
    }
}

pub(crate) fn single_line(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        if count >= max_chars {
            out.push('…');
            break;
        }
        match ch {
            '\r' | '\n' | '\t' => out.push(' '),
            '"' => out.push('\''),
            ch if ch.is_control() => out.push('\u{fffd}'),
            ch => out.push(ch),
        }
        count += 1;
    }
    out
}
