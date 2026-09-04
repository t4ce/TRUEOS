use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::mem::size_of;

use serde_json::{Value, json};

use crate::efi::EfiGuid;

use super::bios_hii::HiiCatalogue;
use super::bios_ifr::{BiosSchema, ConditionKind, Question, QuestionKind, VarStoreBackend};

const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;
const MAX_CONFIG_BYTES: usize = 4 * 1024 * 1024;
const MAX_SECTIONS: usize = 8;
const MAX_CONFIG_RESPONSES: usize = 4096;
const MAX_BLOCKS_PER_RESPONSE: usize = 4096;
const MAX_CURRENT_VALUE_BYTES: usize = 4096;

const CATALOG_MAGIC: [u8; 8] = *b"TRBIOS1\0";
const PAYLOAD_MAGIC: [u8; 8] = *b"TRPAY1\0\0";
const VERSION: u16 = 1;
const SEC_CONFIG: u32 = 3;

const TRUEOS_BIOS_CATALOG_GUID: EfiGuid = EfiGuid {
    data1: 0x184d_a5de,
    data2: 0xfa77,
    data3: 0x4a1f,
    data4: [0xb4, 0x27, 0xd4, 0xdb, 0xfc, 0xe6, 0xd7, 0xf7],
};

#[repr(C)]
#[derive(Clone, Copy)]
struct CatalogHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    flags: u32,
    package_list_count: u32,
    formset_count: u32,
    question_count: u32,
    payload_bytes: u32,
    payload_crc32: u32,
    reserved: u32,
    payload_phys: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PayloadHeader {
    magic: [u8; 8],
    version: u16,
    header_bytes: u16,
    section_entry_bytes: u16,
    reserved0: u16,
    section_count: u32,
    total_bytes: u32,
    capture_flags: u32,
    reserved1: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SectionEntry {
    kind: u32,
    flags: u32,
    offset: u32,
    length: u32,
    crc32: u32,
    reserved: u32,
    status: u64,
}

#[derive(Clone)]
struct ConfigBlock {
    offset: usize,
    bytes: Vec<u8>,
}

#[derive(Clone)]
struct ConfigResponse {
    guid: [u8; 16],
    name: String,
    path: Vec<u8>,
    altcfg: Option<u16>,
    blocks: Vec<ConfigBlock>,
}

struct QuestionCurrent {
    record_key: String,
    formset_index: usize,
    form_id: u16,
    question_id: u16,
    source_offset: u32,
    status: &'static str,
    detail: &'static str,
    display: Option<String>,
    unsigned: Option<u64>,
    boolean: Option<bool>,
    text: Option<String>,
    option_label: Option<String>,
    visibility: &'static str,
    condition_results: Vec<ConditionResult>,
}

struct ConditionResult {
    kind: &'static str,
    result: Tri,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tri {
    False,
    True,
    Unknown,
}

impl Tri {
    const fn name(self) -> &'static str {
        match self {
            Self::False => "false",
            Self::True => "true",
            Self::Unknown => "unknown",
        }
    }
}

/// Build the sanitized current-value view consumed by the Blueprint snapshot.
///
/// Values come only from the pre-ExitBootServices HII Config Routing
/// `ExportConfig()` capture. No Runtime Service or firmware protocol is called
/// here. Raw configuration strings and raw variable buffers are never emitted.
pub(crate) fn snapshot_json(schema: &BiosSchema) -> Value {
    match super::bios_hii::with_catalogue(|catalogue| build_snapshot(schema, catalogue)) {
        Ok(Ok(value)) => value,
        Ok(Err(error)) | Err(error) => json!({
            "state": "unavailable",
            "source": "captured-hii-export-config",
            "captureTiming": "pre-ExitBootServices",
            "live": false,
            "rawConfig": "hidden",
            "detail": bounded_detail(&error),
            "questions": []
        }),
    }
}

fn build_snapshot(schema: &BiosSchema, catalogue: &HiiCatalogue) -> Result<Value, String> {
    let config_bytes = locate_config_section()?;
    let config = decode_utf16_config(config_bytes)?;
    let responses = parse_multi_config(&config)?;
    let current_responses = responses
        .iter()
        .filter(|response| response.altcfg.is_none())
        .count();

    let mut records = Vec::<QuestionCurrent>::with_capacity(schema.stats.questions);
    let mut index_by_key = BTreeMap::<String, usize>::new();
    let mut matched_varstores = BTreeMap::<(usize, u16), bool>::new();

    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        let expected_path = catalogue
            .device_path_packages
            .iter()
            .find(|package| package.list_index == formset.list_index)
            .and_then(|package| package.bytes.get(4..));

        for form in &formset.forms {
            for question in &form.questions {
                let mut current = decode_question_current(
                    formset_index,
                    form.id,
                    question,
                    expected_path,
                    &responses,
                );
                if current.status == "decoded" {
                    matched_varstores.insert((formset_index, question.varstore_id), true);
                }
                let key = current.record_key.clone();
                let index = records.len();
                index_by_key.insert(key, index);
                current.visibility = "visible";
                records.push(current);
            }
        }
    }

    let mut conditions_evaluated = 0usize;
    let mut conditions_unknown = 0usize;
    for (formset_index, formset) in schema.formsets.iter().enumerate() {
        for form in &formset.forms {
            for question in &form.questions {
                let key = record_key(formset_index, form.id, question);
                let Some(&record_index) = index_by_key.get(&key) else {
                    continue;
                };
                let mut state = "visible";
                let mut saw_unknown = false;
                let mut results = Vec::with_capacity(question.conditions.len());
                for condition in &question.conditions {
                    let result = eval_expression(&condition.expression, formset_index, &records);
                    if result == Tri::Unknown {
                        conditions_unknown = conditions_unknown.saturating_add(1);
                        saw_unknown = true;
                    } else {
                        conditions_evaluated = conditions_evaluated.saturating_add(1);
                    }
                    if result == Tri::True {
                        state = match condition.kind {
                            ConditionKind::Suppress => "suppressed",
                            ConditionKind::Disable if state != "suppressed" => "disabled",
                            ConditionKind::GrayOut
                                if state != "suppressed" && state != "disabled" =>
                            {
                                "gray"
                            }
                            _ => state,
                        };
                    }
                    results.push(ConditionResult {
                        kind: condition.kind.name(),
                        result,
                    });
                }
                if state == "visible" && saw_unknown {
                    state = "unknown";
                }
                records[record_index].visibility = state;
                records[record_index].condition_results = results;
            }
        }
    }

    let decoded = records
        .iter()
        .filter(|record| record.status == "decoded")
        .count();
    let unavailable = records.len().saturating_sub(decoded);
    let questions = records
        .into_iter()
        .map(|record| {
            json!({
                "recordKey": record.record_key,
                "formsetIndex": record.formset_index,
                "formId": record.form_id,
                "questionId": record.question_id,
                "sourceOffset": record.source_offset,
                "status": record.status,
                "detail": record.detail,
                "source": "captured-hii-export-config",
                "display": record.display,
                "unsigned": record.unsigned,
                "boolean": record.boolean,
                "text": record.text,
                "optionLabel": record.option_label,
                "visibility": record.visibility,
                "conditions": record
                    .condition_results
                    .iter()
                    .map(|condition| json!({
                        "kind": condition.kind,
                        "result": condition.result.name()
                    }))
                    .collect::<Vec<_>>()
            })
        })
        .collect::<Vec<_>>();

    Ok(json!({
        "state": "ready",
        "source": "captured-hii-export-config",
        "captureTiming": "pre-ExitBootServices",
        "live": false,
        "rawConfig": "hidden",
        "responses": responses.len(),
        "currentResponses": current_responses,
        "matchedVarstores": matched_varstores.len(),
        "questionsDecoded": decoded,
        "questionsUnavailable": unavailable,
        "conditionsEvaluated": conditions_evaluated,
        "conditionsUnknown": conditions_unknown,
        "questions": questions
    }))
}

fn decode_question_current(
    formset_index: usize,
    form_id: u16,
    question: &Question,
    expected_path: Option<&[u8]>,
    responses: &[ConfigResponse],
) -> QuestionCurrent {
    let mut current = QuestionCurrent {
        record_key: record_key(formset_index, form_id, question),
        formset_index,
        form_id,
        question_id: question.id,
        source_offset: question.source_offset,
        status: "unavailable",
        detail: "not-decoded",
        display: None,
        unsigned: None,
        boolean: None,
        text: None,
        option_label: None,
        visibility: "visible",
        condition_results: Vec::new(),
    };

    if question.kind == QuestionKind::Action {
        current.detail = "action-has-no-current-value";
        return current;
    }
    if !question.storage.valid {
        current.detail = "storage-not-validated";
        return current;
    }
    if !matches!(
        question.storage.backend,
        VarStoreBackend::Buffer | VarStoreBackend::Efi
    ) {
        current.detail = "storage-backend-not-block-config";
        return current;
    }
    let (Some(guid), Some(name), Some(offset), Some(width)) = (
        question.storage.variable_guid.as_ref(),
        question.storage.variable.as_deref(),
        question.storage.offset,
        question.storage.width,
    ) else {
        current.detail = "storage-binding-incomplete";
        return current;
    };
    let width = usize::from(width);
    if width == 0 || width > MAX_CURRENT_VALUE_BYTES {
        current.detail = "current-value-width-outside-bound";
        return current;
    }

    let response = match_config_response(responses, guid, name, expected_path);
    let Ok(response) = response else {
        current.detail = response.err().unwrap_or("config-response-unavailable");
        return current;
    };
    let Some(bytes) = read_block_range(response, usize::from(offset), width) else {
        current.detail = "config-block-range-unavailable";
        return current;
    };

    match question.kind {
        QuestionKind::Checkbox => {
            let value = bytes.iter().copied().enumerate().fold(0u64, |acc, (index, byte)| {
                if index < 8 {
                    acc | (u64::from(byte) << (index * 8))
                } else {
                    acc
                }
            });
            let boolean = value != 0;
            current.unsigned = Some(value);
            current.boolean = Some(boolean);
            current.display = Some(if boolean { "Enabled" } else { "Disabled" }.to_string());
        }
        QuestionKind::Numeric | QuestionKind::OneOf => {
            if bytes.len() > 8 {
                current.detail = "numeric-current-value-too-wide";
                return current;
            }
            let mut value = 0u64;
            for (index, byte) in bytes.iter().copied().enumerate() {
                value |= u64::from(byte) << (index * 8);
            }
            current.unsigned = Some(value);
            if question.kind == QuestionKind::OneOf {
                current.option_label = question
                    .options
                    .iter()
                    .find(|option| option.value.unsigned == Some(value))
                    .and_then(|option| option.text.clone());
            }
            current.display = current
                .option_label
                .clone()
                .or_else(|| Some(value.to_string()));
        }
        QuestionKind::String => {
            if bytes.len() % 2 != 0 {
                current.detail = "string-current-value-is-not-utf16";
                return current;
            }
            let mut units = Vec::with_capacity(bytes.len() / 2);
            for pair in bytes.chunks_exact(2) {
                let unit = u16::from_le_bytes([pair[0], pair[1]]);
                if unit == 0 {
                    break;
                }
                units.push(unit);
            }
            let Ok(text) = String::from_utf16(&units) else {
                current.detail = "string-current-value-invalid-utf16";
                return current;
            };
            current.text = Some(text.clone());
            current.display = Some(if text.is_empty() {
                String::from("Empty")
            } else {
                text
            });
        }
        QuestionKind::Action => unreachable!(),
    }

    current.status = "decoded";
    current.detail = "validated-captured-current-value";
    current
}

fn match_config_response<'a>(
    responses: &'a [ConfigResponse],
    guid: &EfiGuid,
    name: &str,
    expected_path: Option<&[u8]>,
) -> Result<&'a ConfigResponse, &'static str> {
    let wanted_guid = guid_bytes(guid);
    let mut candidates = responses
        .iter()
        .filter(|response| {
            response.altcfg.is_none() && response.guid == wanted_guid && response.name == name
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err("config-response-not-found");
    }
    if let Some(path) = expected_path {
        let path_matches = candidates
            .iter()
            .copied()
            .filter(|response| response.path.as_slice() == path)
            .collect::<Vec<_>>();
        if path_matches.len() == 1 {
            return Ok(path_matches[0]);
        }
        if path_matches.len() > 1 {
            return Err("config-response-path-ambiguous");
        }
    }
    if candidates.len() == 1 {
        return Ok(candidates.remove(0));
    }
    Err("config-response-ambiguous-without-path-match")
}

fn read_block_range(response: &ConfigResponse, offset: usize, width: usize) -> Option<Vec<u8>> {
    let end = offset.checked_add(width)?;
    let mut out = alloc::vec![0u8; width];
    let mut filled = alloc::vec![false; width];
    for block in &response.blocks {
        let block_end = block.offset.checked_add(block.bytes.len())?;
        let overlap_start = core::cmp::max(offset, block.offset);
        let overlap_end = core::cmp::min(end, block_end);
        if overlap_start >= overlap_end {
            continue;
        }
        for absolute in overlap_start..overlap_end {
            let dst = absolute - offset;
            let src = absolute - block.offset;
            out[dst] = block.bytes[src];
            filled[dst] = true;
        }
    }
    if filled.iter().all(|value| *value) {
        Some(out)
    } else {
        None
    }
}

fn eval_expression(
    expression: &[super::bios_ifr::OpaqueOpcode],
    formset_index: usize,
    records: &[QuestionCurrent],
) -> Tri {
    let mut stack = Vec::<Tri>::new();
    for opcode in expression {
        match opcode.opcode {
            0x12 if opcode.raw.len() >= 6 => {
                let question_id = read_u16(&opcode.raw, 2);
                let wanted = u64::from(read_u16(&opcode.raw, 4));
                let actual = unique_unsigned(records, formset_index, question_id);
                stack.push(match actual {
                    Some(actual) if actual == wanted => Tri::True,
                    Some(_) => Tri::False,
                    None => Tri::Unknown,
                });
            }
            0x15 => {
                let right = stack.pop().unwrap_or(Tri::Unknown);
                let left = stack.pop().unwrap_or(Tri::Unknown);
                stack.push(tri_and(left, right));
            }
            0x16 => {
                let right = stack.pop().unwrap_or(Tri::Unknown);
                let left = stack.pop().unwrap_or(Tri::Unknown);
                stack.push(tri_or(left, right));
            }
            0x17 => {
                let value = stack.pop().unwrap_or(Tri::Unknown);
                stack.push(match value {
                    Tri::True => Tri::False,
                    Tri::False => Tri::True,
                    Tri::Unknown => Tri::Unknown,
                });
            }
            0x46 => stack.push(Tri::True),
            0x47 => stack.push(Tri::False),
            _ => return Tri::Unknown,
        }
    }
    if stack.len() == 1 {
        stack[0]
    } else {
        Tri::Unknown
    }
}

fn unique_unsigned(
    records: &[QuestionCurrent],
    formset_index: usize,
    question_id: u16,
) -> Option<u64> {
    let mut found = None;
    for record in records {
        if record.formset_index != formset_index
            || record.question_id != question_id
            || record.status != "decoded"
        {
            continue;
        }
        let value = record.unsigned.or_else(|| record.boolean.map(u64::from))?;
        if found.is_some() {
            return None;
        }
        found = Some(value);
    }
    found
}

const fn tri_and(left: Tri, right: Tri) -> Tri {
    match (left, right) {
        (Tri::False, _) | (_, Tri::False) => Tri::False,
        (Tri::True, Tri::True) => Tri::True,
        _ => Tri::Unknown,
    }
}

const fn tri_or(left: Tri, right: Tri) -> Tri {
    match (left, right) {
        (Tri::True, _) | (_, Tri::True) => Tri::True,
        (Tri::False, Tri::False) => Tri::False,
        _ => Tri::Unknown,
    }
}

fn record_key(formset_index: usize, form_id: u16, question: &Question) -> String {
    format!(
        "fs{}-form{:04X}-q{:04X}-off{:X}",
        formset_index, form_id, question.id, question.source_offset
    )
}

fn parse_multi_config(text: &str) -> Result<Vec<ConfigResponse>, String> {
    let starts = config_response_starts(text);
    if starts.is_empty() {
        return Err(String::from("captured ExportConfig has no GUID headers"));
    }
    if starts.len() > MAX_CONFIG_RESPONSES {
        return Err(format!(
            "captured ExportConfig responses={} exceeds bound={}",
            starts.len(),
            MAX_CONFIG_RESPONSES
        ));
    }
    let mut responses = Vec::with_capacity(starts.len());
    for (index, start) in starts.iter().copied().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(text.len());
        let segment = text[start..end].trim_matches('&');
        responses.push(parse_config_response(segment)?);
    }
    Ok(responses)
}

fn config_response_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let needle = b"GUID=";
    let mut starts = Vec::new();
    let mut cursor = 0usize;
    while cursor.saturating_add(needle.len()) <= bytes.len() {
        let Some(relative) = text[cursor..].find("GUID=") else {
            break;
        };
        let start = cursor + relative;
        if start == 0 || bytes.get(start.wrapping_sub(1)) == Some(&b'&') {
            starts.push(start);
        }
        cursor = start.saturating_add(needle.len());
    }
    starts
}

fn parse_config_response(segment: &str) -> Result<ConfigResponse, String> {
    let mut guid = None;
    let mut name = None;
    let mut path = None;
    let mut altcfg = None;
    let mut blocks = Vec::new();
    let mut pending_offset = None;
    let mut pending_width = None;

    for token in segment.split('&') {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        match key {
            "GUID" => guid = Some(decode_fixed_hex::<16>(value, "GUID")?),
            "NAME" => name = Some(decode_config_name(value)?),
            "PATH" => path = Some(decode_hex_bytes(value, "PATH")?),
            "ALTCFG" => altcfg = Some(parse_hex_u16(value, "ALTCFG")?),
            "OFFSET" => pending_offset = Some(parse_hex_usize(value, "OFFSET")?),
            "WIDTH" => pending_width = Some(parse_hex_usize(value, "WIDTH")?),
            "VALUE" => {
                if blocks.len() >= MAX_BLOCKS_PER_RESPONSE {
                    return Err(String::from("ExportConfig block count exceeds bound"));
                }
                let offset = pending_offset
                    .take()
                    .ok_or_else(|| String::from("VALUE is missing OFFSET"))?;
                let width = pending_width
                    .take()
                    .ok_or_else(|| String::from("VALUE is missing WIDTH"))?;
                let bytes = decode_hex_bytes(value, "VALUE")?;
                if bytes.len() != width {
                    return Err(format!(
                        "ExportConfig VALUE bytes={} does not match WIDTH={}",
                        bytes.len(), width
                    ));
                }
                blocks.push(ConfigBlock { offset, bytes });
            }
            _ => {}
        }
    }

    Ok(ConfigResponse {
        guid: guid.ok_or_else(|| String::from("ConfigResp is missing GUID"))?,
        name: name.ok_or_else(|| String::from("ConfigResp is missing NAME"))?,
        path: path.ok_or_else(|| String::from("ConfigResp is missing PATH"))?,
        altcfg,
        blocks,
    })
}

fn decode_config_name(value: &str) -> Result<String, String> {
    if value.len() % 4 != 0 {
        return Err(String::from("ConfigHdr NAME is not CHAR16 hex"));
    }
    let mut units = Vec::with_capacity(value.len() / 4);
    let bytes = value.as_bytes();
    for index in (0..bytes.len()).step_by(4) {
        let chunk = core::str::from_utf8(&bytes[index..index + 4])
            .map_err(|_| String::from("ConfigHdr NAME is not ASCII hex"))?;
        let unit = u16::from_str_radix(chunk, 16)
            .map_err(|_| String::from("ConfigHdr NAME contains non-hex data"))?;
        units.push(unit);
    }
    String::from_utf16(&units).map_err(|_| String::from("ConfigHdr NAME is invalid UTF-16"))
}

fn decode_hex_bytes(value: &str, field: &str) -> Result<Vec<u8>, String> {
    if value.len() % 2 != 0 {
        return Err(format!("Config {field} has odd hex length"));
    }
    let mut out = Vec::with_capacity(value.len() / 2);
    let bytes = value.as_bytes();
    for index in (0..bytes.len()).step_by(2) {
        let chunk = core::str::from_utf8(&bytes[index..index + 2])
            .map_err(|_| format!("Config {field} is not ASCII"))?;
        out.push(
            u8::from_str_radix(chunk, 16)
                .map_err(|_| format!("Config {field} contains non-hex data"))?,
        );
    }
    Ok(out)
}

fn decode_fixed_hex<const N: usize>(value: &str, field: &str) -> Result<[u8; N], String> {
    let bytes = decode_hex_bytes(value, field)?;
    bytes
        .try_into()
        .map_err(|_| format!("Config {field} has wrong byte length"))
}

fn parse_hex_usize(value: &str, field: &str) -> Result<usize, String> {
    usize::from_str_radix(value, 16).map_err(|_| format!("Config {field} is invalid hex"))
}

fn parse_hex_u16(value: &str, field: &str) -> Result<u16, String> {
    u16::from_str_radix(value, 16).map_err(|_| format!("Config {field} is invalid hex"))
}

fn guid_bytes(guid: &EfiGuid) -> [u8; 16] {
    let mut bytes = [0u8; 16];
    bytes[0..4].copy_from_slice(&guid.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&guid.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&guid.data3.to_le_bytes());
    bytes[8..16].copy_from_slice(&guid.data4);
    bytes
}

fn locate_config_section() -> Result<&'static [u8], String> {
    if let Some(payload) = limine_payload()? {
        return config_section_from_payload(payload);
    }

    let tables = crate::efi::configuration_tables()
        .map_err(|error| format!("configuration tables: {error:?}"))?;
    let entry = tables
        .iter()
        .find(|entry| guid_eq(&entry.vendor_guid, &TRUEOS_BIOS_CATALOG_GUID))
        .ok_or_else(|| String::from("captured HII configuration payload is absent"))?;
    if entry.vendor_table == 0 {
        return Err(String::from("TRBIOS1 table pointer is zero"));
    }
    let catalog_phys = crate::limine::try_as_phys_addr(entry.vendor_table as u64)
        .ok_or_else(|| String::from("TRBIOS1 table pointer is not mappable"))?;
    require_range(catalog_phys, size_of::<CatalogHeader>(), "catalog header")?;
    let mapping = crate::pci::mmio::map_limine_struct::<CatalogHeader>(catalog_phys)
        .map_err(|error| format!("catalog map: {error:?}"))?;
    let catalog = unsafe { core::ptr::read_unaligned(mapping.as_ptr()) };
    if catalog.magic != CATALOG_MAGIC || catalog.version != VERSION {
        return Err(String::from("unsupported TRBIOS1 magic/version"));
    }
    let payload_len = catalog.payload_bytes as usize;
    if payload_len == 0 || payload_len > MAX_PAYLOAD_BYTES {
        return Err(String::from("TRBIOS1 payload length is outside bound"));
    }
    let payload_phys = crate::limine::try_as_phys_addr(catalog.payload_phys)
        .ok_or_else(|| String::from("TRBIOS1 payload pointer is not mappable"))?;
    require_range(payload_phys, payload_len, "catalog payload")?;
    let payload_mapping = crate::pci::mmio::map_mmio_region_exact(payload_phys, payload_len)
        .map_err(|error| format!("payload map: {error:?}"))?;
    let payload = unsafe { core::slice::from_raw_parts(payload_mapping.as_ptr(), payload_len) };
    if crc32fast::hash(payload) != catalog.payload_crc32 {
        return Err(String::from("TRBIOS1 aggregate CRC mismatch"));
    }
    config_section_from_payload(payload)
}

fn limine_payload() -> Result<Option<&'static [u8]>, String> {
    let Some(response) = crate::limine::trueos_hii_capture_response() else {
        return Ok(None);
    };
    let len = usize::try_from(response.size)
        .map_err(|_| String::from("Limine HII payload size does not fit usize"))?;
    if len == 0 || len > MAX_PAYLOAD_BYTES {
        return Err(String::from("Limine HII payload length is outside bound"));
    }
    let phys = crate::limine::try_as_phys_addr(response.address)
        .ok_or_else(|| String::from("Limine HII payload pointer is not mappable"))?;
    require_range(phys, len, "limine HII payload")?;
    let mapping = crate::pci::mmio::map_mmio_region_exact(phys, len)
        .map_err(|error| format!("Limine HII payload map: {error:?}"))?;
    Ok(Some(unsafe {
        core::slice::from_raw_parts(mapping.as_ptr(), len)
    }))
}

fn config_section_from_payload(payload: &'static [u8]) -> Result<&'static [u8], String> {
    let header = read_struct::<PayloadHeader>(payload, 0)?;
    if header.magic != PAYLOAD_MAGIC || header.version != VERSION {
        return Err(String::from("unsupported TRPAY1 magic/version"));
    }
    let header_bytes = usize::from(header.header_bytes);
    let entry_bytes = usize::from(header.section_entry_bytes);
    if header_bytes < size_of::<PayloadHeader>() || entry_bytes < size_of::<SectionEntry>() {
        return Err(String::from("TRPAY1 header or entry size is too small"));
    }
    let count = header.section_count as usize;
    if count == 0 || count > MAX_SECTIONS || header.total_bytes as usize != payload.len() {
        return Err(String::from("TRPAY1 section directory shape is invalid"));
    }
    let directory_end = header_bytes
        .checked_add(
            count
                .checked_mul(entry_bytes)
                .ok_or_else(|| String::from("section directory overflow"))?,
        )
        .ok_or_else(|| String::from("section directory overflow"))?;
    if directory_end > payload.len() {
        return Err(String::from("TRPAY1 section directory is truncated"));
    }

    let mut ranges = Vec::<(usize, usize)>::with_capacity(count);
    let mut config = None;
    for index in 0..count {
        let entry_offset = header_bytes
            .checked_add(
                index
                    .checked_mul(entry_bytes)
                    .ok_or_else(|| String::from("section entry overflow"))?,
            )
            .ok_or_else(|| String::from("section entry overflow"))?;
        let entry = read_struct::<SectionEntry>(payload, entry_offset)?;
        let start = entry.offset as usize;
        let end = start
            .checked_add(entry.length as usize)
            .ok_or_else(|| String::from("section range overflow"))?;
        if entry.length == 0 || start < directory_end || end > payload.len() {
            return Err(format!("TRPAY1 section {index} range invalid"));
        }
        if ranges
            .iter()
            .any(|&(left, right)| start < right && end > left)
        {
            return Err(format!("TRPAY1 section {index} overlaps another"));
        }
        ranges.push((start, end));
        let bytes = &payload[start..end];
        if crc32fast::hash(bytes) != entry.crc32 {
            return Err(format!("TRPAY1 section {index} CRC mismatch"));
        }
        if entry.kind == SEC_CONFIG {
            if config.is_some() {
                return Err(String::from("TRPAY1 contains duplicate config sections"));
            }
            if bytes.len() > MAX_CONFIG_BYTES {
                return Err(String::from("captured config exceeds bound"));
            }
            config = Some(bytes);
        }
    }
    config.ok_or_else(|| String::from("TRPAY1 config section is absent"))
}

fn decode_utf16_config(bytes: &[u8]) -> Result<String, String> {
    if bytes.len() < 2 || bytes.len() % 2 != 0 {
        return Err(String::from("captured config is not even-length UTF-16"));
    }
    let mut units = Vec::with_capacity(bytes.len() / 2);
    for pair in bytes.chunks_exact(2) {
        units.push(u16::from_le_bytes([pair[0], pair[1]]));
    }
    if units.last().copied() != Some(0) {
        return Err(String::from("captured config is not NUL terminated"));
    }
    units.pop();
    String::from_utf16(&units).map_err(|_| String::from("captured config is invalid UTF-16"))
}

fn require_range(phys: u64, len: usize, what: &str) -> Result<(), String> {
    if !crate::limine::memmap_contains_phys_range(phys, len) {
        return Err(format!("{what} lies outside captured memory map"));
    }
    Ok(())
}

fn read_struct<T: Copy>(bytes: &[u8], offset: usize) -> Result<T, String> {
    let end = offset
        .checked_add(size_of::<T>())
        .ok_or_else(|| String::from("structure offset overflow"))?;
    if end > bytes.len() {
        return Err(String::from("structure is truncated"));
    }
    Ok(unsafe { core::ptr::read_unaligned(bytes[offset..end].as_ptr().cast::<T>()) })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    bytes
        .get(offset..offset.saturating_add(2))
        .and_then(|raw| raw.try_into().ok())
        .map(u16::from_le_bytes)
        .unwrap_or(0)
}

fn guid_eq(left: &EfiGuid, right: &EfiGuid) -> bool {
    left.data1 == right.data1
        && left.data2 == right.data2
        && left.data3 == right.data3
        && left.data4 == right.data4
}

fn bounded_detail(text: &str) -> String {
    text.chars().take(240).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_current_and_alt_config_responses() {
        let guid = "0102030405060708090A0B0C0D0E0F10";
        let name = "0054006500730074"; // Test
        let path = "01010600001C7FFF0400";
        let text = format!(
            "GUID={guid}&NAME={name}&PATH={path}&OFFSET=0&WIDTH=4&VALUE=01020304&GUID={guid}&NAME={name}&PATH={path}&ALTCFG=0000&OFFSET=0&WIDTH=4&VALUE=00000000"
        );
        let parsed = parse_multi_config(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].name, "Test");
        assert_eq!(parsed[0].altcfg, None);
        assert_eq!(parsed[0].blocks[0].bytes, [1, 2, 3, 4]);
        assert_eq!(parsed[1].altcfg, Some(0));
    }

    #[test]
    fn reads_ranges_across_exported_blocks() {
        let response = ConfigResponse {
            guid: [0; 16],
            name: String::from("X"),
            path: Vec::new(),
            altcfg: None,
            blocks: alloc::vec![
                ConfigBlock {
                    offset: 0,
                    bytes: alloc::vec![1, 2],
                },
                ConfigBlock {
                    offset: 2,
                    bytes: alloc::vec![3, 4],
                },
            ],
        };
        assert_eq!(read_block_range(&response, 1, 3), Some(alloc::vec![2, 3, 4]));
    }

    #[test]
    fn tri_state_boolean_operators_are_conservative() {
        assert_eq!(tri_and(Tri::True, Tri::Unknown), Tri::Unknown);
        assert_eq!(tri_and(Tri::False, Tri::Unknown), Tri::False);
        assert_eq!(tri_or(Tri::True, Tri::Unknown), Tri::True);
        assert_eq!(tri_or(Tri::False, Tri::Unknown), Tri::Unknown);
    }
}
