use alloc::string::String;
use core::fmt::Write;

use super::bios_hii::{HiiCatalogue, StringSource};
use super::bios_ifr::{BiosSchema, Form, FormSet, IfrValue, OpaqueOpcode, Question};

const DUMP_FORMAT: &str = "trueos.bios.tlb.ndjson.v1";
const RAW_ROW_BYTES: usize = 32;

/// Append the complete cached, read-only BIOS surface to the ordinary TLB dump.
///
/// The line-oriented JSON records include every indexed package, every decoded
/// string, the full IFR schema, all retained opaque opcodes, and the exact validated
/// bytes of the complete HII export. Captured configuration contents stay redacted.
pub(crate) fn append_dump(out: &mut String) {
    writeln!(out, "=== BIOS HII Catalogue and IFR Object Model ===").unwrap();
    writeln!(out, "dump_format={DUMP_FORMAT}").unwrap();
    writeln!(
        out,
        "coverage=all-indexed-packages,all-decoded-strings,all-schema-records,all-opaque-opcodes,all-hii-export-bytes"
    )
    .unwrap();
    writeln!(out, "bulk_strings=included-explicit-tlb-dump").unwrap();
    writeln!(out, "configuration_content=captured-redacted-if-present").unwrap();
    writeln!(out, "active_write_path=none").unwrap();

    if let Err(error) = super::bios_hii::with_catalogue(|catalogue| {
        append_catalogue_records(out, catalogue)
    }) {
        push_record(
            out,
            serde_json::json!({
                "record": "error",
                "stage": "catalogue",
                "detail": error,
                "active_write_path": "none",
            }),
        );
        finish_dump(out);
        return;
    }

    if let Err(error) =
        super::bios_ifr::with_schema(|schema| append_schema_records(out, schema))
    {
        push_record(
            out,
            serde_json::json!({
                "record": "error",
                "stage": "ifr-schema",
                "detail": error,
                "active_write_path": "none",
            }),
        );
    }

    if let Err(error) =
        super::bios_hii::with_raw_hii(|bytes| append_raw_hii_export(out, bytes))
    {
        push_record(
            out,
            serde_json::json!({
                "record": "error",
                "stage": "raw-hii-export",
                "detail": error,
                "active_write_path": "none",
            }),
        );
    }

    finish_dump(out);
}

fn append_catalogue_records(out: &mut String, catalogue: &HiiCatalogue) {
    push_record(
        out,
        serde_json::json!({
            "record": "catalogue",
            "format": DUMP_FORMAT,
            "source": catalogue.capture.source,
            "hii_bytes": catalogue.capture.hii_bytes,
            "package_lists": catalogue.lists.len(),
            "packages": catalogue.packages.len(),
            "string_packages": catalogue.string_packages.len(),
            "form_packages": catalogue.form_packages.len(),
            "strings_resolved": catalogue.stats.decoded_strings,
            "duplicate_strings": catalogue.stats.duplicate_strings,
            "unresolved_duplicates": catalogue.stats.unresolved_duplicates,
            "skipped_string_ids": catalogue.stats.skipped_string_ids,
            "extension_blocks": catalogue.stats.extension_blocks,
            "truncated_strings": catalogue.stats.truncated_strings,
            "malformed_packages": catalogue.stats.malformed_packages,
            "current_configuration": if catalogue.capture.config_captured {
                "captured-redacted"
            } else {
                "not-captured"
            },
            "bulk_strings": "included-explicit-tlb-dump",
            "active_write_path": "none",
        }),
    );

    for (list_index, list) in catalogue.lists.iter().enumerate() {
        push_record(
            out,
            serde_json::json!({
                "record": "package-list",
                "list": list_index,
                "guid": list.guid.fmt_canonical(),
                "first_package": list.first_package,
                "package_count": list.package_count,
            }),
        );
    }

    for package in &catalogue.packages {
        push_record(
            out,
            serde_json::json!({
                "record": "package",
                "list": package.list_index,
                "package": package.package_index,
                "package_type": package.package_type,
                "package_type_hex": alloc::format!("0x{:02X}", package.package_type),
                "package_type_name": package_type_name(package.package_type),
                "offset": package.offset,
                "length": package.length,
            }),
        );
    }

    for package in &catalogue.string_packages {
        let language_name = package
            .strings
            .get(&package.language_name_id)
            .map(|resolved| resolved.text.as_str());
        push_record(
            out,
            serde_json::json!({
                "record": "string-package",
                "list": package.list_index,
                "package": package.package_index,
                "language": package.language.as_str(),
                "language_name_id": package.language_name_id,
                "language_name": language_name,
                "max_string_id": package.max_string_id,
                "resolved": package.strings.len(),
                "duplicate_blocks": package.duplicate_blocks,
                "unresolved_duplicates": package.unresolved_duplicates,
                "skipped_ids": package.skipped_ids,
                "extension_blocks": package.extension_blocks,
                "truncated_strings": package.truncated_strings,
            }),
        );

        for (string_id, resolved) in &package.strings {
            push_record(
                out,
                serde_json::json!({
                    "record": "string",
                    "list": package.list_index,
                    "package": package.package_index,
                    "language": package.language.as_str(),
                    "string_id": string_id,
                    "string_id_hex": alloc::format!("0x{:04X}", string_id),
                    "source": string_source_name(resolved.source),
                    "truncated": resolved.truncated,
                    "text": resolved.text.as_str(),
                }),
            );
        }
    }
}
