use alloc::string::String;
use core::fmt::Write;

use super::bios_hii::{FormPackageRecord, HiiCatalogue};

const TIANO_GUID: &str = "0F0B1735-87A0-4193-B266-538C38AF48CE";

#[derive(Clone, Copy, Default)]
pub(crate) struct ObservedDecodeStats {
    pub opcode_instances: usize,
    pub decoded_instances: usize,
    pub unresolved_instances: usize,
    pub tiano_extensions: usize,
}

#[derive(Clone, Copy, Default)]
struct ScopeContext {
    formset_index: Option<usize>,
    form_id: Option<u16>,
}

/// Emit a source-ordered IFR node stream for the complete captured forms payload.
///
/// This is deliberately presentation-oriented: every opcode is represented in order,
/// standard opcodes observed on the acceptance board are decoded into typed fields,
/// Tiano GUID extensions are decoded when recognized, and anything future remains a
/// bounded raw node instead of disappearing from the UI model.
pub(crate) fn append_ordered_ifr_records(
    out: &mut String,
    catalogue: &HiiCatalogue,
) -> ObservedDecodeStats {
    let mut stats = ObservedDecodeStats::default();
    let mut next_formset_index = 0usize;

    for package in &catalogue.form_packages {
        append_package_nodes(
            out,
            catalogue,
            package,
            &mut next_formset_index,
            &mut stats,
        );
    }

    stats
}

fn append_package_nodes(
    out: &mut String,
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    next_formset_index: &mut usize,
    stats: &mut ObservedDecodeStats,
) {
    if package.bytes.len() < 4 {
        return;
    }

    let mut stack = alloc::vec::Vec::<ScopeContext>::new();
    let mut cursor = 4usize;
    while cursor.saturating_add(2) <= package.bytes.len() {
        let opcode = package.bytes[cursor];
        let length = package.bytes[cursor + 1] & 0x7f;
        let scope = package.bytes[cursor + 1] & 0x80 != 0;
        if length < 2 {
            break;
        }
        let Some(end) = cursor.checked_add(length as usize) else {
            break;
        };
        if end > package.bytes.len() {
            break;
        }
        let bytes = &package.bytes[cursor..end];
        let mut context = stack.last().copied().unwrap_or_default();

        if opcode == 0x0e {
            context.formset_index = Some(*next_formset_index);
            context.form_id = None;
            *next_formset_index = next_formset_index.saturating_add(1);
        } else if opcode == 0x01 && bytes.len() >= 6 {
            context.form_id = read_u16(bytes, 2);
        }

        let opcode_name = opcode_name(opcode);
        stats.opcode_instances = stats.opcode_instances.saturating_add(1);
        if opcode_name.is_some() {
            stats.decoded_instances = stats.decoded_instances.saturating_add(1);
        } else {
            stats.unresolved_instances = stats.unresolved_instances.saturating_add(1);
        }

        let details = decode_details(catalogue, package, opcode, bytes, stats);
        push_record(
            out,
            serde_json::json!({
                "record": "ifr-node",
                "list": package.list_index,
                "package": package.package_index,
                "source_offset": cursor,
                "formset_index": context.formset_index,
                "form_id": context.form_id,
                "opcode": opcode,
                "opcode_hex": alloc::format!("0x{:02X}", opcode),
                "opcode_name": opcode_name.unwrap_or("opaque-future-opcode"),
                "length": length,
                "scope": scope,
                "details": details,
                "raw_hex": hex_bytes(bytes),
            }),
        );

        if opcode == 0x29 {
            let _ = stack.pop();
        } else if scope {
            stack.push(context);
        }
        cursor = end;
    }
}

fn decode_details(
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    opcode: u8,
    bytes: &[u8],
    stats: &mut ObservedDecodeStats,
) -> serde_json::Value {
    match opcode {
        0x01 if bytes.len() >= 6 => {
            let form_id = read_u16(bytes, 2);
            let title_id = read_u16(bytes, 4);
            serde_json::json!({
                "form_id": form_id,
                "title_id": title_id,
                "title": resolve(catalogue, package.list_index, title_id),
            })
        }
        0x02 if bytes.len() >= 7 => statement_json(catalogue, package, bytes, Some(bytes[6])),
        0x03 if bytes.len() >= 8 => {
            let mut value = statement_json(catalogue, package, bytes, None);
            if let Some(object) = value.as_object_mut() {
                let text_two_id = read_u16(bytes, 6);
                object.insert("text_two_id".into(), serde_json::json!(text_two_id));
                object.insert(
                    "text_two".into(),
                    serde_json::json!(resolve(catalogue, package.list_index, text_two_id)),
                );
            }
            value
        }
        0x05 | 0x06 | 0x07 | 0x0c | 0x1c if bytes.len() >= 13 => {
            question_header_json(catalogue, package, bytes)
        }
        0x08 if bytes.len() >= 17 => {
            let mut value = question_header_json(catalogue, package, bytes);
            if let Some(object) = value.as_object_mut() {
                object.insert("minimum_chars".into(), serde_json::json!(read_u16(bytes, 13)));
                object.insert("maximum_chars".into(), serde_json::json!(read_u16(bytes, 15)));
            }
            value
        }
        0x0e if bytes.len() >= 23 => {
            let title_id = read_u16(bytes, 18);
            let help_id = read_u16(bytes, 20);
            serde_json::json!({
                "guid": guid_at(bytes, 2),
                "title_id": title_id,
                "title": resolve(catalogue, package.list_index, title_id),
                "help_id": help_id,
                "help": resolve(catalogue, package.list_index, help_id),
                "flags": bytes[22],
            })
        }
        0x0f if bytes.len() >= 15 => {
            let mut value = question_header_json(catalogue, package, bytes);
            if let Some(object) = value.as_object_mut() {
                object.insert("target_form_id".into(), serde_json::json!(read_u16(bytes, 13)));
                if bytes.len() >= 17 {
                    object.insert("target_question_id".into(), serde_json::json!(read_u16(bytes, 15)));
                }
                if bytes.len() >= 33 {
                    object.insert("target_formset_guid".into(), serde_json::json!(guid_at(bytes, 17)));
                }
                if bytes.len() >= 35 {
                    let device_path_id = read_u16(bytes, 33);
                    object.insert("device_path_id".into(), serde_json::json!(device_path_id));
                    object.insert(
                        "device_path".into(),
                        serde_json::json!(resolve(catalogue, package.list_index, device_path_id)),
                    );
                }
            }
            value
        }
        0x12 if bytes.len() >= 6 => serde_json::json!({
            "question_id": read_u16(bytes, 2),
            "value": read_u16(bytes, 4),
        }),
        0x2c if bytes.len() >= 7 => serde_json::json!({
            "varstore_id": read_u16(bytes, 2),
            "varstore_info": read_u16(bytes, 4),
            "varstore_type": bytes[6],
        }),
        0x45 if bytes.len() >= 10 => serde_json::json!({
            "value": read_u64(bytes, 2),
        }),
        0x5f if bytes.len() >= 18 => decode_guid_extension(bytes, stats),
        _ => serde_json::Value::Null,
    }
}

fn statement_json(
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    bytes: &[u8],
    flags: Option<u8>,
) -> serde_json::Value {
    let prompt_id = read_u16(bytes, 2);
    let help_id = read_u16(bytes, 4);
    serde_json::json!({
        "prompt_id": prompt_id,
        "prompt": resolve(catalogue, package.list_index, prompt_id),
        "help_id": help_id,
        "help": resolve(catalogue, package.list_index, help_id),
        "flags": flags,
    })
}

fn question_header_json(
    catalogue: &HiiCatalogue,
    package: &FormPackageRecord,
    bytes: &[u8],
) -> serde_json::Value {
    let prompt_id = read_u16(bytes, 2);
    let help_id = read_u16(bytes, 4);
    serde_json::json!({
        "prompt_id": prompt_id,
        "prompt": resolve(catalogue, package.list_index, prompt_id),
        "help_id": help_id,
        "help": resolve(catalogue, package.list_index, help_id),
        "question_id": read_u16(bytes, 6),
        "varstore_id": read_u16(bytes, 8),
        "varstore_info": read_u16(bytes, 10),
        "question_flags": bytes[12],
    })
}

fn decode_guid_extension(bytes: &[u8], stats: &mut ObservedDecodeStats) -> serde_json::Value {
    let guid = guid_at(bytes, 2);
    let payload = if bytes.len() > 18 { &bytes[18..] } else { &[] };
    if guid.as_deref() != Some(TIANO_GUID) || payload.is_empty() {
        return serde_json::json!({
            "guid": guid,
            "extension": "generic-guid",
            "payload_hex": hex_bytes(payload),
        });
    }

    stats.tiano_extensions = stats.tiano_extensions.saturating_add(1);
    let extend_opcode = payload[0];
    let (name, value) = match extend_opcode {
        0 if payload.len() >= 3 => ("label", Some(read_u16(payload, 1))),
        1 if payload.len() >= 6 => ("banner", Some(read_u16(payload, 1))),
        2 if payload.len() >= 3 => ("timeout", Some(read_u16(payload, 1))),
        3 if payload.len() >= 3 => ("class", Some(read_u16(payload, 1))),
        4 if payload.len() >= 3 => ("subclass", Some(read_u16(payload, 1))),
        _ => ("tiano-unknown", None),
    };
    serde_json::json!({
        "guid": guid,
        "extension": "tiano",
        "extend_opcode": extend_opcode,
        "extend_name": name,
        "value": value,
        "payload_hex": hex_bytes(payload),
    })
}

fn opcode_name(opcode: u8) -> Option<&'static str> {
    Some(match opcode {
        0x01 => "form",
        0x02 => "subtitle",
        0x03 => "text",
        0x05 => "one-of",
        0x06 => "checkbox",
        0x07 => "numeric",
        0x08 => "password",
        0x09 => "one-of-option",
        0x0a => "suppress-if",
        0x0c => "action",
        0x0e => "form-set",
        0x0f => "ref",
        0x12 => "eq-id-val",
        0x16 => "or",
        0x19 => "gray-out-if",
        0x1c => "string",
        0x1e => "disable-if",
        0x24 => "varstore",
        0x25 => "varstore-name-value",
        0x26 => "varstore-efi",
        0x29 => "end",
        0x2c => "set-expression",
        0x2e => "write-expression",
        0x45 => "uint64-expression",
        0x46 => "true-expression",
        0x58 => "this-expression",
        0x5b => "default",
        0x5c => "defaultstore",
        0x5f => "guid-extension",
        _ => return None,
    })
}

fn resolve(catalogue: &HiiCatalogue, list_index: usize, id: u16) -> Option<String> {
    catalogue.resolve_string_owned(list_index, id)
}

fn guid_at(bytes: &[u8], offset: usize) -> Option<String> {
    let end = offset.checked_add(16)?;
    let raw = bytes.get(offset..end)?;
    let d1 = u32::from_le_bytes(raw[0..4].try_into().ok()?);
    let d2 = u16::from_le_bytes(raw[4..6].try_into().ok()?);
    let d3 = u16::from_le_bytes(raw[6..8].try_into().ok()?);
    Some(alloc::format!(
        "{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        d1, d2, d3, raw[8], raw[9], raw[10], raw[11], raw[12], raw[13], raw[14], raw[15]
    ))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    bytes
        .get(offset..offset.saturating_add(2))
        .and_then(|raw| raw.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    bytes
        .get(offset..offset.saturating_add(8))
        .and_then(|raw| raw.try_into().ok())
        .map(u64::from_le_bytes)
        .unwrap_or(0)
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        write!(out, "{:02X}", byte).unwrap();
    }
    out
}

fn push_record(out: &mut String, value: serde_json::Value) {
    if let Ok(line) = serde_json::to_string(&value) {
        out.push_str(&line);
        out.push('\n');
    }
}
