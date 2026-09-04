use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use serde_json::{Value, json};
use spin::Mutex;

use super::bios_ifr::{
    BiosSchema, DefaultStore, Form, FormSet, IfrValue, OpaqueOpcode, Question, QuestionDefault,
    QuestionOption, StorageBinding, VarStore, VisibilityCondition,
};

const BIOS_SCHEMA_API: &str = "trueos-bios-schema/v3";
const BIOS_PRESENTATION_API: &str = "trueos-bios-presentation/v1";
const MAX_BIOS_SCHEMA_JSON_BYTES: usize = 16 * 1024 * 1024;
const MAX_PRESENTATION_NODES: usize = 4096;
const ERROR_DETAIL_CHARS: usize = 240;

static BIOS_SCHEMA_JSON_CACHE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

/// Length of the immutable, read-only BIOS schema JSON exposed to Blueprints.
///
/// Building this snapshot consumes only the already captured and validated HII
/// handoff. It does not call Runtime Services, invoke the firmware browser, or
/// write firmware state. Current values, when available, are decoded from the
/// pre-ExitBootServices `ExportConfig()` capture and are attached only to
/// validated question storage bindings.
pub(crate) fn snapshot_len() -> usize {
    with_snapshot(|bytes| bytes.len())
}

/// Copy a slice of the immutable BIOS schema JSON into `out`.
pub(crate) fn snapshot_read(offset: usize, out: &mut [u8]) -> usize {
    with_snapshot(|bytes| {
        if out.is_empty() || offset >= bytes.len() {
            return 0;
        }
        let count = core::cmp::min(out.len(), bytes.len() - offset);
        out[..count].copy_from_slice(&bytes[offset..offset + count]);
        count
    })
}

fn with_snapshot<R>(f: impl FnOnce(&[u8]) -> R) -> R {
    let mut cache = BIOS_SCHEMA_JSON_CACHE.lock();
    if cache.is_none() {
        *cache = Some(build_snapshot());
    }
    f(cache
        .as_deref()
        .expect("BIOS schema snapshot cache initialized"))
}

fn build_snapshot() -> Vec<u8> {
    let result = super::bios_ifr::with_schema(serialize_schema);
    match result {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(error)) | Err(error) => error_snapshot(&error),
    }
}

fn serialize_schema(schema: &BiosSchema) -> Result<Vec<u8>, String> {
    preflight_size(schema)?;

    let mut question_record = 0usize;
    let formsets = schema
        .formsets
        .iter()
        .enumerate()
        .map(|(formset_index, formset)| formset_json(formset_index, formset, &mut question_record))
        .collect::<Vec<_>>();

    let mut unknown_counts = BTreeMap::<u8, usize>::new();
    for opcode in &schema.unknown_opcodes {
        *unknown_counts.entry(opcode.opcode).or_default() += 1;
    }
    let unknown_opcodes = unknown_counts
        .into_iter()
        .map(|(opcode, count)| json!({ "opcode": opcode, "count": count }))
        .collect::<Vec<_>>();

    let (presentation_nodes, presentation_stats) = presentation_snapshot()?;
    let current = super::bios_current::snapshot_json(schema);
    let platform = super::bios::platform_snapshot_json();
    let runtime = super::bios::runtime_snapshot_json();
    let current_ready = current
        .get("state")
        .and_then(Value::as_str)
        .is_some_and(|state| state == "ready");

    let document = json!({
        "api": BIOS_SCHEMA_API,
        "state": schema.state(),
        "source": schema.capture.source,
        "readOnly": true,
        "activeWritePath": "none",
        "platform": platform,
        "runtime": runtime,
        "capture": {
            "hiiBytes": schema.capture.hii_bytes,
            "currentConfiguration": if schema.capture.config_captured {
                "captured"
            } else {
                "not-captured"
            },
            "currentValues": if current_ready {
                "decoded-from-captured-export-config"
            } else if schema.capture.config_captured {
                "captured-not-decoded"
            } else {
                "not-captured"
            },
            "bulkStrings": "hidden",
            "rawPackageBytes": "hidden",
            "rawConfig": "hidden"
        },
        "capabilities": {
            "getVariable": false,
            "setVariable": false,
            "routeConfig": false,
            "formBrowser": false,
            "firmwareWrites": false,
            "questionCallbacks": false,
            "capturedConfigDecode": current_ready,
            "currentValueDecode": current_ready
        },
        "stats": {
            "packageLists": schema.package_lists,
            "packages": schema.packages,
            "formsets": schema.formsets.len(),
            "forms": schema.stats.forms,
            "questions": schema.stats.questions,
            "questionRecords": question_record,
            "stringsResolved": schema.catalogue_strings_resolved,
            "stringReferencesResolved": schema.stats.string_references_resolved,
            "stringReferencesUnresolved": schema.stats.string_references_unresolved,
            "malformedPackages": schema.stats.malformed_packages,
            "varstores": schema.varstores.len(),
            "defaultStores": schema.default_stores.len(),
            "opaqueMetadata": schema.unknown_opcodes.len()
        },
        "presentation": {
            "api": BIOS_PRESENTATION_API,
            "ordered": true,
            "completeForCapturedHii": true,
            "completeMotherboardSetupSurface": "not-claimed",
            "rawBytes": "hidden",
            "stats": {
                "opcodeInstances": presentation_stats.opcode_instances,
                "decodedInstances": presentation_stats.decoded_instances,
                "semanticallyUnresolvedOpcodes": presentation_stats.unresolved_instances,
                "tianoExtensions": presentation_stats.tiano_extensions,
                "nodes": presentation_nodes.len()
            },
            "nodes": presentation_nodes
        },
        "current": current,
        "unknownOpcodes": unknown_opcodes,
        "varstores": schema.varstores.iter().map(varstore_json).collect::<Vec<_>>(),
        "defaultStores": schema
            .default_stores
            .iter()
            .map(default_store_json)
            .collect::<Vec<_>>(),
        "formsets": formsets
    });

    let bytes = serde_json::to_vec(&document)
        .map_err(|error| format!("BIOS schema JSON serialization failed: {error}"))?;
    if bytes.len() > MAX_BIOS_SCHEMA_JSON_BYTES {
        return Err(format!(
            "BIOS schema JSON bytes={} exceeds bound={}",
            bytes.len(),
            MAX_BIOS_SCHEMA_JSON_BYTES
        ));
    }
    Ok(bytes)
}

fn presentation_snapshot() -> Result<(Vec<Value>, super::bios_observed::ObservedDecodeStats), String> {
    super::bios_hii::with_catalogue(|catalogue| {
        let mut ndjson = String::new();
        let stats = super::bios_observed::append_ordered_ifr_records(&mut ndjson, catalogue);
        let mut nodes = Vec::new();

        for line in ndjson.lines() {
            if line.trim().is_empty() {
                continue;
            }
            let mut node = serde_json::from_str::<Value>(line)
                .map_err(|error| format!("BIOS presentation JSON decode failed: {error}"))?;
            sanitize_presentation_node(&mut node);
            nodes.push(node);
            if nodes.len() > MAX_PRESENTATION_NODES {
                return Err(format!(
                    "BIOS presentation nodes={} exceeds bound={}",
                    nodes.len(),
                    MAX_PRESENTATION_NODES
                ));
            }
        }

        Ok((nodes, stats))
    })?
}

fn sanitize_presentation_node(node: &mut Value) {
    let Some(object) = node.as_object_mut() else {
        return;
    };
    object.remove("raw_hex");
    if let Some(details) = object.get_mut("details").and_then(Value::as_object_mut) {
        details.remove("payload_hex");
    }
}

fn formset_json(formset_index: usize, formset: &FormSet, question_record: &mut usize) -> Value {
    json!({
        "index": formset_index,
        "packageList": formset.list_index,
        "package": formset.package_index,
        "guid": formset.guid.fmt_canonical(),
        "titleId": formset.title_id,
        "title": formset.title.as_deref(),
        "helpId": formset.help_id,
        "help": formset.help.as_deref(),
        "flags": formset.flags,
        "forms": formset
            .forms
            .iter()
            .map(|form| form_json(formset_index, form, question_record))
            .collect::<Vec<_>>()
    })
}

fn form_json(formset_index: usize, form: &Form, question_record: &mut usize) -> Value {
    json!({
        "formId": form.id,
        "titleId": form.title_id,
        "title": form.title.as_deref(),
        "sourceOffset": form.source_offset,
        "questions": form
            .questions
            .iter()
            .map(|question| {
                *question_record = question_record.saturating_add(1);
                question_json(formset_index, form.id, *question_record, question)
            })
            .collect::<Vec<_>>()
    })
}

fn question_json(formset_index: usize, form_id: u16, record: usize, question: &Question) -> Value {
    let numeric_range = question.numeric.map(|bounds| {
        json!({
            "minimum": bounds.minimum,
            "maximum": bounds.maximum,
            "step": bounds.step
        })
    });
    let string_limits = question.string_limits.map(|limits| {
        json!({
            "minimumChars": limits.minimum_chars,
            "maximumChars": limits.maximum_chars,
            "multiline": limits.multiline
        })
    });

    json!({
        "record": record,
        "recordKey": format!(
            "fs{}-form{:04X}-q{:04X}-off{:X}",
            formset_index,
            form_id,
            question.id,
            question.source_offset
        ),
        "promptId": question.prompt_id,
        "prompt": question.prompt.as_deref(),
        "helpId": question.help_id,
        "help": question.help.as_deref(),
        "questionId": question.id,
        "kind": question.kind.name(),
        "varstoreId": question.varstore_id,
        "varstoreInfo": question.varstore_info,
        "width": question.width,
        "questionFlags": question.question_flags,
        "kindFlags": question.kind_flags,
        "numericRange": numeric_range,
        "stringLimits": string_limits,
        "sourceOffset": question.source_offset,
        "storage": storage_json(&question.storage),
        "options": question.options.iter().map(option_json).collect::<Vec<_>>(),
        "defaults": question.defaults.iter().map(default_json).collect::<Vec<_>>(),
        "visibility": question
            .conditions
            .iter()
            .map(condition_json)
            .collect::<Vec<_>>(),
        "policy": {
            "requiresReset": question.requires_reset(),
            "callback": question.callback(),
            "firmwareReadOnly": question.read_only(),
            "trueosWrite": "locked"
        },
        "currentValue": "see-current-record"
    })
}

fn storage_json(storage: &StorageBinding) -> Value {
    json!({
        "backend": storage.backend.name(),
        "varstoreId": storage.varstore_id,
        "variable": storage.variable.as_deref(),
        "variableGuid": storage.variable_guid.as_ref().map(|guid| guid.fmt_canonical()),
        "offset": storage.offset,
        "width": storage.width,
        "attributes": storage.attributes,
        "validated": storage.valid,
        "detail": storage.detail,
        "configContent": "hidden"
    })
}

fn option_json(option: &QuestionOption) -> Value {
    json!({
        "textId": option.text_id,
        "text": option.text.as_deref(),
        "flags": option.flags,
        "standardDefault": option.flags & 0x10 != 0,
        "manufacturingDefault": option.flags & 0x20 != 0,
        "value": ifr_value_json(&option.value),
        "sourceOffset": option.source_offset
    })
}

fn default_json(default: &QuestionDefault) -> Value {
    json!({
        "defaultId": default.default_id,
        "label": default.label,
        "value": default.value.as_ref().map(ifr_value_json),
        "source": default.source,
        "sourceOffset": default.source_offset
    })
}

fn ifr_value_json(value: &IfrValue) -> Value {
    json!({
        "typeCode": value.type_code,
        "display": ifr_value_display(value),
        "unsigned": value.unsigned,
        "boolean": value.boolean,
        "stringId": value.string_id,
        "rawBytes": value.raw.len(),
        "raw": "hidden"
    })
}

fn ifr_value_display(value: &IfrValue) -> String {
    if let Some(boolean) = value.boolean {
        return if boolean { "true" } else { "false" }.to_string();
    }
    if let Some(unsigned) = value.unsigned {
        return unsigned.to_string();
    }
    if let Some(string_id) = value.string_id {
        return format!("string-id:0x{string_id:04X}");
    }
    String::from("opaque")
}

fn condition_json(condition: &VisibilityCondition) -> Value {
    json!({
        "kind": condition.kind.name(),
        "sourceOffset": condition.source_offset,
        "opaqueExpressionOpcodes": condition.expression.len(),
        "expression": condition
            .expression
            .iter()
            .map(opcode_metadata_json)
            .collect::<Vec<_>>()
    })
}

fn opcode_metadata_json(opcode: &OpaqueOpcode) -> Value {
    json!({
        "packageList": opcode.list_index,
        "package": opcode.package_index,
        "sourceOffset": opcode.source_offset,
        "opcode": opcode.opcode,
        "length": opcode.length,
        "scope": opcode.scope,
        "rawBytes": opcode.raw.len(),
        "raw": "hidden"
    })
}

fn varstore_json(varstore: &VarStore) -> Value {
    json!({
        "formsetIndex": varstore.formset_index,
        "packageList": varstore.list_index,
        "package": varstore.package_index,
        "varstoreId": varstore.id,
        "backend": varstore.backend.name(),
        "guid": varstore.guid.fmt_canonical(),
        "name": varstore.name.as_deref(),
        "size": varstore.size,
        "attributes": varstore.attributes,
        "sourceOffset": varstore.source_offset
    })
}

fn default_store_json(default_store: &DefaultStore) -> Value {
    json!({
        "formsetIndex": default_store.formset_index,
        "packageList": default_store.list_index,
        "package": default_store.package_index,
        "nameId": default_store.name_id,
        "name": default_store.name.as_deref(),
        "defaultId": default_store.id,
        "sourceOffset": default_store.source_offset
    })
}

fn preflight_size(schema: &BiosSchema) -> Result<(), String> {
    let mut estimate = 16 * 1024usize;
    add_estimate(&mut estimate, schema.varstores.len().saturating_mul(640))?;
    add_estimate(&mut estimate, schema.default_stores.len().saturating_mul(384))?;
    add_estimate(&mut estimate, schema.unknown_opcodes.len().saturating_mul(8))?;
    add_estimate(&mut estimate, MAX_PRESENTATION_NODES.saturating_mul(384))?;
    add_estimate(&mut estimate, schema.stats.questions.saturating_mul(768))?;

    for varstore in &schema.varstores {
        add_text_estimate(&mut estimate, varstore.name.as_deref())?;
    }
    for default_store in &schema.default_stores {
        add_text_estimate(&mut estimate, default_store.name.as_deref())?;
    }
    for formset in &schema.formsets {
        add_estimate(&mut estimate, 768)?;
        add_text_estimate(&mut estimate, formset.title.as_deref())?;
        add_text_estimate(&mut estimate, formset.help.as_deref())?;
        for form in &formset.forms {
            add_estimate(&mut estimate, 512)?;
            add_text_estimate(&mut estimate, form.title.as_deref())?;
            for question in &form.questions {
                add_estimate(&mut estimate, 1536)?;
                add_text_estimate(&mut estimate, question.prompt.as_deref())?;
                add_text_estimate(&mut estimate, question.help.as_deref())?;
                add_text_estimate(&mut estimate, question.storage.variable.as_deref())?;
                add_estimate(&mut estimate, question.options.len().saturating_mul(512))?;
                for option in &question.options {
                    add_text_estimate(&mut estimate, option.text.as_deref())?;
                }
                add_estimate(&mut estimate, question.defaults.len().saturating_mul(512))?;
                for default in &question.defaults {
                    add_text_estimate(&mut estimate, Some(default.label.as_str()))?;
                }
                for condition in &question.conditions {
                    add_estimate(&mut estimate, 320)?;
                    add_estimate(&mut estimate, condition.expression.len().saturating_mul(256))?;
                }
            }
        }
    }

    if estimate > MAX_BIOS_SCHEMA_JSON_BYTES {
        return Err(format!(
            "BIOS schema JSON estimate={} exceeds bound={}",
            estimate, MAX_BIOS_SCHEMA_JSON_BYTES
        ));
    }
    Ok(())
}

fn add_text_estimate(total: &mut usize, text: Option<&str>) -> Result<(), String> {
    let bytes = text
        .map(str::len)
        .unwrap_or(0)
        .checked_mul(6)
        .ok_or_else(|| String::from("BIOS schema JSON size estimate overflow"))?;
    add_estimate(total, bytes)
}

fn add_estimate(total: &mut usize, amount: usize) -> Result<(), String> {
    *total = total
        .checked_add(amount)
        .ok_or_else(|| String::from("BIOS schema JSON size estimate overflow"))?;
    if *total > MAX_BIOS_SCHEMA_JSON_BYTES {
        return Err(format!(
            "BIOS schema JSON estimate={} exceeds bound={}",
            *total, MAX_BIOS_SCHEMA_JSON_BYTES
        ));
    }
    Ok(())
}

fn error_snapshot(error: &str) -> Vec<u8> {
    let document = json!({
        "api": BIOS_SCHEMA_API,
        "state": "unavailable",
        "readOnly": true,
        "activeWritePath": "none",
        "detail": bounded_detail(error),
        "platform": super::bios::platform_snapshot_json(),
        "runtime": super::bios::runtime_snapshot_json(),
        "capture": {
            "currentConfiguration": "redacted",
            "currentValues": "not-decoded",
            "bulkStrings": "hidden",
            "rawPackageBytes": "hidden",
            "rawConfig": "hidden"
        },
        "capabilities": {
            "getVariable": false,
            "setVariable": false,
            "routeConfig": false,
            "formBrowser": false,
            "firmwareWrites": false,
            "questionCallbacks": false,
            "capturedConfigDecode": false,
            "currentValueDecode": false
        },
        "presentation": {
            "api": BIOS_PRESENTATION_API,
            "ordered": true,
            "completeForCapturedHii": false,
            "completeMotherboardSetupSurface": "not-claimed",
            "rawBytes": "hidden",
            "nodes": []
        },
        "current": {
            "state": "unavailable",
            "source": "captured-hii-export-config",
            "rawConfig": "hidden",
            "questions": []
        },
        "formsets": []
    });
    serde_json::to_vec(&document).unwrap_or_else(|_| {
        Vec::from(
            &b"{\"api\":\"trueos-bios-schema/v3\",\"state\":\"unavailable\",\"readOnly\":true,\"activeWritePath\":\"none\",\"platform\":{},\"runtime\":{\"state\":\"unavailable\"},\"presentation\":{\"api\":\"trueos-bios-presentation/v1\",\"ordered\":true,\"nodes\":[]},\"current\":{\"state\":\"unavailable\",\"questions\":[]},\"formsets\":[]}"[..],
        )
    })
}

fn bounded_detail(text: &str) -> String {
    text.chars().take(ERROR_DETAIL_CHARS).collect()
}
