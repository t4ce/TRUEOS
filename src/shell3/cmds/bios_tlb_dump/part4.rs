fn append_raw_hii_export(out: &mut String, bytes: &[u8]) {
    push_record(
        out,
        serde_json::json!({
            "record": "raw-hii-export",
            "bytes": bytes.len(),
            "crc32": alloc::format!("0x{:08X}", crc32fast::hash(bytes)),
            "encoding": "hex",
            "row_bytes": RAW_ROW_BYTES,
            "complete": true,
            "configuration_content": "redacted-not-included",
        }),
    );

    for (row, chunk) in bytes.chunks(RAW_ROW_BYTES).enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "raw-hii-bytes",
                "offset": row.saturating_mul(RAW_ROW_BYTES),
                "hex": hex_bytes(chunk),
            }),
        );
    }
}

fn append_opaque_record(
    out: &mut String,
    record: &'static str,
    question_key: Option<&str>,
    index: usize,
    opcode: &OpaqueOpcode,
) {
    push_record(
        out,
        serde_json::json!({
            "record": record,
            "question_key": question_key,
            "index": index,
            "list": opcode.list_index,
            "package": opcode.package_index,
            "source_offset": opcode.source_offset,
            "opcode": opcode.opcode,
            "opcode_hex": alloc::format!("0x{:02X}", opcode.opcode),
            "length": opcode.length,
            "scope": opcode.scope,
            "raw_hex": hex_bytes(&opcode.raw),
        }),
    );
}

fn ifr_value_json(value: &IfrValue) -> serde_json::Value {
    serde_json::json!({
        "type_code": value.type_code,
        "type_code_hex": alloc::format!("0x{:02X}", value.type_code),
        "unsigned": value.unsigned,
        "boolean": value.boolean,
        "string_id": value.string_id,
        "raw_hex": hex_bytes(&value.raw),
    })
}
