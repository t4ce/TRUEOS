fn formset_key(index: usize, formset: &FormSet) -> String {
    alloc::format!(
        "l{}:p{}:fs{}:{}",
        formset.list_index,
        formset.package_index,
        index,
        formset.guid.fmt_canonical()
    )
}

fn form_key(formset_key: &str, form: &Form) -> String {
    alloc::format!(
        "{}:f{:04X}:o{:X}",
        formset_key,
        form.id,
        form.source_offset
    )
}

fn question_key(form_key: &str, question: &Question) -> String {
    alloc::format!(
        "{}:q{:04X}:o{:X}",
        form_key,
        question.id,
        question.source_offset
    )
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        write!(out, "{:02X}", byte).unwrap();
    }
    out
}

fn string_source_name(source: StringSource) -> &'static str {
    match source {
        StringSource::Scsu => "scsu",
        StringSource::Ucs2 => "ucs2",
        StringSource::Duplicate => "duplicate",
    }
}

fn package_type_name(package_type: u8) -> &'static str {
    match package_type {
        0x00 => "all",
        0x01 => "guid",
        0x02 => "forms",
        0x04 => "strings",
        0x05 => "fonts",
        0x06 => "images",
        0x07 => "simple-fonts",
        0x08 => "device-path",
        0x09 => "keyboard-layout",
        0x0A => "animations",
        0xDF => "end",
        0xE0..=0xFF => "system",
        _ => "unknown",
    }
}

fn push_record(out: &mut String, value: serde_json::Value) {
    match serde_json::to_string(&value) {
        Ok(line) => {
            out.push_str(&line);
            out.push('\n');
        }
        Err(_) => out.push_str(
            "{\"record\":\"serialization-error\",\"active_write_path\":\"none\"}\n",
        ),
    }
}

fn finish_dump(out: &mut String) {
    match super::bios_hii::with_catalogue(|catalogue| {
        super::bios_observed::append_ordered_ifr_records(out, catalogue)
    }) {
        Ok(stats) => push_record(
            out,
            serde_json::json!({
                "record": "ordered-ifr-summary",
                "opcode_instances": stats.opcode_instances,
                "decoded_instances": stats.decoded_instances,
                "semantically_unresolved_opcodes": stats.unresolved_instances,
                "tiano_extensions_decoded": stats.tiano_extensions,
                "active_write_path": "none",
            }),
        ),
        Err(error) => push_record(
            out,
            serde_json::json!({
                "record": "error",
                "stage": "ordered-ifr-decode",
                "detail": error,
                "active_write_path": "none",
            }),
        ),
    }

    push_record(
        out,
        serde_json::json!({
            "record": "end",
            "format": DUMP_FORMAT,
            "active_write_path": "none",
        }),
    );
    writeln!(out, "active_write_path=none").unwrap();
    writeln!(out, "=== END BIOS HII Catalogue and IFR Object Model ===").unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_rows_are_complete_and_uppercase() {
        assert_eq!(hex_bytes(&[0x00, 0x01, 0xA5, 0xFF]), "0001A5FF");
    }
}
